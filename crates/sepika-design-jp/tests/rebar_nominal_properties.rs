#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod input;

use sepika_core::{
    dof::Dof6Mask,
    ids::{ElemId, SectionId},
    model::{MaterialCategory, Model},
    section_shape::{
        RebarSize::{DiameterMm, Standard},
        SectionShape,
        StandardRebar::{D10, D19},
    },
};
use sepika_design_jp::{
    ultimate::{
        collect_rc_ultimate_checks, MemberDemand, StaticBeamHingeMechanism, UltimateShearOptions,
    },
    BondMethod, CheckKind, CheckOutcome, CheckResult, DesignCheck, DesignCtx, LoadTerm,
    MemberForcesAt, MemberKind, RcDesign, SrcDesign,
};
use std::f64::consts::PI;

fn beam_model() -> Model {
    let mut model = input::model(false);
    model.nodes[1].coord[0] = 4000.0;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    model.ultimate_gravity_combination = Some(0);
    model.sections[0] = input::section(350.0, 609.5, 19.0, &[2], &[2]);
    model
}

fn assert_near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9 * expected.abs().max(1.0),
        "actual={actual} expected={expected}",
    );
}

fn checked(outcome: CheckOutcome) -> CheckResult {
    match outcome {
        CheckOutcome::Checked(result) => result,
        CheckOutcome::Skipped { reason } => panic!("{reason}"),
    }
}

fn ratio(result: &CheckResult, kind: CheckKind) -> f64 {
    result
        .components
        .iter()
        .find(|c| c.kind == kind)
        .unwrap()
        .ratio
}

#[test]
fn beam_ultimate_reference_mu_uses_nominal_main_bar_area() {
    let mut model = beam_model();
    let static_demand = MemberDemand {
        mz: -10e6,
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
        q_long: Some(560_400.0),
        ..Default::default()
    };
    let total_demand = MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, -10e6, 0.0, 580_000.0, 0.0, 0.0)
    };
    let opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    for (main, shear, mu) in [
        (DiameterMm(19.0), DiameterMm(10.0), 96_839_240.117_732),
        (Standard(D19), DiameterMm(10.0), 97_854_075.0),
        (Standard(D19), Standard(D10), 97_854_075.0),
    ] {
        let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
            unreachable!()
        };
        rebar.main_dia = main;
        rebar.stirrup.dia = shear;
        for demand in [static_demand, total_demand] {
            let result =
                &collect_rc_ultimate_checks(&model, &[(ElemId(0), demand)], &opts).unwrap()[0];
            assert_near(result.mu, mu);
            assert_eq!(result.qsu.is_finite(), demand.shear.is_some());
            if let Some(shear) = demand.shear {
                assert!(result.unchecked_reason.is_none(), "{result:?}");
                assert_near(result.qmu, shear);
                assert!(!result.ok, "{result:?}");
            } else {
                assert!(!result.ok && result.qmu.is_nan(), "{result:?}");
                assert!(result
                    .unchecked_reason
                    .as_deref()
                    .unwrap()
                    .contains("ultimate_beam_hinge_shear_upper_bound_unverified:"));
            }
        }
    }
}

fn beam_forces() -> MemberForcesAt {
    MemberForcesAt {
        pos: 0.0,
        n: 0.0,
        qy: 20_000.0,
        qz: 0.0,
        mx: 0.0,
        my: 0.0,
        mz: -30e6,
    }
}

fn beam_ctx(model: &Model) -> DesignCtx {
    DesignCtx {
        rebar_material: Some(model.materials[1].clone()),
        shear_rebar_material: Some(model.materials[1].clone()),
        length: 4000.0,
        ..Default::default()
    }
}

#[test]
fn beam_bond_checks_use_perimeter_and_transverse_area() {
    let mut model = beam_model();
    let mut ctx = beam_ctx(&model);
    let forces = beam_forces();
    for (main, shear, bond_1991, length_1999) in [
        (
            DiameterMm(19.0),
            DiameterMm(10.0),
            0.226_050_287_1,
            202.580_379_6,
        ),
        (Standard(D19), Standard(D10), 0.224_883_341_8, 203.693_634_6),
    ] {
        let sec = &mut model.sections[0];
        let Some(SectionShape::RcBeamRect { rebar, .. }) = sec.shape.as_mut() else {
            unreachable!()
        };
        rebar.main_dia = main;
        rebar.stirrup.dia = shear;
        ctx.bond_method = BondMethod::Rc1991;
        ctx.term = LoadTerm::Long;
        let result = checked(RcDesign.check(&forces, sec, &model.materials[0], &ctx));
        assert_near(ratio(&result, CheckKind::Bond), bond_1991);

        ctx.bond_method = BondMethod::Rc1999;
        ctx.term = LoadTerm::Short;
        let result = checked(RcDesign.check(&forces, sec, &model.materials[0], &ctx));
        assert_near(ratio(&result, CheckKind::Bond), length_1999 / 2275.0);
    }
}

#[test]
fn rc_and_src_beam_bending_and_rc_beam_shear_use_nominal_areas() {
    let mut model = beam_model();
    let mut ctx = beam_ctx(&model);
    ctx.steel_material = Some(input::material("SS400", MaterialCategory::Steel));
    let forces = beam_forces();

    let j = 481.25;
    let fc_shear = 0.73;
    let alpha = 4.0 / (1.0 + 30e6 / (20_000.0 * 550.0));
    // 内蔵 H-250×100×20×12 の断面係数。外形矩形からウェブ両側の矩形を控除。
    let z_steel = (100.0 * 250.0_f64.powi(3) - 80.0 * 226.0_f64.powi(3)) / (6.0 * 250.0);

    for (main, shear, at, aw) in [
        (
            DiameterMm(19.0),
            DiameterMm(10.0),
            2.0 * PI * 9.5_f64.powi(2),
            2.0 * PI * 25.0,
        ),
        (Standard(D19), Standard(D10), 573.0, 142.66),
    ] {
        let sec = &mut model.sections[0];
        let Some(SectionShape::RcBeamRect { rebar, .. }) = sec.shape.as_mut() else {
            unreachable!()
        };
        rebar.main_dia = main;
        rebar.stirrup.dia = shear;
        let rebar = rebar.clone();
        let pw = aw / 35_000.0;
        let rc_ma = at * 215.0 * j;
        let rc = checked(RcDesign.check(&forces, sec, &model.materials[0], &ctx));
        assert_near(ratio(&rc, CheckKind::Bending), 30e6 / rc_ma);
        assert_near(
            ratio(&rc, CheckKind::Shear),
            20_000.0 / (350.0 * j * (alpha * fc_shear + 97.5 * (pw - 0.002))),
        );

        let src = SectionShape::SrcBeamRect {
            b: 350.0,
            d: 609.5,
            rebar,
            steel_height: 250.0,
            steel_width: 100.0,
            steel_web_thick: 20.0,
            steel_flange_thick: 12.0,
        }
        .to_section(SectionId(0), "SRC梁".into());
        let src_forces = MemberForcesAt { qy: 0.0, ..forces };
        let src = checked(SrcDesign.check(&src_forces, &src, &model.materials[0], &ctx));
        assert_near(
            ratio(&src, CheckKind::Bending),
            30e6 / (z_steel * 235.0 / 1.5 + rc_ma),
        );
        assert_eq!(src.components.len(), 1);
    }
}

#[test]
fn rc_column_axial_checks_use_nominal_area_for_all_main_bars() {
    let mut model = beam_model();
    let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_ref() else {
        unreachable!()
    };
    let section = SectionShape::RcColumnCircle {
        d: 600.0,
        rebar: sepika_core::section_shape::RcCircleColumnRebar {
            main_dia: Standard(D19),
            count: 8,
            cover: 40.0,
            hoop: sepika_core::section_shape::CircleColumnHoop {
                dia: rebar.stirrup.dia,
                pitch: 100.0,
            },
        },
    };
    let old_section = &model.sections[0];
    let mut column = section.to_section(old_section.id, old_section.name.clone());
    column.frame_use = column.rc_frame_use();
    column.material = old_section.material;
    column.rebar_material = old_section.rebar_material;
    column.shear_rebar_material = old_section.shear_rebar_material;
    model.sections[0] = column;
    let mut ctx = beam_ctx(&model);
    ctx.kind = MemberKind::Column;
    let forces = MemberForcesAt {
        n: 200_000.0,
        qy: 0.0,
        mz: 0.0,
        ..beam_forces()
    };
    let result = checked(RcDesign.check(&forces, &model.sections[0], &model.materials[0], &ctx));
    assert_near(
        ratio(&result, CheckKind::AxialBending),
        200_000.0 / (8.0 * 286.5 * 215.0),
    );

    let demand = MemberDemand::from_pushover(-200_000.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let result = &collect_rc_ultimate_checks(
        &model,
        &[(ElemId(0), demand)],
        &UltimateShearOptions::default(),
    )
    .unwrap()[0];
    assert_near(result.axial.unwrap().nut, -8.0 * 286.5 * 345.0);
}

#[test]
fn standard_deformed_size_with_round_bar_grade_is_not_checked() {
    let mut model = beam_model();
    model.materials[1] = input::material("SR235", MaterialCategory::Rebar);
    let ctx = beam_ctx(&model);
    let result = RcDesign.check(
        &beam_forces(),
        &model.sections[0],
        &model.materials[0],
        &ctx,
    );
    assert!(matches!(result, CheckOutcome::Checked(_)), "{result:?}");

    let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        unreachable!()
    };
    rebar.main_dia = Standard(D19);
    let result = RcDesign.check(
        &beam_forces(),
        &model.sections[0],
        &model.materials[0],
        &ctx,
    );
    assert!(
        matches!(result, CheckOutcome::Skipped { reason } if reason.contains("D19") && reason.contains("SR235"))
    );
    let demand = MemberDemand {
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, -10e6, 0.0, 580_000.0, 0.0, 0.0)
    };
    let error = collect_rc_ultimate_checks(
        &model,
        &[(ElemId(0), demand)],
        &UltimateShearOptions::default(),
    )
    .unwrap_err();
    assert!(error.contains("D19") && error.contains("SR235"), "{error}");
}
