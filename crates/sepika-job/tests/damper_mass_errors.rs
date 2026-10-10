use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{
    DamperSpec, ElementData, ElementKind, EndCondition, ForceRegime, LoadCfg, LocalAxis,
    MassMethod, Model, Node,
};
use sepika_job::compute::{compute_eigen, compute_lumped_mass, compute_time_history};
use sepika_job::error::JobError;
use sepika_job::settings::AnalysisSettings;
use sepika_load::story_gen::generate_stories_with_opts;
use sepika_solver::dynamic::timehistory::GroundMotion;

fn generated_model(method: MassMethod) -> Model {
    let mut model = Model {
        nodes: [0.0, 3000.0]
            .into_iter()
            .enumerate()
            .map(|(i, z)| Node {
                id: NodeId(i as u32),
                coord: [0.0, 0.0, z],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Damper,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }],
        load_cfg: Some(LoadCfg {
            dampers: vec![DamperSpec {
                elem: ElemId(0),
                total_weight: 19613.3,
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let generated = generate_stories_with_opts(&model, &[], true, method).unwrap();
    for (node, story) in model.nodes.iter_mut().zip(generated.node_story) {
        node.story = story;
    }
    model.stories = generated.stories;
    model.constraints = generated.constraints;
    model.generated_masters = generated.generated_masters;
    model.damper_mass_generation = Some(generated.damper_mass_generation);
    model.nodes.extend(generated.rep_nodes);
    model.mass_method = method;
    model
}

#[test]
fn 公開固有値と時刻歴ジョブはダンパー質量不備をinvalid_inputで返す() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let baseline = generated_model(method);
        for case in 0..7 {
            let mut model = baseline.clone();
            let master = model.stories[1]
                .dynamic_mass
                .unwrap()
                .lumped_mass
                .unwrap()
                .master
                .index();
            match case {
                0 => model.nodes[master].mass = None,
                1 => model.nodes[master].mass.as_mut().unwrap()[0] = 0.5,
                2 => model.stories[1].elevation = 4000.0,
                3 => model.stories[1].dynamic_mass.as_mut().unwrap().lumped_mass = None,
                4 => model.load_cfg.as_mut().unwrap().dampers[0].total_weight = -1.0,
                5 => model.load_cfg.as_mut().unwrap().dampers[0].total_weight = f64::NAN,
                _ => model.elements[0].nodes[1] = NodeId(999),
            }
            let expected = model.validate_damper_mass_placement().unwrap_err();
            let check = |error: JobError| {
                assert_eq!(error.kind(), "invalid_input");
                assert!(matches!(error, JobError::InvalidInput(message) if message == expected));
            };
            check(compute_eigen(model.clone(), 1).unwrap_err());
            for nonlinear in [false, true] {
                let cfg = AnalysisSettings {
                    th_nonlinear: nonlinear,
                    ..Default::default()
                };
                let wave = GroundMotion {
                    dt: 0.01,
                    accel_x: vec![0.0; 2],
                    accel_y: None,
                    accel_theta: None,
                };
                check(compute_time_history(model.clone(), cfg, wave).unwrap_err());
            }
        }
    }
}

fn separated_model(method: MassMethod) -> Model {
    let mut model = generated_model(method);
    model.nodes.truncate(2);
    model.nodes[0].coord = [0.0, 0.0, 3000.0];
    model.nodes[1].coord = [4000.0, 0.0, 3000.0];
    model.generated_masters.clear();
    model.damper_mass_generation = None;
    model.stories.clear();
    model.constraints.clear();
    for (id, coord) in [
        (2, [0.0, 0.0, 0.0]),
        (3, [0.0, 4000.0, 3000.0]),
        (4, [4000.0, 4000.0, 3000.0]),
        (5, [0.0, 8000.0, 3000.0]),
        (6, [4000.0, 8000.0, 3000.0]),
    ] {
        model.nodes.push(Node {
            id: NodeId(id),
            coord,
            story: None,
            ..model.nodes[1].clone()
        });
    }
    for (id, nodes) in [(1, [NodeId(3), NodeId(4)]), (2, [NodeId(5), NodeId(6)])] {
        model.elements.push(ElementData {
            id: ElemId(id),
            kind: if id == 1 {
                ElementKind::Damper
            } else {
                ElementKind::Beam
            },
            nodes: nodes.into_iter().collect(),
            ..model.elements[0].clone()
        });
    }
    model.load_cfg.as_mut().unwrap().dampers.push(DamperSpec {
        elem: ElemId(1),
        total_weight: 19613.3,
    });
    let generated = generate_stories_with_opts(&model, &[], true, method).unwrap();
    for (node, story) in model.nodes.iter_mut().zip(generated.node_story) {
        node.story = story;
    }
    model.stories = generated.stories;
    model.constraints = generated.constraints;
    model.generated_masters = generated.generated_masters;
    model.damper_mass_generation = Some(generated.damper_mass_generation);
    model.nodes.extend(generated.rep_nodes);
    model
}

fn assert_job_input_error(model: Model) {
    assert!(matches!(
        compute_eigen(model.clone(), 1),
        Err(JobError::InvalidInput(_))
    ));
    for nonlinear in [false, true] {
        let cfg = AnalysisSettings {
            th_nonlinear: nonlinear,
            ..Default::default()
        };
        let wave = GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0; 2],
            accel_y: None,
            accel_theta: None,
        };
        assert!(matches!(
            compute_time_history(model.clone(), cfg, wave),
            Err(JobError::InvalidInput(_))
        ));
    }
    assert_lumped_input_error(&model);
}

fn assert_lumped_input_error(model: &Model) {
    use sepika_job::lumped_mass::{build_lumped_mass, LumpedMassBuildInput};
    use sepika_solver::dynamic::lumped_mass::{LumpedStiffnessSource, StickDim};
    use sepika_solver::statics::analysis::SeismicDir;
    let expected = model.validate_damper_mass_placement().unwrap_err();
    for dim in [StickDim::Planar, StickDim::Spatial] {
        for nonlinear in [false, true] {
            let input = LumpedMassBuildInput {
                model,
                dim,
                source: LumpedStiffnessSource::StoryQd,
                dir: SeismicDir::X,
                nonlinear,
                secant_ratio: 0.75,
                res_x: None,
                res_y: None,
                po_x: None,
                po_y: None,
            };
            let error = build_lumped_mass(input).unwrap_err();
            assert!(matches!(error, JobError::InvalidInput(message) if message == expected));
            let cfg = AnalysisSettings {
                lumped_dim: dim,
                lumped_nonlinear: nonlinear,
                ..Default::default()
            };
            for accel in [None, Some(&[0.0, 0.0][..])] {
                let error = compute_lumped_mass(model.clone(), cfg, None, None, None, None, accel)
                    .unwrap_err();
                assert!(matches!(error, JobError::InvalidInput(message) if message == expected));
            }
        }
    }
}

fn sample_pushover() -> sepika_solver::nonlinear::pushover::PushoverResult {
    use sepika_solver::nonlinear::pushover::{CapacityPoint, MechanismType, PushoverResult};
    PushoverResult {
        steps: vec![],
        wall_history: None,
        wall_run: None,
        confirmed_history: None,
        ds_evaluation: None,
        capacity_evaluation: None,
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
fn 質点系job入口も不正指定と編集後の古い質量を拒否する() {
    use sepika_job::lumped_mass::{build_lumped_mass, LumpedMassBuildInput};
    use sepika_solver::dynamic::lumped_mass::{LumpedStiffnessSource, StickDim};
    use sepika_solver::statics::analysis::SeismicDir;
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let model = separated_model(method);
        let po = sample_pushover();
        let input = LumpedMassBuildInput {
            model: &model,
            dim: StickDim::Planar,
            source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            secant_ratio: 0.75,
            res_x: None,
            res_y: None,
            po_x: Some(&po),
            po_y: None,
        };
        let valid = build_lumped_mass(input).unwrap();
        assert!((valid.stories[0].mass - 4.0).abs() < 1e-10);
        let cfg = AnalysisSettings {
            lumped_dim: StickDim::Planar,
            lumped_nonlinear: true,
            lumped_n_modes: 1,
            ..Default::default()
        };
        let result =
            compute_lumped_mass(model.clone(), cfg, None, None, Some(po), None, None).unwrap();
        assert!((result.model.stories[0].mass - 4.0).abs() < 1e-10);
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
                4 => edited.nodes[0].coord[0] += 500.0,
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
            assert_lumped_input_error(&edited);
        }
    }
}

#[test]
fn 動的質量の全量と重心と慣性の単独不整合をjob直接と公開computeで拒否する() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let baseline = separated_model(method);
        for case in 0..16 {
            let mut model = baseline.clone();
            match case {
                0 => {
                    model.stories[1]
                        .dynamic_mass
                        .as_mut()
                        .unwrap()
                        .mass_equiv_weight_n *= 0.5
                }
                1 => {
                    model.stories[1]
                        .dynamic_mass
                        .as_mut()
                        .unwrap()
                        .mass_equiv_weight_n = 0.0
                }
                2 => {
                    model.stories[1]
                        .dynamic_mass
                        .as_mut()
                        .unwrap()
                        .mass_equiv_weight_n = -1.0
                }
                3 => {
                    model.stories[1]
                        .dynamic_mass
                        .as_mut()
                        .unwrap()
                        .mass_equiv_weight_n = f64::NAN
                }
                4 => model.stories[1].dynamic_mass.as_mut().unwrap().center_xy_mm[0] += 500.0,
                5 => model.stories[1].dynamic_mass.as_mut().unwrap().center_xy_mm[1] += 500.0,
                6 => {
                    model.stories[1].dynamic_mass.as_mut().unwrap().center_xy_mm[0] = f64::INFINITY
                }
                7 => model.stories[1].dynamic_mass.as_mut().unwrap().center_xy_mm[1] = f64::NAN,
                8 => {
                    model.stories[1]
                        .dynamic_mass
                        .as_mut()
                        .unwrap()
                        .inertia_t_mm2 *= 0.5
                }
                9 => {
                    model.stories[1]
                        .dynamic_mass
                        .as_mut()
                        .unwrap()
                        .inertia_t_mm2 = -1.0
                }
                10 => {
                    model.stories[1]
                        .dynamic_mass
                        .as_mut()
                        .unwrap()
                        .inertia_t_mm2 = f64::INFINITY
                }
                11 => model.stories[1].dynamic_mass = None,
                12 => model.stories[1].dynamic_mass.as_mut().unwrap().lumped_mass = None,
                13 => model
                    .damper_mass_generation
                    .as_mut()
                    .unwrap()
                    .dynamic_masses
                    .clear(),
                14 => model.damper_mass_generation = None,
                _ => {
                    model.stories[1].dynamic_mass = None;
                    model.damper_mass_generation = None;
                }
            }
            assert_job_input_error(model);
        }
    }
}

#[test]
fn 記録集合の部分欠落と重複と余分な要素も公開jobで拒否する() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let baseline = generated_model(method);
        assert_eq!(
            baseline
                .damper_mass_generation
                .as_ref()
                .unwrap()
                .dynamic_masses
                .len(),
            2
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
            let mut model = baseline.clone();
            let unrelated = sepika_core::model::MassPlacementNode {
                id: NodeId(1),
                coord: baseline.nodes[1].coord,
                restraint: baseline.nodes[1].restraint,
                structural: sepika_core::dof::structural_nodes(&baseline)[1],
            };
            let extra_constraint = baseline.constraints[1].clone();
            let record = model.damper_mass_generation.as_mut().unwrap();
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
                15 => record.placements[0].anchors.push(NodeId(1)),
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
            assert_eq!(model.nodes, baseline.nodes);
            assert_eq!(model.stories, baseline.stories);
            assert_eq!(model.constraints, baseline.constraints);
            assert_eq!(model.load_cfg, baseline.load_cfg);
            assert_job_input_error(model);
        }
    }
}

#[test]
fn 同階合計保存の重量再配分と指定先変更と配置編集も公開ジョブで拒否する() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let baseline = separated_model(method);
        let master = baseline.stories[1]
            .dynamic_mass
            .unwrap()
            .lumped_mass
            .unwrap()
            .master
            .index();
        for case in 0..6 {
            let mut model = baseline.clone();
            match case {
                0 => {
                    let d = &mut model.load_cfg.as_mut().unwrap().dampers;
                    d[0].total_weight += 9806.65;
                    d[1].total_weight -= 9806.65;
                }
                1 => model.load_cfg.as_mut().unwrap().dampers[0].elem = ElemId(2),
                2 => model.nodes[0].coord[0] += 500.0,
                3 => model.nodes[master].coord[0] += 500.0,
                4 => model.nodes[master]
                    .restraint
                    .set_fixed(sepika_core::dof::Dof::Ux),
                _ => {
                    for c in &mut model.constraints {
                        if let sepika_core::model::Constraint::RigidDiaphragm {
                            story,
                            slaves,
                            ..
                        } = c
                        {
                            if story.0 == 1 {
                                slaves.pop();
                            }
                        }
                    }
                }
            }
            assert_eq!(
                model
                    .load_cfg
                    .as_ref()
                    .unwrap()
                    .dampers
                    .iter()
                    .map(|d| d.total_weight)
                    .sum::<f64>(),
                39226.6
            );
            assert_job_input_error(model);
        }
    }
}

#[test]
fn 指定なし階生成後のゼロ重量指定追加も公開ジョブで再生成を求める() {
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        let mut model = separated_model(method);
        model.load_cfg.as_mut().unwrap().dampers.clear();
        model.elements[0].kind = ElementKind::Beam;
        model.elements[1].kind = ElementKind::Beam;
        let gen = generate_stories_with_opts(&model, &[], true, method).unwrap();
        model.stories = gen.stories;
        model.constraints = gen.constraints;
        model.damper_mass_generation = Some(gen.damper_mass_generation);
        model.generated_masters = gen.generated_masters;
        for n in gen.rep_nodes {
            let idx = n.id.index();
            model.nodes[idx] = n;
        }
        model.load_cfg.as_mut().unwrap().dampers.push(DamperSpec {
            elem: ElemId(0),
            total_weight: 0.0,
        });
        assert_job_input_error(model);
    }
}
