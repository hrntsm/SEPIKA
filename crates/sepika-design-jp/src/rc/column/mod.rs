//! 鉄筋コンクリート造柱の断面検定（RC 規準14条: 軸力・軸力+曲げ・せん断）。

use super::{
    axis_props_from_shape, main_rebar_grade, rc_allow, rebar_allowable_tension,
    rebar_info_from_shape, rebar_sigma_y_of, seismic_design_shear, shear_alpha, shear_capacity,
    shear_rebar_grade, AxisProps, ColumnGravityMode, ColumnMechanism,
};
use crate::member_design_check::combine_incomplete_reasons;
use crate::ultimate::rc_props::RcDirection;
use crate::{
    CheckComponent, CheckKind, CheckOutcome, CheckResult, DesignCtx, LoadTerm, MemberForcesAt,
    MemberKind, QdMethod,
};
use sepika_core::model::Material;
use sepika_core::section_shape::SectionShape;

pub(crate) mod long_term_shear;
mod nm_interaction;

pub(crate) use nm_interaction::interp_ma;
use nm_interaction::*;

const MAX_SHEAR_ALPHA: f64 = 1.5;

pub(super) fn qd1_mechanism(ctx: &DesignCtx) -> Option<&ColumnMechanism> {
    (ctx.kind == MemberKind::Column
        && ctx.term == LoadTerm::Short
        && ctx
            .seismic_qd
            .as_ref()
            .is_some_and(|qd| qd.method != QdMethod::Qd2))
    .then_some(ctx.column_sum_my.as_ref())
    .flatten()
}

fn gravity_shear_reference_issue(
    ctx: &DesignCtx,
    pos: f64,
    q_index: usize,
) -> Option<&'static str> {
    let qd = ctx.seismic_qd.as_ref()?;
    if pos.is_finite()
        && qd
            .long_at
            .iter()
            .find(|(p, _)| (p - pos).abs() < 1e-6)
            .is_some_and(|(_, values)| values[q_index].is_finite())
    {
        return None;
    }
    Some(if q_index == 1 {
        "RC柱: Qy方向の設計用せん断力に必要な同位置の重力参照内力が不足または非有限です"
    } else {
        "RC柱: Qz方向の設計用せん断力に必要な同位置の重力参照内力が不足または非有限です"
    })
}

pub(super) fn gravity_shear_reference_issues<'a>(
    ctx: &'a DesignCtx,
    pos: f64,
    shape: &'a SectionShape,
) -> impl Iterator<Item = &'static str> + 'a {
    let column = matches!(
        shape,
        SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. }
    );
    [(RcDirection::Strong, 1), (RcDirection::Weak, 2)]
        .into_iter()
        .filter(move |(direction, _)| {
            column
                && axis_props_from_shape(shape, *direction, true)
                    .is_some_and(|props| ctx.term == LoadTerm::Long || props.pw <= 0.002)
        })
        .filter_map(move |(_, index)| gravity_shear_reference_issue(ctx, pos, index))
}

#[allow(clippy::too_many_arguments)]
fn seismic_column_design_shear(
    ctx: &DesignCtx,
    forces: &MemberForcesAt,
    fallback_qy: Option<f64>,
    fallback_qz: Option<f64>,
    shear_rebar_valid: bool,
    strict_gravity: bool,
    boundary_shear_only: bool,
    incomplete: &mut Option<String>,
) -> (Option<f64>, Option<f64>) {
    let qd = ctx.seismic_qd.as_ref().expect("設計用せん断力を使う条件");
    let mechanism = qd1_mechanism(ctx);
    let mut demand = |q_signed, q_index, sum_my: Option<&Result<Option<f64>, String>>, fallback| {
        if !strict_gravity
            && !boundary_shear_only
            && !shear_rebar_valid
            && gravity_shear_reference_issue(ctx, forces.pos, q_index).is_some()
        {
            return None;
        }
        let sum_my = match (qd.method, sum_my) {
            (QdMethod::Qd2, _) => 0.0,
            (_, Some(Err(_))) => return None,
            (_, Some(Ok(Some(sum_my)))) => *sum_my,
            _ => match fallback {
                Some(value) => value,
                None => {
                    let reason = if q_index == 1 {
                        "RC柱: Qy方向のQD1の代替算定に必要な検定位置の軸力Nが非有限です"
                    } else {
                        "RC柱: Qz方向のQD1の代替算定に必要な検定位置の軸力Nが非有限です"
                    };
                    *incomplete = combine_incomplete_reasons(incomplete.take(), Some(reason));
                    return None;
                }
            },
        };
        if strict_gravity || boundary_shear_only {
            if qd.method != QdMethod::Qd1 && !f64::is_finite(q_signed) {
                *incomplete =
                    combine_incomplete_reasons(incomplete.take(), Some(shear_input_issue(q_index)));
                return None;
            }
            match super::column_design_shear(qd, forces.pos, q_signed, q_index, sum_my) {
                Ok(value) => Some(value),
                Err(reason) => {
                    if qd.method == QdMethod::Qd1 || boundary_shear_only && shear_rebar_valid {
                        *incomplete = combine_incomplete_reasons(incomplete.take(), Some(reason));
                    }
                    None
                }
            }
        } else {
            Some(seismic_design_shear(
                ctx, forces.pos, q_signed, q_index, sum_my, true,
            ))
        }
    };
    (
        demand(forces.qy, 1, mechanism.map(|m| &m.qy), fallback_qy),
        demand(forces.qz, 2, mechanism.map(|m| &m.qz), fallback_qz),
    )
}

fn shear_input_issue(q_index: usize) -> &'static str {
    if q_index == 1 {
        "RC柱: Qy方向のせん断検定に必要な需要または許容せん断力を有限な値で算定できません"
    } else {
        "RC柱: Qz方向のせん断検定に必要な需要または許容せん断力を有限な値で算定できません"
    }
}

fn column_allowable_moment(curve: &[(f64, f64)], n_design: f64) -> f64 {
    if n_design < 0.0 && curve.first().is_some_and(|point| n_design < point.0) {
        // 引張側の算定範囲外では、端点の曲げ耐力を外挿できない。
        0.0
    } else {
        interp_ma(curve, n_design)
    }
}

fn column_moment_ratio(
    forces: &MemberForcesAt,
    na_n: f64,
    ma_z_nmm: f64,
    ma_y_nmm: f64,
    circle: bool,
    finite_only: bool,
) -> Result<f64, String> {
    let nonzero = |m: f64| m != 0.0 && (!finite_only || m.is_finite());
    if (ma_z_nmm <= 0.0 && nonzero(forces.mz)) || (ma_y_nmm <= 0.0 && nonzero(forces.my)) {
        return Err(format!(
            "RC柱: 許容曲げが非正で非零の曲げ需要があるため検定不能: NA={na_n} N, N={} N, MA_z={ma_z_nmm} N·mm, MA_y={ma_y_nmm} N·mm, mz={} N·mm, my={} N·mm{}",
            -forces.n, forces.mz, forces.my,
            if circle { "（円形柱、等価矩形近似）" } else { "" },
        ));
    }
    Ok(available_column_moment_ratio(
        forces,
        ma_z_nmm,
        ma_y_nmm,
        circle,
        finite_only,
    ))
}

fn available_column_moment_ratio(
    forces: &MemberForcesAt,
    ma_z_nmm: f64,
    ma_y_nmm: f64,
    circle: bool,
    finite_only: bool,
) -> f64 {
    [(forces.mz, ma_z_nmm), (forces.my, ma_y_nmm)]
        .into_iter()
        .filter(|(m, ma)| *ma > 0.0 && (!finite_only || m.is_finite()))
        .map(|(m, ma)| {
            let ratio = m.abs() / ma;
            if circle {
                ratio.powi(2)
            } else {
                ratio
            }
        })
        .sum()
}

fn column_axial_overload(n_design_n: f64, na_n: f64, nt_n: f64) -> Option<CheckComponent> {
    let (label, limit_n) = if n_design_n < 0.0 {
        ("NT", nt_n)
    } else {
        ("NA", na_n)
    };
    let demand_n = n_design_n.abs();
    if !(demand_n.is_finite() && limit_n.is_finite() && limit_n > 0.0 && demand_n > limit_n) {
        return None;
    }
    let ratio = demand_n / limit_n;
    let detail = if ratio.is_finite() {
        format!("{label}={limit_n:.1} N, N={n_design_n:.1} N（圧縮正）")
    } else {
        format!("{label}={limit_n:e} N, N={n_design_n:e} N（圧縮正、検定比の下限）")
    };
    Some(CheckComponent {
        kind: CheckKind::Axial,
        ratio: ratio.min(f64::MAX),
        detail,
    })
}

/// 矩形柱の直交方向主筋総断面積 at_perp [mm²]。
///
/// 全主筋 ag から検討方向の引張側・圧縮側最外段 at・ac を除いた残り（負にならない）。
fn rect_column_at_perp(ag_mm2: f64, at_mm2: f64, ac_mm2: f64) -> f64 {
    (ag_mm2 - at_mm2 - ac_mm2).max(0.0)
}

/// 柱の断面検定（RC 規準 14条）。
/// 許容曲げが非正の方向に非零需要があれば、理由付き未検定を返す。
#[allow(clippy::too_many_arguments)]
pub(super) fn column_check(
    forces: &MemberForcesAt,
    mat: &Material,
    ctx: &DesignCtx,
    shape: &SectionShape,
    fc_raw: f64,
    main_rebar_valid: bool,
    shear_rebar_valid: bool,
    shear_reference_valid: bool,
    column_gravity: ColumnGravityMode,
    incomplete: &mut Option<String>,
) -> CheckOutcome {
    let long_term = ctx.term == LoadTerm::Long;
    let strict_gravity = matches!(
        column_gravity,
        ColumnGravityMode::TimeHistory {
            strict_gravity: true
        }
    );
    let ordinary = matches!(column_gravity, ColumnGravityMode::Ordinary { .. });
    let saved_long_term = matches!(
        column_gravity,
        ColumnGravityMode::Ordinary {
            saved_long_term: true,
            ..
        }
    );
    let allow = rc_allow(
        fc_raw,
        mat.concrete_class,
        shear_rebar_valid.then(|| shear_rebar_grade(ctx.shear_rebar_material.as_ref())),
        long_term,
    );
    let check_finite = saved_long_term || !long_term;
    let axial_available = !check_finite || forces.n.is_finite();
    let bending_partial = check_finite && !(forces.my.is_finite() && forces.mz.is_finite());
    let short_force_issue = !long_term && (!axial_available || bending_partial);
    if short_force_issue {
        for (value, reason) in [
            (
                forces.n,
                "RC柱: 軸力Nが非有限のため、軸力・曲げを検定できません",
            ),
            (
                forces.my,
                "RC柱: 曲げMyが非有限のため、当該方向の曲げを検定できません",
            ),
            (
                forces.mz,
                "RC柱: 曲げMzが非有限のため、当該方向の曲げを検定できません",
            ),
        ] {
            if !value.is_finite() {
                *incomplete = combine_incomplete_reasons(incomplete.take(), Some(reason));
            }
        }
    }
    let partial_detail = if bending_partial {
        "、有限成分による検定比の下限"
    } else {
        ""
    };
    let spans = [
        ctx.shear_span.unwrap_or((forces.mz.abs(), forces.qy.abs())),
        ctx.shear_span_y
            .unwrap_or((forces.my.abs(), forces.qz.abs())),
    ];
    let alpha_available = [(forces.mz, spans[0]), (forces.my, spans[1])]
        .map(|(local_m, (m, q))| local_m.is_finite() && m.is_finite() && q.is_finite());
    let short_damage_control = !long_term && ctx.rc_damage_control;
    let mut span_issue = false;
    if short_damage_control {
        for (available, reason) in alpha_available.iter().zip([
            "RC柱: Qy方向の損傷制御用せん断スパンに必要な曲げMzまたはせん断力が非有限です",
            "RC柱: Qz方向の損傷制御用せん断スパンに必要な曲げMyまたはせん断力が非有限です",
        ]) {
            if !available {
                *incomplete = combine_incomplete_reasons(incomplete.take(), Some(reason));
                span_issue = true;
            }
        }
    }
    let shear_capacity = |props: &AxisProps, axis: usize| {
        let (m, q) = spans[axis];
        let alpha = if !long_term && !alpha_available[axis] {
            None
        } else {
            Some(shear_alpha(m, q, props.d, MAX_SHEAR_ALPHA))
        };
        let capacity = if short_damage_control && alpha.is_none() {
            None
        } else {
            super::shear_capacity(
                props,
                &allow,
                alpha.unwrap_or(1.0),
                ctx.term,
                ctx.rc_damage_control,
                true,
            )
        };
        (
            capacity,
            alpha.map_or_else(|| "未算定".into(), |value| format!("{value:.3}")),
        )
    };
    let n_design = -forces.n;
    let mut bending_issue = None;
    let missing_gravity = ctx.seismic_qd.as_ref().is_some_and(|qd| {
        strict_gravity
            && ([1, 2]
                .into_iter()
                .any(|index| super::column_qd2_reference_issue(qd, forces.pos, index).is_some())
                || qd1_mechanism(ctx).is_some_and(|m| m.missing_gravity_axial))
    });
    let finish_bending = |bending_issue: Option<String>, incomplete: &mut Option<String>| {
        if let Some(reason) = bending_issue {
            if !(ordinary
                || short_force_issue
                || strict_gravity && (missing_gravity || incomplete.is_some()))
            {
                return Err(CheckOutcome::Skipped { reason });
            }
            *incomplete = combine_incomplete_reasons(incomplete.take(), Some(&reason));
        }
        Ok(())
    };
    let short_partial = short_force_issue
        || span_issue
        || matches!(
            column_gravity,
            ColumnGravityMode::Ordinary {
                response_end_axial_issue: Some(_),
                ..
            }
        );
    let push_axial_bending = |components: &mut Vec<CheckComponent>,
                              bending_unchecked,
                              na,
                              nt,
                              ratio_moment: f64,
                              mut detail: String| {
        let mut ratio_axial = if forces.n > 0.0 {
            forces.n / nt
        } else if forces.n < 0.0 && na > 0.0 {
            n_design / na
        } else {
            0.0
        };
        if (ordinary || short_force_issue) && bending_unchecked {
            if let Some(axial_ng) = column_axial_overload(n_design, na, nt) {
                components.push(axial_ng);
                // 有限曲げ成分の下限に、単独で記録した軸力比を重ねない。
                ratio_axial = 0.0;
            }
            if !bending_partial {
                return;
            }
        }
        let ratio = if short_partial
            && (ratio_moment == f64::INFINITY
                || ratio_axial == f64::INFINITY
                    && column_axial_overload(n_design, na, nt).is_some())
        {
            if !bending_partial {
                detail.push_str("、検定比の下限");
            }
            detail.push_str("（表現可能な最大値）");
            f64::MAX
        } else {
            ratio_axial.max(ratio_moment)
        };
        components.push(CheckComponent {
            kind: CheckKind::AxialBending,
            ratio,
            detail,
        });
    };
    let shear_reference_valid = |boundary_shear_only: bool, incomplete: &mut Option<String>| {
        if !shear_reference_valid {
            return false;
        }
        if let ColumnGravityMode::Ordinary {
            boundary_shear_issue,
            response_end_axial_issue,
            ..
        } = column_gravity
        {
            if let Some(reason) =
                response_end_axial_issue.or(boundary_shear_issue.filter(|_| boundary_shear_only))
            {
                *incomplete = combine_incomplete_reasons(incomplete.take(), Some(reason));
                return false;
            }
        }
        true
    };
    let demand_without_main_rebar = |q, index| {
        if strict_gravity {
            let qd = ctx.seismic_qd.as_ref().expect("設計用せん断力を使う条件");
            if qd.method == QdMethod::Qd2 {
                return super::column_design_shear(qd, forces.pos, q, index, 0.0)
                    .ok()
                    .filter(|q| q.is_finite());
            }
            return None;
        }
        super::shear_demand_without_main_rebar(ctx, forces.pos, q, index)
    };
    let shear_ratio = |demands: [Option<f64>; 2],
                       capacities: [Option<f64>; 2],
                       boundary_shear_only,
                       incomplete: &mut Option<String>| {
        demands
            .into_iter()
            .zip(capacities)
            .enumerate()
            .filter_map(|(axis, (demand, capacity))| {
                let q = demand?;
                let qa = capacity?;
                let (m, span_q) = spans[axis];
                let ratio = if qa > 0.0 { q / qa } else { 0.0 };
                let lower_bound = short_partial && ratio == f64::INFINITY && q > qa;
                if (boundary_shear_only || short_partial)
                    && (!q.is_finite()
                        || !qa.is_finite()
                        || qa <= 0.0
                        || !ratio.is_finite() && !lower_bound
                        || (long_term || ctx.rc_damage_control)
                            && !(m.is_finite() && span_q.is_finite()))
                {
                    *incomplete = combine_incomplete_reasons(
                        incomplete.take(),
                        Some(shear_input_issue(axis + 1)),
                    );
                    return None;
                }
                Some((if lower_bound { f64::MAX } else { ratio }, lower_bound))
            })
            .reduce(|(a, a_bound), (b, b_bound)| (a.max(b), a_bound || b_bound))
    };
    let shear_ratio_note = |lower_bound| {
        if lower_bound {
            "、検定比の下限"
        } else {
            ""
        }
    };
    let capacity = |qa: Option<f64>| qa.map_or_else(|| "未算定".into(), |qa| format!("{qa:.1} N"));

    if let SectionShape::RcColumnCircle { d, .. } = shape {
        let d_full = *d;
        let props = axis_props_from_shape(shape, RcDirection::Strong, true)
            .expect("円形柱の断面諸元を算定できる形状のみ来る");
        let info =
            rebar_info_from_shape(shape, true).expect("円形柱の鉄筋情報を算定できる形状のみ来る");
        let gross_area = std::f64::consts::PI * d_full * d_full / 4.0;
        let as_total = info.main_area;
        let mut components = Vec::new();
        if main_rebar_valid && axial_available {
            let grade = main_rebar_grade(ctx.rebar_material.as_ref());
            let ft = rebar_allowable_tension(
                grade,
                info.main_dia.designation_or_diameter_mm(),
                long_term,
            );
            let na = column_axial_capacity(gross_area, as_total, allow.fc, ft, allow.n_ratio);

            let axis = ColumnAxis {
                props,
                at_perp: 0.0,
                ft,
            };
            let curve = column_nm_curve(&axis, &allow, na);
            let ma = column_allowable_moment(&curve, n_design);
            let ratio_moment = match column_moment_ratio(forces, na, ma, ma, true, check_finite) {
                Ok(ratio) => ratio,
                Err(reason) => {
                    bending_issue = Some(reason);
                    available_column_moment_ratio(forces, ma, ma, true, check_finite)
                }
            };
            let nt = as_total * ft;
            let axial_bending_detail = format!(
            "NA={:.1} N, NT={:.1} N, N={:.1} N, MA={:.1} N·mm（等価矩形近似）, mz={:.1} N·mm, my={:.1} N·mm{partial_detail}",
            na, nt, n_design, ma, forces.mz, forces.my,
        );
            push_axial_bending(
                &mut components,
                bending_issue.is_some() || short_force_issue && bending_partial,
                na,
                nt,
                ratio_moment,
                axial_bending_detail,
            );
        }

        let boundary_shear_only = ordinary && bending_issue.is_some();
        let confirmed_shear_only =
            boundary_shear_only || ordinary && (short_force_issue || span_issue);
        let (q_design_y, q_design_z) =
            if saved_long_term || !shear_reference_valid(confirmed_shear_only, incomplete) {
                (None, None)
            } else if !main_rebar_valid {
                (
                    demand_without_main_rebar(forces.qy, 1),
                    demand_without_main_rebar(forces.qz, 2),
                )
            } else if ctx.seismic_qd.is_some() {
                let mu_inp = sepika_core::rc_capacity::RcCapacityInput {
                    b: gross_area / d_full,
                    d: d_full,
                    at: props.at,
                    d_eff: props.d,
                    sigma_y: rebar_sigma_y_of(ctx.rebar_material.as_ref()),
                    fc: fc_raw,
                    pw: props.pw,
                    sigma_wy: 0.0,
                    clear_span: 0.0,
                    sigma_0: 0.0,
                };
                let sum_mu_fallback = axial_available.then(|| {
                    2.0 * sepika_core::rc_capacity::rc_column_mu_simple(&mu_inp, as_total, n_design)
                });
                seismic_column_design_shear(
                    ctx,
                    forces,
                    sum_mu_fallback,
                    sum_mu_fallback,
                    shear_rebar_valid,
                    strict_gravity,
                    confirmed_shear_only,
                    incomplete,
                )
            } else {
                (Some(forces.qy.abs()), Some(forces.qz.abs()))
            };

        let (qay, alpha_y) = shear_capacity(&props, 0);
        let (qaz, alpha_z) = shear_capacity(&props, 1);

        if let Err(outcome) = finish_bending(bending_issue, incomplete) {
            return outcome;
        }
        let basis = "RC 規準14条（円形柱、等価矩形近似）".to_string();
        let mut detail = format!("at={:.1} mm², d={:.1} mm", props.at, props.d);

        if let Some((ratio, lower_bound)) = shear_ratio(
            [q_design_y, q_design_z],
            [qay, qaz],
            confirmed_shear_only,
            incomplete,
        ) {
            components.push(CheckComponent {
                kind: CheckKind::Shear,
                ratio,
                detail: format!(
                    "QAy={}, QAz={}, αy={}, αz={}, pw={:.5}{}",
                    capacity(qay),
                    capacity(qaz),
                    alpha_y,
                    alpha_z,
                    props.pw,
                    shear_ratio_note(lower_bound),
                ),
            });
        }

        if (!boundary_shear_only || short_force_issue)
            && crate::rc::provisions::is_member_level_station(forces.pos)
        {
            let prov = crate::rc::provisions::column_provisions_info(
                &rebar_info_from_shape(shape, true).unwrap(),
                d_full,
                ctx.clear_length.filter(|&l| l > 1e-9).unwrap_or(ctx.length),
                mat.concrete_class,
                long_term,
                gross_area,
                as_total,
                axial_available.then_some(n_design),
                fc_raw,
                props.pw,
            );
            if let Some(c) = prov.provision_component() {
                components.push(c);
            }
            detail.push_str(&prov.warning_suffix());
        }

        return CheckOutcome::Checked(CheckResult {
            basis,
            detail,
            components,
        });
    }

    if let SectionShape::RcColumnRect { b, d, .. } = shape {
        let props_z = axis_props_from_shape(shape, RcDirection::Strong, true)
            .expect("矩形柱の強軸断面諸元を算定できる形状のみ来る");
        let props_y = axis_props_from_shape(shape, RcDirection::Weak, true)
            .expect("矩形柱の弱軸断面諸元を算定できる形状のみ来る");
        let info =
            rebar_info_from_shape(shape, true).expect("矩形柱の鉄筋情報を算定できる形状のみ来る");
        let gross_area = b * d;
        let as_total = info.main_area;
        let mut components = Vec::new();
        if main_rebar_valid && axial_available {
            let grade = main_rebar_grade(ctx.rebar_material.as_ref());
            let ft_z = rebar_allowable_tension(
                grade,
                info.main_dia.designation_or_diameter_mm(),
                long_term,
            );
            let ft_y = ft_z;
            let na = column_axial_capacity(gross_area, as_total, allow.fc, ft_z, allow.n_ratio);

            let at_perp_for_z = rect_column_at_perp(as_total, props_z.at, props_z.ac);
            let at_perp_for_y = rect_column_at_perp(as_total, props_y.at, props_y.ac);

            let axis_z = ColumnAxis {
                props: props_z,
                at_perp: at_perp_for_z,
                ft: ft_z,
            };
            let axis_y = ColumnAxis {
                props: props_y,
                at_perp: at_perp_for_y,
                ft: ft_y,
            };

            let curve_z = column_nm_curve(&axis_z, &allow, na);
            let curve_y = column_nm_curve(&axis_y, &allow, na);
            let ma_z = column_allowable_moment(&curve_z, n_design);
            let ma_y = column_allowable_moment(&curve_y, n_design);
            let ratio_moment =
                match column_moment_ratio(forces, na, ma_z, ma_y, false, check_finite) {
                    Ok(ratio) => ratio,
                    Err(reason) => {
                        bending_issue = Some(reason);
                        available_column_moment_ratio(forces, ma_z, ma_y, false, check_finite)
                    }
                };
            let nt = as_total * ft_z;
            let axial_bending_detail = format!(
            "NA={:.1} N, NT={:.1} N, N={:.1} N, MA_z={:.1} N·mm, MA_y={:.1} N·mm, mz={:.1} N·mm, my={:.1} N·mm{partial_detail}",
            na, nt, n_design, ma_z, ma_y, forces.mz, forces.my,
        );
            push_axial_bending(
                &mut components,
                bending_issue.is_some() || short_force_issue && bending_partial,
                na,
                nt,
                ratio_moment,
                axial_bending_detail,
            );
        }

        let boundary_shear_only = ordinary && bending_issue.is_some();
        let confirmed_shear_only =
            boundary_shear_only || ordinary && (short_force_issue || span_issue);
        let (q_design_y, q_design_z) =
            if saved_long_term || !shear_reference_valid(confirmed_shear_only, incomplete) {
                (None, None)
            } else if !main_rebar_valid {
                (
                    demand_without_main_rebar(forces.qy, 1),
                    demand_without_main_rebar(forces.qz, 2),
                )
            } else if ctx.seismic_qd.is_some() {
                let sigma_y = rebar_sigma_y_of(ctx.rebar_material.as_ref());
                let mu_of = |b_dir: f64, d_dir: f64, props: &AxisProps| {
                    let mu_inp = sepika_core::rc_capacity::RcCapacityInput {
                        b: b_dir,
                        d: d_dir,
                        at: props.at,
                        d_eff: props.d,
                        sigma_y,
                        fc: fc_raw,
                        pw: props.pw,
                        sigma_wy: 0.0,
                        clear_span: 0.0,
                        sigma_0: 0.0,
                    };
                    sepika_core::rc_capacity::rc_column_mu_simple(&mu_inp, as_total, n_design)
                };
                let fallback_z = axial_available.then(|| 2.0 * mu_of(*b, *d, &props_z));
                let fallback_y = axial_available.then(|| 2.0 * mu_of(*d, *b, &props_y));
                seismic_column_design_shear(
                    ctx,
                    forces,
                    fallback_z,
                    fallback_y,
                    shear_rebar_valid,
                    strict_gravity,
                    confirmed_shear_only,
                    incomplete,
                )
            } else {
                (Some(forces.qy.abs()), Some(forces.qz.abs()))
            };

        let (qay, alpha_y) = shear_capacity(&props_z, 0);
        let (qaz, alpha_z) = shear_capacity(&props_y, 1);

        if let Err(outcome) = finish_bending(bending_issue, incomplete) {
            return outcome;
        }
        let basis = "RC 規準14条（柱、軸力+二軸曲げ+せん断）".to_string();
        let mut detail = String::new();

        if let Some((ratio, lower_bound)) = shear_ratio(
            [q_design_y, q_design_z],
            [qay, qaz],
            confirmed_shear_only,
            incomplete,
        ) {
            components.push(CheckComponent {
                kind: CheckKind::Shear,
                ratio,
                detail: format!(
                    "QAy={}, QAz={}, αy={}, αz={}, pw_z={:.5}, pw_y={:.5}{}",
                    capacity(qay),
                    capacity(qaz),
                    alpha_y,
                    alpha_z,
                    props_z.pw,
                    props_y.pw,
                    shear_ratio_note(lower_bound),
                ),
            });
        }

        if (!boundary_shear_only || short_force_issue)
            && crate::rc::provisions::is_member_level_station(forces.pos)
        {
            let d_min = b.min(*d);
            let prov = crate::rc::provisions::column_provisions_info(
                &rebar_info_from_shape(shape, true).unwrap(),
                d_min,
                ctx.clear_length.filter(|&l| l > 1e-9).unwrap_or(ctx.length),
                mat.concrete_class,
                long_term,
                gross_area,
                as_total,
                axial_available.then_some(n_design),
                fc_raw,
                props_z.pw.min(props_y.pw),
            );
            if let Some(c) = prov.provision_component() {
                components.push(c);
            }
            detail.push_str(&prov.warning_suffix());
        }

        return CheckOutcome::Checked(CheckResult {
            basis,
            detail,
            components,
        });
    }

    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rc::beam::beam_moment_capacity;
    use crate::rc::tests::{
        ctx_column, make_material, make_section, rc_column_rect_shape, rc_rect_shape,
    };
    use crate::DesignCheck;

    #[test]
    fn outside_curve_restriction_applies_only_to_tension() {
        let curve = [(10.0, 20.0), (100.0, 0.0)];
        assert_eq!(column_allowable_moment(&curve, -1.0), 0.0);
        assert_eq!(column_allowable_moment(&curve, 0.0), 20.0);
        assert_eq!(column_allowable_moment(&curve, 5.0), 20.0);
        assert_eq!(column_allowable_moment(&curve, 110.0), 0.0);
    }

    #[test]
    fn one_zero_capacity_axis_preserves_the_other_axis_demand() {
        for zero_z in [true, false] {
            let (ma_z, ma_y) = if zero_z { (0.0, 20.0) } else { (20.0, 0.0) };
            let mut forces = MemberForcesAt {
                pos: 0.5,
                n: -100.0,
                qy: 0.0,
                qz: 0.0,
                mz: if zero_z { 0.0 } else { 30.0 },
                mx: 0.0,
                my: if zero_z { 30.0 } else { 0.0 },
            };
            assert_eq!(
                column_moment_ratio(&forces, 100.0, ma_z, ma_y, false, false),
                Ok(1.5)
            );
            if zero_z {
                forces.mz = -1.0;
            } else {
                forces.my = -1.0;
            }
            let reason = column_moment_ratio(&forces, 100.0, ma_z, ma_y, false, false).unwrap_err();
            for field in [
                "NA=100 N", "N=100 N", "MA_z=", "MA_y=", "20 N·mm", "30 N·mm", "-1 N·mm",
            ] {
                assert!(reason.contains(field), "{reason}");
            }
        }
    }

    #[test]
    fn public_column_check_uses_195_for_nominal_d29_diameter_mm() {
        use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};

        let shape = SectionShape::RcColumnRect {
            b: 800.0,
            d: 800.0,
            rebar: RcRectColumnRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(28.6),
                x: vec![6],
                y: vec![4],
                cover: 65.0 - 6.35 - 28.6 / 2.0,
                hoop: RectColumnHoop {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(6.35),
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        };
        let sec = make_section(shape);
        let mat = make_material(24.0, "Fc24");
        let forces = MemberForcesAt {
            n: -100_000.0,
            mz: 1_000_000.0,
            pos: 0.5,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
        };
        let mut ctx = ctx_column(LoadTerm::Long);
        ctx.shear_rebar_material = Some(crate::rc::tests::make_rebar_material("SD345", 345.0));
        let sd345 = super::super::RcDesign
            .check(&forces, &sec, &mat, &ctx)
            .unwrap_checked();
        ctx.rebar_material = Some(crate::rc::tests::make_rebar_material("SD295A", 295.0));
        let sd295 = super::super::RcDesign
            .check(&forces, &sec, &mat, &ctx)
            .unwrap_checked();

        // 令90条表2で両材種とも長期195 N/mm²。同じ幾何・需要なら耐力と検定比も一致する。
        assert_eq!(sd345.components, sd295.components);
        assert_eq!(sd345.detail, sd295.detail);
    }

    #[test]
    fn test_column_axial_capacity_handcalc() {
        let fc = 8.0; // 長期許容圧縮（Fc=24 なら 8.0）
        let ft = 215.0;
        let n_ratio = 15.0;
        let gross = 400.0 * 400.0;
        let as_total = 8.0 * std::f64::consts::PI * (22.0 / 2.0f64).powi(2);
        let na = column_axial_capacity(gross, as_total, fc, ft, n_ratio);

        let ae = gross + (n_ratio - 1.0) * as_total;
        let expected = (fc * ae).min(ft * ae / n_ratio);
        assert!((na - expected).abs() < 1e-6);
    }

    #[test]
    fn test_column_n0_moment_close_to_beam_ma_t() {
        // 梁側のj≒7d/8による近似差を含む比較なので、柱と梁の曲げ耐力には20%の差を許容する。
        let b = 400.0;
        let d_full = 400.0;
        let shape = rc_rect_shape(b, d_full, 8, 22.0, 2, 40.0, 10.0, 100.0, 2);
        let rebar = match &shape {
            SectionShape::RcColumnRect { rebar, .. } => rebar.clone(),
            _ => unreachable!(),
        };
        let sec = make_section(shape.clone());

        let allow = rc_allow(
            24.0,
            sepika_core::units::ConcreteClass::Normal,
            Some("SD345"),
            true,
        );
        let ft = rebar_allowable_tension("SD345", 22.0, true);

        let props_z = axis_props_from_shape(&shape, RcDirection::Strong, true).unwrap();
        let bm = beam_moment_capacity(&props_z, ft, allow.fc, allow.n_ratio).unwrap();

        let gross_area = sec.width * sec.depth;
        let as_total = rebar.total_main_area();
        let na = column_axial_capacity(gross_area, as_total, allow.fc, ft, allow.n_ratio);

        let axis_z = ColumnAxis {
            props: props_z,
            at_perp: rebar.y_direction_area_mm2(),
            ft,
        };
        let curve = column_nm_curve(&axis_z, &allow, na);
        let ma_at_n0 = interp_ma(&curve, 0.0);

        let rel_diff = (ma_at_n0 - bm.ma).abs() / bm.ma;
        assert!(
            rel_diff < 0.2,
            "N=0 の柱 MA={ma_at_n0} が梁 MA={} と 20% 以上乖離",
            bm.ma
        );
    }

    #[test]
    fn test_column_moment_increases_then_decreases_with_compression() {
        // 過大配筋では耐力のピークが引張側へずれ得るため、圧縮側の山形を確認する入力は軽配筋とする。
        let b = 400.0;
        let d_full = 400.0;
        let shape = rc_rect_shape(b, d_full, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let rebar = match &shape {
            SectionShape::RcColumnRect { rebar, .. } => rebar.clone(),
            _ => unreachable!(),
        };
        let sec = make_section(shape.clone());

        let allow = rc_allow(
            24.0,
            sepika_core::units::ConcreteClass::Normal,
            Some("SD345"),
            true,
        );
        let ft = rebar_allowable_tension("SD345", 19.0, true);
        let props_z = axis_props_from_shape(&shape, RcDirection::Strong, true).unwrap();
        let gross_area = sec.width * sec.depth;
        let as_total = rebar.total_main_area();
        let na = column_axial_capacity(gross_area, as_total, allow.fc, ft, allow.n_ratio);

        let axis_z = ColumnAxis {
            props: props_z,
            at_perp: rebar.y_direction_area_mm2(),
            ft,
        };
        let curve = column_nm_curve(&axis_z, &allow, na);

        let m_at_0 = interp_ma(&curve, 0.0);
        let m_at_mid = interp_ma(&curve, na * 0.3);
        let m_at_near_na = interp_ma(&curve, na * 0.98);

        assert!(
            m_at_mid > m_at_0,
            "圧縮軸力の増加で MA は一旦増加するはず: m0={m_at_0}, mid={m_at_mid}"
        );
        assert!(
            m_at_near_na < m_at_mid,
            "軸力が NA に近づくと MA は減少するはず: mid={m_at_mid}, near_na={m_at_near_na}"
        );
    }

    #[test]
    fn test_column_biaxial_linear_sum() {
        let b = 400.0;
        let d_full = 400.0;
        let shape = rc_rect_shape(b, d_full, 8, 22.0, 2, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_column(LoadTerm::Long);

        let forces_z_only = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1.0,
        };
        let design = crate::rc::RcDesign;
        let r0 = design
            .check(&forces_z_only, &sec, &mat, &ctx)
            .unwrap_checked();
        let ma_z_approx = 1.0 / r0.ratio().max(1e-30);

        let mz_test = ma_z_approx * 0.3;
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: mz_test,
        };
        let r = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(
            (r.ratio() - 0.3).abs() < 0.05,
            "mz 単独 0.3 割合のとき ratio ≒ 0.3 のはず: ratio={}",
            r.ratio()
        );
    }

    /// 矩形柱: components に AxialBending・Shear が含まれ、その最大値が
    /// ratio と一致することを確認する。
    #[test]
    fn test_column_check_components_axial_bending_and_shear() {
        let b = 400.0;
        let d_full = 400.0;
        let shape = rc_rect_shape(b, d_full, 8, 22.0, 2, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_column(LoadTerm::Long);
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -200_000.0,
            qy: 50_000.0,
            qz: 30_000.0,
            mx: 0.0,
            my: 10.0e6,
            mz: 20.0e6,
        };
        let design = crate::rc::RcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert_eq!(result.components.len(), 2);
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::AxialBending));
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Shear));
    }

    /// メカニズム方向欠落（内側 None）は外側未算定と同じく 2·Mu フォールバック。
    #[test]
    fn test_column_sum_my_missing_direction_uses_2mu_fallback() {
        use crate::{QdMethod, SeismicQd};
        let shape = rc_rect_shape(400.0, 400.0, 8, 22.0, 2, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -500_000.0,
            qy: 150_000.0,
            qz: 80_000.0,
            mx: 0.0,
            my: 40.0e6,
            mz: 60.0e6,
        };
        let mut ctx_outer_none = ctx_column(LoadTerm::Short);
        ctx_outer_none.seismic_qd = Some(SeismicQd {
            long_at: vec![(0.0, [forces.n, 40_000.0, 20_000.0, 0.0, 0.0, 0.0])],
            n_factor: 1.5,
            n_mechanism: 1.0,
            q_simple: None,
            clear_length: 3500.0,
            method: QdMethod::Qd1,
        });
        ctx_outer_none.column_sum_my = None;
        let mut ctx_dirs_missing = ctx_column(LoadTerm::Short);
        ctx_dirs_missing.seismic_qd = Some(SeismicQd {
            long_at: vec![(0.0, [forces.n, 40_000.0, 20_000.0, 0.0, 0.0, 0.0])],
            n_factor: 1.5,
            n_mechanism: 1.0,
            q_simple: None,
            clear_length: 3500.0,
            method: QdMethod::Qd1,
        });
        ctx_dirs_missing.column_sum_my = Some(ColumnMechanism {
            qy: Ok(None),
            qz: Ok(None),
            missing_gravity_axial: false,
        });

        let design = crate::rc::RcDesign;
        let r0 = design
            .check(&forces, &sec, &mat, &ctx_outer_none)
            .unwrap_checked();
        let r1 = design
            .check(&forces, &sec, &mat, &ctx_dirs_missing)
            .unwrap_checked();
        let shear = |r: &crate::CheckResult| {
            r.components
                .iter()
                .find(|c| c.kind == crate::CheckKind::Shear)
                .map(|c| c.ratio)
                .unwrap()
        };
        assert!(
            (shear(&r0) - shear(&r1)).abs() < 1e-9,
            "外側 None と内側 (None,None) は同じ 2·Mu: {} vs {}",
            shear(&r0),
            shear(&r1)
        );
    }

    /// `RcColumnRect` 柱の検定値が、`axis_props_from_shape` 経由の諸元と一致する。
    #[test]
    fn test_old_rect_column_check_matches_axis_props_from_shape() {
        let shape = rc_rect_shape(400.0, 400.0, 8, 22.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape.clone());
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_column(LoadTerm::Short);
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -300_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 40_000_000.0,
        };
        let r = crate::rc::RcDesign
            .check(&forces, &sec, &mat, &ctx)
            .unwrap_checked();

        let rebar = match &shape {
            SectionShape::RcColumnRect { rebar, .. } => rebar,
            _ => unreachable!(),
        };
        let allow = rc_allow(
            24.0,
            sepika_core::units::ConcreteClass::Normal,
            Some("SD345"),
            false,
        );
        let props_z = axis_props_from_shape(&shape, RcDirection::Strong, true).unwrap();
        let ft =
            rebar_allowable_tension("SD345", rebar.main_dia.designation_or_diameter_mm(), false);
        let ft_axial =
            rebar_allowable_tension("SD345", rebar.main_dia.designation_or_diameter_mm(), false);
        let gross_area = sec.width * sec.depth;
        let as_total = rebar.total_main_area();
        let na = column_axial_capacity(gross_area, as_total, allow.fc, ft_axial, allow.n_ratio);
        let axis_z = ColumnAxis {
            props: props_z,
            at_perp: rect_column_at_perp(as_total, props_z.at, props_z.ac),
            ft,
        };
        let ma_z = interp_ma(&column_nm_curve(&axis_z, &allow, na), -forces.n);
        let expected = ((-forces.n) / na).max(forces.mz.abs() / ma_z);

        assert!(
            (r.ratio() - expected).abs() < 1e-9,
            "RcColumnRect 柱の検定比が axis_props_from_shape 経由と一致しない: {} vs {}",
            r.ratio(),
            expected
        );
    }

    /// 新 `RcColumnRect` の許容 N-M 用 at_perp は、全主筋 ag から検討方向の
    /// 引張側・圧縮側最外段 at・ac を除いた残りであり、交点筋を二重計上しない。
    #[test]
    fn test_rc_column_rect_at_perp_excludes_intersection_bars() {
        let shape = rc_column_rect_shape();
        let rebar = match &shape {
            SectionShape::RcColumnRect { rebar, .. } => rebar,
            _ => unreachable!(),
        };
        let ag = rebar.total_main_area();
        let props_z = axis_props_from_shape(&shape, RcDirection::Strong, true).unwrap();
        let props_y = axis_props_from_shape(&shape, RcDirection::Weak, true).unwrap();

        let at_perp_z = rect_column_at_perp(ag, props_z.at, props_z.ac);
        let at_perp_y = rect_column_at_perp(ag, props_y.at, props_y.ac);

        assert!(
            (props_z.at + props_z.ac + at_perp_z - ag).abs() < 1e-9,
            "強軸: at+ac+at_perp が全主筋 ag に一致しない: {} + {} + {} != {}",
            props_z.at,
            props_z.ac,
            at_perp_z,
            ag
        );
        assert!(
            (props_y.at + props_y.ac + at_perp_y - ag).abs() < 1e-9,
            "弱軸: at+ac+at_perp が全主筋 ag に一致しない: {} + {} + {} != {}",
            props_y.at,
            props_y.ac,
            at_perp_y,
            ag
        );
        assert!(
            (at_perp_z - rebar.y_direction_area_mm2()).abs() > 1e-9,
            "強軸 at_perp が Y 方向全筋（交点筋を二重計上）になっている: {}",
            at_perp_z
        );
        assert!(
            (at_perp_y - rebar.x_direction_area_mm2()).abs() > 1e-9,
            "弱軸 at_perp が X 方向全筋（交点筋を二重計上）になっている: {}",
            at_perp_y
        );
    }
}
