#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column_input;

use sepika_core::{
    dof::Dof6Mask,
    ids::ElemId,
    model::{default_combinations, default_load_cases, Model},
    section_shape::{RebarSize, SectionShape},
};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, StaticBeamHingeMechanism, UltimateCheck,
    UltimateShearOptions,
};

const GRAVITY_SHEAR_N: f64 = 12_000.0;

fn static_beam() -> (Model, [(ElemId, MemberDemand); 1]) {
    let mut model = beam_input::model(false);
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.ultimate_gravity_combination = Some(0);
    let elem = &mut model.elements[0];
    elem.rigid_zone.face_i = Some(0.0);
    elem.rigid_zone.face_j = Some(0.0);
    let demand = MemberDemand {
        mz: 1.0e7,
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::IBottomJTop),
        q_long: Some(GRAVITY_SHEAR_N),
        ..Default::default()
    };
    let input = [(elem.id, demand)];
    (model, input)
}

#[test]
fn static_hinge_rejects_nonpositive_and_nonfinite_strength_factors() {
    let (model, input) = static_beam();
    for factor in [0.0, -0.0, -0.5, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let opts = UltimateShearOptions {
            upper_strength_factor: factor,
            ..Default::default()
        };
        let reason = collect_rc_ultimate_checks(&model, &input, &opts).unwrap_err();
        assert!(
            reason.contains("ultimate_upper_strength_factor_invalid:"),
            "{reason}"
        );
        assert!(reason.contains("member=0"), "{reason}");
    }
}

#[test]
fn a_positive_factor_does_not_verify_the_beam_required_shear_upper_bound() {
    let (model, input) = static_beam();
    for factor in [f64::from_bits(1), 0.5, 1.0, 1.5] {
        let opts = UltimateShearOptions {
            upper_strength_factor: factor,
            ..Default::default()
        };
        let check = collect_rc_ultimate_checks(&model, &input, &opts)
            .unwrap()
            .remove(0);
        assert!(check.mu > 0.0 && check.qsu > 0.0 && check.qbu > 0.0);
        assert!(!check.ok && check.qmu.is_nan());
        assert!(check.shear_margin.is_nan() && check.bond_margin.is_nan());
        assert!(check
            .unchecked_reason
            .unwrap()
            .contains("ultimate_beam_hinge_shear_upper_bound_unverified:"));
    }
}

fn static_column(strong_bars: u32, weak_bars: u32) -> Model {
    let mut model = column_input::column_model(false, 150_000.0);
    let section = &mut model.sections[0];
    let Some(SectionShape::RcColumnRect { mut rebar, .. }) = section.shape.take() else {
        unreachable!();
    };
    rebar.main_dia = RebarSize::DiameterMm(25.0);
    rebar.x = vec![strong_bars];
    rebar.y = vec![weak_bars];
    let shape = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar,
    };
    *section = sepika_core::model::Section {
        material: section.material,
        rebar_material: section.rebar_material,
        shear_rebar_material: section.shear_rebar_material,
        frame_use: section.frame_use,
        ..shape.to_section(section.id, section.name.clone())
    };
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    model.load_cases = default_load_cases();
    model.combinations = default_combinations();
    model.ultimate_gravity_combination = Some(0);
    model.validate().unwrap();
    model
}

fn column_check(
    model: &Model,
    factor: f64,
    axial_n: f64,
    ql_n: f64,
    biaxial_shear: bool,
) -> UltimateCheck {
    let demand = MemberDemand {
        q_long: Some(ql_n),
        q_long_weak: biaxial_shear.then_some(0.0),
        ..MemberDemand::axial(axial_n)
    };
    let opts = UltimateShearOptions {
        upper_strength_factor: factor,
        biaxial_shear,
        ..Default::default()
    };
    collect_rc_ultimate_checks(model, &[(model.elements[0].id, demand)], &opts)
        .unwrap()
        .remove(0)
}

fn assert_relative(actual: f64, expected: f64) {
    assert!(
        (actual / expected - 1.0).abs() < 1e-12,
        "actual={actual:e}, expected={expected:e}"
    );
}

#[test]
fn static_column_compares_hinge_shear_when_the_product_would_overflow() {
    let model = static_column(4, 4);
    for (factor, qmu_n, shear, bond, biaxial) in [
        (
            1e302,
            2.462_490_597_643_624e307,
            2.946_831_844_255_06e-302,
            6.554_773_169_187_389e-302,
            2.083_724_780_089_213e-302,
        ),
        (
            7e302,
            1.723_743_418_350_537e308,
            4.209_759_777_507_229e-303,
            9.363_961_670_267_7e-303,
            2.976_749_685_841_733e-303,
        ),
    ] {
        let check = column_check(&model, factor, 150_000.0, 0.0, true);
        assert_relative(check.qmu, qmu_n);
        assert_relative(check.shear_margin, shear);
        assert_relative(check.bond_margin, bond);
        assert_relative(check.biaxial_shear_margin.unwrap(), biaxial);
        assert!(!check.ok);
        assert_eq!(check.unchecked_reason, None);
    }
}

#[test]
fn static_column_reports_an_unrepresentable_hinge_demand_by_axis() {
    let model = static_column(4, 4);
    let check = column_check(&model, 1e303, 150_000.0, 0.0, true);
    assert!(!check.ok && check.qmu.is_nan());
    assert!(check.shear_margin.is_nan() && check.bond_margin.is_nan());
    assert_eq!(check.biaxial_shear_margin, None);
    let reason = check.unchecked_reason.unwrap();
    assert!(reason.contains("ultimate_hinge_shear_overflow"), "{reason}");
    assert!(
        reason.contains("強軸") && reason.contains("弱軸"),
        "{reason}"
    );
    assert!(reason.contains("member=0"), "{reason}");
    assert!(check.detail.contains("Qmu=未算定"), "{}", check.detail);

    let check = column_check(&model, 1e303, 150_000.0, 2_000_000.0, false);
    assert!(!check.ok && check.qmu.is_nan());
    assert_eq!(check.shear_margin, 0.0);
    assert_eq!(check.bond_margin, 0.0);
    assert_eq!(check.unchecked_reason, None);
    assert!(check.detail.contains("ultimate_hinge_shear_overflow"));
}

#[test]
fn static_column_reports_weak_axis_overflow_alongside_a_strong_axis_failure() {
    let model = static_column(2, 6);
    let check = column_check(&model, 7e302, 150_000.0, 0.0, true);
    assert_relative(check.qmu, 9.650_487_925_086_016e307);
    assert!(check.shear_margin > 0.0 && check.shear_margin < 1.0);
    assert!(!check.ok);
    assert_eq!(check.biaxial_shear_margin, None);
    assert_eq!(check.unchecked_reason, None);
    assert!(check.detail.contains("ultimate_hinge_shear_overflow: 弱軸"));
}

#[test]
fn static_column_preserves_a_weak_axis_failure_when_strong_axis_demand_overflows() {
    let model = static_column(6, 2);
    let check = column_check(&model, 7e302, 150_000.0, 0.0, true);
    assert!(!check.ok && check.qmu.is_nan());
    assert!(check.shear_margin.is_nan() && check.bond_margin.is_nan());
    assert_eq!(check.biaxial_shear_margin, None);
    assert_eq!(check.unchecked_reason, None);
    assert!(check.detail.contains("ultimate_hinge_shear_overflow: 強軸"));
    assert!(check.detail.contains("弱軸せん断耐力を超えます"));
}

#[test]
fn static_column_preserves_zero_hinge_shear_at_the_largest_factor() {
    let model = static_column(4, 4);
    let equality = column_check(&model, f64::MAX, 20_000_000.0, 0.0, false);
    for (ql_n, shear, bond, ok) in [
        (equality.qsu, f64::INFINITY, f64::INFINITY, true),
        (equality.qsu + 1.0, 0.0, f64::INFINITY, false),
        (equality.qbu + 1.0, 0.0, 0.0, false),
    ] {
        let check = column_check(&model, f64::MAX, 20_000_000.0, ql_n, false);
        assert_eq!(check.mu, 0.0);
        assert_eq!(check.qmu, 0.0);
        assert_eq!(check.shear_margin, shear);
        assert_eq!(check.bond_margin, bond);
        assert_eq!(check.ok, ok);
        assert_eq!(check.unchecked_reason, None);
        assert!(check.detail.contains("長期せん断力を耐力と直接比較"));
    }
}

#[test]
fn static_column_distinguishes_positive_subnormal_shear_from_underflow_to_zero() {
    let mut model = static_column(4, 4);
    let min_positive = f64::from_bits(1);
    let check = column_check(&model, min_positive, 150_000.0, 0.0, true);
    assert_eq!(check.qmu, f64::from_bits(246_249));
    assert!(check.ok);
    assert_eq!(check.biaxial_shear_margin, Some(f64::INFINITY));
    assert!(!check.detail.contains("Qmu=0 N"));

    model.nodes[1].coord[2] = 1e12;
    let check = column_check(&model, min_positive, 150_000.0, 0.0, true);
    assert!(check.mu > 0.0 && check.qmu.is_nan());
    assert!(check.shear_margin.is_nan() && check.bond_margin.is_nan());
    assert!(!check.ok);
    assert_eq!(check.biaxial_shear_margin, None);
    let reason = check.unchecked_reason.unwrap();
    assert!(
        reason.contains("ultimate_hinge_shear_underflow"),
        "{reason}"
    );
    assert!(
        reason.contains("強軸") && reason.contains("弱軸"),
        "{reason}"
    );
}

#[test]
fn static_column_preserves_the_normal_factor_comparison_with_gravity_shear() {
    let check = column_check(&static_column(4, 4), 1.1, 150_000.0, 12_000.0, false);
    assert_relative(check.qmu, 270_873.965_740_798_6);
    assert_relative(check.shear_margin, 2.634_636_994_292_733_5);
    assert_relative(check.bond_margin, 5.914_583_653_322_124);
    assert!(check.ok);
    assert_eq!(check.unchecked_reason, None);
}
