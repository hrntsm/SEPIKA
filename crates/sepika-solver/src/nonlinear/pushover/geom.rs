//! 幾何ヘルパ。
//!
//! - [`dot3`] — 3 次元ベクトルの内積（`sepika_core::geom::vec3::dot` の別名）
//! - [`axial_compression`] — 材端力から部材の軸方向圧縮力を算定
//! - [`axial_force_signed`] — 材端力から部材の軸方向力を圧縮正の符号付きで算定
//! - [`member_end_forces_at_face`] — 材端力を局所座標・剛域フェイス位置へ変換

use sepika_core::model::{ElementData, ElementKind, Model};
use sepika_element::behavior::{Ctx, ElementBehavior, LoadedFrameResponse, LocalVec};
use sepika_element::transform::LocalFrame;

pub(crate) use sepika_core::geom::vec3::dot as dot3;

pub(crate) fn member_local_frame(
    elem: &ElementData,
    p_i: [f64; 3],
    p_j: [f64; 3],
) -> Result<LocalFrame, &'static str> {
    if sepika_core::geom::vec3::dist(p_i, p_j) < 1e-9 {
        match elem.kind {
            ElementKind::NodalSpring => {
                return Ok(LocalFrame {
                    rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                });
            }
            ElementKind::Isolator => {
                return LocalFrame::from_nodes(
                    p_i,
                    [p_i[0], p_i[1], p_i[2] + 1.0],
                    elem.local_axis.ref_vector,
                );
            }
            _ => {}
        }
    }
    LocalFrame::from_nodes(p_i, p_j, elem.local_axis.ref_vector)
}

/// 部材の軸方向圧縮力 N_compress [N]を算定する。
///
/// 引張は 0 とし、両端のうち大きい方を部材の代表値とする。
pub(crate) fn axial_compression(f_i: [f64; 3], f_j: [f64; 3], ex: [f64; 3]) -> f64 {
    let from_i = dot3(f_i, ex).max(0.0);
    let from_j = (-dot3(f_j, ex)).max(0.0);
    from_i.max(from_j)
}

/// 部材の軸方向力 N [N]を**圧縮正・引張負**の符号付きで算定する。
///
/// 両端のうち絶対値が大きい方を部材の代表値とする。
pub(crate) fn axial_force_signed(f_i: [f64; 3], f_j: [f64; 3], ex: [f64; 3]) -> f64 {
    let from_i = dot3(f_i, ex);
    let from_j = -dot3(f_j, ex);
    if from_i.abs() >= from_j.abs() {
        from_i
    } else {
        from_j
    }
}

/// 部材の材端力を局所座標・剛域フェイス位置の 12 成分へ変換する.
///
/// 2 節点の線材専用。4 節点の耐震壁は `None` を返す。
pub(crate) fn member_end_forces_at_face(
    model: &Model,
    elem: &ElementData,
    f_global: &[f64],
) -> Option<[f64; 12]> {
    if elem.nodes.len() != 2 || f_global.len() < 12 {
        return None;
    }
    let pi = model.nodes.get(elem.nodes[0].index())?;
    let pj = model.nodes.get(elem.nodes[1].index())?;
    let frame = member_local_frame(elem, pi.coord, pj.coord)
        .unwrap_or_else(|reason| panic!("部材 ID {} の局所座標: {reason}", elem.id.0));
    let mut g = [0.0_f64; 12];
    g.copy_from_slice(&f_global[..12]);
    let local = frame.rotate_to_local(&g);
    let d = [
        pj.coord[0] - pi.coord[0],
        pj.coord[1] - pi.coord[1],
        pj.coord[2] - pi.coord[2],
    ];
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let (li, lj) = sepika_element::frame::rigid_arm::resolve_lengths(
        model.analysis_rigid_zone(elem).rigid_length_i(),
        model.analysis_rigid_zone(elem).rigid_length_j(),
        length,
    );
    Some(sepika_element::frame::rigid_arm::to_flex_force(
        &local, li, lj,
    ))
}

/// 増分解析の記録用に同一試行状態の荷重応答を取得する。所有要素の欠落・不正応答はエラー。
pub(crate) fn member_recording_responses(
    model: &Model,
    behaviors: &[Box<dyn ElementBehavior>],
) -> Result<Vec<Option<LoadedFrameResponse>>, String> {
    crate::common::member_recording::member_recording_responses(model, behaviors, "増分解析")
}

pub(crate) fn recording_force(
    behavior: &dyn ElementBehavior,
    response: Option<&LoadedFrameResponse>,
    ctx: &Ctx,
) -> LocalVec {
    match response {
        Some(response) => LocalVec {
            data: response.physical_frame_end_force_global.as_slice().into(),
        },
        None => behavior.internal_force(ctx),
    }
}

pub(crate) fn recording_face_force(
    model: &Model,
    elem: &ElementData,
    force: &[f64],
    response: Option<&LoadedFrameResponse>,
) -> Option<[f64; 12]> {
    match response {
        Some(response) => Some(response.flex_face_force_local),
        None => member_end_forces_at_face(model, elem, force),
    }
}
