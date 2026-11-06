//! RC 柱梁接合部（許容応力度・終局）のせん断検定配線。

use super::common::MemberInfo;
use crate::rc::joint::{rc_joint_shear_check, JointShape, RcJointInput};
use crate::{CheckComponent, CheckKind, CheckOutcome, CheckResult};
use sepika_core::ids::NodeId;
use sepika_core::section_shape::{RcBeamRebar, SectionShape};

struct RcBeam<'a> {
    member: &'a MemberInfo<'a>,
    width: f64,
    depth: f64,
    rebar: &'a RcBeamRebar,
}

/// RC 柱梁接合部（許容応力度・終局）の検定を `out` へ追加する。
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
            for label in ["接合部(RC)", "接合部終局(RC)"] {
                out.push((
                    nid,
                    label.to_string(),
                    CheckOutcome::Skipped {
                        reason: reason.clone(),
                    },
                ));
            }
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

        let bi = (col.sec.width - beam0.member.sec.width) / 2.0;
        let bai = (bi / 2.0).min(col.sec.depth / 4.0).max(0.0);
        let bj = beam0.member.sec.width + 2.0 * bai;
        let sigma_y = crate::material_strength::rebar_sigma_y_of(beam0.member.rebar_mat);
        let (t_top, t_bottom) = (
            beam0.rebar.top_area() * sigma_y,
            beam0.rebar.bottom_area() * sigma_y,
        );
        let col_shears: Vec<f64> = cols
            .iter()
            .filter_map(|c| c.end_forces(nid))
            .map(|f| f[1].abs().max(f[2].abs()))
            .collect();
        let qcu = if col_shears.is_empty() {
            0.0
        } else {
            col_shears.iter().sum::<f64>() / col_shears.len() as f64
        };
        let phi = if beams.len() >= 4 { 1.0 } else { 0.85 };
        let u = crate::ultimate::rc_joint_ultimate(&crate::ultimate::RcJointUltimateInput {
            shape,
            phi,
            fc: col.mat.fc.unwrap_or(0.0),
            bj,
            dj: col.sec.depth,
            t_top,
            t_bottom,
            qcu,
            alpha: 1.0,
        });
        let ratio = if u.vju > 0.0 {
            u.qdu / u.vju
        } else {
            f64::INFINITY
        };
        out.push((
                nid,
                "接合部終局(RC)".to_string(),
                CheckOutcome::Checked(CheckResult {
                    basis: "靭性保証型指針 柱梁接合部終局(Vju=κ·φ·Fj·bj·Dj)".to_string(),
                    detail: String::new(),
                    components: vec![CheckComponent {
                        kind: CheckKind::Shear,
                        ratio,
                        detail: format!(
                            "κ={:.2}, φ={:.2}, Fj={:.3} N/mm², bj={:.1} mm, Dj={:.1} mm, \
                             Vju={:.1} N, T={:.1} N, T′={:.1} N, Qcu={:.1} N, Qdu={:.1} N, 余裕率={:.3}",
                            u.kappa, phi, u.fj, bj, col.sec.depth, u.vju, t_top, t_bottom, qcu, u.qdu, u.margin
                        ),
                    }],
                }),
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
