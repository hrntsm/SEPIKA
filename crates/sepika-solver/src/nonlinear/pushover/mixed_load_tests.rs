use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{ElementKind, ForceRegime, LocalAxis, MemberLoadKind};
use sepika_element::behavior::Ctx;

fn mixed_model(kind: ElementKind) -> Model {
    let mut model = super::super::tests::prepared_wall_story_model_with(4000., 1000.);
    for elem in &mut model.elements {
        let face_i = elem.rigid_zone.face_i;
        let face_j = elem.rigid_zone.face_j;
        elem.rigid_zone = sepika_core::model::RigidZone {
            face_i,
            face_j,
            ..Default::default()
        };
    }
    let steel = super::super::tests::single_column_model(1e10, 0.);
    let mut material = steel.materials[0].clone();
    material.id = MaterialId(2);
    model.materials.push(material);
    let mut section = steel.sections[0].clone();
    section.id = SectionId(3);
    section.material = Some(MaterialId(2));
    section.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
    section.as_y = 10000.;
    section.as_z = 10000.;
    model.sections.push(section);
    for (i, x) in [0., 6000., 7000.].into_iter().enumerate() {
        let mut node = model.nodes[0].clone();
        node.id = NodeId(4 + i as u32);
        node.coord = [x, 10000., 0.];
        node.story = None;
        node.restraint = if i == 2 {
            Dof6Mask(0b111110)
        } else {
            Dof6Mask::FIXED
        };
        model.nodes.push(node);
    }
    for (id, kind, nodes, force_regime) in [
        (5, kind, [4, 5], ForceRegime::AxialBendingInteract),
        (
            6,
            ElementKind::Beam,
            [5, 6],
            ForceRegime::UniaxialBendingShear,
        ),
    ] {
        let mut elem = model.elements[2].clone();
        elem.id = ElemId(id);
        elem.kind = kind;
        elem.nodes = nodes.map(NodeId).into_iter().collect();
        elem.section = Some(SectionId(3));
        elem.local_axis = LocalAxis {
            ref_vector: [0., 1., 0.],
        };
        elem.force_regime = force_regime;
        elem.plastic_zone = Some(100.);
        model.elements.push(elem);
    }
    model.load_cases[0].nodal.clear();
    model.load_cases[0].member = [
        (ElemId(3), [1., 0., 0.], 3000., 1.),
        (ElemId(5), [0., 1., 0.], 6000., 1.),
        (ElemId(6), [1., 0., 0.], 1000., 2.),
    ]
    .map(|(id, dir, length, w)| {
        MemberLoad::manual(
            id,
            dir,
            MemberLoadKind::Distributed {
                a: 0.,
                b: length,
                w1: w,
                w2: w,
            },
        )
    })
    .into();
    model
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-6 * expected.abs().max(1.),
        "{actual} != {expected}"
    );
}

#[test]
fn mixed_gravity_and_confirmed_pushover_keep_all_member_load_paths_consistent() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let model = mixed_model(kind);
        let map = DofMap::build(&model);
        let reducer = Reducer::build(&model, &map);
        let mut reference = model.clone();
        reference.load_cases[0].member.clear();
        let capacities = crate::nonlinear::wall_reference::resolve(
            &reference,
            true,
            StrengthBasis::MaterialStrength,
        )
        .unwrap();
        let mut behaviors: Vec<_> = model
            .elements
            .iter()
            .map(|elem| {
                build_nonlinear_behavior(
                    elem,
                    &model,
                    StrengthBasis::MaterialStrength,
                    sepika_core::model::AnalysisKind::Incremental,
                    capacities
                        .iter()
                        .find(|(id, _)| *id == elem.id)
                        .map(|(_, q)| *q),
                )
            })
            .collect();
        assert!(behaviors[3].owns_frame_member_loads());
        assert!(behaviors[5].owns_frame_member_loads());
        assert!(behaviors[6].owns_frame_member_loads());
        let (rhs, loads) =
            prepare_nonlinear_loads(&model, &map, &behaviors, &model.load_cases[0]).unwrap();
        let active = map.active(6 * 6).unwrap() as usize;
        close(rhs[active], 0.);
        let mut displacement = vec![0.; map.n_active()];
        let mut solver = SolverState::new(map.n_active(), reducer.n_indep);
        apply_initial_load(
            &model,
            &map,
            &reducer,
            &mut behaviors,
            &rhs,
            &loads,
            false,
            &mut displacement,
            &mut solver,
        )
        .unwrap();
        close(displacement[active], 1e6 / (205000. * 10000.));
        let responses = super::super::geom::member_recording_responses(&model, &behaviors).unwrap();
        let ordinary = responses[5].as_ref().unwrap();
        close(ordinary.physical_frame_end_force_global[1], -3000.);
        close(ordinary.physical_frame_end_force_global[7], -3000.);
        close(ordinary.physical_frame_end_force_global[5], -3e6);
        close(ordinary.physical_frame_end_force_global[11], 3e6);
        close(ordinary.section.at(0.5).unwrap()[5], -1.5e6);
        close(
            ordinary.physical_frame_end_force_global[1]
                + ordinary.physical_frame_end_force_global[7]
                + 6000.,
            0.,
        );
        close(
            ordinary.physical_frame_end_force_global[5]
                + ordinary.physical_frame_end_force_global[11]
                + 6000. * ordinary.physical_frame_end_force_global[7]
                + 18e6,
            0.,
        );
        let wall = responses[3].as_ref().unwrap();
        close(wall.physical_frame_end_force_global[0], -1500.);
        close(wall.physical_frame_end_force_global[6], -1500.);
        close(wall.physical_frame_end_force_global[4], 0.);
        close(wall.physical_frame_end_force_global[10], 0.);
        close(wall.section.at(0.5).unwrap()[5].abs(), 1.125e6);
        let legacy = behaviors[6].internal_force(&Ctx { model: &model });
        close(legacy.data[0], -2000.);
        close(legacy.data[6], 0.);
        let legacy_support_reaction = legacy.data[0];
        close(legacy_support_reaction + 2000., 0.);
        let applied = responses[6]
            .as_ref()
            .unwrap()
            .section
            .applied_nodal_forces_global();
        close(applied[0][0] * 0.1 + applied[1][0] * -0.3, -200.);
        close(
            responses[6].as_ref().unwrap().section.at(0.5).unwrap()[0],
            1000.,
        );
        let reduced_internal = reducer.reduce_f(&compute_f_int(&model, &map, &behaviors));
        for (actual, expected) in reduced_internal.into_iter().zip(reducer.reduce_f(&rhs)) {
            close(actual, expected);
        }
        let heights = super::super::response::story_heights(&model);
        let mut recorder = super::super::step_record::StepRecorder::new(
            &model,
            SeismicDir::X,
            &heights,
            DuctilityMethod::default(),
            super::super::loads::PhysicalPushoverLoads::empty(),
        )
        .unwrap();
        recorder.constant_external = rhs;
        recorder.reference_external = vec![0.; map.n_active()];
        recorder.physical_loads = super::super::loads::PhysicalPushoverLoads::new(
            &model,
            &[(model.load_cases[0].id, 1.)],
            super::super::loads::PhysicalPushoverLoads::empty().reference,
        )
        .unwrap();
        recorder
            .record(0., &model, &map, &behaviors, &displacement)
            .unwrap();
        let recorded = recorder.finish();
        close(recorded.member_history_steps[0][5].mz_i.abs() as f64, 3e6);
        close(recorded.member_history_steps[0][5].mz_j.abs() as f64, 3e6);
        close(recorded.member_history_steps[0][3].mz_i.abs() as f64, 0.);
        close(recorded.member_history_steps[0][3].mz_j.abs() as f64, 0.);
        close(recorded.member_history_steps[0][6].n as f64, -2000.);
        let error =
            pushover_analysis(&model, &map, &reducer, SeismicDir::X, 1, 0., false).unwrap_err();
        assert!(
            error.contains("壁 ID 0")
                && error.contains("部材 ID 3")
                && error.contains("部材荷重は未対応"),
            "{error}"
        );
        let mut axial_model = model.clone();
        axial_model.load_cases[0].member[0].dir = [0., 0., -1.];
        axial_model.load_cases[0].member[0].kind = MemberLoadKind::Distributed {
            a: 0.,
            b: 3000.,
            w1: 0.001,
            w2: 0.001,
        };
        for node in &mut axial_model.nodes[2..4] {
            node.restraint = Dof6Mask(0b111100);
        }
        let axial_map = DofMap::build(&axial_model);
        let axial_reducer = Reducer::build(&axial_model, &axial_map);
        let result = pushover_analysis(
            &axial_model,
            &axial_map,
            &axial_reducer,
            SeismicDir::X,
            1,
            0.,
            false,
        )
        .unwrap();
        assert_eq!(result.steps[0].load_factor, 0.);
        let owner = result
            .member_response
            .iter()
            .find(|r| r.elem == ElemId(5))
            .unwrap();
        close(owner.m_strong, 3e6);
        close(owner.shear_strong, 3000.);
        let wall = result
            .member_response
            .iter()
            .find(|r| r.elem == ElemId(3))
            .unwrap();
        close(wall.axial, 1.5);
        close(wall.shear_strong, 0.);
        let legacy = result
            .member_response
            .iter()
            .find(|r| r.elem == ElemId(6))
            .unwrap();
        close(legacy.m_strong, 0.);
        close(legacy.shear_strong, 0.);
    }
}

#[test]
fn fixed_member_full_initial_load_matches_one_committed_trial_and_restores_on_failure() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let mut model = super::super::tests::single_column_model(1e10, 0.);
        model.elements[0].kind = kind;
        for node in &mut model.nodes {
            node.restraint = Dof6Mask::FIXED;
        }
        let map = DofMap::build(&model);
        let reducer = Reducer::build(&model, &map);
        let build = |model: &Model, elem: &sepika_core::model::ElementData| {
            build_nonlinear_behavior(
                elem,
                model,
                StrengthBasis::MaterialStrength,
                sepika_core::model::AnalysisKind::Incremental,
                None,
            )
        };
        let mut actual = vec![build(&model, &model.elements[0])];
        let mut expected = vec![build(&model, &model.elements[0])];
        let load = MemberLoad::manual(
            model.elements[0].id,
            [1., 0., 0.],
            MemberLoadKind::Distributed {
                a: 0.,
                b: 3000.,
                w1: 2.,
                w2: 2.,
            },
        );
        let loads = vec![vec![load.clone()]];
        apply_du_trial_with_member_loads(&model, &map, &mut expected, &[], &loads).unwrap();
        expected[0].commit_state();
        let mut solver = SolverState::new(0, 0);
        apply_initial_load(
            &model,
            &map,
            &reducer,
            &mut actual,
            &[],
            &loads,
            false,
            &mut [],
            &mut solver,
        )
        .unwrap();
        assert_eq!(
            actual[0].serialize_checkpoint(),
            expected[0].serialize_checkpoint()
        );
        let response = actual[0].state_frame_load_response().unwrap().unwrap();
        close(response.physical_frame_end_force_global[0], -3000.);
        close(response.physical_frame_end_force_global[6], -3000.);

        let mut duplicate = model.elements[0].clone();
        duplicate.id = ElemId(1);
        model.elements.push(duplicate.clone());
        actual.push(build(&model, &duplicate));
        let before: Vec<_> = actual.iter().map(|b| b.serialize_checkpoint()).collect();
        let mut invalid = load.clone();
        invalid.elem = ElemId(1);
        if let MemberLoadKind::Distributed { w1, w2, .. } = &mut invalid.kind {
            *w1 = f64::MAX;
            *w2 = f64::MAX;
        }
        let error = apply_initial_load(
            &model,
            &map,
            &reducer,
            &mut actual,
            &[],
            &[vec![load], vec![invalid]],
            false,
            &mut [],
            &mut solver,
        )
        .unwrap_err();
        assert!(
            error.contains("長期荷重の初期載荷") && error.contains("1"),
            "{error}"
        );
        assert_eq!(
            actual
                .iter()
                .map(|b| b.serialize_checkpoint())
                .collect::<Vec<_>>(),
            before
        );
    }
}

#[test]
fn initial_newton_factor_failure_preserves_displacement_and_loaded_element_snapshot() {
    let mut model = super::super::tests::single_column_model(1e10, 0.);
    for node in &mut model.nodes {
        node.restraint = Dof6Mask::FIXED;
    }
    let mut disconnected = model.nodes[1].clone();
    disconnected.id = NodeId(2);
    disconnected.restraint = Dof6Mask(0b111110);
    model.nodes.push(disconnected);
    model
        .constraints
        .push(sepika_core::model::Constraint::RigidLink {
            master: NodeId(2),
            slaves: vec![],
            dofs: Dof6Mask::FIXED,
        });
    let map = DofMap::build(&model);
    let reducer = Reducer::build(&model, &map);
    assert_eq!(map.n_active(), 1);
    assert_eq!(reducer.n_indep, 1);
    let mut behaviors = vec![build_nonlinear_behavior(
        &model.elements[0],
        &model,
        StrengthBasis::MaterialStrength,
        sepika_core::model::AnalysisKind::Incremental,
        None,
    )];
    let before = behaviors[0].serialize_checkpoint();
    let load = MemberLoad::manual(
        model.elements[0].id,
        [1., 0., 0.],
        MemberLoadKind::Distributed {
            a: 0.,
            b: 3000.,
            w1: 2.,
            w2: 2.,
        },
    );
    let mut displacement = [0.25];
    let mut solver = SolverState::new(1, 1);
    let error = apply_initial_load(
        &model,
        &map,
        &reducer,
        &mut behaviors,
        &[1.],
        &[vec![load]],
        false,
        &mut displacement,
        &mut solver,
    )
    .unwrap_err();
    assert!(error.contains("長期荷重の初期載荷"), "{error}");
    assert_eq!(behaviors[0].serialize_checkpoint(), before);
    assert_eq!(displacement, [0.25]);
}
