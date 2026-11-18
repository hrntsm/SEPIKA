#[test]
fn issue1290_observation() {
    use sepika_core::flexural_strength::{member_flexural_yield_moment, FlexuralStrengthFactors};
    use sepika_design_jp::{run_member_design_checks, MemberDesignCheckOptions};
    use sepika_element::factory::{build_hinge_view, ensure_nonlinear_input, StrengthBasis};
    use sepika_element::frame::beam::MemberForces;
    use sepika_mcp::{compute_job, JobParams};

    let rc = shapes()[0].clone();
    let steel_h = SectionShape::SteelH {
        height: 400.,
        width: 200.,
        web_thick: 8.,
        flange_thick: 12.,
        root_r: Some(0.),
    };
    let mut fixture = Fixture::new(frame(&rc, FrameSectionUse::Girder));
    let path = fixture.dir.join("issue1290.ovika");
    let edit_result = apply_edit(
        &mut fixture.state,
        &edit(0, &steel_h, Some(FrameSectionUse::Girder)),
    )
    .unwrap();
    assert!(edit_result.applied);
    fixture.state.model.elements[0].force_regime = ForceRegime::UniaxialBendingShear;
    assert_eq!(
        fixture.state.model.sections[0].material,
        Some(MaterialId(0))
    );
    assert_eq!(
        fixture.state.model.materials[0].category,
        MaterialCategory::Concrete
    );
    fixture.state.model.validate().unwrap();

    for fy in [None, Some(100.)] {
        let input = &mut fixture.state.model;
        input.materials[0].fy = fy;
        sepika_io::ovika::save_ovika(&path, input, Default::default()).unwrap();
        let model = sepika_io::ovika::load_ovika(&path).unwrap().model;
        assert_eq!(model.materials[0].fy, fy);
        assert_eq!(model.sections[0].shape.as_ref(), Some(&steel_h));
        assert_eq!(model.sections[0].material, Some(MaterialId(0)));
        let elem = &model.elements[0];
        let mut work = model.clone();
        println!(
            "fy={fy:?}; mass={:?}; prep={:?}; nonlinear={:?}; My={}",
            model.element_mass_properties(elem).unwrap_err(),
            sepika_job::prepare_model_for_analysis(&mut work, &Default::default(), None).err(),
            ensure_nonlinear_input(&model),
            member_flexural_yield_moment(elem, &model, FlexuralStrengthFactors::NOMINAL),
        );
        for kind in [
            JobKind::LinearStatic,
            JobKind::Pushover,
            JobKind::DesignCheck,
            JobKind::UltimateCheck,
        ] {
            let err = compute_job(&model, kind, &JobParams::default())
                .err()
                .unwrap()
                .to_string();
            assert!(err.contains("鋼材ではありません"), "{err}");
            println!("{kind:?}: {err}");
        }
        let force = MemberForces {
            at: vec![(0.0, [0.; 6])],
        };
        let reason = run_member_design_checks(
            &model,
            &[(elem.id, force)],
            &[],
            &MemberDesignCheckOptions::default(),
        )
        .err()
        .unwrap();
        assert!(
            reason.contains("部材 0") && reason.contains("鋼材ではありません"),
            "{reason}"
        );
        let view = build_hinge_view(
            elem,
            &model,
            StrengthBasis::MaterialStrength,
            AnalysisKind::Incremental,
            0.0,
            12,
            24,
        )
        .unwrap();
        println!(
            "backbone={:?}; unavailable={:?}",
            view.backbone, view.unavailability_reason
        );
        assert_eq!(view.backbone.is_some(), fy.is_some());
        assert_eq!(view.unavailability_reason.is_none(), fy.is_some());
        let mut shape_none = model.clone();
        shape_none.sections[0].shape = None;
        println!(
            "no-shape nonlinear={:?}; no-shape prep={:?}",
            ensure_nonlinear_input(&shape_none),
            sepika_job::prepare_model_for_analysis(&mut shape_none, &Default::default(), None)
                .err()
        );
    }

    for (shape, expected) in [
        (
            SectionShape::SteelBox {
                height: 300.,
                width: 300.,
                thick: 10.,
                horizontal_thick: 10.,
                corner_r: Some(0.),
            },
            296.57e6,
        ),
        (
            SectionShape::SteelPipe {
                outer_dia: 300.,
                thick: 10.,
            },
            197.71333333333333e6,
        ),
    ] {
        let mut fixture = Fixture::new(frame(&rc, FrameSectionUse::Girder));
        let state = &mut fixture.state;
        assert!(
            apply_edit(state, &edit(0, &shape, Some(FrameSectionUse::Girder)))
                .unwrap()
                .applied
        );
        let model = &mut state.model;
        let my = member_flexural_yield_moment(
            &model.elements[0],
            model,
            FlexuralStrengthFactors::NOMINAL,
        );
        assert!((my - expected).abs() < 1e-4);
        model.materials[0].fy = Some(100.);
        let err = compute_job(model, JobKind::Pushover, &JobParams::default())
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("鋼材ではありません"), "{err}");
        println!("shape={shape:?}; My={my}; Pushover={err}");
    }

    let model = &mut fixture.state.model;
    model.materials[0].category = MaterialCategory::Steel;
    model.materials[0].name = "SS400".into();
    model.materials[0].fy = Some(235.);
    model.materials[0].fc = None;
    model.materials[0].young = 205_000.;
    model.materials[0].density = 7.85e-9;
    model.materials[0].poisson = 0.3;
    sepika_job::prepare_model_for_analysis(model, &Default::default(), None).unwrap();
    ensure_nonlinear_input(model).unwrap();
    println!("H shape + SS400: preparation and nonlinear input both accepted");
}

#[test]
fn issue1290_damper_weight_observation() {
    use sepika_core::model::{DamperSpec, LoadCfg, Story};
    use sepika_mcp::{compute_job, JobOutcome, JobParams};
    use sepika_solver::nonlinear::pushover::HingeLevel;

    let mut fixture = Fixture::new(frame(&shapes()[1], FrameSectionUse::Column));
    let steel_h = SectionShape::SteelH {
        height: 400.,
        width: 200.,
        web_thick: 8.,
        flange_thick: 12.,
        root_r: Some(0.),
    };
    apply_success(
        &mut fixture.state,
        &edit(0, &steel_h, Some(FrameSectionUse::Column)),
    );
    let input = &mut fixture.state.model;
    input.elements[0].force_regime = ForceRegime::UniaxialBendingShear;
    input.elements[0].local_axis.ref_vector = [1., 0., 0.];
    input.stories = [("B", 0.), ("R", 3000.)]
        .into_iter()
        .enumerate()
        .map(|(i, (name, fl))| Story::new(StoryId(i as u32), name.into(), Some(fl), Some(0.)))
        .collect();
    input.load_cases = [
        (DL_CASE_NAME, LoadCaseKind::Dead),
        ("P=0", LoadCaseKind::Live),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, kind))| LoadCase {
        origin: Default::default(),
        id: LoadCaseId(i as u32),
        name: name.into(),
        kind,
        nodal: vec![],
        member: vec![],
    })
    .collect();
    input.combinations.push(LoadCombination {
        name: "G+P".into(),
        terms: vec![(LoadCaseId(0), 1.), (LoadCaseId(1), 1.)],
    });
    input.ultimate_gravity_combination = Some(0);

    let mut default_weight = input.clone();
    default_weight.materials[0].fy = Some(100.);
    let reason =
        sepika_job::prepare_model_for_analysis(&mut default_weight, &Default::default(), None)
            .err()
            .unwrap()
            .to_string();
    assert!(reason.contains("鋼材ではありません"), "{reason}");
    println!("自重を置換しない同じ片持ち柱の準備: {reason}");

    input.load_cfg = Some(LoadCfg {
        dampers: vec![DamperSpec {
            elem: ElemId(0),
            total_weight: 20_000.,
        }],
        ..Default::default()
    });
    let path = fixture.dir.join("damper-weight.ovika");

    for fy in [None, Some(100.)] {
        input.materials[0].fy = fy;
        input.validate().unwrap();
        sepika_io::ovika::save_ovika(&path, input, Default::default()).unwrap();
        let model = sepika_io::ovika::load_ovika(&path).unwrap().model;
        assert_eq!(model.materials[0].category, MaterialCategory::Concrete);
        assert_eq!(model.materials[0].fy, fy);
        assert_eq!(model.sections[0].shape.as_ref(), Some(&steel_h));
        assert_eq!(
            model.load_cfg.as_ref().unwrap().dampers[0].total_weight,
            20_000.
        );
        let mut prepared = model.clone();
        let report =
            sepika_job::prepare_model_for_analysis(&mut prepared, &Default::default(), None)
                .unwrap();
        println!("Fy={fy:?} の準備: 成功、注意={:?}", report.notices);
        let params = JobParams {
            steps: 24,
            max_disp: Some(100.),
            ..Default::default()
        };
        let reason = compute_job(&model, JobKind::DesignCheck, &params)
            .err()
            .unwrap()
            .to_string();
        assert!(
            reason.contains("部材 0") && reason.contains("鋼材ではありません"),
            "{reason}"
        );
        println!("Fy={fy:?} の許容応力度検定: {reason}");

        let params = JobParams {
            load_case: Some(0),
            ..params
        };
        let JobOutcome::LinearStatic {
            summary,
            member_force_rows,
            ..
        } = compute_job(&model, JobKind::LinearStatic, &params).unwrap()
        else {
            panic!("線形静的解析の結果がありません");
        };
        let axial_n = member_force_rows
            .iter()
            .find(|(elem, pos, _)| *elem == 0 && *pos == 0.)
            .unwrap()
            .2[0];
        assert_eq!(member_force_rows.len(), 3);
        assert!((axial_n + 10_000.).abs() < 1e-4);
        println!("Fy={fy:?} の線形静的解析: {summary}、柱脚軸力={axial_n} N");

        let JobOutcome::UltimateCheck { summary } =
            compute_job(&model, JobKind::UltimateCheck, &params).unwrap()
        else {
            panic!("終局検定の結果がありません");
        };
        assert_eq!(summary["n_checks"], 0);
        assert_eq!(summary["n_cft_checks"], 0);
        assert_eq!(summary["all_checks_calculated_and_ok"], false);
        println!("Fy={fy:?} の終局検定: {summary}");

        if fy.is_none() {
            let reason = compute_job(&model, JobKind::Pushover, &params)
                .err()
                .unwrap()
                .to_string();
            assert!(
                reason.contains("降伏強度 fy が設定されていません"),
                "{reason}"
            );
            println!("Fy未入力の増分解析: {reason}");
            continue;
        }

        for (target_mm, classification) in [(100., "Partial"), (150., "Overall")] {
            let params = JobParams {
                max_disp: Some(target_mm),
                ..params
            };
            let JobOutcome::Pushover { summary } =
                compute_job(&model, JobKind::Pushover, &params).unwrap()
            else {
                panic!("増分解析の結果がありません");
            };
            let steps = summary["steps"].as_array().unwrap();
            let last_disp_mm = steps.last().unwrap()["top_disp"].as_f64().unwrap();
            assert!((last_disp_mm - target_mm).abs() < 1e-4);
            assert_eq!(summary["mechanism"], classification);
            assert_eq!(summary["termination"], "TargetReached");
            println!("増分解析: 目標={target_mm} mm、点数={}、最終変位={last_disp_mm} mm、最大ベースシア={} kN、分類={classification}", steps.len(), summary["max_base_shear_kN"]);
        }
        let cfg = sepika_job::AnalysisSettings {
            push_steps: 24,
            push_use_max_disp: true,
            push_max_disp: 150.,
            push_use_drift_angle: false,
            ..Default::default()
        };
        let result = sepika_job::compute::compute_pushover(prepared, cfg).unwrap();
        for level in [HingeLevel::Crack, HingeLevel::Yield] {
            let hinge = result
                .hinges
                .iter()
                .find(|h| {
                    h.elem == ElemId(0)
                        && h.pos == 0.
                        && std::mem::discriminant(&level) == std::mem::discriminant(&h.level)
                })
                .unwrap();
            println!("柱脚の初出ヒンジ: {hinge:?}");
        }
        println!("最終部材応答: {:?}", result.member_response[0]);
    }
}
