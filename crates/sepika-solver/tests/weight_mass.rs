use sepika_core::dof::{Dof, Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    DamperSpec, ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LoadCase,
    LoadCaseKind, LoadCfg, LocalAxis, MassMethod, Material, MaterialCategory, Model, Node,
};
use sepika_core::section_shape::SectionShape;
use sepika_element::behavior::MassOption;
use sepika_load::story_gen::{
    generate_stories_with_opts, generate_stories_with_synced_self_weight, StoryGenResult,
};
use sepika_solver::common::assemble::assemble_global_m;

fn model(kind: ElementKind, damper: bool) -> Model {
    let mut section = SectionShape::SteelFlatBar {
        width: 200.0,
        thick: 20.0,
    }
    .to_section(SectionId(0), "線材".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Girder);
    Model {
        nodes: [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0], [4000.0, 0.0, 3000.0]]
            .into_iter()
            .enumerate()
            .map(|(i, coord)| Node {
                id: NodeId(i as u32),
                coord,
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind,
            nodes: smallvec::smallvec![NodeId(1), NodeId(2)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![Material {
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
        }],
        load_cfg: Some(if damper {
            LoadCfg {
                dampers: vec![DamperSpec {
                    elem: ElemId(0),
                    total_weight: 19613.3,
                }],
                steel_weight_factor: 1.9,
                extra_line_weight: vec![(ElemId(0), 99.0)],
                finish_area_weight: vec![(ElemId(0), 99.0)],
                ..Default::default()
            }
        } else {
            LoadCfg::default()
        }),
        ..Default::default()
    }
}

fn apply_stories(model: &mut Model, generated: StoryGenResult, method: MassMethod) {
    model.damper_mass_generation = Some(generated.damper_mass_generation);
    for (node, story) in model.nodes.iter_mut().zip(&generated.node_story) {
        node.story = *story;
    }
    model.stories = generated.stories;
    model.constraints = generated.constraints;
    model.generated_masters = generated.generated_masters;
    for node in generated.rep_nodes {
        if node.id.index() < model.nodes.len() {
            let idx = node.id.index();
            model.nodes[idx] = node;
        } else {
            model.nodes.push(node);
        }
    }
    model.mass_method = method;
}

fn assembled_translation_mass(model: &Model, dof: Dof) -> f64 {
    let dofmap = DofMap::build(model);
    let m = assemble_global_m(model, &dofmap, MassOption::Consistent).unwrap();
    let indices: Vec<_> = (0..model.nodes.len())
        .filter_map(|ni| dofmap.active(ni * 6 + dof as usize).map(|i| i as usize))
        .collect();
    let dense = m.to_dense();
    indices
        .iter()
        .flat_map(|&i| indices.iter().map(move |&j| (i, j)))
        .map(|(i, j)| dense[(i, j)])
        .sum()
}

#[test]
fn ダンパー総重量は全体質量行列へ一度だけ計上し両方式で一致する() {
    for kind in [
        ElementKind::Damper,
        ElementKind::Beam,
        ElementKind::Brace {
            tension_only: false,
        },
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
            for synced in [false, true] {
                let mut model = model(kind, true);
                model.stories = generate_stories_with_opts(&model, &[], true, method)
                    .unwrap()
                    .stories;
                model.stories[1].fireproof.steel_kind = sepika_core::model::FireproofKind::Spray;
                model.stories[1].fireproof.steel_beam_area_weight = 99.0;
                if kind == ElementKind::Damper {
                    model.elements[0].section = None;
                }
                let (nodal, member) = sepika_load::self_weight::self_weight_case_content(
                    &model,
                    model.load_cfg.as_ref().unwrap(),
                )
                .unwrap();
                assert!(member.is_empty());
                assert_eq!(nodal.len(), 2);
                assert_eq!(nodal[0].values[2], -9806.65);
                assert_eq!(nodal[1].values[2], -9806.65);
                model.load_cases = vec![LoadCase {
                    id: LoadCaseId(0),
                    name: "DL".into(),
                    kind: LoadCaseKind::Dead,
                    nodal,
                    member,
                }];
                let generated = if synced {
                    generate_stories_with_synced_self_weight(&model, &[LoadCaseId(0)], method)
                        .unwrap()
                } else {
                    generate_stories_with_opts(&model, &[], true, method).unwrap()
                };
                assert_eq!(generated.stories[1].seismic_weight, Some(19613.3));
                assert!(
                    (generated.stories[1]
                        .dynamic_mass
                        .unwrap()
                        .mass_equiv_weight_n
                        - 19613.3)
                        .abs()
                        < 1e-8
                );
                let mass = generated.rep_nodes[1].mass.unwrap();
                assert!((mass[0] - 2.0).abs() < 1e-10);
                assert!((mass[1] - 2.0).abs() < 1e-10);
                assert_eq!(mass[2], 0.0);
                assert!((mass[5] - 8000000.0).abs() < 1e-6);
                apply_stories(&mut model, generated, method);
                for dof in [Dof::Ux, Dof::Uy] {
                    assert!(
                        (assembled_translation_mass(&model, dof) - 2.0).abs() < 1e-10,
                        "{kind:?} {method:?} {synced}"
                    );
                }
                assert_eq!(assembled_translation_mass(&model, Dof::Uz), 0.0);
            }
        }
    }
}

#[test]
fn 通常線材は明示fiberとmultispringでも全体行列の公称並進質量が一致する() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
            let mut model = model(kind, false);
            let generated = generate_stories_with_opts(&model, &[], true, method).unwrap();
            apply_stories(&mut model, generated, method);
            for dof in [Dof::Ux, Dof::Uy] {
                assert!(
                    (assembled_translation_mass(&model, dof) - 0.1256).abs() < 1e-10,
                    "{kind:?} {method:?}"
                );
            }
        }
    }
}

#[test]
fn beam由来の非線形fiberでも元断面の質量を維持する() {
    use sepika_element::factory::{
        build_nonlinear_behavior, resolve_force_regime, AnalysisKind, ResolvedRegime, StrengthBasis,
    };
    let mut model = model(ElementKind::Beam, false);
    model.elements[0].force_regime = ForceRegime::AxialBendingInteract;
    assert!(matches!(
        resolve_force_regime(&model.elements[0], &model),
        ResolvedRegime::Fiber
    ));
    let behavior = build_nonlinear_behavior(
        &model.elements[0],
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::TimeHistory,
    );
    let dofmap = DofMap::build(&model);
    let dofs = behavior.global_dofs(&dofmap);
    let matrix = behavior.mass_matrix(MassOption::Consistent);
    for d in 0..3 {
        let indices = [dofs[d], dofs[6 + d]];
        let total: f64 = matrix
            .to_triplets(&dofs)
            .iter()
            .filter(|t| indices.contains(&t.row) && indices.contains(&t.col))
            .map(|t| t.val)
            .sum();
        assert!((total - 0.1256).abs() < 1e-10);
    }
}

#[test]
fn ダンパー帰属階に床面節点が無ければ両方式とも生成と直接組立を停止する() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let mut model = model(ElementKind::Beam, true);
        let initial = generate_stories_with_opts(&model, &[], true, method).unwrap();
        apply_stories(&mut model, initial, method);
        model.stories[1].elevation = 4000.0;
        assert!(generate_stories_with_opts(&model, &[], false, method)
            .unwrap_err()
            .contains("床面代表質点へ配置できません"));
        for synced in [false, true] {
            let (nodal, member) = sepika_load::self_weight::self_weight_case_content(
                &model,
                model.load_cfg.as_ref().unwrap(),
            )
            .unwrap();
            model.load_cases = vec![LoadCase {
                id: LoadCaseId(0),
                name: "DL".into(),
                kind: LoadCaseKind::Dead,
                nodal,
                member,
            }];
            let result = if synced {
                generate_stories_with_synced_self_weight(&model, &[LoadCaseId(0)], method)
            } else {
                generate_stories_with_opts(&model, &[], true, method)
            };
            assert!(result.unwrap_err().contains("床面代表質点へ配置できません"));
        }
        let dofmap = DofMap::build(&model);
        let error = assemble_global_m(&model, &dofmap, MassOption::Consistent).unwrap_err();
        assert!(matches!(
            error,
            sepika_math::solver::SolveError::InvalidInput(_)
        ));
        assert!(error.to_string().contains("床面代表質点へ配置できません"));
    }
}

#[test]
fn gravity_cases_onlyでも不正ダンパー入力を拒否し直接質量組立で迂回できない() {
    let valid_dofmap = DofMap::build(&model(ElementKind::Beam, false));
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        for case in 0..9 {
            let mut model = model(ElementKind::Beam, true);
            model.mass_method = method;
            match case {
                0 => model.load_cfg.as_mut().unwrap().dampers[0].total_weight = -1.0,
                1 => model.load_cfg.as_mut().unwrap().dampers[0].total_weight = f64::NAN,
                2 => model.load_cfg.as_mut().unwrap().dampers[0].total_weight = f64::INFINITY,
                3 => {
                    let duplicate = model.load_cfg.as_ref().unwrap().dampers[0].clone();
                    model.load_cfg.as_mut().unwrap().dampers.push(duplicate);
                }
                4 => model.elements[0].kind = ElementKind::Shell,
                5 => model.load_cfg.as_mut().unwrap().dampers[0].elem = ElemId(999),
                6 => model.elements[0].nodes[1] = NodeId(999),
                7 => {
                    model.elements[0].kind = ElementKind::Damper;
                    model.load_cfg.as_mut().unwrap().dampers.clear();
                }
                _ => model.nodes[1].coord[2] = f64::NAN,
            }
            assert!(model.validate().is_err());
            assert!(generate_stories_with_opts(&model, &[], false, method).is_err());
            assert!(model.element_mass_properties(&model.elements[0]).is_err());
            let error =
                assemble_global_m(&model, &valid_dofmap, MassOption::Consistent).unwrap_err();
            assert!(matches!(
                error,
                sepika_math::solver::SolveError::InvalidInput(_)
            ));
            assert_eq!(
                error.to_string(),
                model.validate_damper_weights().unwrap_err()
            );
        }
        let mut valid = model(ElementKind::Beam, true);
        valid.load_cases = vec![LoadCase {
            id: LoadCaseId(0),
            name: "手入力".into(),
            kind: LoadCaseKind::Dead,
            nodal: vec![sepika_core::model::NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -9806.65, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        }];
        let result = generate_stories_with_opts(&valid, &[LoadCaseId(0)], false, method).unwrap();
        assert_eq!(result.stories[1].seismic_weight, Some(9806.65));
        assert!((result.rep_nodes[1].mass.unwrap()[0] - 1.0).abs() < 1e-10);
        apply_stories(&mut valid, result, method);
        assert!((assembled_translation_mass(&valid, Dof::Ux) - 1.0).abs() < 1e-10);
        let lumped = sepika_solver::dynamic::lumped_mass::build_lumped_mass_model(
            &valid,
            &sample_pushover(),
            sepika_solver::dynamic::lumped_mass::LumpedMassType::EquivalentShear,
            0.75,
        )
        .unwrap();
        assert!((lumped.stories[0].mass - 1.0).abs() < 1e-10);
        let result = generate_stories_with_opts(&valid, &[], false, method).unwrap();
        apply_stories(&mut valid, result, method);
        assert_eq!(assembled_translation_mass(&valid, Dof::Ux), 0.0);
        assert!(
            matches!(sepika_solver::dynamic::lumped_mass::build_lumped_mass_model(
            &valid, &sample_pushover(), sepika_solver::dynamic::lumped_mass::LumpedMassType::EquivalentShear, 0.75,
        ), Err(sepika_math::solver::SolveError::InvalidInput(message)) if message.contains("質量が 0 以下"))
        );
    }
}

#[test]
fn 生成したダンパー質量の欠落と不足と未反映は全公開solverでinvalid_inputになる() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        for synced in [false, true] {
            let mut baseline = model(ElementKind::Beam, true);
            let (nodal, member) = sepika_load::self_weight::self_weight_case_content(
                &baseline,
                baseline.load_cfg.as_ref().unwrap(),
            )
            .unwrap();
            baseline.load_cases = vec![LoadCase {
                id: LoadCaseId(0),
                name: "DL".into(),
                kind: LoadCaseKind::Dead,
                nodal,
                member,
            }];
            let generated = if synced {
                generate_stories_with_synced_self_weight(&baseline, &[LoadCaseId(0)], method)
                    .unwrap()
            } else {
                generate_stories_with_opts(&baseline, &[], true, method).unwrap()
            };
            apply_stories(&mut baseline, generated, method);
            assert!((assembled_translation_mass(&baseline, Dof::Ux) - 2.0).abs() < 1e-10);
            let master = baseline.stories[1]
                .dynamic_mass
                .unwrap()
                .lumped_mass
                .unwrap()
                .master
                .index();
            for case in 0..11 {
                let mut corrupted = baseline.clone();
                match case {
                    0 => corrupted.nodes[master].mass = None,
                    1 => corrupted.nodes[master].mass.as_mut().unwrap()[0] = 1.0,
                    2 => corrupted.nodes[master].mass.as_mut().unwrap()[1] = 1.0,
                    3 => corrupted.nodes[master].mass.as_mut().unwrap()[5] = 0.0,
                    4 => {
                        corrupted.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .lumped_mass = None
                    }
                    5 => {
                        corrupted.mass_method = if method == MassMethod::CorrectedLumped {
                            MassMethod::LumpedOnly
                        } else {
                            MassMethod::CorrectedLumped
                        }
                    }
                    6 => corrupted.load_cfg.as_mut().unwrap().dampers[0].total_weight *= 2.0,
                    7 => corrupted.stories[1].elevation = 4000.0,
                    8 => corrupted.load_cfg.as_mut().unwrap().dampers[0].total_weight = 0.0,
                    9 => corrupted.load_cfg.as_mut().unwrap().dampers.clear(),
                    _ => corrupted.load_cfg = None,
                }
                assert_public_mass_error(&corrupted);
            }
        }
    }
}

fn assert_public_mass_error(model: &Model) {
    use sepika_math::solver::{make_solver, SolveError, SolverBackend};
    use sepika_solver::common::constraint::Reducer;
    use sepika_solver::dynamic::damping::{Damping, DampingAccumulation};
    use sepika_solver::dynamic::eigen::{solve_eigen, solve_eigen_with_solver};
    use sepika_solver::dynamic::timehistory::{
        linear_time_history_analysis, linear_time_history_from_state,
        nonlinear_time_history_analysis, GroundMotion, NewmarkCfg, NonlinearThCfg, TimeStepState,
    };
    let dofmap = DofMap::build(model);
    let reducer = Reducer::build(model, &dofmap);
    let expected = model.validate_damper_mass_placement().unwrap_err();
    let check = |error: SolveError| {
        assert!(matches!(error, SolveError::InvalidInput(_)));
        assert_eq!(error.to_string(), expected);
    };
    check(assemble_global_m(model, &dofmap, MassOption::Consistent).unwrap_err());
    check(
        sepika_solver::dynamic::lumped_mass::build_lumped_mass_model(
            model,
            &sample_pushover(),
            sepika_solver::dynamic::lumped_mass::LumpedMassType::EquivalentShear,
            0.75,
        )
        .unwrap_err(),
    );
    check(solve_eigen(model, &dofmap, &reducer, 1).unwrap_err());
    let solver = make_solver(SolverBackend::DirectSparseCholesky);
    check(solve_eigen_with_solver(model, &dofmap, &reducer, 1, solver.as_ref()).unwrap_err());
    let wave = GroundMotion {
        dt: 0.01,
        accel_x: vec![0.0; 2],
        accel_y: None,
        accel_theta: None,
    };
    let newmark = NewmarkCfg::average_accel();
    let damping = Damping::Rayleigh {
        h1: 0.02,
        w1: 1.0,
        h2: 0.02,
        w2: 2.0,
    };
    let init = vec![0.0; reducer.n_indep];
    check(
        linear_time_history_analysis(
            model, &dofmap, &reducer, &wave, &newmark, &damping, &init, &init, false, None,
        )
        .unwrap_err(),
    );
    check(
        nonlinear_time_history_analysis(
            model,
            &dofmap,
            &reducer,
            &wave,
            &newmark,
            &damping,
            DampingAccumulation::default(),
            &init,
            &init,
            NonlinearThCfg::default(),
        )
        .unwrap_err(),
    );
    let state = TimeStepState {
        step: 0,
        time: 0.0,
        disp_red: init.clone(),
        vel_red: init.clone(),
        accel_red: init,
    };
    check(
        linear_time_history_from_state(
            model, &dofmap, &reducer, &wave, &newmark, &damping, &state, false, None,
        )
        .unwrap_err(),
    );
}

fn sample_pushover() -> sepika_solver::nonlinear::pushover::PushoverResult {
    use sepika_solver::nonlinear::pushover::{CapacityPoint, MechanismType, PushoverResult};
    PushoverResult {
        steps: vec![],
        wall_history: None,
        wall_run: None,
        capacity_curve: [(1.0, 100.0), (2.0, 200.0), (3.0, 250.0)]
            .into_iter()
            .enumerate()
            .map(|(step, (d, q))| CapacityPoint {
                step: step as u32,
                roof_disp: d,
                base_shear: q,
                story_shear: vec![q],
                story_drift: vec![d],
            })
            .collect(),
        hinges: vec![],
        shear_yields: vec![],
        mechanism: MechanismType::Overall,
        qu: 250.0,
        member_response: vec![],
        control: Default::default(),
        member_history: vec![],
        fiber_states: vec![],
        termination: Default::default(),
    }
}

#[test]
fn 質点系solverもダンパー編集後の古い動的質量を拒否する() {
    use sepika_math::solver::SolveError;
    use sepika_solver::dynamic::lumped_mass::{build_lumped_mass_model, LumpedMassType};
    let po = sample_pushover();
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        for synced in [false, true] {
            let mut model = separated_dampers();
            let (nodal, member) = sepika_load::self_weight::self_weight_case_content(
                &model,
                model.load_cfg.as_ref().unwrap(),
            )
            .unwrap();
            model.load_cases = vec![LoadCase {
                id: LoadCaseId(0),
                name: "DL".into(),
                kind: LoadCaseKind::Dead,
                nodal,
                member,
            }];
            let gen = if synced {
                generate_stories_with_synced_self_weight(&model, &[LoadCaseId(0)], method).unwrap()
            } else {
                generate_stories_with_opts(&model, &[], true, method).unwrap()
            };
            apply_stories(&mut model, gen, method);
            let valid = build_lumped_mass_model(&model, &po, LumpedMassType::EquivalentShear, 0.75)
                .unwrap();
            assert!((valid.stories[0].mass - 4.0).abs() < 1e-10);
            for case in 0..8 {
                let mut edited = model.clone();
                match case {
                    0 => edited.load_cfg.as_mut().unwrap().dampers[0].total_weight *= 2.0,
                    1 => edited.load_cfg.as_mut().unwrap().dampers[0].total_weight = 0.0,
                    2 => {
                        let d = &mut edited.load_cfg.as_mut().unwrap().dampers;
                        d[0].total_weight += 9806.65;
                        d[1].total_weight -= 9806.65;
                    }
                    3 => edited.load_cfg.as_mut().unwrap().dampers[0].elem = ElemId(2),
                    4 => edited.nodes[1].coord[0] += 500.0,
                    5 => edited.load_cfg.as_mut().unwrap().dampers[0].total_weight = -1.0,
                    6 => edited.load_cfg.as_mut().unwrap().dampers[0].total_weight = f64::NAN,
                    _ => {
                        let master = edited.stories[1]
                            .dynamic_mass
                            .unwrap()
                            .lumped_mass
                            .unwrap()
                            .master;
                        edited.nodes[master.index()].mass = None;
                    }
                }
                let expected = edited.validate_damper_mass_placement().unwrap_err();
                let error =
                    build_lumped_mass_model(&edited, &po, LumpedMassType::EquivalentShear, 0.75)
                        .unwrap_err();
                assert!(matches!(error, SolveError::InvalidInput(_)));
                assert_eq!(error.to_string(), expected);
            }
        }
    }
}

#[test]
fn 全量物理重量と重心と慣性の生成データ単独不整合も全公開solverで拒否する() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        for mode in 0..3 {
            let mut baseline = model(ElementKind::Beam, true);
            let (nodal, member) = if mode == 2 {
                (
                    vec![
                        sepika_core::model::NodalLoad::manual(
                            NodeId(1),
                            [0.0, 0.0, -4903.325, 0.0, 0.0, 0.0],
                        ),
                        sepika_core::model::NodalLoad::manual(
                            NodeId(2),
                            [0.0, 0.0, -4903.325, 0.0, 0.0, 0.0],
                        ),
                    ],
                    vec![],
                )
            } else {
                sepika_load::self_weight::self_weight_case_content(
                    &baseline,
                    baseline.load_cfg.as_ref().unwrap(),
                )
                .unwrap()
            };
            baseline.load_cases = vec![LoadCase {
                id: LoadCaseId(0),
                name: "DL".into(),
                kind: LoadCaseKind::Dead,
                nodal,
                member,
            }];
            let gen = match mode {
                0 => generate_stories_with_opts(&baseline, &[], true, method),
                1 => generate_stories_with_synced_self_weight(&baseline, &[LoadCaseId(0)], method),
                _ => generate_stories_with_opts(&baseline, &[LoadCaseId(0)], false, method),
            }
            .unwrap();
            apply_stories(&mut baseline, gen, method);
            let mass = if mode == 2 { 1.0 } else { 2.0 };
            assert!((assembled_translation_mass(&baseline, Dof::Ux) - mass).abs() < 1e-10);
            let lumped = sepika_solver::dynamic::lumped_mass::build_lumped_mass_model(
                &baseline,
                &sample_pushover(),
                sepika_solver::dynamic::lumped_mass::LumpedMassType::EquivalentShear,
                0.75,
            )
            .unwrap();
            assert!((lumped.stories[0].mass - mass).abs() < 1e-10);
            for case in 0..16 {
                let mut edited = baseline.clone();
                match case {
                    0 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .mass_equiv_weight_n *= 0.5
                    }
                    1 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .mass_equiv_weight_n = 0.0
                    }
                    2 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .mass_equiv_weight_n = -1.0
                    }
                    3 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .mass_equiv_weight_n = f64::NAN
                    }
                    4 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .center_xy_mm[0] += 500.0
                    }
                    5 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .center_xy_mm[1] += 500.0
                    }
                    6 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .center_xy_mm[0] = f64::INFINITY
                    }
                    7 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .center_xy_mm[1] = f64::NAN
                    }
                    8 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .inertia_t_mm2 *= 0.5
                    }
                    9 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .inertia_t_mm2 = -1.0
                    }
                    10 => {
                        edited.stories[1]
                            .dynamic_mass
                            .as_mut()
                            .unwrap()
                            .inertia_t_mm2 = f64::INFINITY
                    }
                    11 => edited.stories[1].dynamic_mass = None,
                    12 => edited.stories[1].dynamic_mass.as_mut().unwrap().lumped_mass = None,
                    13 => edited
                        .damper_mass_generation
                        .as_mut()
                        .unwrap()
                        .dynamic_masses
                        .clear(),
                    14 => edited.damper_mass_generation = None,
                    _ => {
                        edited.stories[1].dynamic_mass = None;
                        edited.damper_mass_generation = None;
                    }
                }
                assert_public_mass_error(&edited);
            }
        }
    }
}

#[test]
fn 生成記録の部分欠落と重複と余分な要素は全公開solverで拒否する() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        for mode in 0..3 {
            let mut baseline = model(ElementKind::Beam, true);
            baseline.elements[0].nodes = [NodeId(0), NodeId(1)].into_iter().collect();
            baseline.elements[0].local_axis.ref_vector = [1.0, 0.0, 0.0];
            let (nodal, member) = if mode == 2 {
                (
                    vec![
                        sepika_core::model::NodalLoad::manual(
                            NodeId(0),
                            [0.0, 0.0, -4903.325, 0.0, 0.0, 0.0],
                        ),
                        sepika_core::model::NodalLoad::manual(
                            NodeId(1),
                            [0.0, 0.0, -4903.325, 0.0, 0.0, 0.0],
                        ),
                    ],
                    vec![],
                )
            } else {
                sepika_load::self_weight::self_weight_case_content(
                    &baseline,
                    baseline.load_cfg.as_ref().unwrap(),
                )
                .unwrap()
            };
            baseline.load_cases = vec![LoadCase {
                id: LoadCaseId(0),
                name: "DL".into(),
                kind: LoadCaseKind::Dead,
                nodal,
                member,
            }];
            let gen = match mode {
                0 => generate_stories_with_opts(&baseline, &[], true, method),
                1 => generate_stories_with_synced_self_weight(&baseline, &[LoadCaseId(0)], method),
                _ => generate_stories_with_opts(&baseline, &[LoadCaseId(0)], false, method),
            }
            .unwrap();
            apply_stories(&mut baseline, gen, method);
            assert!(
                (assembled_translation_mass(&baseline, Dof::Ux)
                    - if mode == 2 { 1.0 } else { 2.0 })
                .abs()
                    < 1e-10
            );
            assert_eq!(
                baseline
                    .damper_mass_generation
                    .as_ref()
                    .unwrap()
                    .placements
                    .len(),
                2
            );
            for case in 0..23 {
                let mut corrupted = baseline.clone();
                let unrelated = sepika_core::model::MassPlacementNode {
                    id: NodeId(2),
                    coord: baseline.nodes[2].coord,
                    restraint: baseline.nodes[2].restraint,
                    structural: sepika_core::dof::structural_nodes(&baseline)[2],
                };
                let extra_constraint = baseline.constraints[1].clone();
                let record = corrupted.damper_mass_generation.as_mut().unwrap();
                match case {
                    0 => record.dynamic_masses.retain(|(id, _)| id.0 == 0),
                    1 => record.dynamic_masses.clear(),
                    2 => record.dynamic_masses.push(record.dynamic_masses[0]),
                    3 => record
                        .dynamic_masses
                        .push((sepika_core::ids::StoryId(999), record.dynamic_masses[0].1)),
                    4 => record.placements.clear(),
                    5 => record.placements.retain(|p| p.story.0 == 0),
                    6 => record.placements.push(record.placements[0].clone()),
                    7 => {
                        let mut p = record.placements[0].clone();
                        p.story = sepika_core::ids::StoryId(999);
                        record.placements.push(p);
                    }
                    8 => {
                        record.placements[0].nodes.pop();
                    }
                    9 => record.placements[0].nodes.clear(),
                    10 => {
                        let n = record.placements[0].nodes[0];
                        record.placements[0].nodes.push(n);
                    }
                    11 => record.placements[0].nodes.push(unrelated),
                    12 => {
                        record.placements[0].anchors.pop();
                    }
                    13 => record.placements[0].anchors.clear(),
                    14 => {
                        let id = record.placements[0].anchors[0];
                        record.placements[0].anchors.push(id);
                    }
                    15 => record.placements[0].anchors.push(NodeId(2)),
                    16 => {
                        record.placements[0].constraints.pop();
                    }
                    17 => {
                        let c = record.placements[0].constraints[0].clone();
                        record.placements[0].constraints.push(c);
                    }
                    18 => record.placements[0].constraints.push(extra_constraint),
                    19 => {
                        record.inputs.pop();
                    }
                    20 => record.inputs.push(record.inputs[0]),
                    21 => {
                        let mut input = record.inputs[0];
                        input.elem = ElemId(999);
                        record.inputs.push(input);
                    }
                    _ => record.dynamic_masses[1].1 = None,
                }
                assert_eq!(corrupted.nodes, baseline.nodes);
                assert_eq!(corrupted.stories, baseline.stories);
                assert_eq!(corrupted.constraints, baseline.constraints);
                assert_eq!(corrupted.load_cfg, baseline.load_cfg);
                assert_public_mass_error(&corrupted);
            }
            let mut reordered = baseline.clone();
            let record = reordered.damper_mass_generation.as_mut().unwrap();
            record.dynamic_masses.reverse();
            record.placements.reverse();
            for p in &mut record.placements {
                p.nodes.reverse();
            }
            assert!(
                (assembled_translation_mass(&reordered, Dof::Ux)
                    - if mode == 2 { 1.0 } else { 2.0 })
                .abs()
                    < 1e-10
            );
        }
    }
}

fn separated_dampers() -> Model {
    let mut result = model(ElementKind::Beam, true);
    for (id, coord) in [
        (3, [0.0, 4000.0, 3000.0]),
        (4, [4000.0, 4000.0, 3000.0]),
        (5, [0.0, 8000.0, 3000.0]),
        (6, [4000.0, 8000.0, 3000.0]),
    ] {
        result.nodes.push(Node {
            id: NodeId(id),
            coord,
            ..result.nodes[1].clone()
        });
    }
    for (id, nodes) in [(1, [NodeId(3), NodeId(4)]), (2, [NodeId(5), NodeId(6)])] {
        result.elements.push(ElementData {
            id: ElemId(id),
            nodes: nodes.into_iter().collect(),
            section: if id == 1 { Some(SectionId(0)) } else { None },
            ..result.elements[0].clone()
        });
    }
    result.load_cfg.as_mut().unwrap().dampers.push(DamperSpec {
        elem: ElemId(1),
        total_weight: 19613.3,
    });
    result
}

#[test]
fn 同階合計を保つ編集でもダンパーの重量配分と配置の変更は全公開solverで拒否する() {
    use sepika_core::model::Constraint;
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        for synced in [false, true] {
            let mut baseline = separated_dampers();
            let (nodal, member) = sepika_load::self_weight::self_weight_case_content(
                &baseline,
                baseline.load_cfg.as_ref().unwrap(),
            )
            .unwrap();
            baseline.load_cases = vec![LoadCase {
                id: LoadCaseId(0),
                name: "DL".into(),
                kind: LoadCaseKind::Dead,
                nodal,
                member,
            }];
            let gen = if synced {
                generate_stories_with_synced_self_weight(&baseline, &[LoadCaseId(0)], method)
                    .unwrap()
            } else {
                generate_stories_with_opts(&baseline, &[], true, method).unwrap()
            };
            apply_stories(&mut baseline, gen, method);
            assert!((assembled_translation_mass(&baseline, Dof::Ux) - 4.0).abs() < 1e-10);
            let master = baseline.stories[1]
                .dynamic_mass
                .unwrap()
                .lumped_mass
                .unwrap()
                .master;
            for case in 0..9 {
                let mut edited = baseline.clone();
                match case {
                    0 => {
                        let d = &mut edited.load_cfg.as_mut().unwrap().dampers;
                        d[0].total_weight += 9806.65;
                        d[1].total_weight -= 9806.65;
                    }
                    1 => edited.load_cfg.as_mut().unwrap().dampers[0].elem = ElemId(2),
                    2 => edited.nodes[1].coord[0] += 500.0,
                    3 => edited.nodes[master.index()].coord[0] += 500.0,
                    4 => edited.nodes[master.index()].restraint.set_fixed(Dof::Ux),
                    5 => {
                        for c in &mut edited.constraints {
                            if let Constraint::RigidDiaphragm { story, slaves, .. } = c {
                                if story.0 == 1 {
                                    slaves.pop();
                                }
                            }
                        }
                    }
                    6 => edited.constraints.push(Constraint::RigidLink {
                        master,
                        slaves: vec![NodeId(1)],
                        dofs: Dof6Mask::FIXED,
                    }),
                    7 => edited.nodes[3].restraint.set_fixed(Dof::Uy),
                    _ => {
                        edited.elements.pop();
                    }
                }
                assert_eq!(
                    edited
                        .load_cfg
                        .as_ref()
                        .unwrap()
                        .dampers
                        .iter()
                        .map(|d| d.total_weight)
                        .sum::<f64>(),
                    39226.6
                );
                assert_public_mass_error(&edited);
            }
            let mut reordered = baseline.clone();
            reordered.load_cfg.as_mut().unwrap().dampers.reverse();
            for c in &mut reordered.constraints {
                if let Constraint::RigidDiaphragm { ci_override, .. } = c {
                    *ci_override = Some(0.3);
                }
            }
            assert!((assembled_translation_mass(&reordered, Dof::Ux) - 4.0).abs() < 1e-10);
        }
    }
}

#[test]
fn 通常部材の階生成後のゼロ重量置換追加は再生成するまで拒否する() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let mut input = model(ElementKind::Beam, false);
        let generated = generate_stories_with_opts(&input, &[], true, method).unwrap();
        apply_stories(&mut input, generated, method);
        assert!((assembled_translation_mass(&input, Dof::Ux) - 0.1256).abs() < 1e-10);
        input.load_cfg.as_mut().unwrap().dampers.push(DamperSpec {
            elem: ElemId(0),
            total_weight: 0.0,
        });
        assert_public_mass_error(&input);
        let mut missing_record = input.clone();
        missing_record.damper_mass_generation = None;
        assert_public_mass_error(&missing_record);
        let gen = generate_stories_with_opts(&input, &[], true, method).unwrap();
        apply_stories(&mut input, gen, method);
        assert_eq!(assembled_translation_mass(&input, Dof::Ux), 0.0);
    }
    let mut raw = model(ElementKind::Damper, true);
    raw.load_cfg.as_mut().unwrap().dampers[0].total_weight = 0.0;
    let dofmap = DofMap::build(&raw);
    assert_eq!(
        assemble_global_m(&raw, &dofmap, MassOption::Consistent)
            .unwrap()
            .compute_nnz(),
        0
    );
}
