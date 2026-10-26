#[path = "../../../test_fixtures/rc_wall617.rs"]
mod fixture;

use sepika_core::model::RcWallShearMethod::{ArakawaMean, ArakawaMin};
use sepika_core::rc_capacity::{
    rc_column_mu_simple, rc_crack_moment, rc_mu_simple, rc_qmu_simple, rc_qsu_simple,
    RcCapacityInput,
};
use sepika_core::rc_wall_capacity::{wall_shear_ultimate, RcWallShearInput};
use sepika_core::section_shape::SectionShape;
use sepika_element::factory::StrengthBasis;
use sepika_element::wall::wall_element::WallElement;

fn member() -> RcCapacityInput {
    RcCapacityInput {
        b: 400.0,
        d: 500.0,
        at: 2000.0,
        d_eff: 450.0,
        sigma_y: 400.0,
        fc: 25.0,
        pw: 0.004,
        sigma_wy: 250.0,
        clear_span: 6000.0,
        sigma_0: 0.0,
    }
}

#[test]
fn source883_beam_moment_and_zero_axial_cracking() {
    assert_eq!(rc_mu_simple(&member()), 324_000_000.0);
    assert_eq!(rc_qmu_simple(&member()), 108_000.0);
    assert!((rc_crack_moment(25.0, 18_000_000.0) - 50_400_000.0).abs() < 1e-7);
    println!("beam Mu=324000000 Nmm Qmu=108000 N; Mc(N=0,supplied Ze18000000)=50400000 Nmm");
}

#[test]
fn source883_column_all_branches_and_one_newton_boundaries() {
    for (n, expected) in [
        (-1_600_001.0, 0.0),
        (-1_600_000.0, 0.0),
        (-800_000.0, 160_000_000.0),
        (-1.0, 319_999_800.0),
        (0.0, 320_000_000.0),
        (1.0, 320_000_249.99995),
        (1_000_000.0, 520_000_000.0),
        (1_999_999.0, 619_999_949.99995),
        (2_000_000.0, 620_000_000.0),
        (2_000_001.0, 619_999_865.2173913),
        (4_300_000.0, 310_000_000.0),
        (6_600_000.0, 0.0),
        (6_600_001.0, 0.0),
    ] {
        let actual = rc_column_mu_simple(&member(), 4000.0, n);
        assert!(
            (actual - expected).abs() < 1e-6,
            "N={n}: {actual} vs {expected}"
        );
        println!("column N={n} N Mu={actual} Nmm expected={expected}");
    }
}

fn wall() -> RcWallShearInput {
    RcWallShearInput {
        method: ArakawaMin,
        fc: 25.0,
        te: 200.0,
        t: 200.0,
        d_wall: 4000.0,
        dc_compression: 0.0,
        tension_column_at: 8000.0,
        sigma_wh: 250.0,
        pwh_ratio: 0.004,
        sigma_0: 1.0,
        shear_span_ratio: 1.0,
        opening: None,
    }
}

#[test]
fn source883_wall_methods_and_source_span_limits() {
    for (method, q1, q3) in [
        (ArakawaMin, 2_089_375.0, 1_176_314.1025641025),
        (ArakawaMean, 2_599_044.208388216, 1_823_772.3167435793),
    ] {
        let mut input = wall();
        input.method = method;
        for (span, expected) in [(0.5, q1), (1.0, q1), (3.0, q3), (4.0, q3)] {
            input.shear_span_ratio = span;
            let actual = wall_shear_ultimate(&input);
            assert!((actual - expected).abs() < 1e-6);
            println!("wall {method:?} lambda={span} Qu={actual} N expected={expected}");
        }
    }
}

#[test]
fn source883_wall_horizontal_and_axial_caps_are_current_behavior() {
    for (method, q0, qmax) in [
        (ArakawaMin, 1_494_375.0, 2_524_945.230503482),
        (ArakawaMean, 2_004_044.208388216, 3_034_614.4388916977),
    ] {
        let mut input = wall();
        input.method = method;
        for (horizontal, expected) in [(0.0, q0), (0.012, qmax), (0.02, qmax)] {
            input.pwh_ratio = horizontal;
            let actual = wall_shear_ultimate(&input);
            assert!((actual - expected).abs() < 1e-6);
            println!("wall {method:?} horizontal={horizontal} Qu={actual} N expected={expected}");
        }
    }
    let mut input = wall();
    for (axial, expected) in [
        (-1.0, 2_019_375.0),
        (0.0, 2_019_375.0),
        (10.0, 2_719_375.0),
        (11.0, 2_719_375.0),
    ] {
        input.sigma_0 = axial;
        assert!((wall_shear_ultimate(&input) - expected).abs() < 1e-6);
        println!("core wall sigma0={axial} MPa Qu={expected} N (current behavior, not source endorsement)");
    }
}

#[test]
fn source883_public_wall_exceeds_vertical_reinforcement_limit() {
    let mut model = fixture::expanded_model();
    if let Some(SectionShape::RcColumnRect { b, d, .. }) = &mut model.sections[0].shape {
        *b = 300.0;
        *d = 300.0;
    }
    if let Some(SectionShape::RcWall {
        thickness,
        pwh_ratio,
        pwv_ratio,
        ..
    }) = &mut model.sections[2].shape
    {
        *thickness = 300.0;
        *pwh_ratio = Some(0.012);
        *pwv_ratio = Some(0.002);
    }
    let wall = model.elements[4].clone();
    for (method, observed, limited) in [
        (ArakawaMin, 2_881_619.4568420467, 2_145_279.177584084),
        (ArakawaMean, 3_602_890.7995258814, 2_866_550.5202679187),
    ] {
        model.stress_cfg.rc_wall_shear_method = method;
        let actual = WallElement::directional_shear_capacity_at_reference(
            &wall,
            &model,
            StrengthBasis::Nominal,
            0.0,
            [1.5; 2],
        )
        .unwrap();
        assert!(actual.iter().all(|q| (*q - observed).abs() < 1e-6));
        if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut model.sections[2].shape {
            *pwh_ratio = Some(0.004);
        }
        let capped = WallElement::directional_shear_capacity_at_reference(
            &wall,
            &model,
            StrengthBasis::Nominal,
            0.0,
            [1.5; 2],
        )
        .unwrap();
        assert!(capped.iter().all(|q| (*q - limited).abs() < 1e-6));
        println!("public element {method:?}: horizontal=.012 vertical=.002 Qu={actual:?} N; source-limited horizontal=.004 Qu={capped:?} N; excess=736340.279257963 N");
        if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut model.sections[2].shape {
            *pwh_ratio = Some(0.012);
        }
    }
}

#[test]
fn source883_tensile_member_axial_stress_is_zeroed() {
    let zero = rc_qsu_simple(&member());
    let mut input = member();
    input.sigma_0 = -1.0;
    assert_eq!(rc_qsu_simple(&input), zero);
    println!("member sigma0=-1 and 0 MPa both Qsu={zero} N; applying signed 0.1sigma0 term would subtract 15750 N (not a validated tensile formula)");
}
