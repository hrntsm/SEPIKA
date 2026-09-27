use super::*;

/// 荷重計算と表示が共有する外周スラブの派生形状 [mm]。
#[derive(Clone, Debug, PartialEq)]
pub struct PerimeterSlab {
    pub beam: ElemId,
    pub story: StoryId,
    pub boundary: [[f64; 3]; 4],
    pub extent_mm: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PerimeterSlabError {
    MissingBeamSection(ElemId),
    MissingColumn { beam: ElemId, node: NodeId },
    MissingColumnSection { beam: ElemId, node: NodeId },
    InvalidColumnDimension { beam: ElemId, node: NodeId },
}

impl std::fmt::Display for PerimeterSlabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBeamSection(id) => {
                write!(f, "外周スラブの大梁 {} の断面が未解決です", id.0)
            }
            Self::MissingColumn { beam, node } => {
                write!(f, "大梁 {} の端部柱（節点 {}）が未解決です", beam.0, node.0)
            }
            Self::MissingColumnSection { beam, node } => write!(
                f,
                "大梁 {} の端部柱（節点 {}）の断面が未解決です",
                beam.0, node.0
            ),
            Self::InvalidColumnDimension { beam, node } => write!(
                f,
                "大梁 {} の端部柱（節点 {}）の直交寸法が不正です",
                beam.0, node.0
            ),
        }
    }
}

impl std::error::Error for PerimeterSlabError {}

fn same_edge(a: [NodeId; 2], b: [NodeId; 2]) -> bool {
    (a[0] == b[0] && a[1] == b[1]) || (a[0] == b[1] && a[1] == b[0])
}

fn has_outer_attached(model: &Model, beam_nodes: [NodeId; 2], floor_edge: [NodeId; 2]) -> bool {
    let Some(f0) = model.nodes.get(floor_edge[0].index()).map(|n| n.coord) else {
        return false;
    };
    let Some(f1) = model.nodes.get(floor_edge[1].index()).map(|n| n.coord) else {
        return false;
    };
    let fd = [f1[0] - f0[0], f1[1] - f0[1]];
    for slab in &model.slabs {
        let SlabShape::Attached {
            anchor: RegionAnchor::Line { nodes, .. },
            extent,
        } = &slab.shape
        else {
            continue;
        };
        if !same_edge(*nodes, beam_nodes) {
            continue;
        }
        let Some(a) = model.nodes.get(nodes[0].index()).map(|n| n.coord) else {
            continue;
        };
        let Some(b) = model.nodes.get(nodes[1].index()).map(|n| n.coord) else {
            continue;
        };
        let along = (b[0] - a[0]) * fd[0] + (b[1] - a[1]) * fd[1];
        let outward_sign = if along >= 0.0 { -1.0 } else { 1.0 };
        if extent.iter().any(|value| value * outward_sign > 0.0) {
            return true;
        }
    }
    false
}

fn column_dimension(
    model: &Model,
    beam: &ElementData,
    node: NodeId,
    story: StoryId,
) -> Result<f64, PerimeterSlabError> {
    let bottom = model
        .layers()
        .into_iter()
        .find(|layer| layer.top == story)
        .map(|layer| layer.bottom_elevation)
        .ok_or(PerimeterSlabError::MissingColumn {
            beam: beam.id,
            node,
        })?;
    let mut current = node;
    let mut visited = std::collections::HashSet::from([node]);
    let mut column_index = None;
    loop {
        let current_z = model
            .nodes
            .get(current.index())
            .ok_or(PerimeterSlabError::MissingColumn {
                beam: beam.id,
                node,
            })?
            .coord[2];
        if current != node && (current_z - bottom).abs() <= crate::model::DIAPHRAGM_LEVEL_TOL_MM {
            break;
        }
        let lower: Vec<_> = model
            .elements
            .iter()
            .enumerate()
            .filter_map(|(index, element)| {
                if element.kind != ElementKind::Beam || element.nodes.len() != 2 {
                    return None;
                }
                let other = if element.nodes[0] == current {
                    element.nodes[1]
                } else if element.nodes[1] == current {
                    element.nodes[0]
                } else {
                    return None;
                };
                let (a, b) = (
                    model.nodes.get(current.index())?,
                    model.nodes.get(other.index())?,
                );
                (crate::geom::is_vertical_axis(a.coord, b.coord) && b.coord[2] < current_z - 1e-9)
                    .then_some((index, other))
            })
            .collect();
        if lower.len() != 1 || !visited.insert(lower[0].1) {
            return Err(PerimeterSlabError::MissingColumn {
                beam: beam.id,
                node,
            });
        }
        if column_index.is_none() {
            column_index = Some(lower[0].0);
        }
        current = lower[0].1;
    }
    let column = &model.elements[column_index.ok_or(PerimeterSlabError::MissingColumn {
        beam: beam.id,
        node,
    })?];
    let section =
        model
            .element_section(column)
            .ok_or(PerimeterSlabError::MissingColumnSection {
                beam: beam.id,
                node,
            })?;
    let ref_vec = [
        column.local_axis.ref_vector[0],
        column.local_axis.ref_vector[1],
    ];
    let norm = (ref_vec[0] * ref_vec[0] + ref_vec[1] * ref_vec[1]).sqrt();
    if section.width <= 0.0
        || section.depth <= 0.0
        || !section.width.is_finite()
        || !section.depth.is_finite()
        || column
            .local_axis
            .ref_vector
            .iter()
            .any(|value| !value.is_finite())
        || !norm.is_finite()
        || norm <= f64::EPSILON
    {
        return Err(PerimeterSlabError::InvalidColumnDimension {
            beam: beam.id,
            node,
        });
    }
    let u = [ref_vec[0] / norm, ref_vec[1] / norm];
    let a = model
        .nodes
        .get(beam.nodes[0].index())
        .ok_or(PerimeterSlabError::InvalidColumnDimension {
            beam: beam.id,
            node,
        })?
        .coord;
    let b = model
        .nodes
        .get(beam.nodes[1].index())
        .ok_or(PerimeterSlabError::InvalidColumnDimension {
            beam: beam.id,
            node,
        })?
        .coord;
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f64::EPSILON {
        return Err(PerimeterSlabError::InvalidColumnDimension {
            beam: beam.id,
            node,
        });
    }
    let normal = [-dy / len, dx / len];
    let cross = [-u[1], u[0]];
    Ok(section.depth * (normal[0] * u[0] + normal[1] * u[1]).abs()
        + section.width * (normal[0] * cross[0] + normal[1] * cross[1]).abs())
}

/// 標準床荷重が設定された階の外周スラブ形状を算定する。
pub fn perimeter_slabs(model: &Model) -> Result<Vec<PerimeterSlab>, PerimeterSlabError> {
    let mut result = Vec::new();
    for beam in model
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Beam && e.nodes.len() == 2)
    {
        let Some(a) = model.nodes.get(beam.nodes[0].index()).map(|n| n.coord) else {
            continue;
        };
        let Some(b) = model.nodes.get(beam.nodes[1].index()).map(|n| n.coord) else {
            continue;
        };
        if (a[2] - b[2]).abs() > 1.0 {
            continue;
        }
        let Some(story) = model
            .stories
            .iter()
            .find(|s| (s.elevation - a[2]).abs() <= 1.0 && s.standard_floor_load.is_some())
        else {
            continue;
        };
        let edges: Vec<_> = model
            .floor_regions
            .iter()
            .filter_map(|r| {
                if r.slab_ids.is_empty() && r.secondary_joists.is_empty() {
                    return None;
                }
                let pos = r
                    .boundary
                    .iter()
                    .enumerate()
                    .find(|(_, n)| **n == beam.nodes[0]);
                if let Some((i, _)) = pos {
                    let next = r.boundary[(i + 1) % r.boundary.len()];
                    if next == beam.nodes[1] {
                        return Some((r, [beam.nodes[0], beam.nodes[1]]));
                    }
                }
                let pos = r
                    .boundary
                    .iter()
                    .enumerate()
                    .find(|(_, n)| **n == beam.nodes[1]);
                if let Some((i, _)) = pos {
                    let next = r.boundary[(i + 1) % r.boundary.len()];
                    if next == beam.nodes[0] {
                        return Some((r, [beam.nodes[1], beam.nodes[0]]));
                    }
                }
                None
            })
            .collect();
        if edges.len() != 1 || has_outer_attached(model, [beam.nodes[0], beam.nodes[1]], edges[0].1)
        {
            continue;
        }
        if model.element_section(beam).is_none() {
            return Err(PerimeterSlabError::MissingBeamSection(beam.id));
        }
        let extent = (column_dimension(model, beam, beam.nodes[0], story.id)?
            + column_dimension(model, beam, beam.nodes[1], story.id)?)
            / 4.0;
        let edge = edges[0].1;
        let p0 = model
            .nodes
            .get(edge[0].index())
            .ok_or(PerimeterSlabError::MissingColumn {
                beam: beam.id,
                node: edge[0],
            })?
            .coord;
        let p1 = model
            .nodes
            .get(edge[1].index())
            .ok_or(PerimeterSlabError::MissingColumn {
                beam: beam.id,
                node: edge[1],
            })?
            .coord;
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len <= f64::EPSILON {
            continue;
        }
        let n = [dy / len, -dx / len];
        result.push(PerimeterSlab {
            beam: beam.id,
            story: story.id,
            boundary: [
                p0,
                p1,
                [p1[0] + n[0] * extent, p1[1] + n[1] * extent, p1[2]],
                [p0[0] + n[0] * extent, p0[1] + n[1] * extent, p0[2]],
            ],
            extent_mm: extent,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(id: u32, nodes: [u32; 2], section: SectionId, ref_vector: [f64; 3]) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: nodes.into_iter().map(NodeId).collect(),
            section: Some(section),
            local_axis: LocalAxis { ref_vector },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }
    }

    fn node(id: u32, coord: [f64; 3]) -> Node {
        Node {
            id: NodeId(id),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        }
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn uses_average_of_different_column_orthogonal_dimensions() {
        let mut model = Model::default();
        model.nodes = vec![
            node(0, [0.0, 0.0, 3000.0]),
            node(1, [6000.0, 0.0, 3000.0]),
            node(2, [6000.0, 4000.0, 3000.0]),
            node(3, [0.0, 4000.0, 3000.0]),
            node(4, [0.0, 0.0, 0.0]),
            node(5, [6000.0, 0.0, 0.0]),
        ];
        model.sections = vec![
            Section {
                id: SectionId(0),
                name: "B".into(),
                depth: 400.0,
                width: 200.0,
                ..Section::zero(SectionId(0), "B".into())
            },
            Section {
                id: SectionId(1),
                name: "C".into(),
                depth: 400.0,
                width: 400.0,
                ..Section::zero(SectionId(1), "C".into())
            },
            Section {
                id: SectionId(2),
                name: "C2".into(),
                depth: 400.0,
                width: 800.0,
                ..Section::zero(SectionId(2), "C2".into())
            },
        ];
        model.elements = vec![
            element(0, [0, 1], SectionId(0), [0.0, 0.0, 1.0]),
            element(1, [0, 4], SectionId(1), [1.0, 0.0, 0.0]),
            element(2, [1, 5], SectionId(2), [1.0, 0.0, 0.0]),
        ];
        let mut region = FloorRegion::new(
            FloorRegionId(0),
            vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        );
        region.slab_ids.push(SlabId(0));
        model.floor_regions.push(region);
        model.stories.insert(
            0,
            Story {
                id: StoryId(1),
                name: "基部".into(),
                elevation: 0.0,
                node_ids: Vec::new(),
                seismic_weight: None,
                weight_override: None,
                structure: StoryStructure::default(),
                level_kind: StoryLevelKind::default(),
                dynamic_mass: None,
                standard_floor_load: None,
            },
        );
        model.stories.push(Story {
            id: StoryId(2),
            name: "1F".into(),
            elevation: 3000.0,
            node_ids: Vec::new(),
            seismic_weight: None,
            weight_override: None,
            structure: StoryStructure::default(),
            level_kind: StoryLevelKind::default(),
            dynamic_mass: None,
            standard_floor_load: Some(StandardFloorLoad {
                dead: 0.005,
                floor: 0.004,
                joist: 0.003,
                frame: 0.006,
                seismic: 0.002,
            }),
        });
        let slabs = perimeter_slabs(&model).unwrap();
        assert_eq!(slabs.len(), 1);
        assert_eq!((400.0 + 800.0) / 4.0, 300.0);
        assert_eq!(slabs[0].extent_mm, 300.0);
        let standard = model.stories[1].standard_floor_load.unwrap();
        assert_eq!(standard.intensity(None), 0.005);
        assert_eq!(standard.intensity(Some(LoadPurpose::Floor)), 0.004);
        assert_eq!(standard.intensity(Some(LoadPurpose::Joist)), 0.003);
        assert_eq!(standard.intensity(Some(LoadPurpose::Frame)), 0.006);
        assert_eq!(standard.intensity(Some(LoadPurpose::Seismic)), 0.002);
        assert_eq!(
            standard.intensity(Some(LoadPurpose::Frame)) * slabs[0].extent_mm,
            1.8
        );
        assert_eq!(
            standard.intensity(Some(LoadPurpose::Seismic)) * slabs[0].extent_mm,
            0.6
        );

        model.slabs.push(Slab {
            id: SlabId(0),
            shape: SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span: [0.0, 1.0],
                    transfer: LoadTransfer::default(),
                },
                extent: [1000.0, -1000.0],
            },
            plate: SlabPlate::default(),
        });
        assert!(perimeter_slabs(&model).unwrap().is_empty());
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn projects_column_section_dimensions_onto_beam_normal() {
        let dimensions = |beam_end: [f64; 3], ref_vector: [f64; 3], depth: f64, width: f64| {
            let mut model = Model::default();
            model.nodes = vec![
                node(0, [0.0, 0.0, 0.0]),
                node(1, beam_end),
                node(2, [0.0, 0.0, -3000.0]),
            ];
            model.sections = vec![
                Section {
                    id: SectionId(0),
                    name: "B".into(),
                    depth: 400.0,
                    width: 200.0,
                    ..Section::zero(SectionId(0), "B".into())
                },
                Section {
                    id: SectionId(1),
                    name: "C".into(),
                    depth,
                    width,
                    ..Section::zero(SectionId(1), "C".into())
                },
            ];
            model.elements = vec![
                element(0, [0, 1], SectionId(0), [0.0, 0.0, 1.0]),
                element(1, [0, 2], SectionId(1), ref_vector),
            ];
            model.stories = vec![
                Story {
                    id: StoryId(0),
                    name: "1F".into(),
                    elevation: -3000.0,
                    node_ids: Vec::new(),
                    seismic_weight: None,
                    weight_override: None,
                    structure: StoryStructure::default(),
                    level_kind: StoryLevelKind::default(),
                    dynamic_mass: None,
                    standard_floor_load: None,
                },
                Story {
                    id: StoryId(1),
                    name: "2F".into(),
                    elevation: 0.0,
                    node_ids: Vec::new(),
                    seismic_weight: None,
                    weight_override: None,
                    structure: StoryStructure::default(),
                    level_kind: StoryLevelKind::default(),
                    dynamic_mass: None,
                    standard_floor_load: None,
                },
            ];
            column_dimension(&model, &model.elements[0], NodeId(0), StoryId(1))
        };

        assert_eq!(
            dimensions([6000.0, 0.0, 0.0], [1.0, 0.0, 0.0], 400.0, 600.0).unwrap(),
            600.0
        );
        assert_eq!(
            dimensions([0.0, 6000.0, 0.0], [0.0, 1.0, 0.0], 400.0, 600.0).unwrap(),
            600.0
        );
        assert_eq!(
            dimensions([0.0, 6000.0, 0.0], [1.0, 0.0, 0.0], 400.0, 600.0).unwrap(),
            400.0
        );
        assert_eq!(
            dimensions([6000.0, 6000.0, 0.0], [1.0, 1.0, 0.0], 400.0, 600.0).unwrap(),
            600.0
        );
        assert_eq!(
            dimensions([6000.0, 6000.0, 0.0], [1.0, -1.0, 0.0], 400.0, 600.0).unwrap(),
            400.0
        );
        assert_eq!(
            dimensions([6000.0, -6000.0, 0.0], [1.0, -1.0, 0.0], 400.0, 600.0).unwrap(),
            600.0
        );
        assert_eq!(
            dimensions([6000.0, -6000.0, 0.0], [-1.0, 1.0, 0.0], 400.0, 600.0).unwrap(),
            600.0
        );

        for (depth, width, ref_vector) in [
            (0.0, 600.0, [1.0, 0.0, 0.0]),
            (-1.0, 600.0, [1.0, 0.0, 0.0]),
            (400.0, 0.0, [1.0, 0.0, 0.0]),
            (400.0, -1.0, [1.0, 0.0, 0.0]),
            (400.0, 600.0, [f64::NAN, 0.0, 0.0]),
            (400.0, 600.0, [f64::INFINITY, 0.0, 0.0]),
            (400.0, 600.0, [1.0e308, 0.0, 0.0]),
        ] {
            assert_eq!(
                dimensions([6000.0, 0.0, 0.0], ref_vector, depth, width),
                Err(PerimeterSlabError::InvalidColumnDimension {
                    beam: ElemId(0),
                    node: NodeId(0),
                })
            );
        }
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn selects_the_column_segment_below_the_target_floor_independent_of_order() {
        let mut model = Model::default();
        model.nodes = vec![
            node(0, [0.0, 0.0, 3000.0]),
            node(1, [6000.0, 0.0, 3000.0]),
            node(2, [0.0, 0.0, 0.0]),
            node(3, [0.0, 0.0, 6000.0]),
            node(4, [3000.0, 3000.0, 0.0]),
        ];
        model.sections = vec![
            Section {
                id: SectionId(0),
                name: "梁".into(),
                depth: 300.0,
                width: 300.0,
                ..Section::zero(SectionId(0), "梁".into())
            },
            Section {
                id: SectionId(1),
                name: "下階柱".into(),
                depth: 400.0,
                width: 400.0,
                ..Section::zero(SectionId(1), "下階柱".into())
            },
            Section {
                id: SectionId(2),
                name: "上階柱".into(),
                depth: 800.0,
                width: 800.0,
                ..Section::zero(SectionId(2), "上階柱".into())
            },
            Section {
                id: SectionId(3),
                name: "斜材".into(),
                depth: 1200.0,
                width: 1200.0,
                ..Section::zero(SectionId(3), "斜材".into())
            },
        ];
        model.elements = vec![
            element(0, [0, 1], SectionId(0), [0.0, 0.0, 1.0]),
            element(1, [0, 3], SectionId(2), [1.0, 0.0, 0.0]),
            element(2, [0, 4], SectionId(3), [1.0, 0.0, 0.0]),
            element(3, [0, 2], SectionId(1), [1.0, 0.0, 0.0]),
        ];
        model.stories = vec![
            Story {
                id: StoryId(0),
                name: "1F".into(),
                elevation: 0.0,
                node_ids: Vec::new(),
                seismic_weight: None,
                weight_override: None,
                structure: StoryStructure::default(),
                level_kind: StoryLevelKind::default(),
                dynamic_mass: None,
                standard_floor_load: None,
            },
            Story {
                id: StoryId(1),
                name: "2F".into(),
                elevation: 3000.0,
                node_ids: Vec::new(),
                seismic_weight: None,
                weight_override: None,
                structure: StoryStructure::default(),
                level_kind: StoryLevelKind::default(),
                dynamic_mass: None,
                standard_floor_load: None,
            },
        ];
        let expected = column_dimension(&model, &model.elements[0], NodeId(0), StoryId(1));
        model.elements.swap(1, 3);
        assert_eq!(
            column_dimension(&model, &model.elements[0], NodeId(0), StoryId(1)),
            expected
        );
        assert_eq!(expected.unwrap(), 400.0);
    }
}
