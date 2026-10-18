use sepika_core::section_shape::{CftCoreTorsion, CoreTorsionSolution, SectionShape};

fn box_shape(r: Option<f64>) -> SectionShape {
    SectionShape::CftBox {
        height: 400.0,
        width: 300.0,
        thick: 12.0,
        corner_r: r,
    }
}

fn pipe() -> SectionShape {
    SectionShape::CftPipe {
        outer_dia: 400.0,
        thick: 12.0,
    }
}

fn quality(solution: &CoreTorsionSolution, rounded: bool) {
    assert!(solution.levels.len() >= 3);
    assert!(solution.internal_changes.into_iter().all(|v| v <= 0.001));
    assert_eq!(solution.boundary_changes.is_some(), rounded);
    if let Some(changes) = solution.boundary_changes {
        assert!(changes.into_iter().all(|v| v <= 0.001));
    }
    assert!(solution
        .levels
        .iter()
        .all(|level| level.relative_residual <= 1e-10));
    let accepted = &solution.levels[solution.accepted_level];
    assert_eq!(accepted.j_hat, solution.j_hat);
    assert!(accepted.free_dofs > 0);
}

#[test]
fn rounded_and_rectangular_core_j_match_independent_quadratic_warping_references() {
    for (r, reference) in [
        (0.0, 1_449_158_972.081_095_5),
        (12.0, 1_449_158_972.081_095_5),
        (30.0, 1_448_741_968.108_572),
    ] {
        let result = box_shape(Some(r)).try_cft_core_torsion().unwrap().unwrap();
        assert!((result.j_mm4() / reference - 1.0).abs() <= 0.001);
        let CftCoreTorsion::Numerical(solution) = result else {
            panic!("角形コアは数値解");
        };
        quality(&solution, r > 12.0);
        eprintln!(
            "ro={r}, J={}, levels={}, seconds={}",
            solution.j_mm4,
            solution.levels.len(),
            solution.total_seconds
        );
    }
}

#[test]
fn circular_core_has_analytic_value_without_numerical_quality_levels() {
    let result = pipe().try_cft_core_torsion().unwrap().unwrap();
    let CftCoreTorsion::AnalyticCircle { j_mm4 } = result else {
        panic!("円形コアは解析値");
    };
    assert!((j_mm4 / 1_962_236_157.627_097 - 1.0).abs() < 1e-14);
    let core = pipe().try_cft_core_props().unwrap().unwrap();
    assert_eq!(core.inner_width, 376.0);
    assert_eq!(core.inner_height, 376.0);
    assert_eq!(core.iy, core.iz);
    assert_eq!(j_mm4, 2.0 * core.iy);
}

#[test]
fn supplied_steel_j_difference_reaches_composite_j_without_recalculation() {
    for shape in [box_shape(Some(30.0)), pipe()] {
        let first = shape
            .try_cft_equivalent_props(
                205_000.0,
                0.3,
                205_000.0 / (2.0 * (1.0 + 0.3)),
                60.0,
                24.0,
                100_000_000.0,
            )
            .unwrap()
            .unwrap();
        let second = shape
            .try_cft_equivalent_props(
                205_000.0,
                0.3,
                205_000.0 / (2.0 * (1.0 + 0.3)),
                60.0,
                24.0,
                123_000_000.0,
            )
            .unwrap()
            .unwrap();
        assert!((second.j - first.j - 23_000_000.0).abs() < 1e-6);
        assert_eq!(first.area_ax, second.area_ax);
        assert_eq!(first.iy, second.iy);
        assert_eq!(first.iz, second.iz);
        assert_eq!(first.as_y, second.as_y);
        assert_eq!(first.as_z, second.as_z);
        let reference = if matches!(shape, SectionShape::CftBox { .. }) {
            1_448_741_968.108_572
        } else {
            1_962_236_157.627_097
        };
        // Fc60・γ24のEc=33500、Gs=205000/2.6、Gc=33500/2.4。
        assert!(
            ((first.j - 100_000_000.0) / (reference * (33_500.0 / 2.4) / (205_000.0 / 2.6)) - 1.0)
                .abs()
                <= 0.001
        );
    }
}

#[test]
fn supplied_steel_j_keeps_core_available_outside_shape_steel_j_domain() {
    for shape in [
        box_shape(Some(6.0)),
        SectionShape::CftBox {
            height: 100.0,
            width: 100.0,
            thick: 20.0,
            corner_r: Some(25.0),
        },
    ] {
        assert!(shape.try_calc_j().is_err());
        assert!(shape.try_cft_core_props().unwrap().is_some());
        let p = shape
            .try_cft_equivalent_props(
                205_000.0,
                0.3,
                205_000.0 / (2.0 * (1.0 + 0.3)),
                60.0,
                24.0,
                1_000_000.0,
            )
            .unwrap()
            .unwrap();
        assert!(p.j > 1_000_000.0);
    }
}

#[test]
fn only_non_cft_shapes_return_none() {
    let shape = SectionShape::SteelPipe {
        outer_dia: 400.0,
        thick: 12.0,
    };
    assert!(shape.try_cft_core_props().unwrap().is_none());
    assert!(shape.try_cft_core_torsion().unwrap().is_none());
    assert!(shape
        .try_cft_equivalent_props(
            f64::NAN,
            f64::NAN,
            f64::NAN / (2.0 * (1.0 + f64::NAN)),
            f64::NAN,
            f64::NAN,
            f64::NAN
        )
        .unwrap()
        .is_none());
}

#[test]
fn invalid_radius_and_dimensions_are_reasoned_errors_in_both_core_apis() {
    for shape in [
        box_shape(None),
        box_shape(Some(-1.0)),
        box_shape(Some(151.0)),
        box_shape(Some(f64::NAN)),
        SectionShape::CftBox {
            height: 400.0,
            width: 24.0,
            thick: 12.0,
            corner_r: Some(0.0),
        },
        SectionShape::CftPipe {
            outer_dia: 24.0,
            thick: 12.0,
        },
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: -1.0,
        },
        SectionShape::CftPipe {
            outer_dia: f64::INFINITY,
            thick: 12.0,
        },
    ] {
        assert!(!shape.try_cft_core_props().unwrap_err().is_empty());
        let error = shape.try_cft_core_torsion().unwrap_err();
        assert!(!error.reason.is_empty());
        assert!(error.levels.is_empty());
        assert!(error.failed_attempt.is_none());
        assert!(shape
            .try_cft_equivalent_props(
                205_000.0,
                0.3,
                205_000.0 / (2.0 * (1.0 + 0.3)),
                60.0,
                24.0,
                1_000_000.0
            )
            .is_err());
    }
    assert!(box_shape(None)
        .try_cft_core_torsion()
        .unwrap_err()
        .reason
        .contains("未知"));
}

#[test]
fn material_errors_remain_errors_and_are_detected_before_numerical_core() {
    let shape = box_shape(None);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for (es, nu, fc, gamma, js, name) in [
            (bad, 0.3, 60.0, 24.0, 1e8, "Es"),
            (205000.0, 0.3, bad, 24.0, 1e8, "Fc"),
            (205000.0, 0.3, 60.0, bad, 1e8, "γc"),
            (205000.0, 0.3, 60.0, 24.0, bad, "鋼管Js"),
        ] {
            let error = shape
                .try_cft_equivalent_props(es, nu, es / (2.0 * (1.0 + nu)), fc, gamma, js)
                .unwrap_err();
            assert!(error.contains(name), "{error}");
        }
    }
    for nu in [-1.0, 0.5, -2.0, 1.0, f64::NAN, f64::INFINITY] {
        assert!(shape
            .try_cft_equivalent_props(205000.0, nu, 205000.0 / (2.0 * (1.0 + nu)), 60.0, 24.0, 1e8)
            .unwrap_err()
            .contains("νs"));
    }
    for (es, fc, gamma, js) in [
        (f64::MAX, 60.0, 24.0, 1e8),
        (f64::MIN_POSITIVE, 60.0, 24.0, 1e8),
        (205000.0, 60.0, f64::MAX, 1e8),
        (205000.0, 60.0, 24.0, f64::MAX),
    ] {
        assert!(pipe()
            .try_cft_equivalent_props(es, 0.3, es / (2.0 * (1.0 + 0.3)), fc, gamma, js)
            .is_err());
    }
    let small_steel_j = f64::from_bits(1);
    assert_eq!((1e-200 / 2.6) * small_steel_j, 0.0);
    let p = pipe()
        .try_cft_equivalent_props(
            1e-200,
            0.3,
            1e-200 / (2.0 * (1.0 + 0.3)),
            60.0,
            24.0,
            small_steel_j,
        )
        .unwrap()
        .unwrap();
    assert!(p.j.is_finite() && p.j > 0.0);
    assert!((1e-200 / 2.6) * p.j > 0.0);
    for nu in [-0.5, 0.0, 0.49] {
        assert!(pipe()
            .try_cft_equivalent_props(205000.0, nu, 205000.0 / (2.0 * (1.0 + nu)), 60.0, 24.0, 1e8)
            .unwrap()
            .is_some());
    }
}

#[test]
fn numerical_failure_retains_completed_levels_through_public_diagnostic_api() {
    let shape = SectionShape::CftBox {
        height: 400e100,
        width: 300e100,
        thick: 12e100,
        corner_r: Some(30e100),
    };
    let error = shape.try_cft_core_torsion().unwrap_err();
    assert!(error.reason.contains("尺度復元"));
    assert!(error.levels.len() >= 3);
    assert!(error
        .levels
        .iter()
        .all(|level| level.relative_residual <= 1e-10));
    assert!(error.total_seconds > 0.0);
}

#[test]
fn core_geometry_is_available_when_numerical_torsion_cannot_be_resolved() {
    let shape = SectionShape::CftBox {
        height: 400.0,
        width: 1e-9,
        thick: 1e-10,
        corner_r: Some(0.0),
    };
    let core = shape.try_cft_core_props().unwrap().unwrap();
    assert!(core.area > 0.0 && core.iy > 0.0 && core.iz > 0.0);
    let error = shape.try_cft_core_torsion().unwrap_err();
    assert!(!error.reason.is_empty());
    assert!(error.failed_attempt.is_some());
    assert!(shape
        .try_cft_equivalent_props(
            205000.0,
            0.3,
            205000.0 / (2.0 * (1.0 + 0.3)),
            60.0,
            24.0,
            1e8
        )
        .is_err());
}
