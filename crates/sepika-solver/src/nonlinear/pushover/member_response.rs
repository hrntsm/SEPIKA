//! 終局（最終確定ステップ）時の部材別応答の算定。
//!
//! - [`compute_member_response`] — 部材端内力を局所座標へ射影し、強軸・弱軸の
//!   設計用曲げ・せん断と軸圧縮力、部材変形角 Rp を [`PushoverMemberResponse`]
//!   として求める
//! - [`record_member_step`] — ヒンジ詳細図用に 1 確定ステップ分の部材端応答
//!   （軸力・剛域フェイスの局所曲げ・弦からの材端回転）を全部材について記録する

use super::geom::{
    axial_compression, axial_force_signed, dot3, member_local_frame, recording_face_force,
    recording_force,
};
use super::types::{MemberStepState, PushoverMemberResponse};
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::dof::DofMap;
use sepika_core::geom::vec3::{cross, dist, norm, sub, unit};
use sepika_core::ids::NodeId;
use sepika_core::model::{
    Constraint, ElementData, FrameSectionUse, LoadCase, Model, SupportMemberId,
};
use sepika_core::rc_capacity::RcUltimateShearActions;
use sepika_core::section_shape::SectionShape;
use sepika_element::behavior::{Ctx, ElementBehavior, LoadedFrameResponse, LocalVec};
use sepika_element::frame::member_load::LoadedFrameSection;
use std::collections::HashSet;

/// 部材の変形角 R [rad]を節点変位から算定する。
///
/// `disp` は `DofMap` アクティブ添字順の全自由節点変位。
pub(crate) fn member_rp_angle(
    model: &Model,
    dofmap: &DofMap,
    disp: &[f64],
    elem: &ElementData,
) -> f64 {
    if elem.nodes.len() < 2 {
        return 0.0;
    }
    let ni = elem.nodes[0].index();
    let nj = elem.nodes[1].index();
    let (Some(pi), Some(pj)) = (model.nodes.get(ni), model.nodes.get(nj)) else {
        return 0.0;
    };
    let length = model.member_length(elem);
    if length <= 0.0 {
        return 0.0;
    }
    let get = |node_index: usize, dof: usize| -> f64 {
        let g = node_index * 6 + dof;
        dofmap
            .active(g)
            .and_then(|a| disp.get(a as usize).copied())
            .unwrap_or(0.0)
    };
    let vertical = sepika_core::geom::is_vertical_axis(pi.coord, pj.coord);
    if vertical {
        let dux = get(nj, 0) - get(ni, 0);
        let duy = get(nj, 1) - get(ni, 1);
        (dux * dux + duy * duy).sqrt() / length
    } else {
        (get(nj, 2) - get(ni, 2)).abs() / length
    }
}

/// 部材が伝達する加力方向の水平力 [N] を材端力から求める。
pub(crate) fn horizontal_force_in_dir(f: &LocalVec, n_nodes: usize, dir_idx: usize) -> f64 {
    let n = n_nodes.min(f.data.len() / 6);
    if n < 2 {
        return 0.0;
    }
    let half = n / 2;
    let sum = |range: std::ops::Range<usize>| -> f64 {
        range.map(|k| f.data[k * 6 + dir_idx]).sum::<f64>()
    };
    sum(0..half).abs().max(sum(half..n).abs())
}

/// ヒンジ詳細図用: 1 確定ステップ分の部材端応答（[`MemberStepState`]）を全部材に
/// ついて算定する（`model.elements` と同じ並び。2 節点の線材以外はゼロ埋め）。
/// f32 変換後に非有限となる成分は、部材・成分・理由を含むエラーを返す。
pub(crate) fn record_member_step(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    total_disp: &[f64],
    responses: &[Option<LoadedFrameResponse>],
) -> Result<Vec<MemberStepState>, String> {
    let ctx = Ctx { model };
    model
        .elements
        .iter()
        .zip(behaviors)
        .zip(responses)
        .map(|((elem, b), response)| {
            if elem.nodes.len() != 2 {
                return Ok(MemberStepState::default());
            }
            let ni = elem.nodes[0].index();
            let nj = elem.nodes[1].index();
            let (Some(pi), Some(pj)) = (model.nodes.get(ni), model.nodes.get(nj)) else {
                return Ok(MemberStepState::default());
            };
            let dx = [
                pj.coord[0] - pi.coord[0],
                pj.coord[1] - pi.coord[1],
                pj.coord[2] - pi.coord[2],
            ];
            let length = (dx[0] * dx[0] + dx[1] * dx[1] + dx[2] * dx[2]).sqrt();
            if length <= 0.0 {
                return Ok(MemberStepState::default());
            }
            let frame = member_local_frame(elem, pi.coord, pj.coord)
                .unwrap_or_else(|reason| panic!("部材 ID {} の局所座標: {reason}", elem.id.0));
            let ex = frame.rot[0];
            let ey = frame.rot[1];
            let ez = frame.rot[2];

            let f = recording_force(b.as_ref(), response.as_ref(), &ctx);
            let (my_i, mz_i, my_j, mz_j) =
                match recording_face_force(model, elem, &f.data, response.as_ref()) {
                    Some(fl) => (fl[4], fl[5], fl[10], fl[11]),
                    None => {
                        let m_i = [f.data[3], f.data[4], f.data[5]];
                        let m_j = [f.data[9], f.data[10], f.data[11]];
                        (dot3(m_i, ey), dot3(m_i, ez), dot3(m_j, ey), dot3(m_j, ez))
                    }
                };
            let f_i = [f.data[0], f.data[1], f.data[2]];
            let f_j = [f.data[6], f.data[7], f.data[8]];
            let n = axial_force_signed(f_i, f_j, ex);

            let get = |node_index: usize, dof: usize| -> f64 {
                let g = node_index * 6 + dof;
                dofmap
                    .active(g)
                    .and_then(|a| total_disp.get(a as usize).copied())
                    .unwrap_or(0.0)
            };
            let u_i = [get(ni, 0), get(ni, 1), get(ni, 2)];
            let u_j = [get(nj, 0), get(nj, 1), get(nj, 2)];
            let r_i = [get(ni, 3), get(ni, 4), get(ni, 5)];
            let r_j = [get(nj, 3), get(nj, 4), get(nj, 5)];
            let chord_v = (dot3(u_j, ey) - dot3(u_i, ey)) / length;
            let chord_w = (dot3(u_j, ez) - dot3(u_i, ez)) / length;
            let ry_i = dot3(r_i, ey) + chord_w;
            let rz_i = dot3(r_i, ez) - chord_v;
            let ry_j = dot3(r_j, ey) + chord_w;
            let rz_j = dot3(r_j, ez) - chord_v;
            let end_spring = b.end_spring_rotations();
            let record = |value, component| {
                finite_history_value(value).map_err(|reason| {
                    format!("部材 {:?} の {component}={value:e}: {reason}", elem.id)
                })
            };

            Ok(MemberStepState {
                n: record(n, "n")?,
                my_i: record(my_i, "my_i")?,
                mz_i: record(mz_i, "mz_i")?,
                my_j: record(my_j, "my_j")?,
                mz_j: record(mz_j, "mz_j")?,
                ry_i: record(ry_i, "ry_i")?,
                rz_i: record(rz_i, "rz_i")?,
                ry_j: record(ry_j, "ry_j")?,
                rz_j: record(rz_j, "rz_j")?,
                spring_rz_i: record(end_spring.map_or(0.0, |g| g[0]), "spring_rz_i")?,
                spring_rz_j: record(end_spring.map_or(0.0, |g| g[1]), "spring_rz_j")?,
            })
        })
        .collect()
}

fn finite_history_value(value: f64) -> Result<f32, &'static str> {
    let recorded = value as f32;
    if recorded.is_finite() {
        Ok(recorded)
    } else if value.is_finite() {
        Err("f32 変換で有限範囲を超えました")
    } else {
        Err("記録元が非有限値です")
    }
}

/// 同一確定ステップの部材別応答と作用荷重から、部材応答を記録する。
pub(crate) fn compute_member_response(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    total_disp: &[f64],
    dir: crate::statics::analysis::SeismicDir,
    responses: &[Option<LoadedFrameResponse>],
    (case, force_scale): (&LoadCase, f64),
) -> Vec<PushoverMemberResponse> {
    let dir_idx = match dir {
        crate::statics::analysis::SeismicDir::X => 0usize,
        crate::statics::analysis::SeismicDir::Y => 1usize,
    };
    let ctx = Ctx { model };
    let mut out = Vec::with_capacity(model.elements.len());
    let mut shear_face_geometry = None;
    let applied = model
        .elements
        .iter()
        .any(|elem| super::rc_beam_shear::beam_section(model, elem).is_some())
        .then(|| super::rc_beam_shear::BeamShearStep::new(case, force_scale));
    let grouped_rc_actions = super::rc_beam_shear::grouped_actions(
        model,
        responses,
        &mut shear_face_geometry,
        applied.as_ref(),
    );
    for ((elem, b), response) in model.elements.iter().zip(behaviors).zip(responses) {
        if elem.nodes.len() != 2 {
            continue;
        }
        let (Some(pi), Some(pj)) = (
            model.nodes.get(elem.nodes[0].index()),
            model.nodes.get(elem.nodes[1].index()),
        ) else {
            continue;
        };
        let frame = member_local_frame(elem, pi.coord, pj.coord)
            .unwrap_or_else(|reason| panic!("部材 ID {} の局所座標: {reason}", elem.id.0));
        let ex = frame.rot[0];
        let ey = frame.rot[1];
        let ez = frame.rot[2];

        let f = recording_force(b.as_ref(), response.as_ref(), &ctx);
        let f_i = [f.data[0], f.data[1], f.data[2]];
        let f_j = [f.data[6], f.data[7], f.data[8]];

        let face_force = recording_face_force(model, elem, &f.data, response.as_ref());
        let (m_strong, m_weak) = match face_force {
            Some(fl) => (fl[5].abs().max(fl[11].abs()), fl[4].abs().max(fl[10].abs())),
            None => {
                let m_i = [f.data[3], f.data[4], f.data[5]];
                let m_j = [f.data[9], f.data[10], f.data[11]];
                (
                    dot3(m_i, ez).abs().max(dot3(m_j, ez).abs()),
                    dot3(m_i, ey).abs().max(dot3(m_j, ey).abs()),
                )
            }
        };
        let shear_strong = dot3(f_i, ey).abs().max(dot3(f_j, ey).abs());
        let shear_weak = dot3(f_i, ez).abs().max(dot3(f_j, ez).abs());
        let axial = axial_compression(f_i, f_j, ex);
        let rc_beam_ultimate_shear = super::rc_beam_shear::beam_section(model, elem).map(|_| {
            grouped_rc_actions
                .get(&elem.id)
                .copied()
                .unwrap_or_else(|| {
                    super::rc_beam_shear::beam_actions(
                        model,
                        std::slice::from_ref(&elem.id),
                        responses,
                        &mut shear_face_geometry,
                        applied.as_ref(),
                    )
                })
        });
        let rc_ultimate_shear = if rc_beam_ultimate_shear.is_none() {
            response.as_ref().and_then(|response| {
                rc_column_ultimate_shear_actions(
                    model,
                    elem,
                    [pi.coord, pj.coord],
                    &response.section,
                    &mut shear_face_geometry,
                )
            })
        } else {
            None
        };
        let rp = member_rp_angle(model, dofmap, total_disp, elem);
        let horizontal_force = horizontal_force_in_dir(&f, elem.nodes.len(), dir_idx);

        out.push(PushoverMemberResponse {
            elem: elem.id,
            m_strong,
            m_weak,
            shear_strong,
            shear_weak,
            axial,
            rp,
            horizontal_force,
            wall_shear_signed: None,
            rc_ultimate_shear,
            rc_beam_ultimate_shear,
        });
    }
    out
}

fn rc_column_ultimate_shear_actions(
    model: &Model,
    elem: &ElementData,
    node_ends: [[f64; 3]; 2],
    response: &LoadedFrameSection,
    geometry: &mut Option<RcShearFaceGeometry>,
) -> Option<RcUltimateShearActions> {
    let section = model.element_section(elem)?;
    if !response.is_unloaded()
        || !elem.kind.is_primary_frame_support()
        || section.frame_use != Some(FrameSectionUse::Column)
        || !matches!(section.shape, Some(SectionShape::RcColumnRect { .. }))
    {
        return None;
    }
    let geometry = geometry.get_or_insert_with(|| RcShearFaceGeometry::new(model));
    let ends = model
        .primary_frame_axis_ends(elem)
        .ok()?
        .unwrap_or(node_ends);
    let length_mm = dist(ends[0], ends[1]);
    let position = |end: usize| {
        let node = elem.nodes[end];
        if geometry.other_connections.contains(&node) {
            return None;
        }
        let others = geometry
            .frame
            .elements_at(model, node)
            .filter(|other| other.id != elem.id);
        let mut others = others.peekable();
        if others.peek().is_none() {
            let prepared = [elem.rigid_zone.face_i, elem.rigid_zone.face_j][end];
            let panel = [
                elem.rigid_zone.panel_offset_i,
                elem.rigid_zone.panel_offset_j,
            ][end];
            return (prepared == Some(0.0) && ends[end] == node_ends[end] && panel == 0.0)
                .then_some(if end == 0 { 0.0 } else { length_mm });
        }
        let inward = unit(sub(ends[1 - end], ends[end]))?;
        let distance = column_beam_face_distance(model, others, node, ends[end], inward)?;
        Some(if end == 0 {
            distance
        } else {
            length_mm - distance
        })
    };
    rc_ultimate_section_actions(response, length_mm, [position(0)?, position(1)?])
}

pub(super) struct RcShearFaceGeometry {
    pub(super) frame: NodeAdjacency,
    pub(super) other_connections: HashSet<NodeId>,
    pub(super) diaphragm_nodes: HashSet<NodeId>,
}

impl RcShearFaceGeometry {
    pub(super) fn new(model: &Model) -> Self {
        let mut other_connections: HashSet<_> = model
            .elements
            .iter()
            .filter(|elem| !elem.kind.is_primary_frame_support())
            .flat_map(|elem| elem.nodes.iter().copied())
            .chain(
                model
                    .wall_plates
                    .iter()
                    .filter_map(|plate| plate.boundary_nodes(model))
                    .flatten(),
            )
            .collect();
        let mut diaphragm_nodes = HashSet::new();
        for constraint in &model.constraints {
            match constraint {
                Constraint::RigidLink { master, slaves, .. } => {
                    other_connections.insert(*master);
                    other_connections.extend(slaves);
                }
                Constraint::Mpc { master, terms } => {
                    other_connections.insert(*master);
                    other_connections.extend(terms.iter().map(|&(node, _, _)| node));
                }
                Constraint::RigidDiaphragm { master, slaves, .. } => {
                    diaphragm_nodes.insert(*master);
                    diaphragm_nodes.extend(slaves);
                }
            }
        }
        Self {
            frame: NodeAdjacency::primary_frame(model),
            other_connections,
            diaphragm_nodes,
        }
    }
}

pub(super) fn has_haunch(model: &Model, id: sepika_core::ids::ElemId) -> bool {
    model.member_detail(id).is_some_and(|detail| {
        [detail.haunch_i, detail.haunch_j]
            .into_iter()
            .flatten()
            .any(|haunch| haunch.depth_increase != 0.0 || haunch.width_increase != 0.0)
    })
}

fn column_beam_face_distance<'a>(
    model: &Model,
    others: impl Iterator<Item = &'a ElementData>,
    node: NodeId,
    origin: [f64; 3],
    inward: [f64; 3],
) -> Option<f64> {
    let parallel = |axis| norm(cross(axis, inward)) <= 32.0 * f64::EPSILON;
    let mut distance = None;
    let mut continuation = false;
    for other in others {
        let section = model.element_section(other)?;
        if has_haunch(model, other.id) {
            return None;
        }
        match section.frame_use? {
            FrameSectionUse::Column => {
                let side = other.nodes.iter().position(|&id| id == node)?;
                let ends = model
                    .calculation_beam_placement(SupportMemberId::Primary(other.id))
                    .ok()?
                    .actual_ends;
                let axis = unit(sub(ends[1 - side], ends[side]))?;
                if continuation
                    || ends[side] != origin
                    || !parallel(axis)
                    || dot3(axis, inward) >= 0.0
                {
                    return None;
                }
                continuation = true;
            }
            FrameSectionUse::Girder => {
                if !matches!(
                    section.shape.as_ref()?,
                    SectionShape::RcBeamRect { .. } | SectionShape::SrcBeamRect { .. }
                ) {
                    return None;
                }
                let placement = model
                    .calculation_beam_placement(SupportMemberId::Primary(other.id))
                    .ok()?;
                if !placement.axes[1..].iter().copied().any(parallel) {
                    return None;
                }
                let face = placement
                    .connection_face_at_contact_mm(origin, inward)
                    .ok()??;
                if !face.is_finite() || distance.is_some_and(|previous| previous != face) {
                    return None;
                }
                distance = Some(face);
            }
            _ => return None,
        }
    }
    Some(distance.unwrap_or(0.0))
}

fn rc_ultimate_section_actions(
    response: &LoadedFrameSection,
    length_mm: f64,
    [i_mm, j_mm]: [f64; 2],
) -> Option<RcUltimateShearActions> {
    if !i_mm.is_finite() || !j_mm.is_finite() || i_mm >= j_mm {
        return None;
    }
    // 全体系を経由して端力を再投影すると、単軸応答の直交軸に丸め誤差が生じる。
    let i = response.at_continuous_section_mm(i_mm, length_mm).ok()?;
    let j = response.at_continuous_section_mm(j_mm, length_mm).ok()?;
    let max_abs = |k: usize| i[k].abs().max(j[k].abs());
    let (n_i, n_j) = (-i[0], -j[0]);
    Some(RcUltimateShearActions {
        axial_range_n: [n_i.min(n_j), n_i.max(n_j)],
        max_mq: [(max_abs(5), max_abs(1)), (max_abs(4), max_abs(2))],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, SectionId};
    use sepika_core::model::{ElementKind, RigidZone};
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RebarSize};
    use sepika_element::behavior::{LocalMat, MassOption};
    use sepika_element::frame::beam::BeamElement;

    #[test]
    fn inclined_uniaxial_frame_recovers_design_faces_without_a_weak_axis_action() {
        let zone = RigidZone {
            length_i: 500.0,
            length_j: 400.0,
            ..Default::default()
        };
        let (mut model, ..) = super::super::tests::rc_column_model_with_rigid_zone(zone);
        model.nodes[1].coord = [1_800.0, 2_400.0, 4_000.0];
        let elem = &mut model.elements[0];
        elem.kind = ElementKind::Beam;
        elem.local_axis.ref_vector = [0.0, 0.0, 1.0];
        let elem = &model.elements[0];
        let beam = BeamElement::new(elem, &model);
        let mut disp = [0.0; 12];
        disp[8] = -1.0;
        let response = beam.frame_load_response(&disp, &[]).unwrap().unwrap();
        let bounds =
            rc_ultimate_section_actions(&response.section, 5_000.0, [200.0, 4_750.0]).unwrap();
        assert_eq!(bounds.max_mq[1], (0.0, 0.0));
        let (m, q) = bounds.max_mq[0];
        assert!(q > 0.0);
        // 可撓長4100 mmの両端回転拘束で、iフェースは可撓端より外側300 mm。
        assert!((m / q - 2_350.0).abs() < 1e-9, "{bounds:?}");
        let [min, max] = bounds.axial_range_n;
        // 軸方向は全長5000 mm、強制変位の圧縮成分0.8 mm、EA=23000×240000 N。
        assert!((min - 883_200.0).abs() < 1e-7, "{bounds:?}");
        assert_eq!(max, min);

        let bounds =
            rc_ultimate_section_actions(&response.section, 5_000.0, [300.0, 4_950.0]).unwrap();
        let (m, q) = bounds.max_mq[0];
        // jフェースが可撓端より外側350 mmに移ると、j端側が支配する。
        assert!((m / q - 2_400.0).abs() < 1e-9, "{bounds:?}");

        assert!(
            rc_ultimate_section_actions(&response.section, 4_999.0, [300.0, 4_900.0]).is_none()
        );
    }

    #[test]
    fn ultimate_span_uses_rectangular_column_width_and_eccentric_girder_depth() {
        let (mut model, ..) = super::super::tests::rc_column_model_with_rigid_zone(RigidZone {
            length_j: 500.0,
            face_i: Some(0.0),
            face_j: Some(300.0),
            ..Default::default()
        });
        model.sections[0].frame_use = Some(FrameSectionUse::Column);
        model.elements[0].kind = ElementKind::Beam;
        let mut tip = model.nodes[1].clone();
        tip.id = NodeId(2);
        tip.coord = [0.0, 3_600.0, 3_000.0];
        model.nodes.push(tip);

        let shape = SectionShape::RcBeamRect {
            b: 400.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia: RebarSize::DiameterMm(25.0),
                top: vec![4],
                bottom: vec![4],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs: 2,
                },
            },
        };
        let mut section = shape.to_section(SectionId(1), "RC梁".into());
        section.frame_use = Some(FrameSectionUse::Girder);
        section.material = model.sections[0].material;
        section.rebar_material = model.sections[0].rebar_material;
        section.shear_rebar_material = model.sections[0].shear_rebar_material;
        model.sections.push(section);

        let mut beam = model.elements[0].clone();
        beam.id = ElemId(1);
        beam.section = Some(SectionId(1));
        beam.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
        beam.local_axis.ref_vector = [0.0, 0.0, 1.0];
        beam.end_offsets_mm = [[0.0, 0.0, -50.0]; 2];
        beam.rigid_zone = RigidZone {
            length_i: 500.0,
            face_i: Some(300.0),
            face_j: Some(0.0),
            ..Default::default()
        };
        model.elements.push(beam);

        let moment_shear_ratio = |model: &Model, index, global_dof| {
            let elem: &ElementData = &model.elements[index];
            let ends = [
                model.node(elem.nodes[0]).unwrap().coord,
                model.node(elem.nodes[1]).unwrap().coord,
            ];
            let beam = BeamElement::new(elem, model);
            let mut disp = [0.0; 12];
            disp[global_dof] = 1.0;
            let response = beam.frame_load_response(&disp, &[]).unwrap().unwrap();
            let actions = if index == 0 {
                rc_column_ultimate_shear_actions(model, elem, ends, &response.section, &mut None)
            } else {
                super::super::rc_beam_shear::beam_actions(
                    model,
                    &[elem.id],
                    &[None, Some(response)],
                    &mut None,
                    None,
                )
                .ok()
            };
            actions.map(|actions| actions.max_mq[0].0 / actions.max_mq[0].1)
        };

        // 柱はz=2650 mmの実梁下面（梁芯z=2950 mm）、梁は柱の幅400 mm側の面を使う。
        let column = moment_shear_ratio(&model, 0, 6).unwrap();
        assert!((column - 1_400.0).abs() < 1e-9, "{column}");
        // 偏心梁の取付け側は柱の有限区間内。単独の先端に偏心剛体アームがある場合は未確定。
        assert!(moment_shear_ratio(&model, 1, 8).is_none());
        model.nodes[2].coord[2] -= 50.0;
        model.elements[1].end_offsets_mm[1][2] = 0.0;
        let girder = moment_shear_ratio(&model, 1, 8).unwrap();
        assert!((girder - 1_850.0).abs() < 1e-9, "{girder}");
        model.elements[1].rigid_zone.face_j = Some(25.0);
        assert!(moment_shear_ratio(&model, 1, 8).is_none());

        use sepika_core::rc_capacity::RcBeamUltimateShearIssue as Issue;
        let mut split = model.nodes[2].clone();
        split.id = NodeId(3);
        split.coord = [0.0, 100.0, 3_000.0];
        split.restraint = Dof6Mask::FREE;
        model.nodes.push(split);
        model.nodes[2].coord[2] = 3_000.0;
        let first = &mut model.elements[1];
        first.end_offsets_mm = [[0.0; 3]; 2];
        first.nodes[1] = NodeId(3);
        first.rigid_zone = RigidZone {
            face_i: Some(200.0),
            face_j: Some(0.0),
            ..Default::default()
        };
        let mut last = first.clone();
        last.id = ElemId(2);
        last.nodes = [NodeId(3), NodeId(2)].into_iter().collect();
        last.rigid_zone.face_i = Some(0.0);
        model.elements.push(last);
        let group = [ElemId(1), ElemId(2)];
        model.girder_groups.push(group.to_vec());
        let response_at = |index, dof| {
            let mut disp = [0.0; 12];
            disp[dof] = 1.0;
            BeamElement::new(&model.elements[index], &model)
                .frame_load_response(&disp, &[])
                .unwrap()
        };
        let mut responses = [None, response_at(1, 2), response_at(2, 8)];
        let reference_n = responses[2].as_ref().unwrap().section.at(0.0).unwrap()[1].abs();
        assert!(responses[1].as_ref().unwrap().section.at(0.0).unwrap()[1].abs() > reference_n);
        let beam_actions = super::super::rc_beam_shear::beam_actions;
        let result = beam_actions(&model, &group, &responses, &mut None, None).unwrap();
        let (m, q) = result.max_mq[0];
        // 実柱面は始端から200 mm。100 mmの第1区間は内法に含まない。
        assert_eq!(q, reference_n);
        assert!((m / q - 1_750.0).abs() < 1e-9, "{result:?}");
        let outside_load = sepika_core::model::MemberLoad::manual(
            ElemId(1),
            [0.0, 1e-16, -1.0],
            sepika_core::model::MemberLoadKind::Point { a: 50.0, p: 50.0 },
        );
        responses[1] = BeamElement::new(&model.elements[1], &model)
            .frame_load_response(&[0.0; 12], &[outside_load])
            .unwrap();
        assert_eq!(
            beam_actions(&model, &group, &responses, &mut None, None),
            Err(Issue::Recovery)
        );
        responses[1] = None;
        assert_eq!(
            beam_actions(&model, &group, &responses, &mut None, None),
            Err(Issue::MissingResponse)
        );
    }

    fn split_rc_beam_for_axial_cuts(coords: [[f64; 3]; 3]) -> Model {
        use sepika_core::dof::Dof;
        use sepika_core::model::ForceRegime;
        let (mut model, _, b, d) =
            super::super::tests::rc_column_model_with_rigid_zone(RigidZone {
                face_i: Some(0.0),
                face_j: Some(0.0),
                ..Default::default()
            });
        let shape = SectionShape::RcBeamRect {
            b,
            d,
            rebar: RcBeamRebar {
                main_dia: RebarSize::DiameterMm(25.0),
                top: vec![4],
                bottom: vec![4],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs: 2,
                },
            },
        };
        let template = &model.sections[0];
        let mut section = shape.to_section(template.id, "RC梁".into());
        section.frame_use = Some(FrameSectionUse::Girder);
        section.material = template.material;
        section.rebar_material = template.rebar_material;
        section.shear_rebar_material = template.shear_rebar_material;
        model.sections[0] = section;
        model.nodes.push(model.nodes[1].clone());
        for (index, (node, coord)) in model.nodes.iter_mut().zip(coords).enumerate() {
            node.id = NodeId(index as u32);
            node.coord = coord;
            node.restraint = Dof6Mask::FREE;
            if index != 1 {
                node.restraint.set_fixed(Dof::Ux);
            }
        }
        let first = &mut model.elements[0];
        first.kind = ElementKind::Beam;
        first.force_regime = ForceRegime::AxialBendingInteract;
        first.local_axis.ref_vector = [0.0, 0.0, 1.0];
        let mut last = first.clone();
        last.id = ElemId(1);
        last.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
        model.elements.push(last);
        model
    }

    fn check_axial_cut_with_zero_recovery(
        model: &Model,
        case: &LoadCase,
    ) -> sepika_core::rc_capacity::RcBeamUltimateShearResponse {
        use super::super::rc_beam_shear::{beam_actions, BeamShearStep};
        let responses: Vec<_> = model
            .elements
            .iter()
            .map(|elem| {
                let member: Vec<_> = case
                    .member
                    .iter()
                    .filter(|load| load.elem == elem.id)
                    .cloned()
                    .collect();
                BeamElement::new(elem, model)
                    .frame_load_response(&[0.0; 12], &member)
                    .unwrap()
            })
            .collect();
        let group = [ElemId(0), ElemId(1)];
        let actions = beam_actions(model, &group, &responses, &mut None, None).unwrap();
        assert_eq!(actions.axial_range_n, [0.0; 2]);
        let applied = BeamShearStep::new(case, 1.0);
        beam_actions(model, &group, &responses, &mut None, Some(&applied))
    }

    #[test]
    fn lost_axial_action_is_rejected_at_member_interiors_and_free_joints() {
        use sepika_core::model::{MemberLoad, MemberLoadKind, NodalLoad};
        use sepika_core::rc_capacity::RcBeamUltimateShearIssue as Issue;

        let model =
            split_rc_beam_for_axial_cuts([[0.0; 3], [2_000.0, 0.0, 0.0], [4_000.0, 0.0, 0.0]]);
        let mut case = super::super::loads::PhysicalPushoverLoads::empty().reference;
        // 3つの入力の合力は1 N。通常加算後のゼロから、釣合いは確認できない。
        let loads_n = [1e16, 1.0, -1e16];
        case.nodal = loads_n
            .map(|n| NodalLoad::manual(NodeId(1), [n, 0.0, 0.0, 0.0, 0.0, 0.0]))
            .into();
        assert_eq!(
            check_axial_cut_with_zero_recovery(&model, &case),
            Err(Issue::Equilibrium)
        );
        case.nodal.clear();
        for (elem, a) in [(ElemId(0), 2_000.0), (ElemId(1), 0.0), (ElemId(1), 1_000.0)] {
            case.member = loads_n
                .map(|p| MemberLoad::manual(elem, [1.0, 0.0, 0.0], MemberLoadKind::Point { a, p }))
                .into();
            assert_eq!(
                check_axial_cut_with_zero_recovery(&model, &case),
                Err(Issue::Equilibrium),
                "{elem:?} at {a} mm"
            );
        }
    }

    #[test]
    fn member_point_loads_at_two_axial_restraints_do_not_act_between_them() {
        use sepika_core::dof::Dof;
        use sepika_core::model::{MemberLoad, MemberLoadKind};
        let mut model =
            split_rc_beam_for_axial_cuts([[0.0; 3], [2_000.0, 0.0, 0.0], [4_000.0, 0.0, 0.0]]);
        model.nodes[1].restraint.set_fixed(Dof::Ux);
        let mut case = super::super::loads::PhysicalPushoverLoads::empty().reference;
        case.member = [0.0, 2_000.0]
            .map(|a| {
                MemberLoad::manual(
                    ElemId(1),
                    [1.0, 0.0, 0.0],
                    MemberLoadKind::Point { a, p: 1_000.0 },
                )
            })
            .into();
        let result = check_axial_cut_with_zero_recovery(&model, &case).unwrap();
        assert_eq!(result.axial_range_n, [0.0; 2]);
    }

    #[test]
    fn collinear_beam_with_unequal_part_lengths_still_checks_axial_equilibrium() {
        use sepika_core::model::NodalLoad;
        use sepika_core::rc_capacity::RcBeamUltimateShearIssue as Issue;

        let model = split_rc_beam_for_axial_cuts([
            [0.0; 3],
            [1_003.0, 2_006.0, 0.0],
            [4_000.0, 8_000.0, 0.0],
        ]);
        let mut case = super::super::loads::PhysicalPushoverLoads::empty().reference;
        // 2区間は同一直線上でも、正規化した局所軸は浮動小数点上で一致しない。
        assert!(check_axial_cut_with_zero_recovery(&model, &case).is_ok());
        case.nodal = [1e16, 1.0, -1e16]
            .map(|n| NodalLoad::manual(NodeId(1), [n, 0.0, 0.0, 0.0, 0.0, 0.0]))
            .into();
        assert_eq!(
            check_axial_cut_with_zero_recovery(&model, &case),
            Err(Issue::Equilibrium)
        );
    }

    #[test]
    fn axial_components_at_a_shared_node_include_the_adjacent_parts_axis() {
        use sepika_core::model::{MemberLoad, MemberLoadKind, NodalLoad};
        use sepika_core::rc_capacity::RcBeamUltimateShearIssue as Issue;

        let model = split_rc_beam_for_axial_cuts([
            [0.0; 3],
            [1_024.0, 0.0, 0.0],
            [2_048.0, 0.0, 2.0_f64.powi(-38)],
        ]);
        let mut case = super::super::loads::PhysicalPushoverLoads::empty().reference;
        case.nodal = [1e32, 1.0, -1e32]
            .map(|p| NodalLoad::manual(NodeId(1), [0.0, 0.0, p, 0.0, 0.0, 0.0]))
            .into();
        assert_eq!(
            check_axial_cut_with_zero_recovery(&model, &case),
            Err(Issue::Equilibrium)
        );

        // 第1区間では直交荷重でも、節点を共有する第2区間の軸には成分がある。
        case.nodal.clear();
        case.member = [1e32, 1.0, -1e32]
            .map(|p| {
                MemberLoad::manual(
                    ElemId(0),
                    [0.0, 0.0, 1.0],
                    MemberLoadKind::Point { a: 1_024.0, p },
                )
            })
            .into();
        assert_eq!(
            check_axial_cut_with_zero_recovery(&model, &case),
            Err(Issue::Equilibrium)
        );
    }

    #[test]
    fn history_conversion_preserves_finite_rounding_at_both_f32_limits() {
        let max = f64::from(f32::MAX);
        let overflow = max + 2.0_f64.powi(103);
        for sign in [1.0, -1.0] {
            for (value, expected) in [
                (0.0, 0.0),
                (1.0 + 2.0_f64.powi(-24), 1.0),
                (f64::from(f32::from_bits(1)), f32::from_bits(1)),
                (2.0_f64.powi(-151), 0.0),
                (max, f32::MAX),
                (max.next_up(), f32::MAX),
                (overflow.next_down(), f32::MAX),
            ] {
                assert_eq!(
                    finite_history_value(sign * value).unwrap().to_bits(),
                    (sign as f32 * expected).to_bits(),
                    "{sign} * {value:e}"
                );
            }
            for value in [overflow, overflow.next_up(), f64::MAX] {
                assert_eq!(
                    finite_history_value(sign * value),
                    Err("f32 変換で有限範囲を超えました")
                );
            }
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(finite_history_value(value), Err("記録元が非有限値です"));
        }
    }

    struct Response {
        forces: [f64; 12],
        spring: [f64; 2],
    }

    impl ElementBehavior for Response {
        fn n_dof(&self) -> usize {
            12
        }
        fn global_dofs(&self, map: &DofMap) -> smallvec::SmallVec<[usize; 24]> {
            sepika_element::behavior::node_global_dofs(
                &[sepika_core::ids::NodeId(0), sepika_core::ids::NodeId(1)],
                map,
            )
        }
        fn tangent_stiffness(&self, _: &Ctx) -> LocalMat {
            LocalMat::zeros(12)
        }
        fn internal_force(&self, _: &Ctx) -> LocalVec {
            LocalVec {
                data: self.forces.as_slice().into(),
            }
        }
        fn mass_matrix(&self, _: MassOption) -> LocalMat {
            LocalMat::zeros(12)
        }
        fn end_spring_rotations(&self) -> Option<[f64; 2]> {
            Some(self.spring)
        }
    }

    fn record(values: [f64; 11]) -> Result<MemberStepState, String> {
        let mut model = super::super::tests::single_column_model(235.0, 80_000.0);
        model.nodes[1].coord = [3000.0, 0.0, 0.0];
        for node in &mut model.nodes {
            node.restraint = Dof6Mask::FREE;
        }
        model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
        let map = DofMap::build(&model);
        let mut disp = vec![0.0; map.n_active()];
        for (global, value) in [
            (4, values[5]),
            (5, values[6]),
            (10, values[7]),
            (11, values[8]),
        ] {
            disp[map.active(global).unwrap() as usize] = value;
        }
        let behaviors: Vec<Box<dyn ElementBehavior>> = vec![Box::new(Response {
            forces: [
                values[0], 0.0, 0.0, 0.0, values[1], values[2], -values[0], 0.0, 0.0, 0.0,
                values[3], values[4],
            ],
            spring: [values[9], values[10]],
        })];
        record_member_step(&model, &map, &behaviors, &disp, &[None]).map(|r| r[0])
    }

    #[test]
    fn member_record_retains_all_signed_force_and_rotation_components() {
        let r = record([
            1.25, -2.5, 3.75, -4.5, 5.25, -0.125, 0.25, -0.375, 0.5, -0.625, 0.75,
        ])
        .unwrap();
        assert_eq!(
            [
                r.n,
                r.my_i,
                r.mz_i,
                r.my_j,
                r.mz_j,
                r.ry_i,
                r.rz_i,
                r.ry_j,
                r.rz_j,
                r.spring_rz_i,
                r.spring_rz_j
            ],
            [1.25, -2.5, 3.75, -4.5, 5.25, -0.125, 0.25, -0.375, 0.5, -0.625, 0.75]
        );
    }

    #[test]
    fn member_record_rejects_each_overflowing_component_with_element_and_reason() {
        for (index, name) in [
            "n",
            "my_i",
            "mz_i",
            "my_j",
            "mz_j",
            "ry_i",
            "rz_i",
            "ry_j",
            "rz_j",
            "spring_rz_i",
            "spring_rz_j",
        ]
        .into_iter()
        .enumerate()
        {
            for value in [1e40, -1e40] {
                let mut values = [0.0; 11];
                values[index] = value;
                let error = record(values).unwrap_err();
                assert!(error.contains("ElemId(0)"), "{error}");
                assert!(error.contains(&format!("の {name}=")), "{error}");
                assert!(error.contains("f32 変換で有限範囲を超えました"), "{error}");
            }
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut values = [0.0; 11];
            values[10] = value;
            let error = record(values).unwrap_err();
            assert!(error.contains("ElemId(0) の spring_rz_j="), "{error}");
            assert!(error.contains("記録元が非有限値です"), "{error}");
        }
    }
}
