#[test]
fn built_box_strength_uses_largest_real_plate_thickness() {
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_core::model::{Material, MaterialCategory};
    use sepika_core::section_shape::SectionShape;
    use sepika_design_jp::material_strength::{plate_thickness, section_steel_f};
    let material = Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    };
    for (t1, t2) in [(9.0, 41.0), (41.0, 9.0)] {
        for cft in [false, true] {
            let shape = if cft {
                SectionShape::CftBox {
                    height: 300.0,
                    width: 200.0,
                    thick: t1,
                    horizontal_thick: t2,
                    corner_r: Some(0.0),
                }
            } else {
                SectionShape::SteelBox {
                    height: 300.0,
                    width: 200.0,
                    thick: t1,
                    horizontal_thick: t2,
                    corner_r: Some(0.0),
                }
            };
            let section = shape.input_section(SectionId(0), "異厚BOX".into()).unwrap();
            assert_eq!(plate_thickness(&section), 41.0);
            assert_eq!(section_steel_f(&section, Some(&material)).unwrap(), 215.0);
        }
    }
}
