use sepika_core::ids::SectionId;
use sepika_core::model::PropertyBasis;
use sepika_core::section_shape::SectionShape;

fn h(r: Option<f64>) -> SectionShape {
    SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
        root_r: r,
    }
}

fn tube(height: f64, width: f64, thick: f64, r: Option<f64>, cft: bool) -> SectionShape {
    if cft {
        SectionShape::CftBox {
            height,
            width,
            thick,
            corner_r: r,
        }
    } else {
        SectionShape::SteelBox {
            height,
            width,
            thick,
            corner_r: r,
        }
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual / expected - 1.0).abs() <= 1e-10,
        "{actual} != {expected}"
    );
}

#[test]
fn h_conventional_torsion_is_available_without_root_radius() {
    for r in [Some(0.0), Some(13.0), None] {
        let shape = h(r);
        close(shape.try_calc_j().unwrap(), 356_762.666_666_666_7);
        let section = shape.input_section(SectionId(0), "H".into()).unwrap();
        close(section.resolved_j().unwrap(), 356_762.666_666_666_7);
        if r.is_none() {
            assert_eq!(section.property_basis.area, PropertyBasis::PendingShape);
            assert!(section.resolved_area().is_err());
        }
    }
}

#[test]
fn box_centerline_j_matches_fixed_values_and_axis_exchange() {
    for cft in [false, true] {
        for (r, expected) in [
            (0.0, 443_315_557.491_124_3),
            (12.0, 446_472_065.379_393_4),
            (30.0, 453_213_215.442_827_4),
        ] {
            for (height, width) in [(400.0, 300.0), (300.0, 400.0)] {
                close(
                    tube(height, width, 12.0, Some(r), cft)
                        .try_calc_j()
                        .unwrap(),
                    expected,
                );
            }
        }
        assert!(tube(400.0, 300.0, 12.0, Some(150.0), cft)
            .try_calc_j()
            .is_ok());
        assert!(tube(400.0, 300.0, 12.0, Some(150.0001), cft)
            .try_calc_j()
            .is_err());
        assert!(tube(120.0, 120.0, 12.0, Some(12.0), cft)
            .try_calc_j()
            .is_ok());
        assert!(tube(119.99, 120.0, 12.0, Some(12.0), cft)
            .try_calc_j()
            .unwrap_err()
            .contains("min(H, B)/t"));
        assert!(tube(60.0, 60.0, 12.0, Some(0.0), cft).try_calc_j().is_ok());
    }
}

#[test]
fn box_rejects_unknown_invalid_and_out_of_domain_with_specific_reasons() {
    for cft in [false, true] {
        assert!(tube(400.0, 300.0, 12.0, None, cft)
            .try_calc_j()
            .unwrap_err()
            .contains("未知"));
        for r in [0.001, 11.999] {
            assert!(tube(400.0, 300.0, 12.0, Some(r), cft)
                .try_calc_j()
                .unwrap_err()
                .contains("0 < 外角R < 板厚"));
        }
        for r in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(tube(400.0, 300.0, 12.0, Some(r), cft).try_calc_j().is_err());
        }
        for (height, width, thick) in [
            (0.0, 300.0, 12.0),
            (400.0, f64::NAN, 12.0),
            (400.0, 300.0, 0.0),
            (24.0, 300.0, 12.0),
        ] {
            assert!(tube(height, width, thick, Some(0.0), cft)
                .try_calc_j()
                .is_err());
        }
    }
}

#[test]
fn property_basis_controls_j_instead_of_stored_cache() {
    let mut section = tube(400.0, 300.0, 12.0, Some(30.0), false)
        .try_to_section(SectionId(0), "Box".into())
        .unwrap();
    for stale in [443_315_557.491_124_3, f64::NAN, -1.0] {
        section.j = stale;
        close(section.resolved_j().unwrap(), 453_213_215.442_827_4);
        assert!(section.ensure_properties_resolved().is_ok());
    }
    section.property_basis.j = PropertyBasis::Supplied;
    section.j = 123.0;
    assert_eq!(section.resolved_j().unwrap(), 123.0);
    let edited = section.with_surface_radius(Some(6.0)).unwrap();
    assert_eq!(edited.resolved_j().unwrap(), 123.0);
    assert_eq!(edited.property_basis.j, PropertyBasis::Supplied);
    assert!(edited.resolved_area().is_ok());
    section.property_basis.j = PropertyBasis::PendingShape;
    assert!(section.resolved_j().unwrap_err().contains("未算定"));
    assert!(section.ensure_properties_resolved().is_err());
    section.property_basis.j = PropertyBasis::Supplied;
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        section.j = invalid;
        assert!(section.resolved_j().is_err());
    }
    section.j = 0.0;
    assert_eq!(section.resolved_j().unwrap(), 0.0);
}

#[test]
fn radius_edits_and_input_keep_geometry_when_only_j_is_unavailable() {
    for cft in [false, true] {
        let shape = tube(400.0, 300.0, 12.0, Some(6.0), cft);
        assert!(shape.try_to_section(SectionId(0), "Box".into()).is_err());
        let section = shape.input_section(SectionId(0), "Box".into()).unwrap();
        assert_eq!(section.property_basis.j, PropertyBasis::PendingShape);
        assert!(section
            .resolved_j()
            .unwrap_err()
            .contains("0 < 外角R < 板厚"));
        assert!(section.resolved_area().unwrap() > 0.0);
        let rounded = section.with_surface_radius(Some(30.0)).unwrap();
        close(rounded.resolved_j().unwrap(), 453_213_215.442_827_4);
        assert_ne!(rounded.area, section.area);
        let pending = rounded.with_surface_radius(Some(6.0)).unwrap();
        assert!(pending.resolved_area().is_ok());
        assert!(pending.resolved_j().is_err());
        let unknown = rounded.with_surface_radius(None).unwrap();
        assert_eq!(unknown.property_basis.area, PropertyBasis::PendingShape);
        assert_eq!(unknown.property_basis.j, PropertyBasis::PendingShape);
        assert!(unknown.resolved_j().unwrap_err().contains("未知"));
    }
}

#[test]
fn lip_channel_shape_j_resolution_does_not_recurse_or_consume_stale_cache() {
    let mut section = SectionShape::SteelLipChannel {
        height: 150.0,
        width: 75.0,
        lip: 20.0,
        thick: 2.3,
    }
    .try_to_section(SectionId(0), "LC".into())
    .unwrap();
    section.j = f64::NAN;
    close(section.resolved_j().unwrap(), 1_341.614_533_333_333_3);
    assert!(section.validate_lip_channel().is_ok());
    assert!(section.ensure_properties_resolved().is_ok());
}
