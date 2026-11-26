use sepika_core::geom::vec3::dist;
use sepika_core::ids::ElemId;
use sepika_core::model::{ElementData, Material, MemberLoadKind, Model, Section};
use sepika_element::frame::beam::MemberForces;
use sepika_element::frame::member_load::{
    frame_static_strong_bending, FrameStaticBendingError, FrameStaticStrongBending,
};

use super::steel_beam_span::BeamSpanLoads;
use super::{combine_incomplete_reasons, strength_outcomes};
use crate::steel::{check_symmetric_h_plate_slenderness, column_span_parameters};
use crate::{CheckKind, CheckOutcome, DesignCtx, MemberForcesAt, SteelDesign};
use FrameStaticBendingError::{Distribution, Stations};

pub(super) fn check(
    model: &Model,
    elem: &ElementData,
    forces: &MemberForces,
    ctx: &DesignCtx,
    loads: Result<&BeamSpanLoads<'_>, &&str>,
    section_issue: Option<&str>,
    outcomes: &mut Vec<(ElemId, f64, CheckOutcome)>,
) -> bool {
    if !forces.at.is_empty()
        && forces.at.iter().all(|(_, f)| f[1] == 0.0 && f[5] == 0.0)
        && loads.as_ref().map_or_else(
            |_| {
                !model
                    .load_cases
                    .iter()
                    .flat_map(|case| &case.member)
                    .any(|load| load.elem == elem.id)
            },
            |loads| loads.for_member(elem.id).is_empty(),
        )
    {
        return false;
    }
    let (Some(sec), Some(mat)) = (model.element_section(elem), model.element_material(elem)) else {
        return false;
    };
    let section_issue = section_issue.or_else(|| {
        crate::steel::symmetric_h_web(sec)
            .is_none()
            .then_some("上下対称H形鋼の実断面形状を確認できないため、強軸曲げ・横座屈は未算定です")
    });
    let recovered = recover(model, elem, loads, forces);
    if recovered
        .as_ref()
        .is_ok_and(|(span, _)| !span.has_strong_bending)
        && forces.at.iter().all(|(_, force)| force[5] == 0.0)
    {
        return false;
    }
    let mut span_reason = None;
    let mut span_description = None;
    let mut saved_parameters = None;
    let parameters = match &recovered {
        Ok((span, axes)) => {
            let length_mm = dist(axes[0], axes[1]);
            let interval = format!("実材軸の補剛区間 [0, {length_mm:.3}] mm");
            let verified = support_interval(model, elem, ctx, *axes, length_mm).and_then(|()| {
                column_span_parameters(span.interior_maximum, span.monotonic, span.end_moments_nmm)
                    .ok_or("符号付き強軸曲げの単調変化または区間内部の絶対値最大を確認できません")
            });
            span_description = Some(match verified {
                Ok((c, p_lambda_b)) => {
                    saved_parameters = saved_monotonic_parameters(span)
                        .filter(|alternative| *alternative != (c, p_lambda_b));
                    let [i, j] = span.end_moments_nmm.map(|moment| moment / 1e6);
                    let distribution = if span.interior_maximum {
                        "区間内部に絶対値最大"
                    } else if saved_parameters.is_some() {
                        "符号付き曲げが単調、回復端部比の係数"
                    } else {
                        "符号付き曲げが単調"
                    };
                    let ignored_direct = if ctx
                        .steel_attr
                        .as_ref()
                        .is_some_and(|a| a.c_direct.is_some())
                    {
                        "。C直接入力は不使用"
                    } else {
                        ""
                    };
                    format!("{interval}。{distribution}（i端 {i:.6}、j端 {j:.6} kN·m）。C={c:.4}, pλb={p_lambda_b:.4}{ignored_direct}")
                }
                Err(reason) => {
                    span_reason = Some(reason.to_owned());
                    format!("実材軸 [0, {length_mm:.3}] mm")
                }
            });
            if let Some(issue) = span.combination_issue {
                span_reason = combine_incomplete_reasons(span_reason, Some(issue));
            } else if let Some(n) = span
                .compression_at_zero_crossing_n
                .filter(|_| section_issue.is_none())
            {
                let uniaxial_compression =
                    MemberForcesAt::from_local(0.0, [n, 0.0, 0.0, 0.0, 0.0, 0.0]);
                if let Err(reason) =
                    check_symmetric_h_plate_slenderness(&uniaxial_compression, sec, mat)
                {
                    span_reason = combine_incomplete_reasons(
                        span_reason,
                        Some(&format!("強軸曲げがゼロになる断面。{reason}")),
                    );
                }
            }
            verified.ok()
        }
        Err(Stations(reason) | Distribution(reason)) => {
            span_reason = Some(reason.clone());
            if let Some(Err(reason)) = ctx.column_lateral_buckling_length {
                span_reason = combine_incomplete_reasons(span_reason, Some(reason));
            }
            None
        }
    };

    let first = outcomes.len();
    let check_section = |xi,
                         x_mm: f64,
                         force: [f64; 6],
                         cut_side: &str,
                         local_issue: Option<&str>|
     -> Vec<CheckOutcome> {
        let section_issue = section_issue.or(local_issue).or_else(|| {
            force
                .iter()
                .any(|value| !value.is_finite())
                .then_some("この断面の保存断面力に非有限な成分があるため、組合せを確定できません")
        });
        let f = MemberForcesAt::from_local(xi, force);
        let parameters = if f.n > 0.0 && f.my != 0.0 {
            None
        } else {
            parameters
        };
        let evaluate = |parameters| {
            SteelDesign.check_static_column_section(&f, sec, mat, ctx, parameters, section_issue)
        };
        let (mut outcome, mut section_reason) = evaluate(parameters);
        if let Some((recovered, saved)) = parameters
            .zip(saved_parameters)
            .filter(|_| f.mz != 0.0 && section_issue.is_none())
        {
            let (saved_outcome, _) = evaluate(Some(saved));
            let recovered_ratio = axial_bending_ratio(&outcome);
            let saved_ratio = axial_bending_ratio(&saved_outcome);
            if recovered_ratio.map(|ratio| ratio > 1.0) != saved_ratio.map(|ratio| ratio > 1.0) {
                let describe = |(c, p): (f64, f64), ratio: Option<f64>| {
                    let ratio = ratio.map_or_else(|| "未算定".into(), |ratio| ratio.to_string());
                    format!("C={c:.10}, pλb={p:.10}, 軸曲げ比={ratio}")
                };
                let reason = format!(
                    "保存端部比と回復端部比で同じ切断面の軸曲げの適否が一致しないため未算定です（保存端部比: {}、回復端部比: {}）",
                    describe(saved, saved_ratio),
                    describe(recovered, recovered_ratio),
                );
                let (independent, incomplete) = evaluate(None);
                outcome = independent;
                section_reason = combine_incomplete_reasons(incomplete, Some(&reason));
            }
        }
        let location = format!(
            "実材軸i端から x={x_mm:.6} mm{cut_side}。N={:.6} kN, Mz={:.6} kN·m",
            f.n / 1e3,
            f.mz / 1e6,
        );
        let detail = match &span_description {
            Some(description) => format!("{location}。{description}"),
            None => location,
        };
        match &mut outcome {
            CheckOutcome::Checked(result) => {
                if !result.detail.is_empty() {
                    result.detail.push('。');
                }
                result.detail.push_str(&detail);
                if let Some((strong, weak)) = ctx.lk_y.zip(ctx.lk_z).filter(|(strong, weak)| {
                    [*strong, *weak]
                        .into_iter()
                        .all(|length| length.is_finite() && length > 0.0)
                }) {
                    result.detail.push_str(&format!(
                        "。採用座屈長さ [mm]　強軸 {strong} ／ 弱軸 {weak}"
                    ));
                }
            }
            CheckOutcome::Skipped { reason } => {
                reason.push_str(&format!("。{detail}"));
            }
        }
        let span_reason = span_reason
            .as_ref()
            .map(|reason| format!("{reason}。{detail}"));
        let section_reason = section_reason.map(|reason| {
            if span_reason.is_some() {
                reason
            } else {
                format!("{reason}。{detail}")
            }
        });
        let incomplete = combine_incomplete_reasons(section_reason, span_reason.as_deref());
        strength_outcomes(outcome, incomplete.as_deref()).collect()
    };

    let sections = recovered
        .as_ref()
        .ok()
        .map_or(&[][..], |(span, _)| span.sections.as_slice());
    let at_i_end =
        |kind: &MemberLoadKind| matches!(kind, MemberLoadKind::Point { a: 0.0, p } if *p != 0.0);
    let load_at_i_end = loads.map_or_else(
        |_| {
            model
                .load_cases
                .iter()
                .flat_map(|case| &case.member)
                .any(|load| load.elem == elem.id && at_i_end(&load.kind))
        },
        |loads| {
            loads
                .for_member(elem.id)
                .iter()
                .any(|(_, load)| at_i_end(&load.kind))
        },
    );
    for &(xi, force) in &forces.at {
        if matches!(recovered, Err(Stations(_)))
            || !(0.0..=1.0).contains(&xi)
            || forces.at.iter().filter(|&&(pos, _)| xi == pos).count() != 1
            || xi == 0.0 && load_at_i_end
            || sections.iter().any(|section| {
                section.recovered.x_mm == xi * ctx.length && section.saved_left == Some(force)
            })
        {
            continue;
        }
        outcomes.extend(
            check_section(xi, xi * ctx.length, force, "", None)
                .into_iter()
                .map(|outcome| (elem.id, xi, outcome)),
        );
    }
    for section in sections {
        let saved_left = section.saved_left;
        let section = &section.recovered;
        let right = section.right.as_ref().ok().and_then(Option::as_ref);
        if !section.is_i_face || right.is_none() {
            let side = if right.is_some() {
                "（集中荷重の左側）"
            } else {
                ""
            };
            let disagreement = if section_issue.is_none() {
                saved_left.and_then(|saved| {
                    section_applicability_difference(sec, mat, saved, section.left)
                })
            } else {
                None
            };
            let issue = disagreement.as_deref();
            let mut checked = check_section(section.xi, section.x_mm, section.left, side, issue);
            if let Some(saved) = saved_left.filter(|saved| *saved != section.left) {
                let mut observed = check_section(section.xi, section.x_mm, saved, side, issue);
                observed.retain(|outcome| adds_unavailable_or_failure(outcome, &checked));
                for outcome in &mut observed {
                    match outcome {
                        CheckOutcome::Checked(result) => {
                            result.detail.push_str("。同じ切断面の保存断面力による照査")
                        }
                        CheckOutcome::Skipped { reason } => {
                            reason.push_str("。同じ切断面の保存断面力による照査")
                        }
                    }
                }
                checked.extend(observed);
            }
            outcomes.extend(
                checked
                    .into_iter()
                    .map(|outcome| (elem.id, section.xi, outcome)),
            );
        }
        if let Some(&force) = right {
            outcomes.extend(
                check_section(section.xi, section.x_mm, force, "（集中荷重の右側）", None)
                    .into_iter()
                    .map(|outcome| (elem.id, section.xi, outcome)),
            );
        }
    }
    if outcomes.len() == first {
        let reason = combine_incomplete_reasons(span_reason, section_issue);
        outcomes.push((
            elem.id,
            0.5,
            CheckOutcome::Skipped {
                reason: format!(
                    "単一補剛区間の強軸曲げ・横座屈は未算定です。{}",
                    reason
                        .as_deref()
                        .unwrap_or("同じ断面で有限な解析断面力を確認できません"),
                ),
            },
        ));
    } else {
        outcomes[first..].sort_by(|(_, a, _), (_, b, _)| a.total_cmp(b));
    }
    true
}

fn saved_monotonic_parameters(span: &FrameStaticStrongBending) -> Option<(f64, f64)> {
    if span.interior_maximum || !span.monotonic {
        return None;
    }
    let i = span
        .sections
        .first()
        .filter(|cut| cut.recovered.is_i_face)?;
    let j = span.sections.last().filter(|cut| cut.recovered.is_j_face)?;
    // i端の集中力は曲げを不連続にしないが、区間外側の断面力では照査しない。
    let i_mz = (*i.recovered.right.as_ref().ok()?).or(i.saved_left)?[5];
    column_span_parameters(false, true, [i_mz, j.saved_left?[5]])
}

fn axial_bending_ratio(outcome: &CheckOutcome) -> Option<f64> {
    let CheckOutcome::Checked(result) = outcome else {
        return None;
    };
    result
        .components
        .iter()
        .find(|component| component.kind == CheckKind::AxialBending && component.ratio.is_finite())
        .map(|component| component.ratio)
}

fn adds_unavailable_or_failure(saved: &CheckOutcome, recovered: &[CheckOutcome]) -> bool {
    match saved {
        CheckOutcome::Skipped { reason } => !recovered.iter().any(
            |outcome| matches!(outcome, CheckOutcome::Skipped { reason: prior } if prior == reason),
        ),
        CheckOutcome::Checked(result) => result.components.iter().any(|saved| {
            saved.ratio > 1.0
                && !recovered.iter().any(|outcome| {
                    matches!(outcome, CheckOutcome::Checked(result) if result.components.iter()
                    .any(|prior| prior.kind == saved.kind && prior.ratio >= saved.ratio))
                })
        }),
    }
}

fn section_applicability_difference(
    sec: &Section,
    mat: &Material,
    saved: [f64; 6],
    recovered: [f64; 6],
) -> Option<String> {
    let biaxial_tension = |f: [f64; 6]| f[0] > 0.0 && f[4] != 0.0 && f[5] != 0.0;
    for (differs, condition) in [
        (
            (saved[3] == 0.0) != (recovered[3] == 0.0),
            "材軸ねじりの有無",
        ),
        (
            (saved[2] == 0.0) != (recovered[2] == 0.0),
            "弱軸せん断の有無",
        ),
        ((saved[0] < 0.0) != (recovered[0] < 0.0), "圧縮軸力の有無"),
        (
            biaxial_tension(saved) != biaxial_tension(recovered),
            "引張軸力と二軸曲げの併存条件",
        ),
    ] {
        if differs {
            return Some(format!("保存断面力と回復断面力で{condition}が一致しません"));
        }
    }
    if [0, 3, 4, 5].into_iter().all(|i| saved[i] == recovered[i]) {
        return None;
    }
    let limitation = |force| {
        let f = MemberForcesAt::from_local(0.0, force);
        (f.mx == 0.0 && (f.n < 0.0 || f.my != 0.0 || f.mz != 0.0))
            .then(|| check_symmetric_h_plate_slenderness(&f, sec, mat).err())
            .flatten()
    };
    let reason = match (limitation(saved), limitation(recovered)) {
        (Some(reason), None) => format!("保存値: {reason}"),
        (None, Some(reason)) => format!("回復値: {reason}"),
        _ => return None,
    };
    Some(format!(
        "保存断面力と回復断面力で局部座屈の適用条件が一致しません。{reason}"
    ))
}

fn node_ends(model: &Model, elem: &ElementData) -> Option<[[f64; 3]; 2]> {
    let &[i, j] = elem.nodes.as_slice() else {
        return None;
    };
    Some([model.node(i)?.coord, model.node(j)?.coord])
}

fn recover(
    model: &Model,
    elem: &ElementData,
    loads: Result<&BeamSpanLoads<'_>, &&str>,
    forces: &MemberForces,
) -> Result<(FrameStaticStrongBending, [[f64; 3]; 2]), FrameStaticBendingError> {
    let loads = loads
        .map_err(|reason| Distribution((*reason).to_owned()))?
        .for_member(elem.id);
    let axes = model
        .primary_frame_axis_ends(elem)
        .map_err(Stations)?
        .or_else(|| node_ends(model, elem))
        .ok_or_else(|| Stations("柱の実材軸両端を確認できません".into()))?;
    let span = frame_static_strong_bending(axes, elem.local_axis.ref_vector, loads, forces)?;
    Ok((span, axes))
}

fn support_interval(
    model: &Model,
    elem: &ElementData,
    ctx: &DesignCtx,
    axes: [[f64; 3]; 2],
    length_mm: f64,
) -> Result<(), &'static str> {
    let lb_mm = ctx
        .column_lateral_buckling_length
        .ok_or("柱の横座屈長さを確認できません")??;
    let has_support_confirmation = ctx
        .steel_attr
        .as_ref()
        .is_some_and(|attr| attr.column_lateral_buckling_end_supports.is_some());
    if lb_mm <= 1e-9
        || (!has_support_confirmation && node_ends(model, elem) != Some(axes))
        || (lb_mm - length_mm).abs() > 32.0 * f64::EPSILON * length_mm
    {
        return Err(
            "採用横座屈長さを定める両端と実材軸の両端が一致せず、単一補剛区間を確認できません",
        );
    }
    Ok(())
}
