use super::*;
use crate::geom::vec3::dist;
use crate::section_shape::SectionShape;

/// H形鋼柱の両実材端支持と中間横補剛なしを確認したときのモデル条件。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ColumnEndSupports {
    column: EndSupportMember,
    connected_members: Vec<EndSupportMember>,
    constraints: Vec<EndSupportConstraint>,
    girder_torsion: GirderTorsionMode,
    actual_end_positions_mm: [[f64; 3]; 2],
}

impl ColumnEndSupports {
    /// 確認した両実材端の間隔 [mm]。
    pub fn length_mm(&self) -> f64 {
        let [i, j] = self.actual_end_positions_mm;
        dist(i, j)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct EndSupportMember {
    id: ElemId,
    kind: ElementKind,
    nodes: Vec<EndSupportNode>,
    end_offsets_mm: [[f64; 3]; 2],
    end_cond: [EndCondition; 2],
    local_axis: LocalAxis,
    section: Option<SectionId>,
    shape: Option<SectionShape>,
    section_dimensions_mm: Option<(f64, f64, Option<f64>)>,
    section_stiffness: Option<EndSupportSectionStiffness>,
    material_moduli_mpa: Option<(f64, f64)>,
    manual_rigid_lengths_mm: [Option<f64>; 2],
    spring: Option<[f64; 6]>,
    damper: Option<DamperProps>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct EndSupportSectionStiffness {
    area_mm2: f64,
    shear_areas_mm2: [f64; 2],
    inertias_mm4: [f64; 2],
    torsion_constant_mm4: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct EndSupportNode {
    id: NodeId,
    coord: [f64; 3],
    restraint: Dof6Mask,
    support_spring: Option<[f64; 6]>,
}

impl From<&Node> for EndSupportNode {
    fn from(node: &Node) -> Self {
        Self {
            id: node.id,
            coord: node.coord,
            restraint: node.restraint,
            support_spring: node.support_spring,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct EndSupportConstraint {
    condition: Constraint,
    nodes: Vec<EndSupportNode>,
}

impl EndSupportConstraint {
    fn at(model: &Model, column_ends: [NodeId; 2], constraint: &Constraint) -> Option<Self> {
        let (master, mut participants) = match constraint {
            Constraint::RigidDiaphragm { master, slaves, .. }
            | Constraint::RigidLink { master, slaves, .. } => (*master, slaves.clone()),
            Constraint::Mpc { master, terms } => {
                (*master, terms.iter().map(|&(node, _, _)| node).collect())
            }
        };
        participants.push(master);
        if !column_ends.iter().any(|node| participants.contains(node)) {
            return None;
        }
        participants.sort_by_key(|node| node.0);
        participants.dedup();
        let nodes = participants
            .into_iter()
            .map(|id| model.node(id).map(EndSupportNode::from))
            .collect::<Option<Vec<_>>>()?;
        let condition = match constraint {
            Constraint::RigidDiaphragm {
                story,
                master,
                slaves,
                ..
            } => Constraint::rigid_diaphragm(*story, *master, slaves.clone()),
            _ => constraint.clone(),
        };
        Some(Self { condition, nodes })
    }
}

impl EndSupportMember {
    fn of(model: &Model, elem: &ElementData) -> Option<Self> {
        let nodes = elem
            .nodes
            .iter()
            .map(|id| model.node(*id).map(EndSupportNode::from))
            .collect::<Option<Vec<_>>>()?;
        let section = model.element_section(elem);
        let zone = elem.rigid_zone;
        Some(Self {
            id: elem.id,
            kind: elem.kind,
            nodes,
            end_offsets_mm: elem.end_offsets_mm,
            end_cond: elem.end_cond,
            local_axis: elem.local_axis,
            section: elem.section,
            shape: section.and_then(|sec| sec.shape.clone()),
            section_dimensions_mm: section.map(|sec| (sec.depth, sec.width, sec.thickness)),
            section_stiffness: section.map(|sec| EndSupportSectionStiffness {
                area_mm2: sec.area,
                shear_areas_mm2: [sec.as_y, sec.as_z],
                inertias_mm4: [sec.iy, sec.iz],
                torsion_constant_mm4: sec.resolved_j().ok(),
            }),
            material_moduli_mpa: model
                .element_material(elem)
                .map(|mat| (mat.young, mat.shear_modulus())),
            manual_rigid_lengths_mm: [
                (zone.source_i == ZoneSource::Manual).then_some(zone.length_i),
                (zone.source_j == ZoneSource::Manual).then_some(zone.length_j),
            ],
            spring: elem.spring,
            damper: (elem.kind == ElementKind::Damper)
                .then(|| model.damper_props(elem.id).unwrap_or_default()),
        })
    }
}

impl Model {
    /// 柱の現行の端条件を取得する。実材端を解決できない場合は `None`。
    pub fn column_end_supports(&self, elem: &ElementData) -> Option<ColumnEndSupports> {
        let column = EndSupportMember::of(self, elem)?;
        let [i_node, j_node] = column.nodes.as_slice() else {
            return None;
        };
        let node_ids = [i_node.id, j_node.id];
        let actual_end_positions_mm = self
            .primary_frame_axis_ends(elem)
            .ok()?
            .unwrap_or([i_node.coord, j_node.coord]);
        let [i, j] = actual_end_positions_mm;
        let length_mm = dist(i, j);
        if !length_mm.is_finite() || length_mm <= 0.0 {
            return None;
        }
        let mut connected_members = self
            .elements
            .iter()
            .filter(|other| {
                other.id != elem.id
                    && other.kind != ElementKind::PanelZone
                    && !self.generated_wall_origins.contains_key(&other.id)
                    && other.nodes.len() >= 2
                    && node_ids.iter().any(|node| other.nodes.contains(node))
            })
            .map(|other| EndSupportMember::of(self, other))
            .collect::<Option<Vec<_>>>()?;
        connected_members.sort_by_key(|member| member.id.0);
        let constraints = self
            .constraints
            .iter()
            .filter_map(|constraint| EndSupportConstraint::at(self, node_ids, constraint))
            .collect();
        Some(ColumnEndSupports {
            column,
            connected_members,
            constraints,
            girder_torsion: self.girder_torsion,
            actual_end_positions_mm,
        })
    }
}
