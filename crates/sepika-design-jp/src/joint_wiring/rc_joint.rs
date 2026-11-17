//! RC 柱梁接合部（許容応力度・終局）のせん断検定配線。

use super::common::MemberInfo;
use crate::rc::joint::{rc_joint_shear_check, JointShape, RcJointInput};
use crate::CheckOutcome;
use sepika_core::ids::NodeId;
use sepika_core::section_shape::{RcBeamRebar, SectionShape};

struct RcBeam<'a> {
    member: &'a MemberInfo<'a>,
    width: f64,
    depth: f64,
    rebar: &'a RcBeamRebar,
}

/// RC 柱梁接合部の許容応力度検定を `out` へ追加する。
pub(super) fn check_rc_joint(
    model: &sepika_core::model::Model,
    cols: &[&MemberInfo<'_>],
    beams: &[&MemberInfo<'_>],
    nid: NodeId,
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
    let rc_col = cols.iter().find(|c| {
        matches!(
            c.sec.shape,
            Some(SectionShape::RcColumnRect { .. }) | Some(SectionShape::RcColumnCircle { .. })
        ) && c.mat.fc.unwrap_or(0.0) > 0.0
    });
    let rc_beams: Vec<RcBeam<'_>> = beams
        .iter()
        .filter_map(|b| match &b.sec.shape {
            Some(SectionShape::RcBeamRect { b: width, d, rebar }) => Some(RcBeam {
                member: b,
                width: *width,
                depth: *d,
                rebar,
            }),
            _ => None,
        })
        .collect();
    if let (Some(col), false) = (rc_col, rc_beams.is_empty()) {
        if let Some(reason) = joint_rebar_issue(model, col, &rc_beams) {
            out.push((
                nid,
                "接合部(RC)".to_string(),
                CheckOutcome::Skipped { reason },
            ));
            return;
        }
        let shape = match (cols.len() >= 2, rc_beams.len() >= 2) {
            (true, true) => JointShape::Cross,
            (false, true) => JointShape::Tee,
            (true, false) => JointShape::Knee,
            (false, false) => JointShape::Corner,
        };
        let beam0 = &rc_beams[0];
        let dt = if beam_tension_is_top(beam0, nid) {
            beam0.rebar.top_centroid_from_edge()
        } else {
            beam0.rebar.bottom_centroid_from_edge()
        };
        let beam_j = 7.0 / 8.0 * (beam0.depth - dt);
        let sum_beam_moments: f64 = rc_beams
            .iter()
            .map(|b| {
                let tension_is_top = beam_tension_is_top(b, nid);
                let at = if tension_is_top {
                    b.rebar.top_area()
                } else {
                    b.rebar.bottom_area()
                };
                let dt = if tension_is_top {
                    b.rebar.top_centroid_from_edge()
                } else {
                    b.rebar.bottom_centroid_from_edge()
                };
                let mu_inp = sepika_core::rc_capacity::RcCapacityInput {
                    b: b.width,
                    d: b.depth,
                    at,
                    d_eff: b.depth - dt,
                    sigma_y: crate::material_strength::rebar_sigma_y_of(b.member.rebar_mat),
                    fc: b.member.mat.fc.unwrap_or(0.0),
                    pw: 0.0,
                    sigma_wy: 0.0,
                    clear_span: 0.0,
                    sigma_0: 0.0,
                };
                sepika_core::rc_capacity::rc_mu_simple(&mu_inp)
            })
            .sum();
        let col_shear = cols
            .iter()
            .filter_map(|c| c.end_forces(nid))
            .map(|f| f[1].abs().max(f[2].abs()))
            .fold(0.0, f64::max);
        let col_height = cols.iter().map(|c| c.length).sum::<f64>() / cols.len() as f64;
        let beam_span =
            rc_beams.iter().map(|b| b.member.length).sum::<f64>() / rc_beams.len() as f64;
        let inp = RcJointInput {
            shape,
            fc: col.mat.fc.unwrap_or(0.0),
            concrete_class: col.mat.concrete_class,
            col_depth: col.sec.depth,
            col_width: col.sec.width,
            beam_width: beam0.member.sec.width,
            beam_j,
            sum_beam_moments,
            col_shear,
            col_height,
            beam_span,
        };
        out.push((
            nid,
            "接合部(RC)".to_string(),
            CheckOutcome::Checked(rc_joint_shear_check(&inp)),
        ));
    }
}

/// 接合部に用いる柱・梁の実配筋幾何を検証する。不整合があれば理由を返す。
fn joint_rebar_issue(
    model: &sepika_core::model::Model,
    col: &MemberInfo<'_>,
    beams: &[RcBeam<'_>],
) -> Option<String> {
    for section in std::iter::once(col.sec.id).chain(beams.iter().map(|beam| beam.member.sec.id)) {
        if let Err(reason) = model.ensure_stb_rebar_supported(section) {
            return Some(reason);
        }
    }
    match col.sec.shape {
        Some(SectionShape::RcColumnRect { b, d, ref rebar }) => {
            if let Err(err) = rebar.validate(b, d) {
                return Some(format!("柱の実配筋の幾何が不整合です（{err}）。断面タブで段別本数・かぶり・断面寸法を見直してください。"));
            }
        }
        Some(SectionShape::RcColumnCircle { d, ref rebar }) => {
            if let Err(err) = rebar.validate(d) {
                return Some(format!("柱の実配筋の幾何が不整合です（{err}）。断面タブで段別本数・かぶり・断面寸法を見直してください。"));
            }
        }
        _ => {}
    }
    for b in beams {
        if let Err(err) = b.rebar.validate(b.width, b.depth) {
            return Some(format!("梁の実配筋の幾何が不整合です（{err}）。断面タブで段別本数・かぶり・断面寸法を見直してください。"));
        }
    }
    None
}

/// 新 `RcBeamRect` の曲げ引張側。`mz>0` は下端引張、`mz<0` は上端引張、
/// `mz==0` は引張鉄筋量が小さい側を引張とする。
fn beam_tension_is_top(b: &RcBeam<'_>, nid: NodeId) -> bool {
    let mz = b.member.end_forces(nid).map(|f| f[5]).unwrap_or(0.0);
    if mz < 0.0 {
        return true;
    }
    if mz > 0.0 {
        return false;
    }
    b.rebar.top_area() < b.rebar.bottom_area()
}

/// 応力の有無によらず、モデルのRC柱・RC矩形梁が接続する節点を未算定として残す。
pub(super) fn check_rc_joint_ultimate(
    model: &sepika_core::model::Model,
    nid: NodeId,
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
    let mut has_column = false;
    let mut has_beam = false;
    for element in &model.elements {
        if !element.nodes.iter().take(2).any(|node| *node == nid) {
            continue;
        }
        let Some(section) = model.element_section(element) else {
            continue;
        };
        match (
            &section.shape,
            crate::MemberKind::try_of_element(element, model),
        ) {
            (
                Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. }),
                Ok(crate::MemberKind::Column),
            ) => has_column = true,
            (Some(SectionShape::RcBeamRect { .. }), Ok(crate::MemberKind::Girder)) => {
                has_beam = true
            }
            _ => {}
        }
    }
    if has_column && has_beam {
        out.push((nid, "接合部終局(RC)".to_string(), CheckOutcome::Skipped {
            reason: "RC柱梁接合部の終局自動算定は未対応です。検討目的・設計状態、検討方向別の柱梁・直交梁と形状・有効幅・せい、左右梁引張力と同一面の上下柱せん断力、材料・配筋・定着条件の成立を一組で確認できません。未指定の材料・配筋・応力を0又は適合とは扱いません。".to_string(),
        }));
    }
}
