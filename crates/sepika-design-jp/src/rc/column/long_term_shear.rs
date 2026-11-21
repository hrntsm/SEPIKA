use super::{axis_props_from_shape, shear_alpha, shear_capacity, MAX_SHEAR_ALPHA};
use crate::member_design_check::combine_incomplete_reasons;
use crate::member_design_check::rc_column_long_term_shear::LongTermColumnShear;
use crate::rc::RcAllow;
use crate::ultimate::rc_props::RcDirection;
use crate::{CheckComponent, CheckKind, CheckResult, LoadTerm};
use sepika_core::section_shape::SectionShape;
use sepika_element::frame::member_load::{FrameShearEnvelope, FrameShearPeak, SectionSide};

#[derive(Default)]
pub(crate) struct LongTermColumnShearCheck {
    pub exact: Option<(f64, CheckComponent)>,
    pub bound_failure: Option<(f64, CheckResult)>,
    pub incomplete: Option<String>,
}

pub(crate) fn check(
    shape: &SectionShape,
    allow: &RcAllow,
    length_mm: f64,
    envelope: &LongTermColumnShear,
) -> LongTermColumnShearCheck {
    let responses = match envelope {
        Ok(responses) => responses,
        Err(reason) => {
            return LongTermColumnShearCheck {
                incomplete: Some(format!("RC柱の長期せん断は未算定です（{reason}）")),
                ..Default::default()
            };
        }
    };
    let mut result = LongTermColumnShearCheck::default();
    let mut exact = None;
    let mut bound = None;
    let mut capacities = ["未算定".to_owned(), "未算定".to_owned()];
    let mut alphas = ["未算定".to_owned(), "未算定".to_owned()];
    let mut detail = String::new();
    let mut bound_detail = format!("αの上限 {MAX_SHEAR_ALPHA}");
    for (axis, direction) in [RcDirection::Strong, RcDirection::Weak]
        .into_iter()
        .enumerate()
    {
        let direction_name = ["Qy", "Qz"][axis];
        let capacity_name = ["QAy", "QAz"][axis];
        let update_reason = |result: &mut LongTermColumnShearCheck, reason: &str| {
            let reason =
                format!("RC柱の長期せん断（局所{direction_name}）は未算定です（{reason}）");
            result.incomplete = combine_incomplete_reasons(result.incomplete.take(), Some(&reason));
        };
        let response = match &responses[axis] {
            Ok(response) => response,
            Err(reason) => {
                update_reason(&mut result, reason);
                continue;
            }
        };
        let peak = &response.peak;
        let xi = peak.x_mm / length_mm;
        let demand_n = peak.q_n.abs();
        let capacity = |props, alpha| {
            shear_capacity(props, allow, alpha, LoadTerm::Long, false, true)
                .filter(|qa| *qa > 0.0 && qa.is_finite() && (demand_n / qa).is_finite())
        };
        let Some(props) = axis_props_from_shape(shape, direction, true) else {
            update_reason(&mut result, "せん断検定の断面諸量を算定できません");
            continue;
        };
        let Some(base_n) = capacity(&props, 1.0) else {
            update_reason(
                &mut result,
                "許容せん断力と検定比を有限な値で算定できません",
            );
            continue;
        };
        let comparison = if demand_n <= base_n {
            Ok((1.0, base_n))
        } else {
            response
                .faces
                .as_ref()
                .map_err(String::as_str)
                .and_then(|faces| {
                    if !props.d.is_finite()
                        || props.d <= 0.0
                        || faces
                            .iter()
                            .any(|face| !(face.q_n.abs() * props.d).is_finite())
                    {
                        return Err(
                            "材端フェースのせん断スパン比に使う断面諸量または内力が不正です",
                        );
                    }
                    let alpha = faces
                        .iter()
                        .map(|face| shear_alpha(face.m_nmm, face.q_n, props.d, MAX_SHEAR_ALPHA))
                        .reduce(f64::min)
                        .expect("両材端フェースの割増係数");
                    capacity(&props, alpha)
                        .map(|qa| (alpha, qa))
                        .ok_or("許容せん断力と検定比を有限な値で算定できません")
                })
        };
        match comparison {
            Ok((alpha, qa)) => {
                let ratio = demand_n / qa;
                if exact.is_none_or(|(_, previous)| ratio > previous) {
                    exact = Some((xi, ratio));
                }
                capacities[axis] = format!("{qa:.1} N");
                alphas[axis] = format!("{alpha:.3}");
                detail.push_str(&demand_detail(axis, peak, Some((ratio, "検定比"))));
                bound_detail.push_str(&demand_detail(axis, peak, None));
            }
            Err(reason) => {
                update_reason(&mut result, reason);
                detail.push_str(&demand_detail(axis, peak, None));
                if let Some(upper_n) = capacity(&props, MAX_SHEAR_ALPHA).filter(|qa| demand_n > *qa)
                {
                    let ratio = demand_n / upper_n;
                    if bound.is_none_or(|(_, previous)| ratio > previous) {
                        bound = Some((xi, ratio));
                    }
                    bound_detail.push_str(&format!("、{capacity_name}の上限 {upper_n:.1} N"));
                    bound_detail.push_str(&demand_detail(
                        axis,
                        peak,
                        Some((ratio, "検定比の下限")),
                    ));
                } else {
                    bound_detail.push_str(&demand_detail(axis, peak, None));
                }
            }
        }
    }
    let [qa_y, qa_z] = capacities;
    let [alpha_y, alpha_z] = alphas;
    result.exact = exact.map(|(xi, ratio)| {
        (
            xi,
            CheckComponent {
                kind: CheckKind::Shear,
                ratio,
                detail: format!("QAy={qa_y}, QAz={qa_z}, αy={alpha_y}, αz={alpha_z}{detail}"),
            },
        )
    });
    result.bound_failure = bound.map(|(xi, ratio)| {
        (
            xi,
            shear_check_result(
                "RC柱の長期せん断（許容せん断力の上限による判定）",
                CheckComponent {
                    kind: CheckKind::Shear,
                    ratio,
                    detail: bound_detail,
                },
            ),
        )
    });
    result
}

pub(crate) fn shear_check_result(basis: &str, component: CheckComponent) -> CheckResult {
    CheckResult {
        basis: basis.into(),
        detail: String::new(),
        components: vec![component],
    }
}

fn demand_detail(axis: usize, peak: &FrameShearPeak, comparison: Option<(f64, &str)>) -> String {
    let direction = ["Qy", "Qz"][axis];
    let side = match peak.side {
        SectionSide::Left => "（i端側）",
        SectionSide::Right => "（j端側）",
        SectionSide::Continuous => "",
    };
    let q = peak.q_n.abs();
    let ratio_detail = comparison.map_or_else(String::new, |(ratio, label)| {
        format!("、{label} {ratio:.6}")
    });
    format!(
        "、局所{direction}最大 |{direction}|={q:.3} N（i端から x={:.6} mm{side}{ratio_detail}）",
        peak.x_mm
    )
}

pub(crate) fn recovered_long_shear_detail(
    responses: &[Result<FrameShearEnvelope, String>; 2],
) -> String {
    let details: String = responses
        .iter()
        .enumerate()
        .filter_map(|(axis, response)| {
            response
                .as_ref()
                .ok()
                .map(|response| demand_detail(axis, &response.peak, None))
        })
        .collect();
    if details.is_empty() {
        details
    } else {
        format!("。確認できた部材の最大せん断力{details}")
    }
}
