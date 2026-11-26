use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    AnalysisKind, DamperSpec, ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse,
    LoadCase, LoadCaseKind, LoadCfg, LoadCombination, LocalAxis, Material, MaterialCategory, Model,
    Node, Story, DL_CASE_NAME,
};
use sepika_core::section_shape::{
    RcRectColumnRebar, RectColumnHoop, SectionShape, SrcHManufacture,
};
use sepika_element::factory::{
    build_hinge_view, ensure_nonlinear_input, nonlinear_input_issues_for_kind, AnalysisHingeModel,
    HingeView, StrengthBasis,
};
use sepika_job::compute::{compute_linear_static, compute_pushover};
use sepika_job::error::JobError;
use sepika_job::{prepare_model_for_analysis, AnalysisSettings};
use sepika_math::solver::SolveError;
use sepika_solver::common::constraint::Reducer;
use sepika_solver::nonlinear::pushover::{pushover_analysis, PushoverTermination};
use sepika_solver::statics::analysis::{Analysis, SeismicDir};
use sepika_solver::statics::linear::linear_static_once;

const DL: LoadCaseId = LoadCaseId(0);

fn cantilever(material: Material, damper_weight: bool) -> Model {
    let mut section = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 12.0,
        root_r: Some(0.0),
    }
    .to_section(SectionId(0), "C1".into());
    section.material = Some(material.id);
    section.frame_use = Some(FrameSectionUse::Column);
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 3000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(section.id),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::UniaxialBendingShear,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        materials: vec![material],
        sections: vec![section],
        stories: vec![
            Story::new(StoryId(0), "B".into(), Some(0.0), Some(0.0)),
            Story::new(StoryId(1), "R".into(), Some(3000.0), Some(0.0)),
        ],
        load_cases: [
            (DL_CASE_NAME, LoadCaseKind::Dead),
            ("P=0", LoadCaseKind::Live),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (name, kind))| LoadCase {
            origin: Default::default(),
            id: LoadCaseId(index as u32),
            name: name.into(),
            kind,
            nodal: vec![],
            member: vec![],
        })
        .collect(),
        combinations: vec![LoadCombination {
            name: "G+P".into(),
            terms: vec![(DL, 1.0), (LoadCaseId(1), 1.0)],
        }],
        ultimate_gravity_combination: Some(0),
        load_cfg: damper_weight.then(|| LoadCfg {
            dampers: vec![DamperSpec {
                elem: ElemId(0),
                total_weight: 20_000.0,
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn concrete(fy: Option<f64>) -> Material {
    Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 22_700.0,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.0),
        fy,
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

fn steel() -> Material {
    Material {
        name: "SS400".into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        fc: None,
        fy: Some(235.0),
        ..concrete(None)
    }
}

fn rebar() -> Material {
    Material {
        name: "SD295".into(),
        category: MaterialCategory::Rebar,
        fy: Some(295.0),
        ..steel()
    }
}

fn hinge_view(model: &Model) -> HingeView {
    build_hinge_view(
        &model.elements[0],
        model,
        StrengthBasis::MaterialStrength,
        AnalysisKind::Incremental,
        0.0,
        12,
        24,
    )
    .unwrap()
}

fn assert_material_mismatch(reason: &str, material: &Material) {
    for expected in [
        "部材 ID 0",
        "鋼材断面「C1」",
        "断面 ID 0",
        &format!("主材料「{}」", material.name),
        &format!("区分が{}", material.category.label()),
        "鋼材を割り当て",
    ] {
        assert!(reason.contains(expected), "{reason}");
    }
    assert!(!reason.contains("降伏強度"), "{reason}");
}

#[test]
fn 鋼断面と主材料の区分不一致は自重方式とfyによらず診断する() {
    for damper_weight in [false, true] {
        for material in [rebar(), concrete(None), concrete(Some(100.0))] {
            let mut model = cantilever(material.clone(), damper_weight);
            model.validate().unwrap();

            let error = prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None)
                .err()
                .unwrap();
            let JobError::InvalidInput(reason) = error else {
                panic!("{error}");
            };
            assert_material_mismatch(&reason, &material);

            let error = compute_linear_static(model.clone(), DL).unwrap_err();
            let JobError::Prepare(reason) = error else {
                panic!("{error}");
            };
            assert_material_mismatch(&reason, &material);

            for error in [
                Analysis::prepare(&model).err().unwrap(),
                linear_static_once(&model, DL).unwrap_err(),
            ] {
                let SolveError::InvalidInput(reason) = error else {
                    panic!("{error}");
                };
                assert_material_mismatch(&reason, &material);
            }
            let dofmap = DofMap::build(&model);
            let reducer = Reducer::build(&model, &dofmap);
            assert_material_mismatch(
                &pushover_analysis(&model, &dofmap, &reducer, SeismicDir::X, 24, 5.0).unwrap_err(),
                &material,
            );

            assert_material_mismatch(&ensure_nonlinear_input(&model).unwrap_err(), &material);
            for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
                let issues = nonlinear_input_issues_for_kind(&model, kind);
                assert_eq!(issues.len(), 1, "{issues:?}");
                assert_material_mismatch(&issues[0], &material);
            }
            let view = hinge_view(&model);
            assert!(matches!(view.model, AnalysisHingeModel::ConcentratedSpring));
            assert!(view.backbone.is_none() && view.total_backbone.is_none());
            assert_material_mismatch(&view.unavailability_reason.unwrap(), &material);
        }
    }
}

#[test]
fn 鋼主材料を割り当てると自重方式ごとに重力を伝え骨格と解析結果を得られる() {
    let settings = AnalysisSettings {
        push_steps: 24,
        push_use_max_disp: true,
        push_max_disp: 5.0,
        push_use_drift_angle: false,
        push_apply_long_term: true,
        ..Default::default()
    };
    // H形の断面積 7,808 mm² と単位体積重量 78.5 kN/m³ による自重の半分。
    let self_weight_at_top_n = 919.392;
    for (damper_weight, expected_compression_n) in [(false, self_weight_at_top_n), (true, 10_000.0)]
    {
        let mut model = cantilever(steel(), damper_weight);
        prepare_model_for_analysis(&mut model, &settings, None).unwrap();
        let result = compute_linear_static(model.clone(), DL).unwrap();
        let axial_n = result.member_forces[0].1.at[0].1[0];
        assert!((axial_n + expected_compression_n).abs() < 1e-6, "{axial_n}");

        ensure_nonlinear_input(&model).unwrap();
        let view = hinge_view(&model);
        assert!(matches!(view.model, AnalysisHingeModel::ConcentratedSpring));
        assert!(view.unavailability_reason.is_none());
        assert!(view.backbone.unwrap().iter().any(|point| point[1] > 0.0));

        let result = compute_pushover(model, settings).unwrap();
        assert!(matches!(
            result.termination,
            PushoverTermination::TargetReached
        ));
    }
}

#[test]
fn rcとsrcの断面の配筋に鉄筋を割り当てると解析できる() {
    let rebar_layout = RcRectColumnRebar {
        main_dia: sepika_core::section_shape::RebarSize::DiameterMm(19.0),
        x: vec![3],
        y: vec![3],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    let rc = SectionShape::RcColumnRect {
        b: 400.0,
        d: 600.0,
        rebar: rebar_layout.clone(),
    };
    let src = SectionShape::SrcColumnRect {
        b: 400.0,
        d: 600.0,
        rebar: rebar_layout,
        steel_height: 300.0,
        steel_width: 150.0,
        steel_web_thick: 6.5,
        steel_flange_thick: 9.0,
        steel_manufacture: SrcHManufacture::BuiltUp,
    };
    // 0.4 m × 0.6 m × 3 m、入力密度 2.4 t/m³ と重力加速度 9.80665 m/s² による自重の半分。
    let self_weight_at_top_n = 8_472.945_6;
    for shape in [rc, src] {
        let mut model = cantilever(concrete(None), false);
        let is_src = matches!(shape, SectionShape::SrcColumnRect { .. });
        let mut section = shape.to_section(SectionId(0), "C1".into());
        section.frame_use = Some(FrameSectionUse::Column);
        section.material = Some(MaterialId(0));
        section.rebar_material = Some(MaterialId(1));
        section.shear_rebar_material = Some(MaterialId(1));
        model.materials.push(Material {
            id: MaterialId(1),
            ..rebar()
        });
        if is_src {
            model.elements[0].force_regime = ForceRegime::AxialBendingInteract;
            section.steel_material = Some(MaterialId(2));
            model.materials.push(Material {
                id: MaterialId(2),
                name: "SN400B".into(),
                ..steel()
            });
        }
        model.sections[0] = section;

        prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
        ensure_nonlinear_input(&model).unwrap();
        let result = compute_linear_static(model, DL).unwrap();
        let axial_n = result.member_forces[0].1.at[0].1[0];
        assert!((axial_n + self_weight_at_top_n).abs() < 1e-6, "{axial_n}");
    }
}

#[test]
fn 形状なし断面のコンクリート主材料は引き続き解析準備できる() {
    for damper_weight in [false, true] {
        let mut model = cantilever(concrete(None), damper_weight);
        model.sections[0].shape = None;
        model.sections[0].property_basis.area = sepika_core::model::PropertyBasis::Supplied;
        prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
        ensure_nonlinear_input(&model).unwrap();
        let view = hinge_view(&model);
        assert!(view.unavailability_reason.is_none());
        assert!(view.backbone.is_some());
    }
}
