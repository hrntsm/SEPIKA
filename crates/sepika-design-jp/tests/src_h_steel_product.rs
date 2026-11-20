#[path = "fixtures/src_h_steel_product_model.rs"]
mod input;
#[path = "fixtures/src_h_steel_product_reference.rs"]
mod reference;

use sepika_core::ids::ElemId;
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckOutcome, DesignCheck, DesignCtx, LoadTerm,
    MemberDesignCheckOptions, MemberForcesAt, MemberKind, SrcDesign,
};
use sepika_element::frame::beam::MemberForces;

fn inputs(column: bool) -> (sepika_core::model::Model, DesignCtx, MemberForcesAt) {
    let (n, mz) = reference::demand(column);
    let model = input::model(column, "SN400B", n, mz);
    let ctx = DesignCtx {
        kind: if column {
            MemberKind::Column
        } else {
            MemberKind::Girder
        },
        term: LoadTerm::Long,
        rebar_material: Some(model.materials[1].clone()),
        shear_rebar_material: Some(model.materials[1].clone()),
        steel_material: Some(model.materials[2].clone()),
        ..Default::default()
    };
    let forces = MemberForcesAt {
        pos: 0.0,
        n,
        qy: 0.0,
        qz: 0.0,
        mx: 0.0,
        my: 0.0,
        mz,
    };
    (model, ctx, forces)
}

#[test]
fn src_beam_and_column_long_term_check_uses_h_grade_but_skips_tube_grades() {
    for column in [false, true] {
        let (model, mut ctx, forces) = inputs(column);
        let check = |ctx: &DesignCtx| {
            SrcDesign.check(&forces, &model.sections[0], &model.materials[0], ctx)
        };
        for grade in ["SN400B", "SS400", "SM400A"] {
            ctx.steel_material.as_mut().unwrap().name = grade.into();
            let outcome = check(&ctx);
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{grade}: {outcome:?}");
            };
            approx::assert_abs_diff_eq!(
                result.ratio(),
                reference::expected_ratio(column),
                epsilon = 1e-12
            );
            assert!(!result.ok(), "{grade}: {result:?}");
        }
        let member = if column { "SRC柱" } else { "SRC梁" };
        for (grade, diagnostic) in [
            ("STK400", "鋼管用"),
            ("STK490", "鋼管用"),
            ("STKR400", "鋼管用"),
            ("STKN400B", "鋼管用"),
            ("STKN490B", "鋼管用"),
            ("BCR295", "鋼管用"),
            ("BCP325", "鋼管用"),
            ("SN400", "種類記号"),
            ("SM490", "種類記号"),
            ("SS490", "溶接"),
            ("SN400C", "実板厚"),
            ("TMCP325", "個別認定"),
            ("TMCP355", "個別認定"),
        ] {
            ctx.steel_material.as_mut().unwrap().name = grade.into();
            let outcome = check(&ctx);
            assert!(
                matches!(&outcome, CheckOutcome::Skipped { reason }
                    if reason.contains(member) && reason.contains(&model.sections[0].name)
                    && reason.contains(grade) && reason.contains(diagnostic)),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn src_column_long_term_check_skips_nonfinite_axial_force() {
    let (model, ctx, mut forces) = inputs(true);
    let check = |forces: &MemberForcesAt| {
        SrcDesign.check(forces, &model.sections[0], &model.materials[0], &ctx)
    };

    forces.n = 0.0;
    forces.mz = 0.0;
    let unloaded = check(&forces);
    let CheckOutcome::Checked(result) = unloaded else {
        panic!("{unloaded:?}");
    };
    assert_eq!(result.ratio(), 0.0);
    assert!(result.ok());

    for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        forces.n = n;
        let outcome = check(&forces);
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason }
                if reason.contains("軸力") && reason.contains("非有限")),
            "N={n}: {outcome:?}"
        );
    }
}

#[test]
fn src_column_short_term_check_without_seismic_qd_skips_nonfinite_axial_force() {
    let (model, mut ctx, mut forces) = inputs(true);
    ctx.term = LoadTerm::Short;
    let check = |forces: &MemberForcesAt| {
        SrcDesign.check(forces, &model.sections[0], &model.materials[0], &ctx)
    };

    forces.n = 3_600_000.0;
    let outcome = check(&forces);
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    // 内蔵H形鋼8,192 mm²と径25 mmの主筋8本の短期純引張耐力。
    let capacity_n = 8_192.0 * 235.0 + 8.0 * std::f64::consts::PI * 12.5_f64.powi(2) * 345.0;
    approx::assert_abs_diff_eq!(result.ratio(), forces.n / capacity_n, epsilon = 1e-12);
    assert!(!result.ok());

    for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        forces.n = n;
        let outcome = check(&forces);
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason }
                if reason.contains("軸力") && reason.contains("非有限")),
            "N={n}: {outcome:?}"
        );
    }
}

#[test]
fn src_h_product_diagnosis_precedes_invalid_plate_thickness() {
    for column in [false, true] {
        let (model, mut ctx, forces) = inputs(column);
        let member = if column { "SRC柱" } else { "SRC梁" };

        for invalid_mm in [101.0, 0.0, f64::NAN, f64::INFINITY] {
            for (web_mm, flange_mm) in [(invalid_mm, 13.0), (8.0, invalid_mm)] {
                let mut section = model.sections[0].clone();
                match section.shape.as_mut().unwrap() {
                    SectionShape::SrcBeamRect {
                        steel_web_thick,
                        steel_flange_thick,
                        ..
                    }
                    | SectionShape::SrcColumnRect {
                        steel_web_thick,
                        steel_flange_thick,
                        ..
                    } => {
                        *steel_web_thick = web_mm;
                        *steel_flange_thick = flange_mm;
                    }
                    _ => panic!("SRC断面"),
                }
                for (grade, diagnostic) in [
                    ("STK400", "鋼管用"),
                    ("BCR295", "鋼管用"),
                    ("SS490", "溶接"),
                    ("SN400", "種類記号"),
                    ("SM520", "種類記号"),
                    ("TMCP355", "個別認定"),
                    ("SN400B", "実板厚"),
                ] {
                    ctx.steel_material.as_mut().unwrap().name = grade.into();
                    let outcome = SrcDesign.check(&forces, &section, &model.materials[0], &ctx);
                    assert!(
                        matches!(&outcome, CheckOutcome::Skipped { reason }
                            if reason.contains(member) && reason.contains(&section.name)
                            && reason.contains(grade) && reason.contains(diagnostic)),
                        "{grade}, web={web_mm}, flange={flange_mm}: {outcome:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn src_member_checks_skip_invalid_h_geometry_and_main_bar_interference() {
    for (column, height_mm, width_mm, web_mm, diagnostics) in [
        (
            true,
            400.0,
            90.0,
            95.0,
            ["ウェブ厚 95 mm", "フランジ幅 90 mm"],
        ),
        (true, 650.0, 200.0, 8.0, ["せい 650 mm", "断面せい 600 mm"]),
        (true, 475.0, 200.0, 8.0, ["SRC主筋", "干渉"]),
        (false, 475.0, 200.0, 8.0, ["SRC主筋", "干渉"]),
    ] {
        let (mut model, ctx, forces) = inputs(column);
        let section = &mut model.sections[0];
        match section.shape.as_mut().unwrap() {
            SectionShape::SrcBeamRect {
                steel_height,
                steel_width,
                steel_web_thick,
                ..
            }
            | SectionShape::SrcColumnRect {
                steel_height,
                steel_width,
                steel_web_thick,
                ..
            } => {
                *steel_height = height_mm;
                *steel_width = width_mm;
                *steel_web_thick = web_mm;
            }
            _ => panic!("SRC断面"),
        }
        let section = &model.sections[0];
        let member = if column { "SRC柱" } else { "SRC梁" };
        let assert_skipped = |outcome: &CheckOutcome| {
            assert!(
                matches!(outcome, CheckOutcome::Skipped { reason }
                    if reason.contains(member) && reason.contains(&section.name)
                    && diagnostics.iter().all(|diagnostic| reason.contains(diagnostic))),
                "{member} {height_mm}×{width_mm}×{web_mm}×13: {outcome:?}"
            );
        };
        assert_skipped(&SrcDesign.check(&forces, section, &model.materials[0], &ctx));

        let report = run_member_design_checks(
            &model,
            &[(
                ElemId(0),
                MemberForces {
                    at: vec![(forces.pos, [forces.n, 0.0, 0.0, 0.0, 0.0, forces.mz])],
                },
            )],
            &[],
            &MemberDesignCheckOptions {
                term: LoadTerm::Long,
                ..Default::default()
            },
        )
        .unwrap();
        let [(ElemId(0), 0.0, outcome)] = &report.member_checks[..] else {
            panic!("{:?}", report.member_checks);
        };
        assert_skipped(outcome);
    }
}
