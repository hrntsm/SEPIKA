pub fn assert_reference(reference: &sepika_design_jp::wall_check::WallShearReference) {
    let close = |actual: f64, expected: f64| {
        assert!(
            (actual - expected).abs() <= expected.abs() * 1e-9,
            "actual={actual:.15}, expected={expected:.15}"
        );
    };
    close(reference.aw_qc_mm2, 840_000.0);
    close(reference.beta_s, 0.15130833333333333);
    assert_eq!(reference.displacement_points[0], (0.0, 0.0));
    assert_eq!(reference.angle_points[0], (0.0, 0.0));
    for (actual, expected) in reference
        .displacement_points
        .iter()
        .skip(1)
        .zip([(2.016, 2_016_000.0), (26.43608525637538, 4_000_000.0)])
    {
        close(actual.0, expected.0);
        close(actual.1, expected.1);
    }
    for (actual, expected) in reference
        .angle_points
        .iter()
        .skip(1)
        .zip([(0.000672, 2_016_000.0), (0.008812028418791793, 4_000_000.0)])
    {
        close(actual.0, expected.0);
        close(actual.1, expected.1);
    }
    close(reference.second_stiffness_n_per_mm, 81244.59759951428);
}
