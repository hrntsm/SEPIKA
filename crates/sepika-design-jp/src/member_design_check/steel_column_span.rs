use sepika_core::geom::vec3::dist;
use sepika_core::ids::ElemId;
use sepika_core::model::{ElementData, Material, MemberLoadKind, Model, Section};
use sepika_element::frame::beam::MemberForces;
use sepika_element::frame::member_load::{
    frame_static_strong_bending, FrameStaticBendingError, FrameStaticStrongBending,
};

use super::steel_beam_span::BeamSpanLoads;
use super::{combine_incomplete_reasons, strength_outcomes};
use crate::steel::{
    check_symmetric_h_plate_slenderness, column_span_parameters, StaticColumnSectionCheck,
    SAME_POINT_SHEAR_FORMULA,
};
use crate::{
    CheckComponent, CheckKind, CheckOutcome, CheckResult, DesignCtx, MemberForcesAt, SteelDesign,
};
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
    let duplicate = forces.at.windows(2).any(|pair| pair[0].0 == pair[1].0);
    if !duplicate
        && !forces.at.is_empty()
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
    let partial_recovery = recovered.is_ok()
        && forces
            .at
            .iter()
            .any(|(_, cut)| cut.iter().any(|value| !value.is_finite()));
    let no_strong_bending = !partial_recovery
        && recovered
            .as_ref()
            .is_ok_and(|(span, _)| !span.has_strong_bending)
        && forces.at.iter().all(|(_, force)| force[5] == 0.0);
    let i_end_has_jump = recovered.as_ref().is_ok_and(|(span, _)| {
        span.sections.first().is_some_and(|cut| {
            cut.recovered.is_i_face && cut.recovered.right.as_ref().is_ok_and(Option::is_some)
        })
    });
    // 強軸曲げがなくても、i端集中荷重で断面力が跳ぶ場合は保存左側を区間内の照査に使えない。
    if no_strong_bending && !i_end_has_jump && !duplicate {
        return false;
    }
    let mut span_reason = None;
    let mut span_description = None;
    let mut saved_parameters = None;
    let parameters = match &recovered {
        Ok((span, _)) if no_strong_bending || duplicate => {
            span_reason = span.combination_issue.map(str::to_owned);
            None
        }
        Ok((span, axes)) => {
            let length_mm = dist(axes[0], axes[1]);
            let interval = format!("実材軸の補剛区間 [0, {length_mm:.3}] mm");
            let verified = support_interval(model, elem, ctx, *axes, length_mm).and_then(|()| {
                column_span_parameters(span.interior_maximum, span.monotonic, span.end_moments_nmm)
                    .ok_or("保存断面力と回復断面力の両方で、符号付き強軸曲げの単調変化または区間内部の絶対値最大を確認できません")
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

    let section_scope = StaticColumnSectionCheck {
        parameters,
        zero_shear_limit: false,
        weak_shear_disagreement: false,
        independent_stress_available: recovered.is_ok(),
        other_cut_on_disagreement: None,
    };
    let first = outcomes.len();
    let check_section = |xi,
                         x_mm: f64,
                         force: [f64; 6],
                         cut_side: &str,
                         scope: StaticColumnSectionCheck,
                         local_issue: Option<&str>|
     -> Vec<CheckOutcome> {
        let section_issue =
            combine_incomplete_reasons(section_issue.map(str::to_owned), local_issue);
        let f = MemberForcesAt::from_local(xi, force);
        let parameters = if f.n > 0.0 && f.my != 0.0 {
            None
        } else {
            scope.parameters
        };
        let evaluate = |parameters| {
            SteelDesign.check_static_column_section(
                &f,
                sec,
                mat,
                ctx,
                StaticColumnSectionCheck {
                    parameters,
                    ..scope
                },
                section_issue.as_deref(),
            )
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
    let check_same_cut =
        |xi, x_mm, mut recovered: [f64; 6], saved: Option<[f64; 6]>, side, zero_shear_limit| {
            if partial_recovery {
                if let Some(saved) = saved {
                    // 同じ断面で保存値が欠けていれば、回復値だけでその成分の適用を確定しない。
                    for (value, observed) in recovered.iter_mut().zip(saved) {
                        if !observed.is_finite() {
                            *value = f64::NAN;
                        }
                    }
                }
            }
            let disagreement = saved
                .and_then(|saved| section_applicability_difference(sec, mat, saved, recovered));
            let issue = disagreement.as_deref();
            let weak_shear_disagreement = saved.is_some_and(|saved| {
                saved[2].is_finite()
                    && recovered[2].is_finite()
                    && (saved[2] == 0.0) != (recovered[2] == 0.0)
            });
            let scope = StaticColumnSectionCheck {
                zero_shear_limit,
                weak_shear_disagreement,
                ..section_scope
            };
            let mut checked = check_section(
                xi,
                x_mm,
                recovered,
                side,
                StaticColumnSectionCheck {
                    other_cut_on_disagreement: saved.filter(|_| issue.is_some()),
                    ..scope
                },
                issue,
            );
            if let Some(saved) = saved.filter(|saved| {
                saved
                    .iter()
                    .zip(recovered)
                    .any(|(s, r)| *s != r && (s.is_finite() || r.is_finite()))
            }) {
                for outcome in check_section(
                    xi,
                    x_mm,
                    saved,
                    side,
                    StaticColumnSectionCheck {
                        other_cut_on_disagreement: issue.map(|_| recovered),
                        ..scope
                    },
                    issue,
                ) {
                    add_saved_check(&mut checked, outcome, recovered[1], saved[1]);
                }
            }
            checked
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
                section.recovered.x_mm == xi * ctx.length && section.saved_left.is_some()
            })
        {
            continue;
        }
        outcomes.extend(
            check_section(xi, xi * ctx.length, force, "", section_scope, None)
                .into_iter()
                .map(|outcome| (elem.id, xi, outcome)),
        );
    }
    for section in sections {
        let [left_zero_limit, right_zero_limit] = section.zero_shear_limits;
        let cut = &section.recovered;
        let right = cut.right.as_ref().ok().and_then(Option::as_ref);
        if !cut.is_i_face || right.is_none() {
            let side = if right.is_some() {
                "（集中荷重の左側）"
            } else {
                ""
            };
            outcomes.extend(
                check_same_cut(
                    cut.xi,
                    cut.x_mm,
                    cut.left,
                    section.saved_left,
                    side,
                    left_zero_limit,
                )
                .into_iter()
                .map(|outcome| (elem.id, cut.xi, outcome)),
            );
        }
        if let Some(&force) = right {
            let side = "（集中荷重の右側）";
            let mut checked = check_same_cut(
                cut.xi,
                cut.x_mm,
                force,
                section.saved_right.as_ref().ok().copied().flatten(),
                side,
                right_zero_limit,
            );
            if let Err(reason) = &section.saved_right {
                let reason = format!("実材軸i端から x={:.6} mm{side}。{reason}", cut.x_mm);
                if let Some(CheckOutcome::Skipped { reason: prior }) = checked
                    .iter_mut()
                    .find(|row| matches!(row, CheckOutcome::Skipped { .. }))
                {
                    prior.push('。');
                    prior.push_str(&reason);
                } else {
                    checked = checked
                        .into_iter()
                        .flat_map(|outcome| strength_outcomes(outcome, Some(&reason)))
                        .collect();
                }
            }
            outcomes.extend(
                checked
                    .into_iter()
                    .map(|outcome| (elem.id, cut.xi, outcome)),
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

fn includes_formula(other: &CheckComponent, component: &CheckComponent) -> bool {
    other.kind == component.kind
        && (component.kind != CheckKind::Shear
            || !component.detail.contains(SAME_POINT_SHEAR_FORMULA)
            || other.detail.contains(SAME_POINT_SHEAR_FORMULA))
}

fn covers_check(other: &CheckResult, other_qy_n: f64, check: &CheckResult, qy_n: f64) -> bool {
    !check.components.is_empty()
        && check.components.iter().all(|component| {
            other.components.iter().any(|other| {
                includes_formula(other, component)
                    && other.ratio.is_finite()
                    && component.ratio >= 0.0
                    && other.ratio >= component.ratio
                    && (component.kind != CheckKind::Shear || other_qy_n.abs() >= qy_n.abs())
            })
        })
}

fn add_saved_check(
    recovered: &mut Vec<CheckOutcome>,
    mut outcome: CheckOutcome,
    recovered_qy_n: f64,
    saved_qy_n: f64,
) {
    const SAVED_CUT: &str = "。同じ切断面の保存断面力による照査";
    let mut replacement = None;
    match &mut outcome {
        CheckOutcome::Skipped { reason } => {
            if recovered.iter().any(
                |row| matches!(row, CheckOutcome::Skipped { reason: prior } if prior == reason),
            ) {
                return;
            }
            reason.push_str(SAVED_CUT);
        }
        CheckOutcome::Checked(saved) => {
            if saved.components.is_empty() {
                return;
            }
            if let Some((index, prior)) =
                recovered
                    .iter()
                    .enumerate()
                    .find_map(|(index, row)| match row {
                        CheckOutcome::Checked(prior) if prior.basis == saved.basis => {
                            Some((index, prior))
                        }
                        _ => None,
                    })
            {
                if covers_check(prior, recovered_qy_n, saved, saved_qy_n) {
                    return;
                }
                if covers_check(saved, saved_qy_n, prior, recovered_qy_n) {
                    replacement = Some(index);
                } else if saved.components.iter().all(|component| {
                    (0.0..=1.0).contains(&component.ratio)
                        && prior.components.iter().any(|prior| {
                            prior.ratio.is_finite()
                                && prior.ratio >= 0.0
                                && includes_formula(prior, component)
                        })
                }) {
                    return;
                }
            }
            saved.detail.push_str(SAVED_CUT);
        }
    }
    if let Some(index) = replacement {
        recovered[index] = outcome;
    } else {
        recovered.push(outcome);
    }
}

fn section_applicability_difference(
    sec: &Section,
    mat: &Material,
    saved: [f64; 6],
    recovered: [f64; 6],
) -> Option<String> {
    let biaxial_tension = |f: [f64; 6]| f[0] > 0.0 && f[4] != 0.0 && f[5] != 0.0;
    let known = |indices: &[usize]| {
        indices
            .iter()
            .all(|&i| saved[i].is_finite() && recovered[i].is_finite())
    };
    for (differs, condition) in [
        (
            known(&[3]) && (saved[3] == 0.0) != (recovered[3] == 0.0),
            "材軸ねじりの有無",
        ),
        (
            known(&[0]) && (saved[0] < 0.0) != (recovered[0] < 0.0),
            "圧縮軸力の有無",
        ),
        (
            known(&[0, 4, 5]) && biaxial_tension(saved) != biaxial_tension(recovered),
            "引張軸力と二軸曲げの併存条件",
        ),
    ] {
        if differs {
            return Some(format!("保存断面力と回復断面力で{condition}が一致しません"));
        }
    }
    if !known(&[0, 3, 4, 5]) || [0, 3, 4, 5].into_iter().all(|i| saved[i] == recovered[i]) {
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
