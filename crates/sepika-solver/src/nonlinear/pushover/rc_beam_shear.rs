use std::collections::{HashMap, HashSet};

use super::member_response::{has_haunch, RcShearFaceGeometry};
use sepika_core::dof::Dof;
use sepika_core::face_distance::girder_column_faces;
use sepika_core::geom::vec3::{cross, dist, local_frame_axes, norm};
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{
    ElementData, EndCondition, FrameSectionUse, LoadCase, MemberLoadKind, Model, Section,
};
use sepika_core::rc_capacity::{
    RcBeamUltimateShearIssue as Issue, RcBeamUltimateShearResponse, RcUltimateShearActions,
};
use sepika_core::section_shape::SectionShape;
use sepika_element::behavior::LoadedFrameResponse;

const AXIS_TOLERANCE: f64 = 32.0 * f64::EPSILON;

struct BeamPart<'a> {
    elem: &'a ElementData,
    ends: [[f64; 3]; 2],
    length_mm: f64,
}

pub(super) struct BeamShearStep {
    nodal: HashMap<NodeId, [bool; 3]>,
    member: HashMap<ElemId, [bool; 3]>,
    tolerance_n: f64,
}

impl BeamShearStep {
    pub(super) fn new(case: &LoadCase, force_scale: f64) -> Self {
        let mut nodal = HashMap::new();
        for load in &case.nodal {
            mark_components(nodal.entry(load.node).or_default(), &load.values[..3]);
        }
        let mut member = HashMap::new();
        for load in &case.member {
            let nonzero = match load.kind {
                MemberLoadKind::Point { p, .. } => p != 0.0,
                MemberLoadKind::Distributed { w1, w2, .. } => w1 != 0.0 || w2 != 0.0,
            };
            if nonzero {
                mark_components(member.entry(load.elem).or_default(), &load.dir);
            }
        }
        Self {
            nodal,
            member,
            tolerance_n: crate::common::newton::STATIC_NEWTON.tol * force_scale,
        }
    }

    fn confirm_zero_components(
        &self,
        model: &Model,
        geometry: &RcShearFaceGeometry,
        parts: &[BeamPart<'_>],
        axes: [[f64; 3]; 3],
        torsion_nmm: f64,
        actions: &mut RcUltimateShearActions,
    ) -> Result<(), Issue> {
        let axial_needed = actions.axial_range_n != [0.0; 2];
        let weak_needed = actions.max_mq[1] != (0.0, 0.0);
        let elements: HashSet<_> = parts.iter().map(|part| part.elem.id).collect();
        let Some(first) = parts.first() else {
            return Err(Issue::GroupTopology);
        };
        let nodes = std::iter::once(first.elem.nodes[0])
            .chain(parts.iter().map(|part| part.elem.nodes[1]))
            .map(|id| model.node(id).ok_or(Issue::GroupTopology))
            .collect::<Result<Vec<_>, _>>()?;
        if nodes.iter().any(|node| {
            geometry.other_connections.contains(&node.id)
                || geometry.diaphragm_nodes.contains(&node.id)
                || geometry
                    .frame
                    .elements_at(model, node.id)
                    .any(|elem| !elements.contains(&elem.id))
        }) || parts.iter().any(|part| {
            let elem = part.elem;
            elem.end_offsets_mm != [[0.0; 3]; 2]
                || elem.rigid_zone.panel_offset_i != 0.0
                || elem.rigid_zone.panel_offset_j != 0.0
        }) {
            return Ok(());
        }
        let [ex, ey, ez] = axes;
        let no_member_component = |axis| {
            parts
                .iter()
                .all(|part| !has_axis_component(self.member.get(&part.elem.id), axis))
        };
        let free_in_axial = |&node: &&sepika_core::model::Node| {
            TRANSLATION.into_iter().zip(ex).all(|(dof, component)| {
                component == 0.0
                    || (!node.restraint.is_fixed(dof)
                        && node
                            .support_spring
                            .is_none_or(|spring| spring[dof as usize] == 0.0))
            })
        };
        let mut free_sides = [&nodes[1..], &nodes[..nodes.len() - 1]]
            .into_iter()
            .filter(|side| side.iter().all(free_in_axial))
            .peekable();
        let has_free_side = free_sides.peek().is_some();
        let no_axial_load_on_free_side = no_member_component(ex)
            && free_sides.any(|side| {
                side.iter()
                    .all(|node| !has_axis_component(self.nodal.get(&node.id), ex))
            });
        if !axial_needed && has_free_side && !no_axial_load_on_free_side {
            // ゼロの回復値だけでは、軸方向の個別荷重が正しく相殺されたとは確認できない。
            return Err(Issue::Equilibrium);
        }
        if axial_needed && no_axial_load_on_free_side && self.tolerance_n.is_finite() {
            if !actions
                .axial_range_n
                .iter()
                .all(|n| n.abs() <= self.tolerance_n)
            {
                return Err(Issue::Equilibrium);
            }
            actions.axial_range_n = [0.0; 2];
        }
        let fixed = |node: &sepika_core::model::Node, dofs: [Dof; 3], axis: [f64; 3]| {
            dofs.into_iter()
                .zip(axis)
                .all(|(dof, component)| component == 0.0 || node.restraint.is_fixed(dof))
        };
        // 弱軸とねじりを完全拘束した節点では、拘束方向の節点荷重は直接反力となる。
        if weak_needed
            && self.tolerance_n.is_finite()
            && no_member_component(ez)
            && nodes.iter().all(|node| {
                fixed(node, TRANSLATION, ez)
                    && fixed(node, ROTATION, ey)
                    && fixed(node, ROTATION, ex)
            })
        {
            let (m, q) = actions.max_mq[1];
            let length_mm = dist(
                first.ends[0],
                parts.last().ok_or(Issue::GroupTopology)?.ends[1],
            );
            if !(q <= self.tolerance_n && m.max(torsion_nmm) / length_mm <= self.tolerance_n) {
                return Err(Issue::Equilibrium);
            }
            actions.max_mq[1] = (0.0, 0.0);
        }
        Ok(())
    }
}

const TRANSLATION: [Dof; 3] = [Dof::Ux, Dof::Uy, Dof::Uz];
const ROTATION: [Dof; 3] = [Dof::Rx, Dof::Ry, Dof::Rz];

fn mark_components(marked: &mut [bool; 3], components: &[f64]) {
    for (marked, component) in marked.iter_mut().zip(components) {
        *marked |= *component != 0.0;
    }
}

fn has_axis_component(marked: Option<&[bool; 3]>, axis: [f64; 3]) -> bool {
    // 内積の相殺でゼロとみなさず、重ならない全体成分だけで作用なしを確認する。
    marked.is_some_and(|marked| {
        marked
            .iter()
            .zip(axis)
            .any(|(nonzero, projection)| *nonzero && projection != 0.0)
    })
}

pub(super) fn beam_section<'a>(model: &'a Model, elem: &ElementData) -> Option<&'a Section> {
    let section = model.element_section(elem)?;
    (elem.kind.is_primary_frame_support()
        && section.frame_use == Some(FrameSectionUse::Girder)
        && matches!(section.shape, Some(SectionShape::RcBeamRect { .. })))
    .then_some(section)
}

pub(super) fn grouped_actions(
    model: &Model,
    responses: &[Option<LoadedFrameResponse>],
    geometry: &mut Option<RcShearFaceGeometry>,
    applied: Option<&BeamShearStep>,
) -> HashMap<ElemId, RcBeamUltimateShearResponse> {
    let mut counts = HashMap::<_, usize>::new();
    for &id in model.girder_groups.iter().flatten() {
        *counts.entry(id).or_default() += 1;
    }
    let mut actions = HashMap::with_capacity(counts.len());
    for group in &model.girder_groups {
        if !group.iter().any(|&id| {
            model
                .element(id)
                .is_some_and(|e| beam_section(model, e).is_some())
        }) {
            continue;
        }
        let result = if group.iter().any(|id| counts[id] != 1) {
            Err(Issue::GroupTopology)
        } else {
            beam_actions(model, group, responses, geometry, applied)
        };
        actions.extend(group.iter().map(|&id| (id, result)));
    }
    actions
}

pub(super) fn beam_actions(
    model: &Model,
    group: &[ElemId],
    responses: &[Option<LoadedFrameResponse>],
    geometry: &mut Option<RcShearFaceGeometry>,
    applied: Option<&BeamShearStep>,
) -> RcBeamUltimateShearResponse {
    let geometry = geometry.get_or_insert_with(|| RcShearFaceGeometry::new(model));
    let response_of = |id: ElemId| responses.get(id.index()).and_then(Option::as_ref);
    let mut first_section: Option<&Section> = None;
    let mut first_axes = None;
    let mut exact_axes = true;
    let mut parts: Vec<BeamPart<'_>> = Vec::with_capacity(group.len());
    for &id in group {
        let elem = model.element(id).ok_or(Issue::GroupTopology)?;
        let section = beam_section(model, elem).ok_or(Issue::Section)?;
        let first = first_section.get_or_insert(section);
        if has_haunch(model, id)
            || section.shape != first.shape
            || section.material != first.material
            || section.rebar_material != first.rebar_material
            || section.shear_rebar_material != first.shear_rebar_material
        {
            return Err(Issue::Section);
        }
        let ends = model
            .primary_frame_axis_end_positions(elem)
            .map_err(|_| Issue::GroupTopology)?
            .ok_or(Issue::GroupTopology)?;
        let axes = local_frame_axes(ends[0], ends[1], elem.local_axis.ref_vector)
            .map_err(|_| Issue::LocalAxis)?;
        let reference = first_axes.get_or_insert(axes);
        let response = response_of(id).ok_or(Issue::MissingResponse)?;
        for axes in [axes, response.section.frame().rot] {
            if reference
                .iter()
                .zip(axes)
                .any(|(&a, b)| dist(a, b) > AXIS_TOLERANCE)
            {
                return Err(Issue::LocalAxis);
            }
            exact_axes &= axes == *reference;
        }
        if let Some(previous) = parts.last() {
            let node = model.node(elem.nodes[0]).ok_or(Issue::GroupTopology)?;
            if previous.elem.nodes[1] != node.id
                || previous.ends[1] != node.coord
                || ends[0] != node.coord
                || previous.elem.end_offsets_mm[1] != [0.0; 3]
                || elem.end_offsets_mm[0] != [0.0; 3]
            {
                return Err(Issue::GroupTopology);
            }
            if previous.elem.end_cond[1] != EndCondition::Fixed
                || elem.end_cond[0] != EndCondition::Fixed
                || geometry.other_connections.contains(&node.id)
                || strong_axis_supported(node, axes)
                || ([axes[1][0], axes[1][1], axes[2][2]]
                    .into_iter()
                    .any(|v| v.abs() > AXIS_TOLERANCE)
                    && geometry.diaphragm_nodes.contains(&node.id))
            {
                return Err(Issue::InteriorSupport);
            }
            for other in geometry.frame.elements_at(model, node.id) {
                if other.id == previous.elem.id || other.id == id {
                    continue;
                }
                if !transverse_joint(model, other, node.id, node.coord, axes[0])
                    || response_of(other.id).is_none()
                {
                    return Err(Issue::InteriorSupport);
                }
            }
        }
        parts.push(BeamPart {
            elem,
            ends,
            length_mm: dist(ends[0], ends[1]),
        });
    }

    let first = parts.first().ok_or(Issue::GroupTopology)?;
    let last = parts.last().ok_or(Issue::GroupTopology)?;
    let origin = first.ends[0];
    let end = last.ends[1];
    let length_mm = dist(origin, end);
    let [lo, hi] = outer_faces(
        model,
        geometry,
        [first.elem, last.elem],
        [origin, end],
        length_mm,
    )?;
    if !(0.0 <= lo && lo < hi && hi <= length_mm) {
        return Err(Issue::FacePosition);
    }

    let mut combined = RcUltimateShearActions {
        axial_range_n: [f64::INFINITY, f64::NEG_INFINITY],
        max_mq: [(0.0, 0.0); 2],
    };
    let mut torsion_nmm = 0.0_f64;
    for &BeamPart {
        elem,
        ends: [i, j],
        length_mm: member_mm,
    } in &parts
    {
        let start_mm = dist(origin, i);
        let end_mm = dist(origin, j);
        let response = response_of(elem.id).ok_or(Issue::MissingResponse)?;
        if hi <= start_mm || lo >= end_mm {
            // フェース外の区間の荷重でも、内法の断面力を生じる節点力に伝わりうる。
            response
                .section
                .verify_load_projections()
                .map_err(|_| Issue::Recovery)?;
            continue;
        }
        let i_mm = if lo <= start_mm { 0.0 } else { lo - start_mm };
        let j_mm = if hi >= end_mm {
            member_mm
        } else {
            hi - start_mm
        };
        let ranges = response
            .section
            .section_force_ranges_mm([i_mm, j_mm], member_mm)
            .map_err(|_| Issue::Recovery)?;
        combined.axial_range_n[0] = combined.axial_range_n[0].min(-ranges[0][1]);
        combined.axial_range_n[1] = combined.axial_range_n[1].max(-ranges[0][0]);
        let max_abs = |k: usize| ranges[k][0].abs().max(ranges[k][1].abs());
        torsion_nmm = torsion_nmm.max(max_abs(3));
        for ((m, q), (mi, qi)) in combined.max_mq.iter_mut().zip([(5, 1), (4, 2)]) {
            *m = m.max(max_abs(mi));
            *q = q.max(max_abs(qi));
        }
    }
    if !combined.axial_range_n.into_iter().all(f64::is_finite) {
        return Err(Issue::Recovery);
    }
    if let Some(applied) = applied.filter(|_| exact_axes) {
        applied.confirm_zero_components(
            model,
            geometry,
            &parts,
            first_axes.ok_or(Issue::GroupTopology)?,
            torsion_nmm,
            &mut combined,
        )?;
    }
    Ok(combined)
}

fn strong_axis_supported(node: &sepika_core::model::Node, axes: [[f64; 3]; 3]) -> bool {
    [Dof::Ux, Dof::Uy, Dof::Uz, Dof::Rx, Dof::Ry, Dof::Rz]
        .into_iter()
        .zip(axes[1].into_iter().chain(axes[2]))
        .any(|(dof, projection)| {
            projection.abs() > AXIS_TOLERANCE
                && (node.restraint.is_fixed(dof)
                    || node
                        .support_spring
                        .is_some_and(|spring| spring[dof as usize] != 0.0))
        })
}

fn transverse_joint(
    model: &Model,
    elem: &ElementData,
    node: NodeId,
    point: [f64; 3],
    forward: [f64; 3],
) -> bool {
    let check = || {
        if model.element_section(elem)?.frame_use != Some(FrameSectionUse::Girder) {
            return None;
        }
        let ends = model.primary_frame_axis_ends(elem).ok()??;
        let side = elem.nodes.iter().position(|&n| n == node)?;
        let other = sepika_core::geom::vec3::unit(sepika_core::geom::vec3::sub(ends[1], ends[0]))?;
        (ends[side] == point && norm(cross(forward, other)) > AXIS_TOLERANCE).then_some(())
    };
    check().is_some()
}

fn outer_faces(
    model: &Model,
    geometry: &RcShearFaceGeometry,
    ends: [&ElementData; 2],
    points: [[f64; 3]; 2],
    length_mm: f64,
) -> Result<[f64; 2], Issue> {
    // 外端フェースは端の分割要素の外側でも、一本部材の実材軸内にありうる。
    let mut beam = ends[0].clone();
    beam.nodes[1] = ends[1].nodes[1];
    beam.end_offsets_mm[1] = ends[1].end_offsets_mm[1];
    let faces = girder_column_faces(model, &beam, &geometry.frame).ok_or(Issue::FacePosition)?;
    let resolve = |side: usize| {
        let elem = ends[side];
        let node = elem.nodes[side];
        if geometry.other_connections.contains(&node) {
            return Err(Issue::FacePosition);
        }
        let mut other = geometry
            .frame
            .elements_at(model, node)
            .filter(|o| o.id != elem.id)
            .peekable();
        if other.peek().is_none() {
            let prepared = [elem.rigid_zone.face_i, elem.rigid_zone.face_j][side];
            let panel = [
                elem.rigid_zone.panel_offset_i,
                elem.rigid_zone.panel_offset_j,
            ][side];
            return (prepared == Some(0.0)
                && model
                    .node(node)
                    .is_some_and(|node| node.coord == points[side])
                && panel == 0.0)
                .then_some(if side == 0 { 0.0 } else { length_mm })
                .ok_or(Issue::FacePosition);
        }
        if other.any(|o| {
            model.element_section(o).is_none_or(|s| {
                !matches!(
                    s.frame_use,
                    Some(FrameSectionUse::Column | FrameSectionUse::Girder)
                )
            })
        }) {
            return Err(Issue::FacePosition);
        }
        let face = faces[side].as_ref().map_err(|_| Issue::FacePosition)?;
        if face.columns.iter().any(|&id| has_haunch(model, id)) {
            return Err(Issue::FacePosition);
        }
        Ok(face.position_mm)
    };
    Ok([resolve(0)?, resolve(1)?])
}
