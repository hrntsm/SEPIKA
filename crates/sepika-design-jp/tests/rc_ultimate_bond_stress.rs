#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column;

use sepika_core::{
    dof::Dof6Mask,
    model::{default_combinations, default_load_cases, Model},
    section_shape::{RebarSize, SectionShape, StandardRebar},
};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, RcUltimateBondAxis, RcUltimateBondBar,
    RcUltimateBondConfinement, RcUltimateBondDemand, RcUltimateBondElasticStress,
    RcUltimateBondInput, RcUltimateBondMomentDistribution, RcUltimateBondPlacement,
    RcUltimateBondTransverseRebar, RcUltimateCutoffBarStress, RcUltimateThroughBarYield,
    UltimateCheck, UltimateShearOptions,
};

fn member(is_beam: bool) -> Model {
    let mut model = if is_beam {
        beam::model(false)
    } else {
        column::column_model(false, 0.0)
    };
    let d19 = RebarSize::Standard(StandardRebar::D19);
    let d10 = RebarSize::Standard(StandardRebar::D10);
    match model.sections[0].shape.as_mut().unwrap() {
        SectionShape::RcBeamRect { rebar, .. } => {
            rebar.main_dia = d19;
            rebar.stirrup.dia = d10;
        }
        SectionShape::RcColumnRect { rebar, .. } => {
            rebar.main_dia = d19;
            rebar.hoop.dia = d10;
        }
        _ => unreachable!(),
    }
    model.load_cases = default_load_cases();
    model.combinations = default_combinations();
    model.ultimate_gravity_combination = Some(0);
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    model.validate().unwrap();
    model
}

fn input(is_beam: bool, bar: RcUltimateBondBar) -> RcUltimateBondInput {
    RcUltimateBondInput {
        bar,
        axis: RcUltimateBondAxis::Strong,
        effective_depth_mm: if is_beam { 290.5 } else { 540.5 },
        bar_designation_mm: 19.0,
        design_yield_stress_mpa: Some(if is_beam { 400.0 } else { 380.0 }),
        placement: RcUltimateBondPlacement::Other,
        confinement: Some(RcUltimateBondConfinement {
            clear_spacing_mm: if is_beam { 46.4 } else { 121.4 },
            minimum_cover_mm: 49.95,
            splitting_plane_bar_count: 3,
            transverse_rebar: Some(RcUltimateBondTransverseRebar {
                area_mm2: 142.66,
                spacing_mm: 100.0,
            }),
        }),
    }
}

fn through(yielding: RcUltimateThroughBarYield) -> RcUltimateBondBar {
    RcUltimateBondBar::Through {
        clear_span_mm: 3000.0,
        cutoff_unnecessary_distance_mm: None,
        yielding,
    }
}

fn cutoff(development_length_mm: f64, unnecessary_distance_mm: f64) -> RcUltimateBondBar {
    RcUltimateBondBar::Cutoff {
        clear_span_mm: 3000.0,
        development_length_mm,
        unnecessary_distance_mm,
        stress: RcUltimateCutoffBarStress::FlexuralYield,
    }
}

fn check(model: &Model, bond: RcUltimateBondInput) -> UltimateCheck {
    check_bond_demand(model, RcUltimateBondDemand::Single(bond))
}

fn check_bond_demand(model: &Model, bond: RcUltimateBondDemand) -> UltimateCheck {
    let demand = MemberDemand {
        bond: Some(bond),
        min_axial_for_shear_n: Some(0.0),
        ..MemberDemand::from_pushover(0.0, 10e6, 0.0, 1000.0, 0.0, 0.0)
    };
    let [result] = collect_rc_ultimate_checks(
        model,
        &[(model.elements[0].id, demand)],
        &UltimateShearOptions::default(),
    )
    .unwrap()
    .try_into()
    .expect("RC部材1本の終局検定結果");
    result
}

#[test]
fn cutoff_geometry_reason_is_kept_alongside_an_earlier_stress_reason_without_directions() {
    let model = member(false);
    let through = RcUltimateBondInput {
        confinement: None,
        ..input(
            false,
            RcUltimateBondBar::Through {
                clear_span_mm: 3000.0,
                cutoff_unnecessary_distance_mm: Some(200.0),
                yielding: RcUltimateThroughBarYield::BothEnds,
            },
        )
    };
    let cutoff = RcUltimateBondInput {
        effective_depth_mm: 541.0,
        ..input(false, cutoff(1600.0, 200.0))
    };
    let mut through_with_wrong_span = through;
    let RcUltimateBondBar::Through { clear_span_mm, .. } = &mut through_with_wrong_span.bar else {
        unreachable!()
    };
    *clear_span_mm = 2999.0;
    for (through, stress_reason) in [
        (through, "横補強筋を確認できません"),
        (
            through_with_wrong_span,
            "付着照査の内法長さが部材の内法長さと一致しません",
        ),
    ] {
        let result = check_bond_demand(
            &model,
            RcUltimateBondDemand::ThroughAndCutoff { through, cutoff },
        );
        assert!(result.beam_bond.is_none() && result.bond_development_length_ok.is_none());
        assert!(result
            .bond_unchecked_reason
            .as_deref()
            .unwrap()
            .contains(stress_reason));
        assert!(result
            .detail
            .contains("付着照査の有効せいが検討方向の配筋と一致しません"));
    }
}

#[test]
fn one_layer_beam_and_column_bond_stresses_match_independent_values() {
    let beam = member(true);
    let column = member(false);
    let mut weak_column_cutoff = input(false, cutoff(1290.5, 950.0));
    weak_column_cutoff.axis = RcUltimateBondAxis::Weak;
    weak_column_cutoff.effective_depth_mm = 340.5;
    weak_column_cutoff
        .confinement
        .as_mut()
        .unwrap()
        .clear_spacing_mm = 221.4;
    for (model, bond, tau_d, kfb, margin, length) in [
        (
            &beam,
            input(true, through(RcUltimateThroughBarYield::BothEnds)),
            1.402472780955896,
            2.599957894736842,
            1.8538383988919667,
            None,
        ),
        (
            &beam,
            input(true, cutoff(1480.5, 1190.0)),
            1.596638655462185,
            2.599957894736842,
            1.6283946814404429,
            Some(true),
        ),
        (
            &column,
            input(
                false,
                through(RcUltimateThroughBarYield::OneEndWithOtherElastic),
            ),
            0.7338890018296401,
            3.75,
            5.109764542936288,
            None,
        ),
        (
            &column,
            weak_column_cutoff,
            1.9,
            3.75,
            1.973684210526316,
            Some(true),
        ),
    ] {
        let result = check(model, bond);
        let stress = result.bond_stress.expect("付着応力度を算定できる");
        assert!((stress.demand_mpa - tau_d).abs() < 1e-12, "{result:?}");
        assert!((stress.capacity_mpa - kfb).abs() < 1e-12, "{result:?}");
        assert!((result.bond_margin - margin).abs() < 1e-12, "{result:?}");
        assert_eq!(result.bond_development_length_ok, length, "{result:?}");
        assert_eq!(result.bond_cutoff_absent, length.is_none(), "{result:?}");
        assert_eq!(result.bond_unchecked_reason, None, "{result:?}");
    }
}

#[test]
fn independent_cutoff_length_is_checked_at_equality_even_without_an_allowable_stress() {
    let column = member(false);
    for (ld, expected) in [(640.800, true), (640.799, false)] {
        let result = check(
            &column,
            RcUltimateBondInput {
                confinement: None,
                ..input(false, cutoff(ld, 100.3))
            },
        );
        assert!(
            result.bond_stress.is_none() && result.bond_margin.is_nan(),
            "{result:?}"
        );
        assert!(
            result
                .bond_unchecked_reason
                .as_deref()
                .unwrap()
                .contains("横補強筋を確認できません"),
            "{result:?}"
        );
        assert_eq!(
            result.bond_development_length_ok,
            Some(expected),
            "{result:?}"
        );
        assert!(!result.ok && !result.bond_cutoff_absent, "{result:?}");
        assert_eq!(result.unchecked_reason.is_some(), expected, "{result:?}");
    }
}

#[test]
fn cutoff_length_must_cover_both_the_input_and_the_model_effective_depth() {
    let column = member(false);
    for (input_depth, ld, expected) in [
        (540.4999998, 640.7999999, false),
        (540.4999998, 640.8, true),
        (540.5000002, 640.8000001, false),
    ] {
        let result = check(
            &column,
            RcUltimateBondInput {
                effective_depth_mm: input_depth,
                confinement: None,
                ..input(false, cutoff(ld, 100.3))
            },
        );
        assert!(result.bond_stress.is_none(), "{result:?}");
        assert_eq!(
            result.bond_development_length_ok,
            Some(expected),
            "{result:?}"
        );
        assert!(!result.ok, "{result:?}");
        assert_eq!(result.unchecked_reason.is_none(), !expected, "{result:?}");
    }
}

#[test]
fn decimal_display_equality_does_not_override_a_shortfall_in_the_stored_lengths() {
    let mut column = member(false);
    let Some(SectionShape::RcColumnRect { d, .. }) = column.sections[0].shape.as_mut() else {
        unreachable!()
    };
    *d = 599.6;
    let result = check(
        &column,
        RcUltimateBondInput {
            effective_depth_mm: 540.1,
            confinement: None,
            ..input(false, cutoff(640.3, 100.2))
        },
    );
    assert_eq!(result.bond_development_length_ok, Some(false), "{result:?}");
    assert!(result.bond_stress.is_none() && result.bond_unchecked_reason.is_some());
    assert!(!result.ok && result.unchecked_reason.is_none());
}

#[test]
fn zero_tensile_demand_of_either_sign_has_positive_infinite_bond_margin() {
    let column = member(false);
    for stress in [0.0, -0.0] {
        let result = check(
            &column,
            input(
                false,
                through(RcUltimateThroughBarYield::WithoutFlexuralYield {
                    moment_distribution: RcUltimateBondMomentDistribution::NearPinnedEnd,
                    stress: RcUltimateBondElasticStress {
                        long_term_tensile_stress_mpa: stress,
                        lateral_tensile_stress_mpa: stress,
                        lateral_factor: 1.5,
                    },
                }),
            ),
        );
        assert_eq!(result.bond_stress.unwrap().demand_mpa, 0.0, "{result:?}");
        assert_eq!(result.bond_margin, f64::INFINITY, "{result:?}");
        assert!(result.bond_unchecked_reason.is_none(), "{result:?}");
        assert!(result.ok, "{result:?}");
    }
}

#[test]
fn through_only_needs_matching_geometry_but_not_a_calculated_stress() {
    let beam = member(true);
    let valid = input(true, through(RcUltimateThroughBarYield::BothEnds));
    let unchecked = check(
        &beam,
        RcUltimateBondInput {
            confinement: None,
            ..valid
        },
    );
    assert!(unchecked.bond_stress.is_none() && unchecked.bond_cutoff_absent);
    for (invalid, reason) in [
        (
            RcUltimateBondInput {
                axis: RcUltimateBondAxis::Weak,
                ..valid
            },
            "強軸方向に限ります",
        ),
        (
            RcUltimateBondInput {
                effective_depth_mm: 300.0,
                ..valid
            },
            "有効せいが検討方向",
        ),
        (
            RcUltimateBondInput {
                bar_designation_mm: 22.0,
                ..valid
            },
            "鉄筋呼び名がモデルと一致しません",
        ),
    ] {
        let result = check(&beam, invalid);
        assert!(!result.bond_cutoff_absent);
        assert!(result.bond_development_length_ok.is_none());
        assert!(result
            .bond_unchecked_reason
            .as_deref()
            .unwrap()
            .contains(reason));
    }
}

#[test]
fn details_identify_the_stress_failure_when_the_rounded_margin_is_one() {
    let result = check(&member(true), input(true, cutoff(1021.2, 200.0)));
    assert!((result.bond_margin - 0.9998890703601107).abs() < 1e-12);
    assert_eq!(result.bond_development_length_ok, Some(true));
    assert!(!result.ok && result.unchecked_reason.is_none());
    assert!(
        result.detail.contains("付着余裕度=1.000（応力度NG）"),
        "{}",
        result.detail
    );
}
