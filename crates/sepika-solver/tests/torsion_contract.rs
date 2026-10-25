use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::{concrete_young_modulus_gamma, SectionShape};
use sepika_solver::statics::analysis::{
    precheck::{model_issues, IssueSeverity, IssueTargets},
    Analysis,
};

const LENGTH_MM: f64 = 3000.0;
const TORQUE_NMM: f64 = 1.0e6;
const STEEL_G_MPA: f64 = 205_000.0 / 2.6;

fn cantilever(shape: SectionShape, kind: ElementKind) -> Model {
    let cft = matches!(
        shape,
        SectionShape::CftBox { .. } | SectionShape::CftPipe { .. }
    );
    let mut section = shape
        .input_section(SectionId(0), "ねじり試験断面".into())
        .unwrap();
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(if cft { 1 } else { 0 }));
    section.steel_material = cft.then_some(MaterialId(0));
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0; 3],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, LENGTH_MM],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![
            Material {
                id: MaterialId(0),
                name: "SN400B".into(),
                category: MaterialCategory::Steel,
                young: 205_000.0,
                poisson: 0.3,
                density: 0.0,
                shear: None,
                fc: None,
                fy: Some(235.0),
                concrete_class: Default::default(),
                strength_factor: None,
            },
            Material {
                id: MaterialId(1),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 25_000.0,
                poisson: 0.2,
                density: 2.3e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
                concrete_class: Default::default(),
                strength_factor: None,
            },
        ],
        load_cases: vec![LoadCase {
            id: LoadCaseId(0),
            name: "純ねじり".into(),
            kind: Default::default(),
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, 0.0, 0.0, 0.0, TORQUE_NMM],
            )],
            member: vec![],
        }],
        girder_torsion: GirderTorsionMode::Keep,
        ..Default::default()
    }
}

fn assert_twist(model: &Model, gj_nmm2: f64) {
    let before = model.clone();
    let result = Analysis::prepare(model)
        .unwrap()
        .linear_static(LoadCaseId(0))
        .unwrap();
    let expected_rad = TORQUE_NMM * LENGTH_MM / gj_nmm2;
    let actual_rad = result.disp[1][5];
    assert!(
        (actual_rad / expected_rad - 1.0).abs() <= 1e-8,
        "theta={actual_rad}, TL/GJ={expected_rad}"
    );
    assert!(model.eq_ignoring_dofmap(&before));
}

fn cft_box() -> SectionShape {
    SectionShape::CftBox {
        height: 400.0,
        width: 300.0,
        thick: 12.0,
        horizontal_thick: 12.0,
        corner_r: Some(30.0),
    }
}

#[test]
fn public_analysis_uses_current_h_and_rounded_box_j_in_tl_over_gj() {
    for root_r in [0.0, 13.0] {
        let mut model = cantilever(
            SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(root_r),
            },
            ElementKind::Beam,
        );
        model.sections[0].j = 1.0;
        assert_twist(&model, STEEL_G_MPA * 356_762.666_666_666_7);
    }
    let mut model = cantilever(
        SectionShape::SteelBox {
            height: 400.0,
            width: 300.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: Some(30.0),
        },
        ElementKind::Beam,
    );
    model.sections[0].j = 1.0;
    assert_twist(&model, STEEL_G_MPA * 453_213_215.442_827_4);
    model.sections[0].j = 8.0e8;
    model.sections[0].property_basis.j = PropertyBasis::Supplied;
    assert_twist(&model, STEEL_G_MPA * 8.0e8);
}

#[test]
fn public_cft_beam_and_fiber_use_supplied_steel_j_and_actual_steel_g() {
    let pipe = SectionShape::CftPipe {
        outer_dia: 400.0,
        thick: 12.0,
    };
    let pipe_core_j_mm4 = std::f64::consts::PI * 188.0_f64.powi(4) / 2.0;
    let rounded_box = cft_box();
    let box_core_j_mm4 = rounded_box.try_cft_core_torsion().unwrap().unwrap().j_mm4();
    let gamma_c = sepika_core::units::concrete_unit_weight_kn_m3(
        24.0,
        Default::default(),
        sepika_core::units::ConcreteComposition::Plain,
    );
    let gc_mpa = concrete_young_modulus_gamma(24.0, gamma_c) / 2.4;
    for (shape, core_j_mm4) in [(pipe, pipe_core_j_mm4), (rounded_box, box_core_j_mm4)] {
        for kind in [ElementKind::Beam, ElementKind::Fiber] {
            let mut model = cantilever(shape.clone(), kind);
            let shape_js_mm4 = model.sections[0].resolved_j().unwrap();
            model.sections[0].j = 1.0;
            assert_twist(&model, STEEL_G_MPA * shape_js_mm4 + gc_mpa * core_j_mm4);
            for steel_g_mpa in [STEEL_G_MPA, 61_000.0] {
                model.materials[0].shear = Some(steel_g_mpa);
                model.sections[0].j = 8.0e8;
                model.sections[0].property_basis.j = PropertyBasis::Supplied;
                assert_twist(&model, steel_g_mpa * 8.0e8 + gc_mpa * core_j_mm4);
            }
        }
    }
}

fn prepare_error(model: &Model) -> String {
    match Analysis::prepare(model) {
        Ok(_) => panic!("未算定のねじり剛性を受理した"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn public_analysis_rejects_pending_j_and_unknown_radius_with_the_reason() {
    let mut pending = cantilever(cft_box(), ElementKind::Beam);
    pending.sections[0].property_basis.j = PropertyBasis::PendingShape;
    let error = prepare_error(&pending);
    assert!(error.contains("J") && error.contains("未算定"), "{error}");
    let mut unknown = cantilever(
        SectionShape::CftBox {
            height: 400.0,
            width: 300.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: None,
        },
        ElementKind::Beam,
    );
    unknown.sections[0].j = 8.0e8;
    unknown.sections[0].property_basis.j = PropertyBasis::Supplied;
    let error = prepare_error(&unknown);
    assert!(error.contains("未算定") || error.contains("角R"), "{error}");
}

#[test]
fn cft_material_failure_groups_all_members_and_recovers_on_the_next_call() {
    let mut model = cantilever(
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 12.0,
        },
        ElementKind::Beam,
    );
    let mut second = model.elements[0].clone();
    second.id = ElemId(1);
    model.elements.push(second);
    model.materials[0].shear = Some(-1.0);
    let issues = model_issues(&model);
    let errors: Vec<_> = issues
        .iter()
        .filter(|issue| issue.message.contains("等価断面性能を算定できません"))
        .collect();
    assert_eq!(errors.len(), 1);
    let issue = errors[0];
    assert_eq!(issue.severity, IssueSeverity::Error);
    assert_eq!(
        issue.targets,
        IssueTargets::Members(vec![ElemId(0), ElemId(1)])
    );
    assert!(
        issue.message.contains("ねじり試験断面") && issue.message.contains("Gs"),
        "{}",
        issue.message
    );
    assert!(issue.short.contains("Gs"), "{}", issue.short);
    let error = prepare_error(&model);
    assert!(error.contains("Gs"), "{error}");
    let direct_error =
        match sepika_element::frame::beam::BeamElement::try_new(&model.elements[0], &model) {
            Ok(_) => panic!("不正な鋼管Gは受理されない"),
            Err(error) => error,
        };
    let once_error = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0))
        .unwrap_err()
        .to_string();
    assert!(once_error.contains(&direct_error), "{once_error}");
    assert!(error.contains(&direct_error), "{error}");
    model.materials[0].shear = Some(61_000.0);
    assert!(sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).is_ok());
    assert!(!model_issues(&model)
        .iter()
        .any(|issue| issue.message.contains("等価断面性能を算定できません")));
}

#[test]
fn cft_diagnostics_distinguish_actual_member_materials_on_the_same_section() {
    let mut model = cantilever(
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 12.0,
        },
        ElementKind::Beam,
    );
    for id in 1..3 {
        let mut member = model.elements[0].clone();
        member.id = ElemId(id);
        model.elements.push(member);
    }
    let mut fc30 = model.materials[1].clone();
    fc30.id = MaterialId(2);
    fc30.name = "Fc30".into();
    fc30.fc = Some(30.0);
    model.materials.push(fc30);
    model.stb_strengths.materials = vec![
        StbGradeMaterial {
            grade: "Fc24".into(),
            material: MaterialId(1),
        },
        StbGradeMaterial {
            grade: "Fc30".into(),
            material: MaterialId(2),
        },
    ];
    model.stb_strengths.members = (0..3)
        .map(|id| StbMemberStrength {
            target: StrengthTarget::Element(ElemId(id)),
            node: NodeId(0),
            node_order: vec![],
            concrete: Some(if id == 2 { "Fc30" } else { "Fc24" }.into()),
        })
        .collect();
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().id,
        MaterialId(1)
    );
    assert_eq!(
        model.element_material(&model.elements[2]).unwrap().id,
        MaterialId(2)
    );
    model.materials[0].shear = Some(-1.0);
    let issues = model_issues(&model);
    let failures: Vec<_> = issues
        .iter()
        .filter(|issue| issue.message.contains("等価断面性能を算定できません"))
        .collect();
    assert_eq!(failures.len(), 2);
    assert_eq!(
        failures[0].targets,
        IssueTargets::Members(vec![ElemId(0), ElemId(1)])
    );
    assert_eq!(failures[1].targets, IssueTargets::Members(vec![ElemId(2)]));
}

#[test]
fn cft_numerical_failure_keeps_reason_and_targets_and_never_uses_steel_only() {
    let shape = SectionShape::CftBox {
        height: 400.0,
        width: 1e-9,
        thick: 1e-10,
        horizontal_thick: 1e-10,
        corner_r: Some(0.0),
    };
    assert!(shape.try_cft_core_props().unwrap().is_some());
    let reason = shape.try_cft_core_torsion().unwrap_err().to_string();
    let mut model = cantilever(shape, ElementKind::Beam);
    model.sections[0].j = 1e8;
    model.sections[0].property_basis.j = PropertyBasis::Supplied;
    let mut second = model.elements[0].clone();
    second.id = ElemId(1);
    model.elements.push(second);
    for _ in 0..2 {
        let issues = model_issues(&model);
        let failures: Vec<_> = issues
            .iter()
            .filter(|issue| issue.message.contains(&reason))
            .collect();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].severity, IssueSeverity::Error);
        assert_eq!(
            failures[0].targets,
            IssueTargets::Members(vec![ElemId(0), ElemId(1)])
        );
        assert!(failures[0].short.contains(&reason));
    }
    let error = prepare_error(&model);
    assert!(error.contains(&reason), "{error}");
    model.sections[0].shape = Some(SectionShape::CftPipe {
        outer_dia: 400.0,
        thick: 12.0,
    });
    assert!(!model_issues(&model)
        .iter()
        .any(|issue| issue.message.contains(&reason)));
}
