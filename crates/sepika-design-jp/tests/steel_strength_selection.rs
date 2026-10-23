use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory, Section};
use sepika_design_jp::{
    floor, CheckOutcome, DesignCheck, DesignCtx, MemberForcesAt, MemberKind, SteelDesign,
};

fn material(name: &str, fy: Option<f64>) -> Material {
    Material {
        id: MaterialId(0),
        name: name.into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy,
        strength_factor: None,
        concrete_class: Default::default(),
    }
}

#[test]
fn issue601_table_strength_uses_actual_maximum_thickness_and_ignores_saved_fy() {
    use sepika_core::section_shape::SectionShape;
    use sepika_design_jp::material_strength::section_steel_f;
    for (web_mm, flange_mm, expected) in [
        (40.0, 12.0, 235.0),
        (12.0, 40.0, 235.0),
        (40.0001, 12.0, 215.0),
        (12.0, 50.0, 215.0),
    ] {
        let sec = SectionShape::SteelH {
            height: 600.0,
            width: 300.0,
            web_thick: web_mm,
            flange_thick: flange_mm,
            root_r: Some(0.0),
        }
        .to_section(SectionId(0), "H実厚".into());
        for fy in [None, Some(235.0), Some(999.0)] {
            assert_eq!(
                section_steel_f(&sec, Some(&material("SS400", fy))).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn issue601_direct_fy_requires_no_thickness_bucket_but_invalid_shapes_are_rejected() {
    use sepika_core::material_grade::resolve_steel_f;
    use sepika_core::section_shape::SectionShape;
    use sepika_design_jp::material_strength::section_steel_f;
    let sec = Section::zero(SectionId(0), "断面諸量直接入力".into());
    assert_eq!(
        section_steel_f(&sec, Some(&material("直接鋼材", Some(277.0)))).unwrap(),
        277.0
    );
    for fy in [
        None,
        Some(0.0),
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        let error = resolve_steel_f(Some(&material("直接鋼材", fy)), None).unwrap_err();
        assert!(error.contains("fy"));
        assert!(error.contains("直接鋼材"));
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let shape = SectionShape::SteelH {
            height: 600.0,
            width: 300.0,
            web_thick: invalid,
            flange_thick: 50.0,
            root_r: Some(0.0),
        };
        let sec = Section {
            shape: Some(shape),
            ..sec.clone()
        };
        assert!(section_steel_f(&sec, Some(&material("直接鋼材", Some(277.0)))).is_err());
    }
}

#[test]
fn issue601_missing_wrong_material_and_known_out_of_range_do_not_fall_back() {
    use sepika_core::material_grade::resolve_steel_f;
    assert!(resolve_steel_f(None, Some(50.0))
        .unwrap_err()
        .contains("未割当"));
    let mut nonsteel = material("SS400", Some(235.0));
    nonsteel.category = MaterialCategory::Concrete;
    assert!(resolve_steel_f(Some(&nonsteel), Some(50.0))
        .unwrap_err()
        .contains("鋼材ではありません"));
    for name in ["SS400", "SN490B", "SM520C"] {
        for thickness in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(100.0001),
        ] {
            assert!(
                resolve_steel_f(Some(&material(name, Some(999.0))), thickness).is_err(),
                "{name} {thickness:?}"
            );
        }
    }
    assert_eq!(
        resolve_steel_f(Some(&material("SS400", None)), Some(100.0)).unwrap(),
        215.0
    );
    assert_eq!(
        resolve_steel_f(Some(&material("BCP325", None)), Some(100.0001)).unwrap(),
        325.0
    );
}

#[test]
fn issue601_supplied_properties_do_not_hide_invalid_actual_dimensions() {
    use sepika_core::section_shape::SectionShape;
    use sepika_design_jp::material_strength::section_steel_f;
    for shape in [
        SectionShape::SteelH {
            height: 0.0,
            width: 300.0,
            web_thick: 12.0,
            flange_thick: 20.0,
            root_r: Some(0.0),
        },
        SectionShape::SteelPipe {
            outer_dia: 20.0,
            thick: 12.0,
        },
        SectionShape::SteelFlatBar {
            width: f64::NAN,
            thick: 12.0,
        },
        SectionShape::SteelBuiltH {
            height: 40.0,
            upper_width: 300.0,
            upper_thick: 20.0,
            lower_width: 300.0,
            lower_thick: 20.0,
            web_thick: 12.0,
        },
    ] {
        let sec = Section {
            shape: Some(shape),
            area: 10_000.0,
            iy: 1.0e8,
            iz: 1.0e8,
            depth: 600.0,
            width: 300.0,
            ..Section::zero(SectionId(0), "供給性能と不正形状".into())
        };
        assert!(section_steel_f(&sec, Some(&material("直接鋼材", Some(277.0)))).is_err());
    }
}

#[test]
fn issue601_thick_secondary_and_primary_use_the_same_table_strength() {
    let material = Material {
        id: MaterialId(0),
        name: "SS400".into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    };
    let section = Section {
        area: 10_000.0,
        iy: 1.0e8,
        iz: 1.0e8,
        depth: 400.0,
        width: 200.0,
        thickness: Some(50.0),
        ..Section::zero(SectionId(0), "実厚50mm".into())
    };
    let (_, ft) = floor::beam_steel_e_and_ft(&section, Some(&material)).unwrap();
    let secondary =
        floor::design_beam_from_forces(4_000.0, 0.0, 150.0e6, 0.0, 0.0, 1.0e6, ft, 250.0);
    assert!((secondary.bending_ratio - 1.0465116279069768).abs() < 1e-12);
    assert!(!secondary.ok);
    assert_eq!(
        sepika_design_jp::steel_f_value_prefix("SS400", 50.0),
        Some(215.0)
    );
    let forces = MemberForcesAt {
        pos: 0.5,
        n: 1.5e6,
        qy: 0.0,
        qz: 0.0,
        my: 0.0,
        mz: 0.0,
    };
    let context = DesignCtx {
        kind: MemberKind::Brace,
        ..Default::default()
    };
    let CheckOutcome::Checked(primary) = SteelDesign.check(&forces, &section, &material, &context)
    else {
        panic!("通常材が未検定")
    };
    assert!((primary.ratio() - 1.0465116279069768).abs() < 1e-12);
    let missing_thickness = Section {
        thickness: None,
        ..section
    };
    assert!(
        matches!(SteelDesign.check(&forces, &missing_thickness, &material, &context), CheckOutcome::Skipped { reason } if reason.contains("実板厚"))
    );
    assert!(floor::beam_steel_e_and_ft(&missing_thickness, Some(&material)).is_err());
    assert!(floor::beam_steel_e_and_ft(&missing_thickness, None).is_err());
}

fn primary_model() -> sepika_core::model::Model {
    use sepika_core::ids::{ElemId, NodeId};
    use sepika_core::model::*;
    use sepika_core::section_shape::SectionShape;
    let mut section = SectionShape::SteelH {
        height: 600.0,
        width: 300.0,
        web_thick: 12.0,
        flange_thick: 50.0,
        root_r: Some(0.0),
    }
    .to_section(SectionId(0), "鋼断面".into());
    section.material = Some(MaterialId(0));
    sepika_core::model::Model {
        sections: vec![section],
        materials: vec![material("SS400", Some(235.0))],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            plastic_zone: None,
            spring: None,
        }],
        ..Default::default()
    }
}

#[test]
fn public_batch_rejects_invalid_steel_inputs_even_without_force_rows() {
    use sepika_design_jp::run_member_design_checks;
    let base = primary_model();
    for case in 0..10 {
        let mut model = base.clone();
        match case {
            0 => model.sections[0].material = None,
            1 => model.sections[0].material = Some(MaterialId(99)),
            2 => model.materials[0].category = MaterialCategory::Concrete,
            3 => model.elements[0].section = None,
            4 => model.elements[0].section = Some(SectionId(99)),
            5 => {
                model.sections[0].shape = None;
                model.sections[0].thickness = None;
            }
            6 => {
                model.sections[0].shape = None;
                model.sections[0].thickness = Some(101.0);
            }
            7..=9 => {
                model.materials[0].name = "直接鋼材".into();
                model.materials[0].fy = [None, Some(0.0), Some(f64::INFINITY)][case - 7];
            }
            _ => unreachable!(),
        }
        let error = run_member_design_checks(&model, &[], &[], &Default::default()).unwrap_err();
        assert!(error.contains("部材 0"), "case={case}: {error}");
    }
    let mut known_no_fy = base;
    known_no_fy.materials[0].fy = None;
    assert!(run_member_design_checks(&known_no_fy, &[], &[], &Default::default()).is_ok());
}

#[test]
fn shared_secondary_inputs_reject_missing_and_wrong_assignments_and_preserve_rc_exclusion() {
    use sepika_core::model::SecondaryMember;
    let mut model = primary_model();
    model.elements.clear();
    model.unassigned_beams.push(SecondaryMember {
        section: Some(SectionId(0)),
        ..Default::default()
    });
    assert!(sepika_design_jp::validate_model_steel_inputs(&model).is_ok());
    for reference in [None, Some(MaterialId(99))] {
        model.sections[0].material = reference;
        let error = sepika_design_jp::validate_model_steel_inputs(&model).unwrap_err();
        assert!(error.contains("小梁 0"));
    }
    model.sections[0].material = Some(MaterialId(0));
    model.materials[0].category = MaterialCategory::Concrete;
    assert!(sepika_design_jp::validate_model_steel_inputs(&model)
        .unwrap_err()
        .contains("鋼材ではありません"));
    model.sections[0].shape = None;
    assert!(
        floor::secondary_beam_steel_e_and_ft(&model, &model.unassigned_beams[0])
            .unwrap()
            .is_none()
    );
    model.unassigned_beams[0].section = None;
    assert!(sepika_design_jp::validate_model_steel_inputs(&model)
        .unwrap_err()
        .contains("断面"));
}
