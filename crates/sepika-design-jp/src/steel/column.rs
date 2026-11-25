//! 鉄骨造柱の許容応力度検定。

use crate::material_strength::{steel_fc, steel_fs, steel_ft};
use crate::{
    CheckComponent, CheckKind, CheckOutcome, CheckResult, DesignCtx, LoadTerm, MemberForcesAt,
};
use sepika_core::material_grade::resolve_steel_f;
use sepika_core::model::{Material, PropertyBasis, Section};
use sepika_core::section_shape::SectionShape;

use super::section::{
    built_h_strong_axis_moduli, steel_c_factor, steel_fb_h_asd2019, steel_p_lambda_b,
    steel_warping_constant,
};
use super::{
    has_section_loss, nonzero, safe_denom, section_modulus, shape_of, shear_component,
    strong_axis_h_tension, strong_axis_tension_bending_ratios, ShapeCategory,
};

pub(super) fn asymmetric_built_h_section_reason(
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
) -> Option<&'static str> {
    if ![forces.n, forces.mz, forces.my, forces.qy, forces.qz]
        .iter()
        .all(|value| value.is_finite())
    {
        return Some("断面力が非有限です");
    }
    if has_section_loss(ctx) {
        return Some("断面欠損を控除した断面係数の適用条件が未確定です");
    }
    let basis = sec.property_basis;
    [basis.area, basis.iy, basis.iz]
        .iter()
        .any(|&property| property != PropertyBasis::Shape)
        .then_some("図心と断面性能の対応を確定するには、形状から求めた断面積と両軸の断面二次モーメントが必要です")
}

fn has_nonzero_stability_length(ctx: &DesignCtx) -> bool {
    [
        ctx.length,
        ctx.lk_y.unwrap_or(0.0),
        ctx.lk_z.unwrap_or(0.0),
        ctx.lb.unwrap_or(0.0),
    ]
    .iter()
    .any(|&length| length != 0.0)
        || ctx.steel_attr.as_ref().is_some_and(|attr| {
            let (i, mid, j) = attr.lb_direct.unwrap_or_default();
            [
                i,
                mid,
                j,
                attr.lk_y_direct.unwrap_or(0.0),
                attr.lk_z_direct.unwrap_or(0.0),
            ]
            .iter()
            .any(|&length| length != 0.0)
        })
}

pub(super) enum ColumnLateralBuckling {
    OutsideScope,
    NoStrongAxisBending,
    Confirmed(f64),
    Unconfirmed(&'static str),
}

impl ColumnLateralBuckling {
    pub(super) fn resolve(ctx: &DesignCtx, sec: &Section, forces: &MemberForcesAt) -> Self {
        if ctx.kind != crate::MemberKind::Column || !super::is_symmetric_h_section(sec) {
            return Self::OutsideScope;
        }
        if forces.mz == 0.0 {
            return Self::NoStrongAxisBending;
        }
        match ctx.column_lateral_buckling_length {
            Some(Ok(length_mm)) if length_mm.is_finite() && length_mm > 0.0 => {
                Self::Confirmed(length_mm)
            }
            Some(Ok(_)) => {
                Self::Unconfirmed("実材軸長を有限な正値で取得できないため、横座屈長さは未算定です")
            }
            Some(Err(reason)) => Self::Unconfirmed(reason),
            None => {
                let span_mm = ctx.node_span_mm.unwrap_or(ctx.length);
                if span_mm.is_finite() && span_mm > 0.0 {
                    Self::Confirmed(span_mm)
                } else {
                    Self::Unconfirmed(super::INVALID_NODE_SPAN_FOR_LATERAL_BUCKLING)
                }
            }
        }
    }

    pub(super) fn length_mm(&self, physical_length_mm: f64) -> Option<f64> {
        match *self {
            Self::OutsideScope => Some(physical_length_mm),
            Self::Confirmed(length_mm) => Some(length_mm),
            Self::NoStrongAxisBending | Self::Unconfirmed(_) => None,
        }
    }
}

pub(super) fn asymmetric_built_h_combination_reasons(
    forces: &MemberForcesAt,
    ctx: &DesignCtx,
) -> impl Iterator<Item = &'static str> {
    [
        has_nonzero_stability_length(ctx).then_some("上下非対称断面の横座屈・曲げねじり座屈の算定条件が未確定です"),
        (forces.my != 0.0).then_some("弱軸曲げMyが同時に作用する場合の上下非対称断面の二軸曲げ照査は未算定です"),
        (forces.qy != 0.0).then_some("強軸せん断Qyが同時に作用する場合の上下非対称断面のウェブ内同一点の組合せ応力度は未算定です"),
    ]
    .into_iter()
    .flatten()
}

pub(super) fn check_symmetric_h_plate_slenderness(
    forces: &MemberForcesAt,
    sec: &Section,
    mat: &Material,
) -> Result<(), String> {
    let shape = sec.shape.as_ref().expect("上下対称H形鋼の形状が必要");
    let (height_mm, flange_width_mm, flange_thick_mm, web_thick_mm, root_r_mm) = match *shape {
        SectionShape::SteelH {
            height,
            width,
            flange_thick,
            web_thick,
            root_r,
        } => (height, width, flange_thick, web_thick, root_r),
        SectionShape::SteelBuiltH {
            height,
            upper_width,
            upper_thick,
            web_thick,
            ..
        } => (height, upper_width, upper_thick, web_thick, Some(0.0)),
        _ => unreachable!("上下対称H形鋼だけを検定する"),
    };
    let web_flat_mm = height_mm - 2.0 * flange_thick_mm - 2.0 * root_r_mm.unwrap_or(0.0);
    if sec.depth != height_mm
        || flange_width_mm < web_thick_mm
        || [web_flat_mm, flange_width_mm, flange_thick_mm, web_thick_mm]
            .iter()
            .any(|&v| !v.is_finite() || v <= 0.0)
        || root_r_mm.is_some_and(|r| r < 0.0)
    {
        return Err("局部座屈の幅厚比照査に必要なH形断面の寸法・全断面性能を確定できません".into());
    }

    let (compressed_flange, compressed_web) = if forces.n > 0.0 {
        tension_compressed_plates(forces, sec, shape, height_mm, web_flat_mm)?
    } else {
        (true, true)
    };

    let flange = if forces.n < 0.0 {
        "上下フランジ b/t"
    } else if forces.mz > 0.0 {
        "上フランジ b/t"
    } else {
        "下フランジ b/t"
    };
    let web_limit_coefficient = |f_mpa: f64| {
        Ok(if forces.n == 0.0 {
            2.4
        } else if forces.n < 0.0 && forces.mz != 0.0 && root_r_mm.is_some() {
            let web_area_mm2 = web_flat_mm * web_thick_mm;
            if !web_area_mm2.is_finite() || web_area_mm2 <= 0.0 {
                return Err("局部座屈の幅厚比照査に必要なウェブ平板部の面積を確定できません");
            }
            let compression_ratio = -forces.n / web_area_mm2 / f_mpa;
            (2.4 - 0.8 * compression_ratio).max(1.6)
        } else {
            1.6
        })
    };
    let mut reasons = Vec::new();
    for (label, width_mm, thick_mm, is_flange, compressed) in [
        (
            flange,
            flange_width_mm / 2.0,
            flange_thick_mm,
            true,
            compressed_flange,
        ),
        (
            "ウェブ d/t",
            web_flat_mm,
            web_thick_mm,
            false,
            compressed_web,
        ),
    ] {
        if !compressed {
            continue;
        }
        let f_mpa = resolve_steel_f(Some(mat), Some(thick_mm))
            .map_err(|reason| format!("{label} の幅厚比の限界値を算定できません。{reason}"))?;
        let coefficient = if is_flange {
            0.53
        } else {
            web_limit_coefficient(f_mpa)?
        };
        let limit = coefficient * (mat.young / f_mpa).sqrt();
        let ratio = width_mm / thick_mm;
        if !ratio.is_finite() || !limit.is_finite() || limit <= 0.0 {
            return Err(format!(
                "{label} の幅厚比または限界値を有限値で算定できません"
            ));
        }
        if ratio > limit {
            reasons.push(format!("{label}={ratio:.4}（上限 {limit:.4}）"));
        }
    }
    if reasons.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "局部座屈の影響を省ける幅厚比の条件を満たさないため、軸曲げ検定は未算定です。{}",
            reasons.join("、"),
        ))
    }
}

fn tension_compressed_plates(
    forces: &MemberForcesAt,
    sec: &Section,
    shape: &SectionShape,
    height_mm: f64,
    web_flat_mm: f64,
) -> Result<(bool, bool), String> {
    let unresolved = "局部座屈の幅厚比照査に必要なH形断面の全断面性能を確定できません";
    let area_mm2 = sec
        .resolved_area()
        .map_err(|reason| format!("{unresolved}。{reason}"))?;
    if [sec.property_basis.iy, sec.property_basis.depth].contains(&PropertyBasis::PendingShape)
        || [area_mm2, sec.iy]
            .iter()
            .any(|&v| !v.is_finite() || v <= 0.0)
    {
        return Err(unresolved.into());
    }

    if sec.property_basis.iy == PropertyBasis::Shape {
        let calculated_iy_mm4 = shape
            .try_calc_iy()
            .map_err(|reason| format!("{unresolved}。{reason}"))?;
        let tolerance_mm4 = 8.0 * f64::EPSILON * sec.iy.max(calculated_iy_mm4);
        if !calculated_iy_mm4.is_finite()
            || calculated_iy_mm4 <= 0.0
            || (sec.iy - calculated_iy_mm4).abs() > tolerance_mm4
        {
            return Err(format!(
                "{unresolved}。強軸断面二次モーメントの記録値が現在の断面形状と一致しません"
            ));
        }
    }

    let tension_mpa = forces.n / area_mm2;
    let gradient_mpa_per_mm = forces.mz.abs() / sec.iy;
    let flange_bending_mpa = gradient_mpa_per_mm * (height_mm / 2.0);
    let web_bending_mpa = gradient_mpa_per_mm * (web_flat_mm / 2.0);
    if [tension_mpa, flange_bending_mpa, web_bending_mpa]
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err("局部座屈の幅厚比照査に必要な合成応力度を有限値で算定できません".into());
    }
    Ok((
        flange_bending_mpa > tension_mpa,
        web_bending_mpa > tension_mpa,
    ))
}

fn validate_built_h_plate_slenderness(
    shape: &SectionShape,
    mat: &Material,
    compressed_flanges: [bool; 2],
    compressed_web: bool,
) -> Result<(), String> {
    let SectionShape::SteelBuiltH {
        height,
        upper_width,
        upper_thick,
        lower_width,
        lower_thick,
        web_thick,
    } = *shape
    else {
        unreachable!("上下非対称組立H形鋼だけを検定する");
    };
    if compressed_web {
        let web_f_mpa = resolve_steel_f(Some(mat), Some(web_thick))?;
        let web_limit = 1.6 * (mat.young / web_f_mpa).sqrt();
        if !web_limit.is_finite() || web_limit <= 0.0 {
            return Err("ヤング係数から幅厚比の限界値を算定できません".into());
        }
        let web_ratio = (height - upper_thick - lower_thick) / web_thick;
        if web_ratio > web_limit {
            return Err(format!(
                "局部座屈を省く幅厚比の条件を満たしません。ウェブ d/t={web_ratio:.4}（上限 {web_limit:.4}）"
            ));
        }
    }

    for (side, width_mm, thick_mm, compressed) in [
        ("上", upper_width, upper_thick, compressed_flanges[0]),
        ("下", lower_width, lower_thick, compressed_flanges[1]),
    ] {
        if !compressed {
            continue;
        }
        let flange_f_mpa = resolve_steel_f(Some(mat), Some(thick_mm))?;
        let flange_limit = 0.53 * (mat.young / flange_f_mpa).sqrt();
        if !flange_limit.is_finite() || flange_limit <= 0.0 {
            return Err("ヤング係数から幅厚比の限界値を算定できません".into());
        }
        let flange_ratio = width_mm / 2.0 / thick_mm;
        if flange_ratio > flange_limit {
            return Err(format!(
                "局部座屈を省く幅厚比の条件を満たしません。{side}フランジ b/t={flange_ratio:.4}（上限 {flange_limit:.4}）"
            ));
        }
    }
    Ok(())
}

fn compressed_built_h_plates(
    shape: &SectionShape,
    mz_nmm: f64,
    sigma_c_mpa: f64,
    sigma_b_outer_mpa: [f64; 2],
) -> Result<([bool; 2], bool), String> {
    let SectionShape::SteelBuiltH {
        upper_thick,
        lower_thick,
        ..
    } = *shape
    else {
        unreachable!("上下非対称組立H形鋼だけを検定する");
    };
    let stress_gradient_mpa_per_mm = mz_nmm / shape.try_calc_iy()?;
    let [upper_outer_mpa, lower_outer_mpa] =
        sigma_b_outer_mpa.map(|stress_mpa| stress_mpa - sigma_c_mpa);
    let upper_inner_mpa = upper_outer_mpa + stress_gradient_mpa_per_mm * upper_thick;
    let lower_inner_mpa = lower_outer_mpa - stress_gradient_mpa_per_mm * lower_thick;
    if ![
        upper_outer_mpa,
        lower_outer_mpa,
        upper_inner_mpa,
        lower_inner_mpa,
    ]
    .iter()
    .all(|stress| stress.is_finite())
    {
        return Err("フランジの内縁・外縁とウェブ両端の合成応力度を有限値で算定できません".into());
    }
    let compressed_flanges = [
        upper_outer_mpa.min(upper_inner_mpa) < 0.0,
        lower_outer_mpa.min(lower_inner_mpa) < 0.0,
    ];
    let compressed_web = upper_inner_mpa.min(lower_inner_mpa) < 0.0;
    Ok((compressed_flanges, compressed_web))
}

pub(super) struct AsymmetricColumnSectionStress {
    area_mm2: f64,
    z_upper_mm3: f64,
    z_lower_mm3: f64,
    ft_mpa: f64,
    sigma_c_mpa: f64,
    sigma_b_upper_mpa: f64,
    sigma_b_lower_mpa: f64,
    compression_side: &'static str,
    tension_side: &'static str,
    compression_ratio: f64,
    tension_ratio: f64,
    pub(super) ratio: f64,
    pub(super) unavailable: Vec<&'static str>,
}

pub(super) fn asymmetric_built_h_section_stress(
    forces: &MemberForcesAt,
    sec: &Section,
    shape: &SectionShape,
    mat: &Material,
    ctx: &DesignCtx,
    f: f64,
    shear_reasons: [Option<&'static str>; 3],
) -> Result<AsymmetricColumnSectionStress, String> {
    let check = || -> Result<AsymmetricColumnSectionStress, String> {
        if let Some(reason) = asymmetric_built_h_section_reason(forces, sec, ctx) {
            return Err(reason.into());
        }
        let pure_bending = forces.n == 0.0;
        let with_combination_reasons = |mut reason: String| {
            for combination_reason in asymmetric_built_h_combination_reasons(forces, ctx) {
                reason.push('。');
                reason.push_str(combination_reason);
            }
            reason
        };
        if pure_bending {
            let compressed_flanges = [
                forces.my != 0.0 || forces.mz > 0.0,
                forces.my != 0.0 || forces.mz < 0.0,
            ];
            validate_built_h_plate_slenderness(shape, mat, compressed_flanges, true)
                .map_err(with_combination_reasons)?;
        }
        let (z_upper_mm3, z_lower_mm3) = built_h_strong_axis_moduli(shape)?;
        let area_mm2 = shape.try_calc_area()?;
        let ft_mpa = steel_ft(f, ctx.term);
        if ![area_mm2, z_upper_mm3, z_lower_mm3, ft_mpa]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
        {
            return Err("全断面積・上下の断面係数・許容応力度を有限な正値で算定できません".into());
        }

        let sigma_c_mpa = if pure_bending {
            0.0
        } else {
            -forces.n / area_mm2
        };
        let sigma_b_upper_mpa = -forces.mz / z_upper_mm3;
        let sigma_b_lower_mpa = forces.mz / z_lower_mm3;
        if !pure_bending && !has_nonzero_stability_length(ctx) {
            let (compressed_flanges, compressed_web) = if forces.my != 0.0 {
                ([true; 2], true)
            } else {
                compressed_built_h_plates(
                    shape,
                    forces.mz,
                    sigma_c_mpa,
                    [sigma_b_upper_mpa, sigma_b_lower_mpa],
                )
                .map_err(with_combination_reasons)?
            };
            validate_built_h_plate_slenderness(
                shape,
                mat,
                compressed_flanges,
                compressed_web || forces.qy != 0.0 || forces.qz != 0.0,
            )
            .map_err(with_combination_reasons)?;
        }
        let (compression_side, tension_side, sigma_bc_mpa, sigma_bt_mpa) = if forces.mz > 0.0 {
            ("上", "下", -sigma_b_upper_mpa, sigma_b_lower_mpa)
        } else {
            ("下", "上", -sigma_b_lower_mpa, sigma_b_upper_mpa)
        };
        // 有限長に対称断面の fc・fb を流用すると適合を誤判定しうるため、各許容値の上限で不適合だけを確定する。
        let compression_ratio = sigma_c_mpa / ft_mpa + sigma_bc_mpa / ft_mpa;
        let tension_ratio = (sigma_bt_mpa - sigma_c_mpa) / ft_mpa;
        if !compression_ratio.is_finite() || !tension_ratio.is_finite() {
            return Err("上下縁の応力度比を有限値で算定できません".into());
        }
        let ratio = compression_ratio.max(tension_ratio);

        let unavailable = asymmetric_built_h_combination_reasons(forces, ctx)
            .chain(shear_reasons.into_iter().flatten())
            .collect::<Vec<_>>();
        Ok(AsymmetricColumnSectionStress {
            area_mm2,
            z_upper_mm3,
            z_lower_mm3,
            ft_mpa,
            sigma_c_mpa,
            sigma_b_upper_mpa,
            sigma_b_lower_mpa,
            compression_side,
            tension_side,
            compression_ratio,
            tension_ratio,
            ratio,
            unavailable,
        })
    };
    check().map_err(|reason| {
        let mut reason = format!("上下非対称組立H形鋼柱は未算定です。{reason}");
        for shear_reason in shear_reasons.into_iter().flatten() {
            reason.push('。');
            reason.push_str(shear_reason);
        }
        reason
    })
}

pub(super) fn check_asymmetric_built_h(
    forces: &MemberForcesAt,
    sec: &Section,
    shape: &SectionShape,
    mat: &Material,
    ctx: &DesignCtx,
    f: f64,
    shear_reasons: [Option<&'static str>; 3],
) -> CheckOutcome {
    let AsymmetricColumnSectionStress {
        area_mm2,
        z_upper_mm3,
        z_lower_mm3,
        ft_mpa,
        sigma_c_mpa,
        sigma_b_upper_mpa,
        sigma_b_lower_mpa,
        compression_side,
        tension_side,
        compression_ratio,
        tension_ratio,
        ratio,
        unavailable,
    } = match asymmetric_built_h_section_stress(forces, sec, shape, mat, ctx, f, shear_reasons) {
        Ok(stress) => stress,
        Err(reason) => return CheckOutcome::Skipped { reason },
    };
    if !unavailable.is_empty() && ratio <= 1.0 {
        return CheckOutcome::Skipped {
            reason: unavailable.join("。"),
        };
    }
    let pure_bending = forces.n == 0.0;
    let term = match ctx.term {
        LoadTerm::Long => "長期",
        LoadTerm::Short => "短期",
    };
    let (check, kind, ratio_label) = if pure_bending {
        ("強軸曲げ", CheckKind::Bending, "曲げ比")
    } else {
        ("式(6.1)・(6.2)", CheckKind::AxialBending, "軸曲げ比")
    };
    let mut basis = format!("鋼構造許容応力度設計規準 {term} 上下非対称組立H形鋼柱 {check}");
    let (fb_label, ratio_suffix) = if !unavailable.is_empty() {
        basis.push_str("（上下縁の応力度照査による不適合）。");
        basis.push_str(&unavailable.join("。"));
        ("fb上限", "の下限")
    } else {
        basis.push_str("（座屈長さ0の断面照査）");
        ("fb", "")
    };
    CheckOutcome::Checked(CheckResult {
        basis,
        detail: String::new(),
        components: vec![CheckComponent {
            kind,
            ratio,
            detail: format!(
                "A={area_mm2:.4} mm², Z上={z_upper_mm3:.4} mm³, Z下={z_lower_mm3:.4} mm³, \
                 σc={sigma_c_mpa:.4} N/mm², 曲げの圧縮側={compression_side}、引張側={tension_side}, \
                 σb上={sigma_b_upper_mpa:.4} N/mm², σb下={sigma_b_lower_mpa:.4} N/mm², \
                 {fb_label}={ft_mpa:.4} N/mm², ft={ft_mpa:.4} N/mm², \
                 {ratio_label}{ratio_suffix}（圧縮側）={compression_ratio:.4}, \
                 {ratio_label}{ratio_suffix}（引張側）={tension_ratio:.4}"
            ),
        }],
    })
}

/// 鉄骨造柱の断面検定。
///
pub(super) fn check_column(
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
    a: ColumnAllowables,
    lambda: f64,
    lateral_buckling: &ColumnLateralBuckling,
) -> CheckOutcome {
    let (shape, _, _) = shape_of(sec);
    let ColumnAllowables {
        area,
        z_strong,
        z_weak,
        ft: ft_val,
        fc: fc_val,
        fb_strong,
        fb_weak,
        fs: fs_val,
    } = a;
    let sigma_bx = forces.mz.abs() / z_strong;
    let sigma_by = forces.my.abs() / z_weak;
    let buckling_note =
        if ctx.lk_y.unwrap_or(ctx.length) == 0.0 && ctx.lk_z.unwrap_or(ctx.length) == 0.0 {
            "（座屈長さ0のため座屈無視 λ=0）"
        } else {
            ""
        };

    let sigma_b_pipe = (forces.mz.powi(2) + forces.my.powi(2)).sqrt() / z_strong;
    let sigma_t = forces.n / area;
    let tension_bending = strong_axis_h_tension(forces, sec)
        .then(|| strong_axis_tension_bending_ratios(sigma_t, sigma_bx, ft_val, fb_strong));

    let (axial_stress, ratio_axial_bend, axial_basis) = if forces.n < 0.0 {
        let sigma_c = forces.n.abs() / area;
        let ratio = match shape {
            ShapeCategory::Pipe => {
                sigma_c / safe_denom(fc_val) + sigma_b_pipe / safe_denom(fb_strong)
            }
            _ => {
                sigma_c / safe_denom(fc_val)
                    + sigma_bx / safe_denom(fb_strong)
                    + sigma_by / safe_denom(fb_weak)
            }
        };
        (sigma_c, ratio, "圧縮+曲げ: σc/fc(座屈考慮)+ΣσB/fb")
    } else if let Some(ratios) = &tension_bending {
        (
            sigma_t,
            ratios.governing(),
            if forces.mz == 0.0 {
                "引張: σt/ft"
            } else {
                "引張+強軸曲げ: 式(6.3)・(6.4)"
            },
        )
    } else {
        let ratio = match shape {
            ShapeCategory::Pipe => {
                sigma_t / safe_denom(ft_val) + sigma_b_pipe / safe_denom(fb_strong)
            }
            _ => {
                sigma_t / safe_denom(ft_val)
                    + sigma_bx / safe_denom(fb_strong)
                    + sigma_by / safe_denom(fb_weak)
            }
        };
        (sigma_t, ratio, "引張+曲げ: σt/ft+ΣσB/fb")
    };

    let term_label = match ctx.term {
        LoadTerm::Long => "長期",
        LoadTerm::Short => "短期",
    };
    let basis = format!("鋼構造許容応力度設計規準 {term_label} 柱: {axial_basis}{buckling_note}");
    let (fb_strong_detail, ratio_label) = match lateral_buckling {
        ColumnLateralBuckling::Unconfirmed(_) => {
            (format!("fbX上限={fb_strong:.4} N/mm²"), "軸曲げ比の下限")
        }
        ColumnLateralBuckling::NoStrongAxisBending => {
            ("fbX=対象外（強軸曲げなし）".into(), "軸曲げ比")
        }
        _ => (format!("fbX={fb_strong:.4} N/mm²"), "軸曲げ比"),
    };
    let lateral_detail = match *lateral_buckling {
        ColumnLateralBuckling::Confirmed(length_mm) => {
            let mut detail = format!("採用横座屈長さ lb={length_mm} mm");
            if ctx
                .steel_attr
                .as_ref()
                .is_some_and(|a| a.column_lateral_buckling_end_supports.is_some())
            {
                detail.push_str("（両実材端で横移動・ねじれを拘束し、中間横補剛なしを確認）");
            } else if ctx.node_span_mm == Some(length_mm) {
                detail.push_str("（節点間距離の既定値）");
            }
            if ctx
                .steel_attr
                .as_ref()
                .is_some_and(|a| a.lb_direct.is_some())
            {
                detail.push_str("。梁用のlb直接入力は不使用");
            }
            detail
        }
        _ => String::new(),
    };
    let axial_bending = CheckComponent {
        kind: CheckKind::AxialBending,
        ratio: ratio_axial_bend,
        detail: if let Some(ratios) = tension_bending {
            if forces.mz == 0.0 {
                format!(
                    "A_N={area:.4} mm², σt={sigma_t:.4} N/mm², ft={ft_val:.4} N/mm², \
                     {ratio_label}={ratio_axial_bend:.4}"
                )
            } else {
                let compression_detail = match lateral_buckling {
                    ColumnLateralBuckling::Unconfirmed(_) => "未算定".to_owned(),
                    _ => format!("{:.4}", ratios.compression),
                };
                format!(
                    "A_N={area:.4} mm², Zt=Zc={z_strong:.4} mm³, σt={sigma_t:.4} N/mm², \
                     σbt=σbc={sigma_bx:.4} N/mm², ft={ft_val:.4} N/mm², {fb_strong_detail}, \
                     式(6.3) 引張側={:.4}, 式(6.4) 曲げ圧縮側={compression_detail}, \
                     {ratio_label}={ratio_axial_bend:.4}",
                    ratios.tension,
                )
            }
        } else {
            format!(
                "σax={:.4} N/mm², σbX={:.4} N/mm², σbY={:.4} N/mm², fc={:.4} N/mm², {fb_strong_detail}, \
fbY={:.4} N/mm², λ={:.3}, {ratio_label}={:.4}",
                axial_stress, sigma_bx, sigma_by, fc_val, fb_weak, lambda, ratio_axial_bend,
            )
        },
    };
    let mut components = vec![axial_bending];
    components.extend(shear_component(forces, sec, ctx, fs_val));
    CheckOutcome::Checked(CheckResult {
        basis,
        detail: lateral_detail,
        components,
    })
}

#[derive(Clone, Copy)]
pub(crate) struct ColumnAllowables {
    pub area: f64,
    pub z_strong: f64,
    pub z_weak: f64,
    pub ft: f64,
    pub fc: f64,
    pub fb_strong: f64,
    pub fb_weak: f64,
    pub fs: f64,
}

pub(crate) fn column_allowables(
    sec: &Section,
    mat: &Material,
    ctx: &DesignCtx,
    f: f64,
    checked_slenderness: Option<f64>,
    lb_mm: Option<f64>,
) -> Result<ColumnAllowables, String> {
    let term = ctx.term;
    let h = sec.depth;
    let b = sec.width;
    let area = nonzero(sec.area);
    let z_strong = nonzero(section_modulus(sec.iy, h / 2.0));
    let z_weak = nonzero(section_modulus(sec.iz, b / 2.0));

    let (shape, tf, _) = shape_of(sec);

    let ft_val = steel_ft(f, term);
    let fs_val = steel_fs(f, term);

    let lambda = checked_slenderness.unwrap_or_else(|| {
        crate::effective_slenderness(sec.iy, sec.iz, area, ctx.length, ctx.lk_y, ctx.lk_z)
    });
    let fc_val = steel_fc(f, mat.young, lambda, term);

    let c = steel_c_factor(ctx, false);
    let fb_strong = match (shape, lb_mm) {
        (ShapeCategory::H, Some(lb_mm)) if lb_mm > 1e-9 => {
            let iz = sec.iz;
            let iw = steel_warping_constant(sec, tf);
            let j = sec.resolved_j()?;
            let e = mat.young;
            let g = mat.shear.unwrap_or(e / (2.0 * (1.0 + mat.poisson)));
            let p_lambda_b = steel_p_lambda_b(ctx, false);
            steel_fb_h_asd2019(f, term, lb_mm, iz, iw, j, e, g, z_strong, c, p_lambda_b)
        }
        _ => ft_val,
    };
    let fb_weak = ft_val;

    Ok(ColumnAllowables {
        area,
        z_strong,
        z_weak,
        ft: ft_val,
        fc: fc_val,
        fb_strong,
        fb_weak,
        fs: fs_val,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steel::test_support::{h_section, mat, rect_section};
    use crate::steel::SteelDesign;
    use crate::{DesignCheck, MemberKind};

    /// 断片が意図した component に配置されていることの確認
    /// （AxialBending の detail に "軸曲げ比" が含まれ、Shear の detail には
    /// 含まれない。逆に Shear 固有の "せん断比" は AxialBending に含まれない）。
    #[test]
    fn test_column_check_detail_fragments_assigned_to_intended_components() {
        let sec = h_section(300.0, 300.0, 10.0, 15.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -500_000.0,
            qy: 20_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 20e6,
            mz: 50e6,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3500.0,
            lk_y: Some(3500.0),
            lk_z: Some(3500.0),
            column_lateral_buckling_length: Some(Ok(3500.0)),
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();
        let axial_bending = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::AxialBending)
            .expect("AxialBending component が存在するはず");
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Shear)
            .expect("Shear component が存在するはず");
        assert!(axial_bending.detail.contains("軸曲げ比"));
        assert!(!shear.detail.contains("軸曲げ比"));
        assert!(shear.detail.contains("せん断比"));
        assert!(!axial_bending.detail.contains("せん断比"));
    }

    #[test]
    fn test_column_check_pipe_combines_biaxial_sigma_b() {
        let mut sec = rect_section(300.0, 300.0, "PIPE-300x12");
        sec.iz = sec.iy;
        sec.thickness = Some(12.0);
        let mat_v = mat("SN400");
        let forces_x_only = MemberForcesAt {
            pos: 0.0,
            n: -100_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30e6,
        };
        let forces_biaxial = MemberForcesAt {
            pos: 0.0,
            n: -100_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 30e6 / std::f64::consts::SQRT_2,
            mz: 30e6 / std::f64::consts::SQRT_2,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3000.0,
            lk_y: Some(3000.0),
            lk_z: Some(3000.0),
            ..Default::default()
        };
        let r1 = SteelDesign
            .check(&forces_x_only, &sec, &mat_v, &ctx)
            .unwrap_checked();
        let r2 = SteelDesign
            .check(&forces_biaxial, &sec, &mat_v, &ctx)
            .unwrap_checked();
        assert!(
            (r1.ratio() - r2.ratio()).abs() < 1e-6,
            "pipe combined sigma_b mismatch: {} vs {}",
            r1.ratio(),
            r2.ratio()
        );
    }

    #[test]
    fn test_column_length_zero_ignores_buckling() {
        let mut sec = rect_section(300.0, 300.0, "一般断面");
        sec.thickness = Some(20.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -100_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 0.0,
            lk_y: Some(0.0),
            lk_z: Some(0.0),
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();
        assert!(result.basis.contains("座屈無視"));
        // λ=0 なので fc=ft、単純圧縮比 = σc/ft と一致するはず。
        let ft = steel_ft(235.0, LoadTerm::Long);
        let expected = (100_000.0 / sec.area) / ft;
        assert!((result.ratio() - expected).abs() < 1e-6);
    }

    /// 両軸に同じ座屈長さを入力した場合、λ=max(lk_y/i_y, lk_z/i_z) が
    /// λ=length/i_min（i_min=√(min(Iy,Iz)/A)）と一致することを、
    /// fc（λ に応じて単調減少）を介して確認する。
    #[test]
    fn test_column_equal_buckling_lengths_use_the_smaller_radius() {
        let mut sec = rect_section(300.0, 200.0, "H-300x200x8x12");
        sec.thickness = Some(12.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -100_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 4000.0,
            lk_y: Some(4000.0),
            lk_z: Some(4000.0),
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let area = sec.area;
        let i_min = (sec.iy.min(sec.iz) / area).sqrt();
        let lambda = 4000.0 / i_min;
        let fc = steel_fc(235.0, mat_v.young, lambda, LoadTerm::Long);
        let expected = (100_000.0 / area) / fc;
        assert!(
            (result.ratio() - expected).abs() < 1e-9,
            "ratio={} expected(i_min)={}",
            result.ratio(),
            expected
        );
    }

    /// lk_z（弱軸）のみを長くすると弱軸細長比が支配し fc が下がる（検定比が
    /// 上がる）ことを確認する。矩形断面 B=300 > D=100 とし、iz（弱軸=B方向）
    /// を十分小さくして弱軸が支配しやすい断面とする。
    #[test]
    fn test_column_lk_z_only_governs_weak_axis_slenderness() {
        let mut sec = rect_section(100.0, 300.0, "H-300x100x8x12");
        sec.thickness = Some(12.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -50_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx_base = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3000.0,
            lk_y: Some(3000.0),
            lk_z: Some(3000.0),
            ..Default::default()
        };
        let base = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx_base)
            .unwrap_checked();

        let ctx_long_z = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3000.0,
            lk_y: Some(3000.0),
            lk_z: Some(9000.0),
            ..Default::default()
        };
        let long_z = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx_long_z)
            .unwrap_checked();

        assert!(
            long_z.ratio() > base.ratio(),
            "lk_z を伸ばすと検定比が上がるはず: base={} long_z={}",
            base.ratio(),
            long_z.ratio()
        );
    }

    /// AIJ-ASD19 fb: H形柱の fbX がその式の
    /// 手計算と一致することを確認する。
    #[test]
    fn test_column_check_fb_basis_asd2019_matches_hand_calc() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -50_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3500.0,
            lk_y: Some(3500.0),
            lk_z: Some(3500.0),
            column_lateral_buckling_length: Some(Ok(3500.0)),
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let f = 235.0;
        let z_strong = sec.iy / (sec.depth / 2.0);
        let iw = steel_warping_constant(&sec, 13.0);
        let e = mat_v.young;
        let g = e / (2.0 * (1.0 + mat_v.poisson));
        let p_lambda_b = steel_p_lambda_b(&ctx, false);
        let c = steel_c_factor(&ctx, false);
        let fb_expected = steel_fb_h_asd2019(
            f,
            LoadTerm::Long,
            3500.0,
            sec.iz,
            iw,
            sec.j,
            e,
            g,
            z_strong,
            c,
            p_lambda_b,
        );
        assert!(
            crate::full_detail(&result).contains(&format!("fbX={:.4}", fb_expected)),
            "detail={}",
            crate::full_detail(&result)
        );
    }
}
