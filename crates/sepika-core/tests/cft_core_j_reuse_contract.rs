use sepika_core::section_shape::{CftCoreTorsion, SectionShape};

fn shape(height: f64, radius: f64) -> SectionShape {
    SectionShape::CftBox {
        height,
        width: 300.0,
        thick: 12.0,
        horizontal_thick: 12.0,
        corner_r: Some(radius),
    }
}

#[test]
fn repeated_composite_preserves_supply_and_material_changes_and_fresh_diagnostics() {
    let shape = shape(400.0, 30.0);
    let first = shape
        .try_cft_equivalent_props(
            205000.0,
            0.3,
            205000.0 / (2.0 * (1.0 + 0.3)),
            60.0,
            24.0,
            1e8,
        )
        .unwrap()
        .unwrap();
    let changed = shape
        .try_cft_equivalent_props(
            410000.0,
            0.3,
            410000.0 / (2.0 * (1.0 + 0.3)),
            60.0,
            24.0,
            2e8,
        )
        .unwrap()
        .unwrap();
    assert!(((changed.j - 2e8) / (first.j - 1e8) - 0.5).abs() < 1e-14);
    for (es, nu, fc, gamma, js, reason) in [
        (f64::NAN, 0.3, 60.0, 24.0, 1e8, "Es"),
        (205000.0, 0.5, 60.0, 24.0, 1e8, "νs"),
        (205000.0, 0.3, -1.0, 24.0, 1e8, "Fc"),
        (205000.0, 0.3, 60.0, 0.0, 1e8, "γc"),
        (205000.0, 0.3, 60.0, 24.0, 0.0, "鋼管Js"),
    ] {
        assert!(shape
            .try_cft_equivalent_props(es, nu, es / (2.0 * (1.0 + nu)), fc, gamma, js)
            .unwrap_err()
            .contains(reason));
    }
    for _ in 0..2 {
        let CftCoreTorsion::Numerical(diagnostic) = shape.try_cft_core_torsion().unwrap().unwrap()
        else {
            panic!("角形コアは数値解");
        };
        assert!(diagnostic.levels.len() >= 9);
        assert!(diagnostic.total_seconds > 0.0);
        assert!(diagnostic.boundary_changes.is_some());
        assert!(diagnostic
            .levels
            .iter()
            .all(|level| level.relative_residual <= 1e-10));
        let expected = 1e8 + diagnostic.j_mm4 * (33500.0 / 2.4) / (205000.0 / 2.6);
        assert!((first.j / expected - 1.0).abs() < 1e-12);
    }
}

#[test]
fn radius_and_dimension_edits_use_their_own_core_j_and_invalid_geometry_is_rejected() {
    let mut values = Vec::new();
    for shape in [shape(400.0, 12.0), shape(400.0, 30.0), shape(410.0, 30.0)] {
        let fresh = shape.try_cft_core_torsion().unwrap().unwrap().j_mm4();
        for _ in 0..2 {
            let composite = shape
                .try_cft_equivalent_props(
                    205000.0,
                    0.3,
                    205000.0 / (2.0 * (1.0 + 0.3)),
                    60.0,
                    24.0,
                    1e8,
                )
                .unwrap()
                .unwrap();
            let expected = 1e8 + fresh * (33500.0 / 2.4) / (205000.0 / 2.6);
            assert!((composite.j / expected - 1.0).abs() < 1e-12);
        }
        values.push(fresh);
    }
    assert!(values[0] > values[1]);
    assert!(values[2] > values[1]);
    for shape in [
        shape(f64::NAN, 30.0),
        shape(400.0, f64::NAN),
        SectionShape::CftBox {
            height: 400.0,
            width: 300.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: None,
        },
    ] {
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
    let unresolved = SectionShape::CftBox {
        height: 400.0,
        width: 1e-9,
        thick: 1e-10,
        horizontal_thick: 1e-10,
        corner_r: Some(0.0),
    };
    for _ in 0..2 {
        assert!(unresolved
            .try_cft_equivalent_props(
                205000.0,
                0.3,
                205000.0 / (2.0 * (1.0 + 0.3)),
                60.0,
                24.0,
                1e8
            )
            .is_err());
        let error = unresolved.try_cft_core_torsion().unwrap_err();
        assert!(error.failed_attempt.is_some());
    }
}
#[test]
fn actual_steel_g_preserves_core_rigidity_and_rejects_invalid_g_after_reuse() {
    for shape in [
        shape(400.0, 30.0),
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 12.0,
        },
    ] {
        let gs_default = 205000.0 / 2.6;
        let supplied_steel_j = 1e8;
        let baseline = shape
            .try_cft_equivalent_props(205000.0, 0.3, gs_default, 60.0, 24.0, supplied_steel_j)
            .unwrap()
            .unwrap();
        let core_rigidity = gs_default * (baseline.j - supplied_steel_j);
        for gs in [gs_default / 2.0, gs_default * 2.0] {
            let changed = shape
                .try_cft_equivalent_props(205000.0, 0.3, gs, 60.0, 24.0, supplied_steel_j)
                .unwrap()
                .unwrap();
            assert!((gs * (changed.j - supplied_steel_j) / core_rigidity - 1.0).abs() < 1e-12);
            let rigidity_increment = gs * changed.j - gs_default * baseline.j;
            assert!(
                (rigidity_increment / ((gs - gs_default) * supplied_steel_j) - 1.0).abs() < 1e-12
            );
            assert_eq!(changed.area_ax, baseline.area_ax);
            assert_eq!(changed.iy, baseline.iy);
            assert_eq!(changed.iz, baseline.iz);
            for (changed_shear, baseline_shear) in
                [(changed.as_y, baseline.as_y), (changed.as_z, baseline.as_z)]
            {
                assert!(changed_shear.is_finite() && changed_shear > 0.0);
                assert_ne!(changed_shear, baseline_shear);
            }
        }
        for gs in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(shape
                .try_cft_equivalent_props(205000.0, 0.3, gs, 60.0, 24.0, supplied_steel_j)
                .unwrap_err()
                .contains("Gs"));
        }
    }
}
