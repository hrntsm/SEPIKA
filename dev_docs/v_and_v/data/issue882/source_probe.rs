use sepika_design_jp::secondary::ds_group::{
    rc_wall_side_column_eligible, rc_wall_tau_over_fc,
};

#[test]
fn source882_wall_stress_and_independent_dimension_boundaries() {
    for (q_n, expected) in [(2_880_000.0, 0.2), (-2_880_000.0, 0.2), (0.0, 0.0)] {
        let actual = rc_wall_tau_over_fc(q_n, 200.0, 3000.0, 24.0).unwrap();
        assert!((actual - expected).abs() < 1e-14);
        println!("stress: Q={q_n} N, ratio={actual}, expected={expected}");
    }
    for (area_mm2, expected) in [(149_999.0, false), (150_000.0, true), (150_001.0, true)] {
        let actual = rc_wall_side_column_eligible(2600.0, 1500.0, 200.0, area_mm2, 500.0);
        assert_eq!(actual, Some(expected));
        println!("area: {area_mm2} mm2, eligible={expected}");
    }
    for (diameter_mm, expected) in [(399.0, false), (400.0, true), (401.0, true)] {
        let actual = rc_wall_side_column_eligible(4800.0, 4800.0, 100.0, 300_000.0, diameter_mm);
        assert_eq!(actual, Some(expected));
        println!("sqrt(st/3): {diameter_mm} mm, eligible={expected}");
        let actual = rc_wall_side_column_eligible(2600.0, 1500.0, 200.0, 200_000.0, diameter_mm);
        assert_eq!(actual, Some(expected));
        println!("2t: {diameter_mm} mm, eligible={expected}");
    }
}
