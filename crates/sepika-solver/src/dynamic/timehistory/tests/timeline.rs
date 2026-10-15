use super::*;

fn model() -> Model {
    let mut model = sdof_model();
    model.materials[0].young = 40_000.0;
    model.stories = fiber_column_model(1e10).stories;
    model.nodes[1].story = Some(sepika_core::ids::StoryId(1));
    model.stories[1].node_ids = vec![NodeId(1)];
    model
}

fn wave(accel: &[f64]) -> GroundMotion {
    GroundMotion {
        dt: 0.1,
        accel_x: accel.to_vec(),
        accel_y: None,
        accel_theta: None,
    }
}

fn damping() -> Damping {
    Damping::MassProportional { h: 0.0, omega: 1.0 }
}

#[test]
fn instantaneous_samples_match_independent_newmark_displacements_and_initial_equilibrium() {
    let model = model();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    for (input, expected) in [
        (
            vec![0.0, 100.0, 0.0],
            vec![0.0, -100.0 / 440.0, -0.8264462809917356],
        ),
        (vec![100.0, 0.0], vec![0.0, -100.0 / 440.0]),
        (vec![100.0], vec![0.0]),
        (vec![0.0, 0.0], vec![0.0, 0.0]),
    ] {
        let wave = wave(&input);
        let (linear, state) = linear_time_history_with_state(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &NewmarkCfg::average_accel(),
            &damping(),
            &[0.0],
            &[0.0],
            false,
            Some(1),
        )
        .unwrap();
        assert_eq!(state.step as usize, input.len() - 1);
        assert_eq!(state.time, (input.len() - 1) as f64 * 0.1);
        if input.len() == 1 {
            assert_eq!(state.disp_red, vec![0.0]);
            assert_eq!(state.vel_red, vec![0.0]);
            assert_eq!(state.accel_red, vec![-100.0]);
        }
        let nonlinear = nonlinear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &NewmarkCfg::average_accel(),
            &damping(),
            DampingAccumulation::Cumulative,
            &[0.0],
            &[0.0],
            NonlinearThCfg {
                apply_long_term: false,
                record_every: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        for result in [linear, nonlinear] {
            assert_eq!(result.time.len(), input.len());
            assert_eq!(result.non_converged_steps, 0);
            let rec = result.recording.unwrap();
            assert_eq!(rec.frame_time, result.time);
            for (i, &u) in expected.iter().enumerate() {
                assert_eq!(result.time[i], i as f64 * 0.1);
                assert!((result.history.node_disp[i] - u).abs() <= 1e-9);
                assert!((rec.node_disp[i][1][0] - u).abs() <= 1e-9);
                assert!((rec.story_x.floor_accel[i][0] + 40.0 * u).abs() < 1e-8);
                assert!((result.history.base_shear[i] - 40.0 * u).abs() < 1e-8);
                assert!((rec.story_x.story_shear[i][0] - 40.0 * u).abs() < 1e-8);
            }
            assert_eq!(rec.story_x.floor_vel[0], vec![0.0]);
            assert_eq!(rec.story_x.floor_accel[0], vec![0.0]);
            assert_eq!(result.history.base_shear[0], 0.0);
            if input.len() == 2 && input[0] == 100.0 {
                assert!((rec.story_x.floor_accel[1][0] - 100.0 / 11.0).abs() < 1e-9);
            }
        }
    }
}

#[test]
fn checkpoint_preserves_committed_state_and_absolute_recording_grid() {
    let model = model();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    let input = [100.0, 0.0, -30.0, 20.0, 0.0, 40.0];
    let full = wave(&input);
    let run = |wave: &GroundMotion| {
        linear_time_history_with_state(
            &model,
            &dofmap,
            &reducer,
            wave,
            &NewmarkCfg::average_accel(),
            &damping(),
            &[0.0],
            &[0.0],
            false,
            Some(3),
        )
        .unwrap()
    };
    let (continuous, final_state) = run(&full);
    assert_eq!(
        continuous.recording.unwrap().frame_time,
        vec![0.0, 3.0 * 0.1, 0.5]
    );
    for step in [0, 2, 5] {
        let (_, anchor) = run(&wave(&input[..=step]));
        let (resumed, state) = linear_time_history_from_state(
            &model,
            &dofmap,
            &reducer,
            &full,
            &NewmarkCfg::average_accel(),
            &damping(),
            &anchor,
            false,
            Some(3),
        )
        .unwrap();
        assert_eq!(state, final_state);
        assert_eq!(resumed.time[0], anchor.time);
        assert_eq!(resumed.time.len(), input.len() - step);
        let rec = resumed.recording.unwrap();
        let expected_steps: Vec<_> = (step..input.len())
            .filter(|&s| s == step || s % 3 == 0 || s == 5)
            .map(|s| s as f64 * 0.1)
            .collect();
        assert_eq!(rec.frame_time, expected_steps);
        assert_eq!(rec.node_disp[0][1][0], anchor.disp_red[0]);
        assert_eq!(rec.story_x.floor_vel[0][0], anchor.vel_red[0]);
        assert!(
            (rec.story_x.floor_accel[0][0] - (anchor.accel_red[0] + input[step])).abs() < 1e-12
        );
    }
    let mut sentinel = final_state.clone();
    sentinel.accel_red[0] = 123.0;
    let (_, unchanged) = linear_time_history_from_state(
        &model,
        &dofmap,
        &reducer,
        &full,
        &NewmarkCfg::average_accel(),
        &damping(),
        &sentinel,
        false,
        Some(3),
    )
    .unwrap();
    assert_eq!(unchanged, sentinel);
    let mut beyond = final_state;
    beyond.step += 1;
    assert!(linear_time_history_from_state(
        &model,
        &dofmap,
        &reducer,
        &full,
        &NewmarkCfg::average_accel(),
        &damping(),
        &beyond,
        false,
        None,
    )
    .is_err());
}

#[test]
fn empty_input_is_rejected_by_both_spatial_solvers() {
    let model = model();
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    assert!(linear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave(&[]),
        &NewmarkCfg::average_accel(),
        &damping(),
        &[0.0],
        &[0.0],
        false,
        None,
    )
    .is_err());
    assert!(nonlinear_time_history_analysis(
        &model,
        &dofmap,
        &reducer,
        &wave(&[]),
        &NewmarkCfg::average_accel(),
        &damping(),
        DampingAccumulation::Cumulative,
        &[0.0],
        &[0.0],
        Default::default(),
    )
    .is_err());
}

#[test]
fn singular_initial_mass_keeps_existing_zero_near_zero_and_error_contract() {
    let mut model = model();
    let mut massless = model.nodes[1].clone();
    massless.id = NodeId(2);
    massless.coord = [2000.0, 0.0, 0.0];
    massless.mass = None;
    massless.story = None;
    massless.support_spring = Some([40.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    model.nodes[1].support_spring = Some([20.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    model.nodes.push(massless);
    model.elements[0].nodes = smallvec::smallvec![NodeId(1), NodeId(2)];
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    for (input, accepted) in [
        (vec![0.0, 0.0], true),
        (vec![0.0, 100.0, 0.0], true),
        (vec![5e-10, 0.0], true),
        (vec![1e-9, 0.0], false),
        (vec![100.0, 0.0], false),
    ] {
        let wave = wave(&input);
        let linear = linear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &NewmarkCfg::average_accel(),
            &damping(),
            &[0.0; 2],
            &[0.0; 2],
            false,
            Some(1),
        );
        let nonlinear = nonlinear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &NewmarkCfg::average_accel(),
            &damping(),
            DampingAccumulation::Cumulative,
            &[0.0; 2],
            &[0.0; 2],
            NonlinearThCfg {
                apply_long_term: false,
                record_every: Some(1),
                ..Default::default()
            },
        );
        for result in [linear, nonlinear] {
            if accepted {
                let result = result.unwrap();
                assert_eq!(result.time.len(), input.len());
                let recording = result.recording.unwrap();
                assert_eq!(recording.story_x.floor_accel[0][0], input[0]);
                if input == [0.0, 100.0, 0.0] {
                    assert!((recording.node_disp[1][1][0] + 100.0 / 440.0).abs() < 1e-9);
                    assert!((recording.node_disp[1][2][0] + 50.0 / 440.0).abs() < 1e-9);
                }
            } else {
                assert!(matches!(
                    result,
                    Err(sepika_math::solver::SolveError::InvalidInput(_))
                ));
            }
        }
    }
}
