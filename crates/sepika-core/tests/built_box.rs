use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory, PropertyBasis, SectionMassProperties};
use sepika_core::panel_zone::PanelGeometry;
use sepika_core::section_shape::SectionShape;

fn shape(t2: f64, cft: bool) -> SectionShape {
    if cft {
        SectionShape::CftBox {
            height: 300.0,
            width: 200.0,
            thick: 9.0,
            horizontal_thick: t2,
            corner_r: Some(0.0),
        }
    } else {
        SectionShape::SteelBox {
            height: 300.0,
            width: 200.0,
            thick: 9.0,
            horizontal_thick: t2,
            corner_r: Some(0.0),
        }
    }
}

fn material(category: MaterialCategory, density: f64, fc: Option<f64>) -> Material {
    Material {
        id: MaterialId(0),
        name: "材料".into(),
        category,
        young: 205000.0,
        poisson: 0.3,
        density,
        shear: None,
        fc,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    }
}

#[test]
fn built_box_geometry_matches_independent_outer_minus_inner_rectangles() {
    for (t2, area, iy, iz, zp) in [
        (6.0, 7584.0, 87700608.0, 55314368.0, 726048.0),
        (9.0, 8676.0, 109875852.0, 58328652.0, 881658.0),
        (12.0, 9768.0, 131127264.0, 61342936.0, 1033992.0),
    ] {
        for cft in [false, true] {
            let shape = shape(t2, cft);
            let section = shape.input_section(SectionId(0), "BOX".into()).unwrap();
            assert_eq!(shape.box_thicknesses(), Some((9.0, t2)));
            assert_eq!((section.area, section.iy, section.iz), (area, iy, iz));
            assert_eq!(section.iy / (section.depth / 2.0), iy / 150.0);
            assert_eq!(section.iz / (section.width / 2.0), iz / 100.0);
            if !cft {
                assert_eq!(shape.try_plastic_modulus_strong().unwrap(), Some(zp));
            }
            if t2 != 9.0 {
                assert_eq!(section.property_basis.j, PropertyBasis::PendingShape);
                assert_eq!(section.property_basis.as_y, PropertyBasis::PendingShape);
                assert_eq!(section.property_basis.as_z, PropertyBasis::PendingShape);
                assert!(section
                    .resolved_j()
                    .unwrap_err()
                    .contains("採用式・適用条件が未確認"));
                assert!(section
                    .ensure_properties_resolved()
                    .unwrap_err()
                    .contains("せん断性能"));
                assert!(shape.try_to_section(SectionId(0), "BOX".into()).is_err());
                let mut supplied = section.clone();
                supplied.property_basis.j = PropertyBasis::Supplied;
                supplied.j = 1.0;
                assert!(supplied.resolved_j().is_err());
                supplied.property_basis.as_y = PropertyBasis::Supplied;
                supplied.property_basis.as_z = PropertyBasis::Supplied;
                assert!(supplied.ensure_properties_resolved().is_err());
            } else {
                assert!(section.ensure_properties_resolved().is_ok());
            }
        }
    }
}

#[test]
fn built_box_mass_uses_each_material_region_without_requiring_torsion_or_shear() {
    let steel = material(MaterialCategory::Steel, 7.85e-9, None);
    let concrete = material(MaterialCategory::Concrete, 2.4e-9, Some(24.0));
    for (t2, area, core_area) in [
        (6.0, 7584.0, 52416.0),
        (9.0, 8676.0, 51324.0),
        (12.0, 9768.0, 50232.0),
    ] {
        let s = shape(t2, false)
            .input_section(SectionId(0), "鋼BOX".into())
            .unwrap();
        let p =
            SectionMassProperties::try_from_section(&s, Some(&steel), None, None, None).unwrap();
        assert!((p.mass_per_length - area * 7.85e-9).abs() < 1e-15);
        let cft = shape(t2, true);
        let core = cft.try_cft_core_props().unwrap().unwrap();
        assert_eq!(
            (core.inner_width, core.inner_height, core.area),
            (182.0, 300.0 - 2.0 * t2, core_area)
        );
        let s = cft.input_section(SectionId(0), "CFT".into()).unwrap();
        let p =
            SectionMassProperties::try_from_section(&s, Some(&concrete), None, None, Some(&steel))
                .unwrap();
        let rho_c = 23.0e-6 / sepika_core::units::GRAVITY_MM_S2;
        assert!((p.mass_per_length - (area * 7.85e-9 + core_area * rho_c)).abs() < 1e-15);
        if t2 != 9.0 {
            assert!(cft
                .try_cft_equivalent_props(205000.0, 0.3, 78846.0, 24.0, 23.0, 1.0)
                .unwrap_err()
                .contains("せん断性能"));
        }
    }
}

#[test]
fn built_box_panel_geometry_uses_directional_resisting_plate_thickness() {
    for cft in [false, true] {
        let s = shape(12.0, cft)
            .input_section(SectionId(0), "BOX".into())
            .unwrap();
        let axes = PanelGeometry::from_column_axes(&s).unwrap();
        assert_eq!((axes[0].tp, axes[0].dc), (9.0, 288.0));
        assert_eq!((axes[1].tp, axes[1].dc), (12.0, 191.0));
    }
}

#[test]
fn built_box_rejects_invalid_inner_dimensions_nonfinite_thickness_and_rounded_corners() {
    for t2 in [0.0, -1.0, 150.0, 151.0, f64::NAN, f64::INFINITY] {
        for cft in [false, true] {
            assert!(shape(t2, cft)
                .input_section(SectionId(0), "不正".into())
                .is_err());
        }
    }
    let mut s = shape(12.0, false);
    if let SectionShape::SteelBox { corner_r, .. } = &mut s {
        *corner_r = Some(10.0);
    }
    assert!(s
        .input_section(SectionId(0), "丸角".into())
        .unwrap_err()
        .contains("鋭角"));
    assert_ne!(shape(6.0, false), shape(12.0, false));
}
