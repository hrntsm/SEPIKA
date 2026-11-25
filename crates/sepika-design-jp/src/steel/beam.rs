//! 鉄骨造梁の許容応力度検定。
//!
//! 検定比には含まれない参考情報として、大梁の必要横補剛数とたわみ
//! （長期のみ）も併せて算定する。

use crate::material_strength::{steel_fc, steel_fs, steel_ft};
use crate::{
    CheckComponent, CheckKind, CheckOutcome, CheckResult, DesignCtx, LoadTerm, MemberForcesAt,
};
use sepika_core::material_grade::resolve_steel_f;
use sepika_core::model::{Material, PropertyBasis, Section};
use sepika_core::section_shape::SectionShape;

use super::section::{
    built_h_strong_axis_moduli, resolve_lb, steel_beam_lateral_parameters, steel_c_factor,
    steel_fb_h_asd2019, steel_h_z_with_loss, steel_warping_constant,
};
use super::{
    has_section_loss, nonzero, safe_denom, section_modulus, shape_of, shear_component,
    strong_axis_h_tension, strong_axis_tension_bending_ratios, ShapeCategory,
};

pub(super) fn asymmetric_built_h_combination_reasons(
    forces: &MemberForcesAt,
) -> impl Iterator<Item = &'static str> {
    [
        (forces.n != 0.0).then_some("軸力Nと強軸曲げMzの組合せは未算定です"),
        (forces.my != 0.0).then_some("弱軸曲げMyと強軸曲げMzの組合せは未算定です"),
        (forces.qy != 0.0).then_some("強軸せん断Qyと強軸曲げMzの同一点の組合せ応力度は未算定です"),
        (forces.qz != 0.0).then_some("弱軸せん断Qzと強軸曲げMzの同一点の組合せ応力度は未算定です"),
    ]
    .into_iter()
    .flatten()
}

fn check_built_h_girder_plate_slenderness(
    shape: &SectionShape,
    mat: &Material,
    upper_in_compression: bool,
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
        unreachable!("上下非対称組立H形鋼大梁だけを検定する");
    };
    if !mat.young.is_finite() || mat.young <= 0.0 {
        return Err("ヤング係数が正の有限値でなく、幅厚比の限界値を算定できません".into());
    }
    let (flange, flange_half_width_mm, flange_thick_mm) = if upper_in_compression {
        ("上フランジ b/t", upper_width / 2.0, upper_thick)
    } else {
        ("下フランジ b/t", lower_width / 2.0, lower_thick)
    };
    for (label, width_mm, thickness_mm, coefficient) in [
        (flange, flange_half_width_mm, flange_thick_mm, 0.53),
        (
            "ウェブ d/t",
            height - upper_thick - lower_thick,
            web_thick,
            2.4,
        ),
    ] {
        let f_mpa = resolve_steel_f(Some(mat), Some(thickness_mm))
            .map_err(|reason| format!("{label} の幅厚比の限界値を算定できません。{reason}"))?;
        let ratio = width_mm / thickness_mm;
        let limit = coefficient * (mat.young / f_mpa).sqrt();
        if !ratio.is_finite() || ratio <= 0.0 || !limit.is_finite() || limit <= 0.0 {
            return Err(format!(
                "{label} の幅厚比または限界値を正の有限値で算定できません"
            ));
        }
        if ratio > limit {
            return Err(format!(
                "局部座屈の影響を省ける幅厚比の条件を満たしません。{label}={ratio:.4}（上限 {limit:.4}）"
            ));
        }
    }
    Ok(())
}

pub(super) fn check_asymmetric_built_h(
    forces: &MemberForcesAt,
    sec: &Section,
    shape: &SectionShape,
    mat: &Material,
    ctx: &DesignCtx,
    f: f64,
    shear_reasons: [Option<&'static str>; 2],
) -> CheckOutcome {
    let checked = || -> Result<CheckResult, String> {
        let reasons = [
            (![
                forces.n, forces.mz, forces.my, forces.qy, forces.qz,
            ]
            .iter()
            .all(|force| force.is_finite()))
            .then_some("断面力が非有限です"),
            has_section_loss(ctx).then_some("断面欠損を控除した断面係数の適用条件が未確定です"),
            (sec.property_basis.iy != PropertyBasis::Shape)
                .then_some("図心と断面性能の対応を確定するには、形状から求めた強軸断面二次モーメントが必要です"),
        ]
        .into_iter()
        .flatten()
        .chain(asymmetric_built_h_combination_reasons(forces))
        .collect::<Vec<_>>();
        if !reasons.is_empty() {
            return Err(reasons.join("。"));
        }
        if !(0.0..=1.0).contains(&forces.pos) {
            return Err("検定位置が無効です".into());
        }
        let direct_lb_mm = ctx.lb.or_else(|| {
            let lengths = ctx.steel_attr.as_ref()?.lb_direct?;
            Some(resolve_lb(forces.pos, ctx.length, Some(lengths), None))
        });
        let lateral_buckling_reason = match direct_lb_mm {
            Some(lb_mm) if !lb_mm.is_finite() || lb_mm < 0.0 => {
                return Err("横座屈長さの直接入力が負または非有限です".into());
            }
            Some(0.0) => None,
            Some(_) => Some("正の横座屈長さに対する上下非対称断面の横座屈の算定条件が未確定です"),
            None if !ctx.length.is_finite() || ctx.length < 0.0 => {
                return Err("横座屈長さの既定値に使う部材長が負または非有限です".into());
            }
            None => Some("評価位置の横座屈長さが0と直接指定されておらず、上下非対称断面の横座屈の算定条件が未確定です"),
        };

        let (z_upper_mm3, z_lower_mm3) = built_h_strong_axis_moduli(shape)?;
        let ft_mpa = steel_ft(f, ctx.term);
        if ![z_upper_mm3, z_lower_mm3, ft_mpa]
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
        {
            return Err("上下の断面係数と許容応力度を有限な正値で算定できません".into());
        }

        let sigma_upper_mpa = -forces.mz / z_upper_mm3;
        let sigma_lower_mpa = forces.mz / z_lower_mm3;
        let upper_in_compression = forces.mz > 0.0;
        let (compression_side, tension_side, compression_mpa, tension_mpa) = if upper_in_compression
        {
            ("上", "下", -sigma_upper_mpa, sigma_lower_mpa)
        } else {
            ("下", "上", -sigma_lower_mpa, sigma_upper_mpa)
        };
        // 横座屈の算定条件が未確定なら、圧縮縁の許容曲げ応力度の上限で不適合だけを確定する。
        let fb_limit_mpa = ft_mpa;
        let compression_ratio = compression_mpa / fb_limit_mpa;
        let tension_ratio = tension_mpa / ft_mpa;
        if !compression_ratio.is_finite() || !tension_ratio.is_finite() {
            return Err("上下縁の応力度比を有限値で算定できません".into());
        }
        let ratio = compression_ratio.max(tension_ratio);
        let term = match ctx.term {
            LoadTerm::Long => "長期",
            LoadTerm::Short => "短期",
        };
        let mut basis =
            format!("鋼構造許容応力度設計規準2019年版 {term} 上下非対称組立H形鋼大梁の強軸曲げ");
        if lateral_buckling_reason.is_none() {
            if let Err(reason) =
                check_built_h_girder_plate_slenderness(shape, mat, upper_in_compression)
            {
                let SectionShape::SteelBuiltH {
                    upper_thick,
                    lower_thick,
                    ..
                } = *shape
                else {
                    unreachable!("上下非対称組立H形鋼大梁だけを検定する");
                };
                let Ok(upper_f_mpa) = resolve_steel_f(Some(mat), Some(upper_thick)) else {
                    return Err(reason);
                };
                let Ok(lower_f_mpa) = resolve_steel_f(Some(mat), Some(lower_thick)) else {
                    return Err(reason);
                };
                let upper_ft_mpa = steel_ft(upper_f_mpa, ctx.term);
                let lower_ft_mpa = steel_ft(lower_f_mpa, ctx.term);
                // 有効断面の図心が移動するため、全断面の片側の縁応力度は下限に使えない。
                let ratio_lower_bound =
                    (compression_mpa + tension_mpa) / (upper_ft_mpa + lower_ft_mpa);
                if !ratio_lower_bound.is_finite() || ratio_lower_bound <= 1.0 {
                    return Err(reason);
                }
                basis.push_str("（上下いずれかの縁の許容上限による不適合）。");
                basis.push_str(&reason);
                basis.push_str("。有効断面による曲げ照査は未算定です。表示する検定比は下限です");
                return Ok(CheckResult {
                    basis,
                    detail: super::lateral_bracing::UNCONFIRMED_DETAIL.to_string(),
                    components: vec![CheckComponent {
                        kind: CheckKind::Bending,
                        ratio: ratio_lower_bound,
                        detail: format!(
                            "全断面の上下縁の曲げ応力度の絶対値の和={:.4} N/mm², \
                             上縁の許容上限={upper_ft_mpa:.4} N/mm², 下縁の許容上限={lower_ft_mpa:.4} N/mm², \
                             曲げ比の下限={ratio_lower_bound:.4}",
                            compression_mpa + tension_mpa
                        ),
                    }],
                });
            }
        }
        let (fb_label, ratio_label) = if let Some(reason) = lateral_buckling_reason {
            if ratio <= 1.0 {
                return Err(reason.into());
            }
            basis.push_str("（材料の許容上限による不適合）。");
            basis.push_str(reason);
            ("fb上限", "曲げ比の下限")
        } else {
            basis.push_str("（横座屈長さ0の断面照査）");
            ("fb", "曲げ比")
        };
        Ok(CheckResult {
            basis,
            detail: super::lateral_bracing::UNCONFIRMED_DETAIL.to_string(),
            components: vec![CheckComponent {
                kind: CheckKind::Bending,
                ratio,
                detail: format!(
                    "Z上={z_upper_mm3:.4} mm³, Z下={z_lower_mm3:.4} mm³, 圧縮側={compression_side}、引張側={tension_side}, \
                     σb上={sigma_upper_mpa:.4} N/mm², σb下={sigma_lower_mpa:.4} N/mm², \
                     {fb_label}={fb_limit_mpa:.4} N/mm², ft={ft_mpa:.4} N/mm², \
                     {ratio_label}（圧縮側）={compression_ratio:.4}, {ratio_label}（引張側）={tension_ratio:.4}"
                ),
            }],
        })
    };
    match checked() {
        Ok(result) => CheckOutcome::Checked(result),
        Err(reason) => {
            let mut reason = format!("上下非対称組立H形鋼大梁の強軸曲げは未算定です。{reason}");
            for shear_reason in shear_reasons.into_iter().flatten() {
                reason.push('。');
                reason.push_str(shear_reason);
            }
            CheckOutcome::Skipped { reason }
        }
    }
}

pub(crate) fn beam_lateral_buckling_length(ctx: &DesignCtx, pos: f64) -> (f64, bool) {
    let specified = ctx.lb.or_else(|| {
        let attr = ctx.steel_attr.as_ref()?;
        let brace_count = attr.lateral_brace_count.filter(|&n| n > 0);
        (attr.lb_direct.is_some() || brace_count.is_some())
            .then(|| resolve_lb(pos, ctx.length, attr.lb_direct, brace_count))
    });
    let lb = specified.unwrap_or_else(|| ctx.node_span_mm.unwrap_or(ctx.length));
    let partial = specified.is_some_and(|lb| lb < ctx.length - 1e-9);
    (lb, partial)
}

/// 鉄骨造梁の軸力・曲げと適用範囲内の独立せん断の検定比を返す。
pub(crate) fn check_beam(
    forces: &MemberForcesAt,
    sec: &Section,
    mat: &Material,
    ctx: &DesignCtx,
    f: f64,
    lambda: f64,
    incomplete: &mut Option<String>,
) -> Result<CheckResult, String> {
    let term = ctx.term;
    let h = sec.depth;
    let b = sec.width;
    let area = nonzero(sec.area);
    let (shape, tf, tw) = shape_of(sec);

    let z_strong = match (&ctx.steel_attr, shape) {
        (Some(attr), ShapeCategory::H)
            if attr.joint_flange_loss > 0.0
                || attr.joint_web_loss > 0.0
                || attr.scallop_web_loss > 0.0 =>
        {
            let is_end = !(0.25 < forces.pos && forces.pos < 0.75);
            nonzero(steel_h_z_with_loss(
                h,
                b,
                tw,
                tf,
                attr.joint_flange_loss,
                attr.joint_web_loss,
                attr.scallop_web_loss,
                is_end,
            ))
        }
        _ => nonzero(section_modulus(sec.iy, h / 2.0)),
    };
    let z_weak = nonzero(section_modulus(sec.iz, b / 2.0));

    let sigma_ax = forces.n.abs() / area;
    let sigma_by = forces.mz.abs() / z_strong;
    let sigma_bz = forces.my.abs() / z_weak;
    let sigma_b_pipe = (forces.mz.powi(2) + forces.my.powi(2)).sqrt() / z_strong;

    let ft_val = steel_ft(f, term);
    let fs_val = steel_fs(f, term);

    let fc_val = steel_fc(f, mat.young, lambda, term);

    let fb_weak = ft_val;
    let mut fb_label = "fb";
    let mut moment_parameters = None;
    let (fb_strong, lateral_length_mm) = match shape {
        ShapeCategory::H
            if ctx.steel_beam_moment_diagram == super::SteelBeamMomentDiagram::UnverifiedGroup =>
        {
            if forces.mz != 0.0 {
                *incomplete = crate::member_design_check::combine_incomplete_reasons(
                    incomplete.take(),
                    Some("一本部材の補剛区間を確認できず、横座屈を考慮した強軸曲げは未算定です"),
                );
                fb_label = "fb上限";
            }
            (ft_val, None)
        }
        ShapeCategory::H => {
            let (lb, partial) = beam_lateral_buckling_length(ctx, forces.pos);
            let length_is_direct_or_braced = ctx.lb.is_some()
                || ctx.steel_attr.as_ref().is_some_and(|attr| {
                    attr.lb_direct.is_some() || attr.lateral_brace_count.is_some_and(|n| n > 0)
                });
            if !length_is_direct_or_braced && ctx.node_span_mm.is_some() && lb <= 0.0 {
                return Err("節点間距離が正でないため横座屈長さを確定できません".into());
            }
            if !lb.is_finite() || lb < 0.0 {
                return Err("横座屈長さが負または非有限です".into());
            }
            let unverified_bracing =
                ctx.steel_beam_moment_diagram == super::SteelBeamMomentDiagram::UnverifiedBracing;
            let stress = if lb == 0.0 || !unverified_bracing && lb <= 1e-9 {
                ft_val
            } else if let Some((c, p_lambda_b)) = match ctx.steel_beam_moment_diagram {
                super::SteelBeamMomentDiagram::UnverifiedBracing => None,
                super::SteelBeamMomentDiagram::Unverified if partial => {
                    Some((steel_c_factor(ctx, true), 0.3))
                }
                _ => steel_beam_lateral_parameters(ctx),
            } {
                moment_parameters = Some((c, p_lambda_b));
                let iz = sec.iz;
                let iw = steel_warping_constant(sec, tf);
                let j = sec.resolved_j()?;
                let e = mat.young;
                let g = mat.shear.unwrap_or(e / (2.0 * (1.0 + mat.poisson)));
                steel_fb_h_asd2019(f, term, lb, iz, iw, j, e, g, z_strong, c, p_lambda_b)
            } else {
                if forces.mz != 0.0 {
                    let reason = if unverified_bracing {
                        "各補剛区間の強軸曲げ分布を確認できず、横座屈を考慮した強軸曲げは未算定です"
                    } else {
                        "単一補剛区間の強軸曲げ分布を確認できず、横座屈を考慮した強軸曲げは未算定です"
                    };
                    *incomplete = crate::member_design_check::combine_incomplete_reasons(
                        incomplete.take(),
                        Some(reason),
                    );
                    fb_label = "fb上限";
                }
                ft_val
            };
            (
                stress,
                (!unverified_bracing || fb_label != "fb上限").then_some(lb),
            )
        }
        _ => (ft_val, None),
    };

    let tension_with_strong_bending = strong_axis_h_tension(forces, sec);
    let (ratio_comb, axial_basis) = if forces.n < 0.0 {
        let ratio = match shape {
            ShapeCategory::Pipe => {
                sigma_ax / safe_denom(fc_val) + sigma_b_pipe / safe_denom(fb_strong)
            }
            _ => {
                sigma_ax / safe_denom(fc_val)
                    + sigma_by / safe_denom(fb_strong)
                    + sigma_bz / safe_denom(fb_weak)
            }
        };
        (ratio, "圧縮+曲げ: σc/fc(座屈考慮)+ΣσB/fb")
    } else if tension_with_strong_bending {
        (
            strong_axis_tension_bending_ratios(sigma_ax, sigma_by, ft_val, fb_strong).governing(),
            "引張+強軸曲げ: 式(6.3)・(6.4)",
        )
    } else {
        let ratio = match shape {
            ShapeCategory::Pipe => (sigma_ax + sigma_b_pipe) / safe_denom(ft_val),
            _ => (sigma_ax + sigma_by + sigma_bz) / safe_denom(ft_val),
        };
        (ratio, "引張+曲げ: (σt+ΣσB)/ft")
    };

    let (ratio_my, ratio_mz) = match shape {
        ShapeCategory::Pipe => (sigma_b_pipe / safe_denom(fb_strong), 0.0),
        _ => (
            sigma_by / safe_denom(fb_strong),
            sigma_bz / safe_denom(fb_weak),
        ),
    };

    let term_label = match term {
        LoadTerm::Long => "長期",
        LoadTerm::Short => "短期",
    };
    let basis = format!(
        "鋼構造 梁: 軸力+二軸曲げ ({}, {}, fb=鋼構造許容応力度設計規準2019年版)",
        term_label, axial_basis
    );
    let mut bending_detail = format!(
        "σax={:.4} N/mm², σby={:.4} N/mm², σbz={:.4} N/mm², fc={:.4} N/mm², {fb_label}={:.4} N/mm², \
組合せ比={:.4}",
        sigma_ax, sigma_by, sigma_bz, fc_val, fb_strong, ratio_comb
    );
    if let Some(lb) = lateral_length_mm {
        bending_detail.push_str(&format!(", 採用横座屈長さ lb={lb} mm"));
    }
    if let Some((c, p_lambda_b)) = moment_parameters {
        bending_detail.push_str(&format!(", C={c:.4}, 塑性限界細長比={p_lambda_b:.4}"));
    }
    let ratio = if tension_with_strong_bending {
        ratio_comb
    } else {
        bending_detail.push_str(&format!(", My比={ratio_my:.4}, Mz比={ratio_mz:.4}"));
        ratio_comb.max(ratio_my).max(ratio_mz)
    };
    let mut components = vec![CheckComponent {
        kind: CheckKind::Bending,
        ratio,
        detail: bending_detail,
    }];
    components.extend(shear_component(forces, sec, ctx, fs_val));

    Ok(CheckResult {
        basis,
        detail: super::lateral_bracing::UNCONFIRMED_DETAIL.to_string(),
        components,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steel::section::steel_p_lambda_b;
    use crate::steel::test_support::{h_section, mat, rect_section};
    use crate::steel::{SteelBeamMomentDiagram, SteelDesign};
    use crate::{DesignCheck, MemberKind};

    #[test]
    fn beam_uses_node_span_for_unspecified_lateral_buckling_and_keeps_explicit_input() {
        use sepika_core::ids::ElemId;
        use sepika_core::model::SteelDesignAttr;

        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let forces = MemberForcesAt::from_local(0.5, [0.0, 0.0, 0.0, 0.0, 0.0, 100e6]);
        let mut ctx = DesignCtx {
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 3500.0,
            node_span_mm: Some(3000.0),
            ..Default::default()
        };
        let material = mat("SN400B");
        let default_check = SteelDesign
            .check(&forces, &sec, &material, &ctx)
            .unwrap_checked();
        assert!(crate::full_detail(&default_check).contains("lb=3000 mm"));
        for (lateral_brace_count, lb_direct, adopted_mm) in [
            (Some(0), None, 3000),
            (Some(1), None, 1750),
            (Some(1), Some((1500.0, 2800.0, 1500.0)), 2800),
        ] {
            ctx.steel_attr = Some(SteelDesignAttr {
                elem: ElemId(1),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct,
                lateral_brace_count,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
                column_lateral_buckling_end_supports: None,
            });
            let check = SteelDesign
                .check(&forces, &sec, &material, &ctx)
                .unwrap_checked();
            assert!(crate::full_detail(&check).contains(&format!("lb={adopted_mm} mm")));
        }
        ctx.lb = Some(3500.0);
        let specified = SteelDesign
            .check(&forces, &sec, &material, &ctx)
            .unwrap_checked();
        assert!(specified.ratio() > default_check.ratio());
        ctx.lb = Some(-1.0);
        assert!(
            matches!(SteelDesign.check(&forces, &sec, &material, &ctx), CheckOutcome::Skipped { reason } if reason.contains("横座屈長さが負"))
        );
    }

    #[test]
    fn strong_axis_bending_with_tension_uses_both_edges_without_a_pure_bending_failure() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 163_840.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 223_907_465.6,
        };
        let ctx = DesignCtx {
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            term: LoadTerm::Short,
            length: 3000.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat("SN400B"), &ctx)
            .unwrap_checked();
        assert!(result.ok(), "{result:?}");
        assert!(
            (result.ratio() - 0.914_893_617_021_276_6).abs() < 1e-12,
            "{result:?}"
        );
        let pure_bending = SteelDesign
            .check(
                &MemberForcesAt { n: 0.0, ..forces },
                &sec,
                &mat("SN400B"),
                &ctx,
            )
            .unwrap_checked();
        assert!(!pure_bending.ok(), "{pure_bending:?}");
    }

    /// Bending の detail に "組合せ比=" が含まれ、Shear の detail には
    /// 含まれない。逆に Shear 固有の "せん断比=" は Bending に含まれない。
    #[test]
    fn test_check_beam_detail_fragments_assigned_to_intended_components() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let m = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 10_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1.0e8,
        };
        let ctx = DesignCtx {
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 4000.0,
            ..Default::default()
        };
        let result = SteelDesign.check(&forces, &sec, &m, &ctx).unwrap_checked();
        let bending = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .expect("Bending component が存在するはず");
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Shear)
            .expect("Shear component が存在するはず");
        assert!(bending.detail.contains("組合せ比="));
        assert!(!shear.detail.contains("組合せ比="));
        assert!(shear.detail.contains("せん断比="));
        assert!(!bending.detail.contains("せん断比="));
    }

    #[test]
    fn test_check_beam_applies_section_loss_attr() {
        use sepika_core::ids::ElemId;
        use sepika_core::model::SteelDesignAttr;
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let m = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1.0e8,
        };
        let ctx_base = DesignCtx {
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 4000.0,
            ..Default::default()
        };
        let base = SteelDesign
            .check(&forces, &sec, &m, &ctx_base)
            .unwrap_checked();
        let ctx_loss = DesignCtx {
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 4000.0,
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 10.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 20.0,
                lb_direct: None,
                lateral_brace_count: None,
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
            }),
            ..Default::default()
        };
        let with_loss = SteelDesign
            .check(&forces, &sec, &m, &ctx_loss)
            .unwrap_checked();
        assert!(
            with_loss.ratio() > base.ratio(),
            "loss ratio={} <= base ratio={}",
            with_loss.ratio(),
            base.ratio()
        );
    }

    #[test]
    fn test_check_beam_lb_from_attr_brace_count() {
        use sepika_core::ids::ElemId;
        use sepika_core::model::SteelDesignAttr;
        let sec = h_section(600.0, 150.0, 8.0, 10.0);
        let m = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1.0e8,
        };
        let ctx_no_brace = DesignCtx {
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 12_000.0,
            ..Default::default()
        };
        let base = SteelDesign
            .check(&forces, &sec, &m, &ctx_no_brace)
            .unwrap_checked();
        let ctx_braced = DesignCtx {
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 12_000.0,
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: Some(5),
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
            }),
            ..Default::default()
        };
        let braced = SteelDesign
            .check(&forces, &sec, &m, &ctx_braced)
            .unwrap_checked();
        assert!(
            braced.ratio() < base.ratio(),
            "braced ratio={} >= base ratio={}",
            braced.ratio(),
            base.ratio()
        );
    }

    /// c_direct=1.5 を与えると、端部モーメント（異符号・自動算定なら
    /// C=2.3）に関わらず fb1 の C=1.5 が採用され、fb・検定比が自動算定時と
    /// 異なることを確認する。
    #[test]
    fn test_check_beam_c_direct_overrides_auto() {
        use sepika_core::ids::ElemId;
        use sepika_core::model::SteelDesignAttr;
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let m = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx_auto = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 6000.0,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::Monotonic([100e6, -100e6]),
            ..Default::default()
        };
        let auto = SteelDesign
            .check(&forces, &sec, &m, &ctx_auto)
            .unwrap_checked();

        let ctx_direct = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 6000.0,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::Monotonic([100e6, -100e6]),
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: None,
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: Some(1.5),
            }),
            ..Default::default()
        };
        let direct = SteelDesign
            .check(&forces, &sec, &m, &ctx_direct)
            .unwrap_checked();

        assert!(
            direct.ratio() > auto.ratio(),
            "direct ratio={} <= auto ratio={}",
            direct.ratio(),
            auto.ratio()
        );

        assert!(
            crate::full_detail(&direct).contains("fb=131.5331"),
            "detail={}",
            crate::full_detail(&direct)
        );
    }

    /// 横補剛により lb が部材の部分区間となる場合（自動算定なら安全側 C=1.0
    /// に落ちる）でも、c_direct の直接入力があればそちらが優先され C=1.0 に
    /// 落ちないことを確認する。
    #[test]
    fn test_check_beam_c_direct_prevents_partial_lb_fallback_to_1_0() {
        use sepika_core::ids::ElemId;
        use sepika_core::model::SteelDesignAttr;
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let m = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx_no_direct = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 6000.0,
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: Some(1),
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
            }),
            ..Default::default()
        };
        let no_direct = SteelDesign
            .check(&forces, &sec, &m, &ctx_no_direct)
            .unwrap_checked();
        assert!(
            crate::full_detail(&no_direct).contains("fb=128.2569"),
            "部分区間では C=1.0 が採用されるはず: detail={}",
            crate::full_detail(&no_direct)
        );

        let ctx_direct = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 6000.0,
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: Some(1),
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: Some(2.0),
            }),
            ..Default::default()
        };
        let direct = SteelDesign
            .check(&forces, &sec, &m, &ctx_direct)
            .unwrap_checked();
        assert!(
            crate::full_detail(&direct).contains("fb=143.7760"),
            "直接入力の C=2.0 が採用され C=1.0 に落ちないはず: detail={}",
            crate::full_detail(&direct)
        );
    }

    /// c_direct ≤ 0 は無効な入力として無視し、内点最大の自動算定に戻す。
    #[test]
    fn test_check_beam_c_direct_non_positive_falls_back_to_auto() {
        use sepika_core::ids::ElemId;
        use sepika_core::model::SteelDesignAttr;
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let m = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 6000.0,
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: None,
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: Some(-2.0),
            }),
            ..Default::default()
        };
        let result = SteelDesign.check(&forces, &sec, &m, &ctx).unwrap_checked();

        assert!(
            crate::full_detail(&result).contains("fb=85.5557"),
            "c_direct<=0 は無視され C=1.0（自動算定）になるはず: detail={}",
            crate::full_detail(&result)
        );
    }

    /// 矩形断面の検算例（手計算照合）。
    /// 矩形 B=200, D=400 ⇒ Z=B·D²/6=5.3333e6 mm³, M=1e8 N·mm
    /// σ=18.75 N/mm², fb=F/1.5=156.6667 N/mm²（矩形は横座屈対象外＝fb=ft）,
    /// 検定比=0.1197（相対 1e-9）。
    #[test]
    fn test_beam_check_bending_rect_section_hand_calc() {
        let mut sec = rect_section(200.0, 400.0, "矩形200x400");
        sec.thickness = Some(20.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1e8,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 0.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let expected_sigma = 18.75;
        let expected_fb = 235.0 / 1.5;
        let expected_ratio = expected_sigma / expected_fb;
        assert!(
            (result.ratio() - expected_ratio).abs() < 1e-9,
            "ratio {} != {}",
            result.ratio(),
            expected_ratio
        );
        assert!(result.ok());
        assert!(crate::full_detail(&result).contains("18.7500"));
        assert_eq!(result.components.len(), 1);
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Bending));
    }

    /// `Section.shape` がある場合は実寸の tw でウェブせん断面積を計算する
    /// （名前推定＋単一板厚近似ではなく、tw=10 が使われること）。
    #[test]
    fn test_beam_check_uses_shape_tw_for_web_shear() {
        let sec = h_section(400.0, 200.0, 10.0, 15.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 100_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 0.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();
        let ay = 10.0 * 400.0;
        let tau = 100_000.0 / ay;
        let fs = 235.0 / (1.5 * 3.0_f64.sqrt());
        let expected = tau / fs;
        assert!(
            (result.ratio() - expected).abs() < 1e-9,
            "ratio={} expected={}",
            result.ratio(),
            expected
        );
    }

    /// F 値の板厚区分は shape の最大板厚で判定する（tf=45 → 40mm 超区分）。
    #[test]
    fn test_f_value_bucket_uses_shape_max_thickness() {
        let sec = h_section(900.0, 400.0, 20.0, 45.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1e6,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 0.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();
        // F=215（40mm 超）→ fb=ft=215/1.5=143.33...
        assert!(
            crate::full_detail(&result).contains("fb=143.3"),
            "detail should show fb from F=215: {}",
            crate::full_detail(&result)
        );
    }

    /// 圧縮軸力+二軸曲げ（H形）: σc/fc+σby/fb+σbz/ft を手計算照合する。
    #[test]
    fn test_beam_check_compression_biaxial_bending_hand_calc() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: -200_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 5e6,
            mz: 8e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 4000.0,
            lk_y: Some(4000.0),
            lk_z: Some(4000.0),
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let f = 235.0;
        let area = sec.area;
        let z_strong = sec.iy / (sec.depth / 2.0);
        let z_weak = sec.iz / (sec.width / 2.0);
        let sigma_c = 200_000.0 / area;
        let sigma_by = 8e7_f64 / z_strong;
        let sigma_bz = 5e6_f64 / z_weak;

        let i_min = (sec.iy.min(sec.iz) / area).sqrt();
        let lambda = 4000.0 / i_min;
        let fc = steel_fc(f, mat_v.young, lambda, LoadTerm::Long);
        let ft = steel_ft(f, LoadTerm::Long);
        // この圧縮・二軸曲げ入力では、座屈を反映したfc・fbにより組合せ式が支配する。
        let fb_strong = 112.36201317432739;
        let expected_comb = sigma_c / fc + sigma_by / fb_strong + sigma_bz / ft;
        assert!(
            (result.ratio() - expected_comb).abs() < 1e-9,
            "ratio={} expected_comb={}",
            result.ratio(),
            expected_comb
        );
    }

    /// 引張軸力+二軸曲げ: (σt+σby+σbz)/ft を手計算照合する。
    #[test]
    fn test_beam_check_tension_biaxial_bending_hand_calc() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 200_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 5e6,
            mz: 8e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 4000.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let f = 235.0;
        let area = sec.area;
        let z_strong = sec.iy / (sec.depth / 2.0);
        let z_weak = sec.iz / (sec.width / 2.0);
        let sigma_t = 200_000.0 / area;
        let sigma_by = 8e7_f64 / z_strong;
        let sigma_bz = 5e6_f64 / z_weak;
        let ft = steel_ft(f, LoadTerm::Long);
        let expected_comb = (sigma_t + sigma_by + sigma_bz) / ft;
        assert!(
            (result.ratio() - expected_comb).abs() < 1e-9,
            "ratio={} expected_comb={}",
            result.ratio(),
            expected_comb
        );
    }

    /// 円形鋼管の合成曲げ: mz 単独と mz/my 分配で検定比がほぼ一致する
    /// （σb=√(mz²+my²)/Z強軸に一本化されるため、合成モーメントの大きさが
    /// 同じであれば分配方法によらず一致するはず）。
    #[test]
    fn test_beam_check_pipe_combines_biaxial_bending() {
        let mut sec = rect_section(300.0, 300.0, "PIPE-300x12");
        sec.iz = sec.iy;
        sec.thickness = Some(12.0);
        let mat_v = mat("SN400");
        let forces_mz_only = MemberForcesAt {
            pos: 0.5,
            n: -50_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30e6,
        };
        let forces_split = MemberForcesAt {
            pos: 0.5,
            n: -50_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 30e6 / std::f64::consts::SQRT_2,
            mz: 30e6 / std::f64::consts::SQRT_2,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 3000.0,
            lk_y: Some(3000.0),
            lk_z: Some(3000.0),
            ..Default::default()
        };
        let r1 = SteelDesign
            .check(&forces_mz_only, &sec, &mat_v, &ctx)
            .unwrap_checked();
        let r2 = SteelDesign
            .check(&forces_split, &sec, &mat_v, &ctx)
            .unwrap_checked();
        assert!(
            (r1.ratio() - r2.ratio()).abs() < 1e-6,
            "pipe combined bending mismatch: {} vs {}",
            r1.ratio(),
            r2.ratio()
        );
    }

    #[test]
    fn test_beam_check_bending_uses_asd2019() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 6000.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let f = 235.0;
        let iz = sec.iz;
        let iw = steel_warping_constant(&sec, 13.0);
        let g = mat_v.young / (2.0 * (1.0 + mat_v.poisson));
        let fb_expected = steel_fb_h_asd2019(
            f,
            LoadTerm::Long,
            6000.0,
            iz,
            iw,
            sec.j,
            mat_v.young,
            g,
            sec.iy / (sec.depth / 2.0),
            1.0,
            steel_p_lambda_b(&ctx, false),
        );
        assert!(
            crate::full_detail(&result).contains(&format!("fb={:.4}", fb_expected)),
            "detail={}",
            crate::full_detail(&result)
        );
    }

    /// AIJ-ASD19 fb: λb ≤ pλb（横座屈長さが短い）では fb=F/ν（全塑性域）。
    #[test]
    fn test_beam_check_fb_basis_asd2019_plastic_region() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 300.0,
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
        let lb = 300.0_f64;

        let my = f * z_strong;
        let pi2 = std::f64::consts::PI.powi(2);
        let pi4 = std::f64::consts::PI.powi(4);
        let me = (pi4 * e * sec.iz * e * iw / lb.powi(4)
            + pi2 * e * sec.iz * g * sec.j / lb.powi(2))
        .sqrt();
        let lambda_b = (my / me).sqrt();
        assert!(
            lambda_b <= p_lambda_b,
            "lambda_b={} p_lambda_b={} 全塑性域前提",
            lambda_b,
            p_lambda_b
        );
        let e_lambda_b = 1.0 / 0.6_f64.sqrt();
        let nu = 1.5 + (2.0 / 3.0) * (lambda_b / e_lambda_b).powi(2);
        let expected = f / nu;

        let fb_expected = steel_fb_h_asd2019(
            f,
            LoadTerm::Long,
            lb,
            sec.iz,
            iw,
            sec.j,
            e,
            g,
            z_strong,
            1.0,
            p_lambda_b,
        );
        assert!(
            (fb_expected - expected).abs() < 1e-6,
            "fb_expected={} expected={}",
            fb_expected,
            expected
        );
        assert!(
            crate::full_detail(&result).contains(&format!("fb={:.4}", fb_expected)),
            "detail={}",
            crate::full_detail(&result)
        );
    }

    /// 梁の Asd2019 経路でも、部分補剛区間の pλb=0.3 を fb に反映する。
    #[test]
    fn test_beam_check_fb_basis_asd2019_partial_lb_uses_p_lambda_b_0_3() {
        use sepika_core::ids::ElemId;
        use sepika_core::model::SteelDesignAttr;

        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 6000.0,
            end_moments_z: Some((100.0, -100.0)),
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: Some(1),
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
            }),
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();
        let iw = steel_warping_constant(&sec, 13.0);
        let e = mat_v.young;
        let g = e / (2.0 * (1.0 + mat_v.poisson));
        let fb_expected = steel_fb_h_asd2019(
            235.0,
            LoadTerm::Long,
            3000.0,
            sec.iz,
            iw,
            sec.j,
            e,
            g,
            sec.iy / (sec.depth / 2.0),
            1.0,
            0.3,
        );
        assert!(
            crate::full_detail(&result).contains(&format!("fb={fb_expected:.4}")),
            "detail={}",
            crate::full_detail(&result)
        );
    }

    /// AIJ-ASD19 fb: eλb < λb（横座屈長さが長い）では弾性域式 fb=F/(2.17λb²)。
    #[test]
    fn test_beam_check_fb_basis_asd2019_elastic_region() {
        let f = 235.0;
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let z_strong = sec.iy / (sec.depth / 2.0);
        let iw = steel_warping_constant(&sec, 13.0);
        let g = mat_v.young / (2.0 * (1.0 + mat_v.poisson));
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            length: 20_000.0,
            ..Default::default()
        };
        let p_lambda_b = steel_p_lambda_b(&ctx, false);
        let lb = 20_000.0;
        let fb = steel_fb_h_asd2019(
            f,
            LoadTerm::Long,
            lb,
            sec.iz,
            iw,
            sec.j,
            mat_v.young,
            g,
            z_strong,
            1.0,
            p_lambda_b,
        );

        let my = f * z_strong;
        let e = mat_v.young;
        let pi2 = std::f64::consts::PI.powi(2);
        let pi4 = std::f64::consts::PI.powi(4);
        let me = (pi4 * e * sec.iz * e * iw / lb.powi(4)
            + pi2 * e * sec.iz * g * sec.j / lb.powi(2))
        .sqrt();
        let lambda_b = (my / me).sqrt();
        let e_lambda_b = 1.0 / 0.6_f64.sqrt();
        assert!(lambda_b > e_lambda_b, "lambda_b={} 弾性域前提", lambda_b);
        let expected = f / (2.17 * lambda_b * lambda_b);
        assert!(
            (fb - expected).abs() < 1e-6,
            "fb={} expected={}",
            fb,
            expected
        );
    }

    #[test]
    fn beam_peak_at_any_interior_section_sets_the_interior_maximum_coefficients() {
        let ctx = DesignCtx {
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            ..Default::default()
        };
        assert_eq!(steel_beam_lateral_parameters(&ctx), Some((1.0, 0.3)));
    }

    /// 単曲率・等曲げ（M2/M1=−1）→ pλb=0.6−0.3=0.3。
    #[test]
    fn test_p_lambda_b_single_curvature_uniform_is_0_3() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, 100.0)),
            ..Default::default()
        };
        let p = steel_p_lambda_b(&ctx, false);
        assert!((p - 0.3).abs() < 1e-9, "p={}", p);
    }

    /// 複曲率・等曲げ（M2/M1=+1）→ pλb=0.6+0.3=0.9。
    #[test]
    fn test_p_lambda_b_double_curvature_uniform_is_0_9() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, -100.0)),
            ..Default::default()
        };
        let p = steel_p_lambda_b(&ctx, false);
        assert!((p - 0.9).abs() < 1e-9, "p={}", p);
    }

    #[test]
    fn end_and_center_samples_alone_do_not_verify_beam_coefficients() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, -100.0)),
            mid_moment_z: Some(0.0),
            ..Default::default()
        };
        assert_eq!(steel_beam_lateral_parameters(&ctx), None);
    }

    /// 部分補剛区間では、全長区間と同じ端部モーメントを与えても pλb=0.3 とする。
    #[test]
    fn test_p_lambda_b_partial_lb_is_0_3_independent_of_end_moments() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, -50.0)),
            ..Default::default()
        };
        let full = steel_p_lambda_b(&ctx, false);
        let partial = steel_p_lambda_b(&ctx, true);
        assert!((full - 0.75).abs() < 1e-9, "full={}", full);
        assert!((partial - 0.3).abs() < 1e-9, "partial={}", partial);
        assert_eq!(steel_c_factor(&ctx, true), 1.0);
    }

    /// 梁検定 detail 末尾に必要横補剛数が出力されることを確認する。
    #[test]
    fn test_beam_check_without_model_reports_unconfirmed_reference() {
        let sec = h_section(400.0, 200.0, 10.0, 15.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Girder,
            steel_beam_moment_diagram: SteelBeamMomentDiagram::InteriorMaximum,
            length: 9000.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();
        assert!(
            crate::full_detail(&result).contains("柱間全長・端条件未確認"),
            "detail={}",
            crate::full_detail(&result)
        );
    }
}
