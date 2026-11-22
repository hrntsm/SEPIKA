//! 鉄骨鉄筋コンクリート造梁の許容応力度検定。
use super::{ratio_or_large, shear_alpha, steel_h_props, SrcAxisProps};
use crate::material_strength::{main_rebar_grade, shear_rebar_grade};
use crate::rc::{
    concrete_allowable_compression, concrete_allowable_shear_class, rebar_allowable_shear,
    rebar_allowable_tension, young_ratio_n,
};
use crate::steel::{steel_fs, steel_ft};
use crate::{CheckComponent, CheckKind, CheckResult, DesignCtx, LoadTerm, MemberForcesAt};
use sepika_core::model::Material;

/// SRC梁の強軸曲げと、負担曲げを確定できる位置の強軸せん断を検定する。
#[allow(clippy::too_many_arguments)]
pub(crate) fn src_beam_check(
    forces: &MemberForcesAt,
    ctx: &DesignCtx,
    mat: &Material,
    props: SrcAxisProps,
    compression_depth_mm: f64,
    main_dia: f64,
    steel_height: f64,
    steel_width: f64,
    steel_web_thick: f64,
    steel_flange_thick: f64,
    f_value: f64,
    short_seismic: bool,
    ordinary_shear_issue: Option<&str>,
    main_rebar_valid: bool,
    shear_rebar_valid: bool,
) -> (CheckResult, Option<String>) {
    let fc_raw = mat.fc.unwrap_or_default();
    let long_term = ctx.term == LoadTerm::Long;
    let (_sa, sz, _sz_weak) = steel_h_props(
        steel_height,
        steel_width,
        steel_web_thick,
        steel_flange_thick,
    );

    let mut result = CheckResult {
        basis: "SRC規準16条 SRC梁: 強軸曲げのみ".into(),
        detail: String::new(),
        components: Vec::new(),
    };
    if let Some((quantity, _)) = [
        ("軸力N", forces.n),
        ("曲げモーメントMy", forces.my),
        ("曲げモーメントMz", forces.mz),
    ]
    .into_iter()
    .find(|(_, value)| !value.is_finite())
    {
        return (
            result,
            Some(format!("SRC梁検定: 組合せの{quantity}が非有限です")),
        );
    }
    if forces.n != 0.0 || forces.my != 0.0 {
        return (
            result,
            Some(
                "SRC梁検定: 軸力Nまたは弱軸曲げMyが作用するため、16条の強軸曲げの適用範囲外です"
                    .into(),
            ),
        );
    }
    if !main_rebar_valid {
        return (result, None);
    }
    let grade = main_rebar_grade(ctx.rebar_material.as_ref());
    let ft = rebar_allowable_tension(grade, main_dia, long_term);
    let fc = concrete_allowable_compression(fc_raw, long_term);
    let n_ratio = young_ratio_n(fc_raw);
    let gamma = props.ac / props.at;
    let dc_over_d = compression_depth_mm / props.d;
    let ft_over_fc = ft / fc;
    let balanced_denominator = 2.0
        * (1.0 + ft_over_fc / n_ratio)
        * (ft_over_fc * (1.0 + gamma * dc_over_d) - n_ratio * gamma * (1.0 - dc_over_d));
    let tensile_ratio = props.at / (props.b * props.d);
    if !balanced_denominator.is_finite()
        || balanced_denominator <= 0.0
        || !tensile_ratio.is_finite()
        || tensile_ratio > balanced_denominator.recip()
    {
        return (
            result,
            Some("SRC梁検定: 引張鉄筋比が釣合鉄筋比以下となる条件を確認できません".into()),
        );
    }
    let s_mo = sz * steel_ft(f_value, ctx.term);
    let r_ma = props.at * ft * props.j;
    let ma = s_mo + r_ma;
    result.components.push(CheckComponent {
        kind: CheckKind::Bending,
        ratio: ratio_or_large(forces.mz, ma),
        detail: format!(
            "sMo={:.1} N·mm, rMA={:.1} N·mm, MA={:.1} N·mm, |mz|={:.1} N·mm",
            s_mo, r_ma, ma, forces.mz,
        ),
    });
    if let Some(reason) = ordinary_shear_issue {
        return (result, Some(reason.into()));
    }
    if !forces.qy.is_finite() {
        return (result, Some("SRC梁検定: せん断力Qyが非有限です".into()));
    }
    let weak_shear_issue =
        (forces.qz != 0.0).then_some("SRC梁検定: 弱軸せん断力Qzは適用範囲外です");
    if short_seismic {
        return (
            result,
            crate::member_design_check::combine_incomplete_reasons(
                Some("SRC梁検定: 地震時の設計用せん断力には付1.4-4の材料別長期せん断力と両端曲げ耐力が必要です".into()),
                weak_shear_issue,
            ),
        );
    }
    if forces.qy == 0.0 {
        return (result, weak_shear_issue.map(str::to_owned));
    }
    let moment = forces.mz.abs();
    if moment < s_mo || moment > ma || moment == 0.0 {
        return (
            result,
            crate::member_design_check::combine_incomplete_reasons(
                Some(
                    "SRC梁検定: 18条のせん断力の分担に必要な材料別の負担曲げを確定できません"
                        .into(),
                ),
                weak_shear_issue,
            ),
        );
    }
    let q = forces.qy.abs();
    let s_q = q * (s_mo / moment);
    let r_m = moment - s_mo;
    let r_q = q * (r_m / moment);
    let s_qa =
        steel_web_thick * (steel_height - 2.0 * steel_flange_thick) * steel_fs(f_value, ctx.term);
    let mut ratio = ratio_or_large(s_q, s_qa);
    let mut detail = format!("sQ={s_q:.1} N, sQA={s_qa:.1} N");
    if shear_rebar_valid {
        let fs = concrete_allowable_shear_class(fc_raw, mat.concrete_class, long_term);
        let pw_ft = ctx.shear_rebar_material.as_ref().map_or(0.0, |rebar| {
            props.pw.min(0.006) * rebar_allowable_shear(shear_rebar_grade(Some(rebar)), long_term)
        });
        let alpha = shear_alpha(r_m, r_q, props.d, 2.0);
        let b_ratio = (props.b - steel_width) / props.b;
        let r_qa = props.b * props.j * (alpha * fs + 0.5 * pw_ft).min(2.0 * b_ratio * fs + pw_ft);
        ratio = ratio.max(ratio_or_large(r_q, r_qa));
        detail.push_str(&format!(", rQ={r_q:.1} N, rQA={r_qa:.1} N"));
    }
    result.basis = "SRC梁: 強軸曲げ（16条）・せん断（18条）".into();
    result.components.push(CheckComponent {
        kind: CheckKind::Shear,
        ratio,
        detail,
    });
    (result, weak_shear_issue.map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srrc::beam_axis_props;
    use crate::srrc::tests::{
        ctx_beam, make_material, make_section, src_beam_rect_shape, zero_forces,
    };
    use crate::steel::steel_f_value_prefix;
    use crate::DesignCheck;
    use sepika_core::section_shape::SectionShape;

    #[test]
    fn test_src_beam_moment_handcalc() {
        let shape = src_beam_rect_shape(
            400.0,
            700.0,
            22.0,
            vec![4, 2],
            vec![4, 2],
            40.0,
            10.0,
            100.0,
            2,
            500.0,
            200.0,
            9.0,
            14.0,
        );
        let sec = make_section(shape.clone());
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_beam(LoadTerm::Long);

        let rebar = match &shape {
            SectionShape::SrcBeamRect { rebar, .. } => rebar.clone(),
            _ => unreachable!(),
        };
        let props = beam_axis_props(400.0, 700.0, &rebar, false);
        let ft = rebar_allowable_tension("SD345", 22.0, true);
        let (_sa, sz, _) = steel_h_props(500.0, 200.0, 9.0, 14.0);
        let f_value = steel_f_value_prefix("SN400B", 14.0).unwrap();
        let s_ft = steel_ft(f_value, LoadTerm::Long);
        let expected_ma = sz * s_ft + props.at * ft * props.j;

        let forces = MemberForcesAt {
            mz: expected_ma * 0.5,
            ..zero_forces()
        };
        let design = crate::SrcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(
            (result.ratio() - 0.5).abs() < 1e-6,
            "ratio={}",
            result.ratio()
        );
        assert!(result.basis.contains("SRC規準16条"));

        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].kind, crate::CheckKind::Bending);
    }

    #[test]
    fn src_beam_does_not_apply_the_tensile_steel_formula_above_balanced_ratio() {
        let section = make_section(src_beam_rect_shape(
            400.0,
            700.0,
            22.0,
            vec![6, 2],
            vec![6, 2],
            40.0,
            10.0,
            100.0,
            2,
            500.0,
            200.0,
            9.0,
            14.0,
        ));
        let outcome = crate::SrcDesign.check(
            &MemberForcesAt {
                mz: 1.0e8,
                ..zero_forces()
            },
            &section,
            &make_material(24.0, "Fc24"),
            &ctx_beam(LoadTerm::Long),
        );
        assert!(
            matches!(outcome, crate::CheckOutcome::Skipped { reason } if reason.contains("釣合鉄筋比"))
        );
    }

    #[test]
    fn src_beam_uses_the_allowable_steel_moment_for_the_shear_split() {
        let sec = make_section(src_beam_rect_shape(
            400.0,
            700.0,
            22.0,
            vec![4, 2],
            vec![4, 2],
            40.0,
            10.0,
            100.0,
            2,
            500.0,
            200.0,
            9.0,
            14.0,
        ));
        let concrete = make_material(24.0, "Fc24");
        let mut ctx = ctx_beam(LoadTerm::Short);
        let forces = MemberForcesAt {
            mz: 450.0e6,
            qy: 800.0e3,
            ..zero_forces()
        };
        let checked = crate::SrcDesign
            .check(&forces, &sec, &concrete, &ctx)
            .unwrap_checked();
        let shear = checked
            .components
            .iter()
            .find(|row| row.kind == CheckKind::Shear)
            .unwrap();
        // ウェブ・フランジの矩形積分によるsMA=385.0535185 kN·m、sQA=576.3572267 kN。
        assert!(
            (shear.ratio - 1.187_700_191_327_678).abs() < 1e-9,
            "{shear:?}"
        );

        ctx.shear_rebar_material = None;
        let zero_shear = MemberForcesAt { qy: 0.0, ..forces };
        assert!(crate::SrcDesign
            .check(&zero_shear, &sec, &concrete, &ctx)
            .unwrap_checked()
            .ok());
        let deficient: Vec<_> = crate::SrcDesign
            .check_outcomes(&forces, &sec, &concrete, &ctx, false)
            .collect();
        assert!(
            matches!(deficient.as_slice(), [crate::CheckOutcome::Checked(result), crate::CheckOutcome::Skipped { reason }]
            if result.components.len() == 1 && result.components[0].kind == CheckKind::Shear
                && result.components[0].ratio > 1.0 && reason.contains("せん断補強筋の材料が未割当"))
        );
    }

    #[test]
    fn src_beam_does_not_pass_for_nonfinite_bending_or_an_unresolved_shear_split() {
        let sec = make_section(src_beam_rect_shape(
            400.0,
            700.0,
            22.0,
            vec![4, 2],
            vec![4, 2],
            40.0,
            10.0,
            100.0,
            2,
            500.0,
            200.0,
            9.0,
            14.0,
        ));
        for (forces, reason) in [
            (
                MemberForcesAt {
                    n: 1.0e3,
                    mz: 1.0e9,
                    ..zero_forces()
                },
                "16条の強軸曲げの適用範囲外",
            ),
            (
                MemberForcesAt {
                    my: 1.0e6,
                    mz: 1.0e9,
                    ..zero_forces()
                },
                "16条の強軸曲げの適用範囲外",
            ),
            (
                MemberForcesAt {
                    n: f64::NAN,
                    ..zero_forces()
                },
                "軸力Nが非有限",
            ),
            (
                MemberForcesAt {
                    my: f64::INFINITY,
                    ..zero_forces()
                },
                "曲げモーメントMyが非有限",
            ),
            (
                MemberForcesAt {
                    mz: f64::NAN,
                    ..zero_forces()
                },
                "モーメントMzが非有限",
            ),
            (
                MemberForcesAt {
                    qy: 1.0e6,
                    ..zero_forces()
                },
                "材料別の負担曲げを確定できません",
            ),
        ] {
            let outcome = crate::SrcDesign.check(
                &forces,
                &sec,
                &make_material(24.0, "Fc24"),
                &ctx_beam(LoadTerm::Short),
            );
            assert!(
                matches!(outcome, crate::CheckOutcome::Skipped { reason: msg } if msg.contains(reason))
            );
            let outcomes: Vec<_> = crate::SrcDesign
                .check_outcomes(
                    &forces,
                    &sec,
                    &make_material(24.0, "Fc24"),
                    &ctx_beam(LoadTerm::Short),
                    false,
                )
                .collect();
            assert!(
                matches!(outcomes.as_slice(), [crate::CheckOutcome::Skipped { .. }]),
                "{outcomes:?}"
            );
        }
    }
}
