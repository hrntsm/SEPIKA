use sepika_core::ids::SectionId;
use sepika_core::model::{Model, SectionMassProperties, SectionPropertyBasis};
use sepika_core::section_shape::SectionShape;

fn lip_channel(height: f64, width: f64, lip: f64, thick: f64) -> SectionShape {
    SectionShape::SteelLipChannel {
        height,
        width,
        lip,
        thick,
    }
}

#[test]
fn lip_channel_independent_rectangle_integrals_and_mass() {
    let shape = lip_channel(150.0, 75.0, 20.0, 2.3);
    let section = shape.try_to_section(SectionId(0), "LC".into()).unwrap();
    assert!((section.area - 760.84).abs() < 1e-9);
    assert!((section.iy - 2_804_950.304533333).abs() < 1e-6);
    assert!((section.iz - 600_198.7674505038).abs() < 1e-6);
    assert!((section.j - 1_341.6145333333333).abs() < 1e-9);
    let material = sepika_core::model::Material {
        id: sepika_core::ids::MaterialId(0),
        name: "steel".into(),
        category: sepika_core::model::MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    };
    let mass = SectionMassProperties::try_from_section(&section, Some(&material), None, None, None)
        .unwrap();
    assert!((mass.mass_per_length - 5.972594e-6).abs() < 1e-15);
    assert!((mass.total_mass(3000.0) - 0.017917782).abs() < 1e-12);
}

#[test]
fn lip_channel_open_geometry_boundaries_are_strict() {
    for shape in [
        lip_channel(150.0, 75.0, 20.0, 2.3),
        lip_channel(150.0, 75.0, 74.999, 2.3),
        lip_channel(150.0, 75.0, 2.300001, 2.3),
        lip_channel(150.0, 4.600001, 20.0, 2.3),
    ] {
        assert!(
            shape.try_to_section(SectionId(0), "LC".into()).is_ok(),
            "{shape:?}"
        );
    }
    for (shape, expected) in [
        (lip_channel(150.0, 75.0, 75.0, 2.3), "接触"),
        (lip_channel(150.0, 75.0, 75.001, 2.3), "重複"),
        (lip_channel(150.0, 75.0, 80.0, 2.3), "重複"),
        (lip_channel(150.0, 75.0, 100.0, 2.3), "重複"),
        (lip_channel(150.0, 75.0, 2.299999, 2.3), "C > t"),
        (lip_channel(150.0, 75.0, 2.3, 2.3), "C > t"),
        (lip_channel(150.0, 4.599999, 20.0, 2.3), "t < B/2"),
        (lip_channel(150.0, 4.6, 20.0, 2.3), "t < B/2"),
        (lip_channel(4.6, 75.0, 20.0, 2.3), "重複"),
        (lip_channel(150.0, 2.3, 20.0, 2.3), "t < B/2"),
        (lip_channel(1e200, 1e200, 1e199, 1e198), "有限な正値"),
        (
            lip_channel(f64::MAX, f64::MAX, f64::MAX / 4.0, 1.0),
            "有限な正値",
        ),
    ] {
        assert_rejected_at_all_result_entries(&shape, expected);
    }
}

fn assert_rejected_at_all_result_entries(shape: &SectionShape, expected: &str) {
    for result in [
        shape.try_calc_area(),
        shape.try_calc_iy(),
        shape.try_calc_iz(),
        shape.try_plastic_modulus_strong().map(|v| v.unwrap()),
    ] {
        let reason = result.unwrap_err();
        for part in ["H=", "B=", "C=", "t=", expected] {
            assert!(reason.contains(part), "{reason}");
        }
    }
    for result in [
        shape.try_to_section(SectionId(7), "LC-invalid".into()),
        shape.input_section(SectionId(7), "LC-invalid".into()),
    ] {
        let reason = result.unwrap_err();
        assert!(
            reason.contains("7") && reason.contains("LC-invalid") && reason.contains(expected),
            "{reason}"
        );
    }
    let mut section =
        lip_channel(150.0, 75.0, 20.0, 2.3).to_section(SectionId(0), "LC-invalid".into());
    section.shape = Some(shape.clone());
    section.property_basis = SectionPropertyBasis::default();
    assert!(section
        .ensure_properties_resolved()
        .unwrap_err()
        .contains(expected));
    assert!(section.resolved_area().unwrap_err().contains(expected));
    assert!(
        SectionMassProperties::try_from_section(&section, None, None, None, None)
            .unwrap_err()
            .contains(expected)
    );
    let mut model = Model::default();
    model.sections.push(section);
    assert!(model
        .validate_lip_channels()
        .unwrap_err()
        .to_string()
        .contains(expected));
    assert!(model.validate().unwrap_err().to_string().contains(expected));
}

#[test]
fn lip_channel_nonpositive_and_nonfinite_dimensions_are_rejected() {
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for shape in [
            lip_channel(value, 75.0, 20.0, 2.3),
            lip_channel(150.0, value, 20.0, 2.3),
            lip_channel(150.0, 75.0, value, 2.3),
            lip_channel(150.0, 75.0, 20.0, value),
        ] {
            assert_rejected_at_all_result_entries(&shape, "有限な正値");
        }
    }
}

#[test]
fn lip_channel_nonfinite_supplied_properties_and_infallible_failure() {
    let shape = lip_channel(150.0, 75.0, 100.0, 2.3);
    assert!(std::panic::catch_unwind(|| shape.calc_j()).is_err());
    let mut section = lip_channel(150.0, 75.0, 20.0, 2.3).to_section(SectionId(0), "LC".into());
    section.property_basis = SectionPropertyBasis::default();
    section.j = f64::INFINITY;
    assert!(section.validate_lip_channel().unwrap_err().contains("J"));
    section.shape = None;
    section.j = 1341.6145333333333;
    assert!(section.ensure_properties_resolved().is_ok());
    assert!((section.resolved_area().unwrap() - 760.84).abs() < 1e-9);
}
