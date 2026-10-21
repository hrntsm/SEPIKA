//! 終局（最終確定ステップ）時の部材別応答の算定。
//!
//! - [`compute_member_response`] — 部材端内力を局所座標へ射影し、強軸・弱軸の
//!   設計用曲げ・せん断と軸圧縮力、部材変形角 Rp を [`PushoverMemberResponse`]
//!   として求める
//! - [`record_member_step`] — ヒンジ詳細図用に 1 確定ステップ分の部材端応答
//!   （軸力・剛域フェイスの局所曲げ・弦からの材端回転）を全部材について記録する

use super::geom::{
    axial_compression, axial_force_signed, dot3, member_recording_responses, recording_face_force,
    recording_force,
};
use super::types::{MemberStepState, PushoverMemberResponse};
use sepika_core::dof::DofMap;
use sepika_core::model::{ElementData, Model};
use sepika_element::behavior::{Ctx, ElementBehavior, LoadedFrameResponse, LocalVec};
use sepika_element::transform::LocalFrame;

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
            let frame = LocalFrame::from_nodes(pi.coord, pj.coord, elem.local_axis.ref_vector);
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

/// 最終確定ステップの部材別応答（[`PushoverMemberResponse`]）を算定する。
pub(crate) fn compute_member_response(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    total_disp: &[f64],
    dir: crate::statics::analysis::SeismicDir,
) -> Result<Vec<PushoverMemberResponse>, String> {
    let responses = member_recording_responses(model, behaviors)?;
    Ok(compute_member_response_with_responses(
        model, dofmap, behaviors, total_disp, dir, &responses,
    ))
}

pub(crate) fn compute_member_response_with_responses(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    total_disp: &[f64],
    dir: crate::statics::analysis::SeismicDir,
    responses: &[Option<LoadedFrameResponse>],
) -> Vec<PushoverMemberResponse> {
    let dir_idx = match dir {
        crate::statics::analysis::SeismicDir::X => 0usize,
        crate::statics::analysis::SeismicDir::Y => 1usize,
    };
    let ctx = Ctx { model };
    let mut out = Vec::with_capacity(model.elements.len());
    for ((elem, b), response) in model.elements.iter().zip(behaviors).zip(responses) {
        if elem.nodes.len() < 2 {
            continue;
        }
        if elem.nodes.len() != 2 {
            continue;
        }
        let (Some(pi), Some(pj)) = (
            model.nodes.get(elem.nodes[0].index()),
            model.nodes.get(elem.nodes[1].index()),
        ) else {
            continue;
        };
        let frame = LocalFrame::from_nodes(pi.coord, pj.coord, elem.local_axis.ref_vector);
        let ex = frame.rot[0];
        let ey = frame.rot[1];
        let ez = frame.rot[2];

        let f = recording_force(b.as_ref(), response.as_ref(), &ctx);
        let f_i = [f.data[0], f.data[1], f.data[2]];
        let f_j = [f.data[6], f.data[7], f.data[8]];

        let (m_strong, m_weak) = match recording_face_force(model, elem, &f.data, response.as_ref())
        {
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
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_element::behavior::{LocalMat, MassOption};

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
