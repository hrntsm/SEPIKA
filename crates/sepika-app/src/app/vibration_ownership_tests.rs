use super::tests::{dummy_lumped, dummy_th, test_tmp, wait_for_job};
use super::*;

fn ownership_wave(value: f64, dt: f64) -> sepika_solver::dynamic::timehistory::GroundMotion {
    sepika_solver::dynamic::timehistory::GroundMotion {
        dt,
        accel_x: vec![0.0, value, -value],
        accel_y: None,
        accel_theta: None,
    }
}

fn ownership_result(marker: f64) -> sepika_solver::dynamic::timehistory::ResponseResult {
    let mut result = dummy_th(vec![0.0, 0.02]);
    result.peak_disp = vec![[marker, 0.0, 0.0, 0.0, 0.0, 0.0]];
    result
}

fn ownership_job(app: &mut App, result: Option<JobResult>) {
    let (tx, rx) = std::sync::mpsc::channel();
    app.core.scoped.job = Some(AnalysisJob {
        label: "所属fixture",
        started: SystemTime::now(),
        input: app.job_input(),
        rx,
        #[cfg(feature = "gui")]
        jump_on_success: None,
    });
    if let Some(result) = result {
        tx.send(result).unwrap();
    }
}

fn ownership_portal() -> App {
    let mut app = App::default();
    app.load_model(crate::app::tests::with_explicit_pushover_gravity(
        crate::sample::portal_frame(),
    ));
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.th_duration = 0.06;
    app.core.analysis_cfg.th_dt = 0.02;
    app.core.analysis_cfg.lumped_th_duration = 0.06;
    app.core.analysis_cfg.lumped_th_dt = 0.02;
    app.generate_stories_action();
    app
}

#[cfg(feature = "gui")]
fn ownership_unset_portal() -> App {
    let mut model = crate::sample::portal_frame();
    let mut base = model.elements[2].clone();
    base.id = sepika_core::ids::ElemId(3);
    base.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    model.elements.push(base);
    let mut app = ownership_portal();
    app.load_model(crate::app::tests::with_explicit_pushover_gravity(model));
    app.generate_stories_action();
    app.ensure_preparation();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let (floor, wall) = app.core.model.unset_plate_assignment_regions();
    assert!(!floor.is_empty() || !wall.is_empty());
    app
}

#[test]
fn time_history_ownership_sample_amplitude_changes_actual_input_and_preserves_other_settings() {
    let mut app = ownership_portal();
    for lumped in [false, true] {
        let make = |app: &App| {
            if lumped {
                let wave = sepika_job::sample_lumped_ground_motion(&app.core.analysis_cfg).unwrap();
                let accel =
                    sepika_job::lumped_accel_from_wave(&wave, app.core.analysis_cfg.lumped_dir)
                        .unwrap();
                app.lumped_time_history_input(accel, "同名サンプル".into())
                    .source
            } else {
                app.time_history_input(
                    App::sample_wave(&app.core.analysis_cfg).unwrap(),
                    "同名サンプル".into(),
                )
                .source
            }
        };
        let before = make(&app);
        let wave_before = if lumped {
            sepika_job::sample_lumped_ground_motion(&app.core.analysis_cfg).unwrap()
        } else {
            App::sample_wave(&app.core.analysis_cfg).unwrap()
        };
        if lumped {
            app.core.analysis_cfg.lumped_th_amp *= 2.0;
        } else {
            app.core.analysis_cfg.th_amp *= 2.0;
        }
        let wave_after = if lumped {
            sepika_job::sample_lumped_ground_motion(&app.core.analysis_cfg).unwrap()
        } else {
            App::sample_wave(&app.core.analysis_cfg).unwrap()
        };
        assert!(wave_before.accel_x.iter().any(|a| a.abs() > 0.0));
        assert_eq!(
            wave_after.accel_x,
            wave_before
                .accel_x
                .iter()
                .map(|a| 2.0 * a)
                .collect::<Vec<_>>()
        );
        assert_eq!(wave_after.dt, wave_before.dt);
        let after = make(&app);
        assert_ne!(before.identity, after.identity);
        if lumped {
            app.apply_lumped_mass_result(Ok(dummy_lumped(0.21, true)), Some(before));
            app.apply_lumped_mass_result(Ok(dummy_lumped(0.31, true)), Some(after));
            let rerun = make(&app);
            assert_eq!(
                rerun.target,
                Some(app.core.model.lumped_vibration_cases[1].id.0)
            );
            app.apply_lumped_mass_result(Ok(dummy_lumped(0.41, true)), Some(rerun));
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .lumped_results
                    .len(),
                2
            );
        } else {
            app.apply_time_history_result(Ok(ownership_result(11.0)), before);
            app.apply_time_history_result(Ok(ownership_result(22.0)), after);
            let rerun = make(&app);
            assert_eq!(rerun.target, Some(app.core.model.vibration_cases[1].id.0));
            app.apply_time_history_result(Ok(ownership_result(33.0)), rerun);
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .time_histories
                    .len(),
                2
            );
        }
    }
}

#[test]
fn lumped_time_history_ownership_distinguishes_current_pushover_generation_conditions() {
    use sepika_solver::dynamic::lumped_mass::{LumpedStiffnessSource, StickDim};
    #[cfg(not(feature = "gui"))]
    let mut app = ownership_portal();
    #[cfg(feature = "gui")]
    let mut app = ownership_unset_portal();
    app.core.analysis_cfg.ai_mode = AiMode::Approx;
    app.core.analysis_cfg.lumped_dim = StickDim::Planar;
    app.core.analysis_cfg.lumped_stiffness = LumpedStiffnessSource::StoryQd;
    app.core.analysis_cfg.lumped_dir = SeismicDir::X;
    app.core.analysis_cfg.lumped_nonlinear = true;
    app.core.analysis_cfg.push_dir = SeismicDir::X;
    app.core.analysis_cfg.push_steps = 10;
    app.run_seismic(SeismicDir::X);
    let mut sources = Vec::new();
    let mut predecessors = Vec::new();
    for max_disp_mm in [5.0, 100.0] {
        app.core.analysis_cfg.push_use_max_disp = true;
        app.core.analysis_cfg.push_use_drift_angle = false;
        app.core.analysis_cfg.push_max_disp = max_disp_mm;
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert!(app
            .require_result_input(ResultInputKey::Pushover(SeismicDir::X))
            .is_ok());
        predecessors.push(
            bincode::serialize(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .pushover_x
                    .as_ref()
                    .unwrap(),
            )
            .unwrap(),
        );
        let input = app.lumped_time_history_input(vec![0.0, 1.0, -1.0], "同名.csv".into());
        sources.push(input.source.identity.clone());
        app.start_lumped_mass_th_input_job(input);
        assert!(
            app.core.scoped.job.is_some(),
            "{:?}",
            app.core.scoped.last_error
        );
        wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
    }
    assert_ne!(predecessors[0], predecessors[1]);
    assert_ne!(sources[0], sources[1]);
    assert_eq!(app.core.model.lumped_vibration_cases.len(), 2);
    let first = app.core.model.lumped_vibration_cases[0].id;
    let second = app.core.model.lumped_vibration_cases[1].id;
    let original = bincode::serialize(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped_result_for(first)
            .unwrap(),
    )
    .unwrap();
    let rerun = app.lumped_time_history_input(vec![0.0, 1.0, -1.0], "同名.csv".into());
    assert_eq!(rerun.source.target, Some(second.0));
    assert_eq!(rerun.source.identity, sources[1]);
    app.start_lumped_mass_th_input_job(rerun);
    assert!(
        app.core.scoped.job.is_some(),
        "{:?}",
        app.core.scoped.last_error
    );
    wait_for_job(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_eq!(app.core.model.lumped_vibration_cases.len(), 2);
    assert_eq!(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped_results
            .len(),
        2
    );
    assert_eq!(
        original,
        bincode::serialize(
            app.core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .lumped_result_for(first)
                .unwrap()
        )
        .unwrap()
    );
    let path = test_tmp().join("ownership611-upstream.ovika");
    app.save_project_to(path.clone());
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    assert_eq!(
        loaded.core.model.lumped_vibration_cases,
        app.core.model.lumped_vibration_cases
    );
    assert!(loaded
        .require_result_input(ResultInputKey::LumpedTimeHistory(first))
        .is_ok());
    assert!(loaded
        .require_result_input(ResultInputKey::LumpedTimeHistory(second))
        .is_ok());
    let _ = std::fs::remove_file(path);
    #[cfg(feature = "gui")]
    {
        let input = app.lumped_time_history_input(vec![0.0, 1.0, -1.0], "同名.csv".into());
        let cfg = app.core.analysis_cfg;
        app.request_analysis(PendingAnalysis::LumpedMassTimeHistory(Box::new(input)));
        assert!(app.core.scoped.pending_unset_analysis.is_some());
        assert!(app.core.scoped.job.is_none());
        app.core.analysis_cfg.push_max_disp = 5.0;
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.core.analysis_cfg = cfg;
        let before = bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap();
        let cases = app.core.model.lumped_vibration_cases.clone();
        let view = app.core.scoped.view_lumped_vibration_case;
        let fresh = (
            app.core.scoped.staleness.results_stale,
            app.core.scoped.staleness.last_run,
        );
        app.resume_pending_analysis();
        assert!(app.core.scoped.job.is_none());
        assert!(app
            .core
            .scoped
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("増分解析 X") || e.contains("生成入力が変更")));
        assert_eq!(
            before,
            bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap()
        );
        assert_eq!(cases, app.core.model.lumped_vibration_cases);
        assert_eq!(view, app.core.scoped.view_lumped_vibration_case);
        assert_eq!(
            fresh,
            (
                app.core.scoped.staleness.results_stale,
                app.core.scoped.staleness.last_run
            )
        );
    }
}

#[cfg(feature = "gui")]
#[test]
fn time_history_ownership_real_request_waits_then_runs_original_input_after_selection_change() {
    let mut app = ownership_unset_portal();
    let input_a = app.time_history_input(ownership_wave(1.0, 0.02), "A.csv".into());
    app.apply_time_history_result(Ok(ownership_result(11.0)), input_a.source);
    let id_a = app.core.model.vibration_cases[0].id;
    let input = app.time_history_input(ownership_wave(0.0, 0.02), "B.csv".into());
    let expected_identity = input.source.identity.clone();
    app.request_analysis(PendingAnalysis::TimeHistory(Box::new(input)));
    assert!(app.core.scoped.pending_unset_analysis.is_some());
    assert!(app.core.scoped.job.is_none());
    app.set_wave_library_selection(Some("A.csv".into()));
    app.resume_pending_analysis();
    assert!(app.core.scoped.pending_unset_analysis.is_none());
    assert!(
        app.core.scoped.job.is_some(),
        "{:?}",
        app.core.scoped.last_error
    );
    wait_for_job(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_eq!(app.core.model.vibration_cases.len(), 2);
    assert_eq!(app.core.model.vibration_cases[1].wave_name, "B.csv");
    assert_eq!(
        app.core.model.vibration_cases[1].input_identity.as_ref(),
        Some(&expected_identity)
    );
    assert_eq!(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .time_history_for(id_a)
            .unwrap()
            .peak_disp[0][0],
        11.0
    );
    let id_b = app.core.model.vibration_cases[1].id;
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history_for(id_b)
        .unwrap()
        .peak_disp
        .iter()
        .flatten()
        .all(|d| *d == 0.0));
    assert_eq!(app.core.scoped.view_vibration_case, Some(id_b));
}

#[cfg(feature = "gui")]
#[test]
fn time_history_ownership_real_request_cancellation_preserves_both_result_kinds() {
    for lumped in [false, true] {
        let mut app = ownership_unset_portal();
        let source = app
            .time_history_input(ownership_wave(1.0, 0.02), "A".into())
            .source;
        app.apply_time_history_result(Ok(ownership_result(11.0)), source);
        let source = app
            .lumped_time_history_input(vec![0.0, 1.0], "A".into())
            .source;
        app.apply_lumped_mass_result(Ok(dummy_lumped(0.21, true)), Some(source));
        let before = bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap();
        let cases = (
            app.core.model.vibration_cases.clone(),
            app.core.model.lumped_vibration_cases.clone(),
        );
        let views = (
            app.core.scoped.view_vibration_case,
            app.core.scoped.view_lumped_vibration_case,
        );
        let fresh = (
            app.core.scoped.staleness.results_stale,
            app.core.scoped.staleness.design_stale,
            app.core.scoped.staleness.last_run,
        );
        let pending = if lumped {
            PendingAnalysis::LumpedMassTimeHistory(Box::new(
                app.lumped_time_history_input(vec![0.0, 2.0], "B".into()),
            ))
        } else {
            PendingAnalysis::TimeHistory(Box::new(
                app.time_history_input(ownership_wave(2.0, 0.02), "B".into()),
            ))
        };
        app.request_analysis(pending);
        assert!(app.core.scoped.pending_unset_analysis.is_some());
        assert!(app.core.scoped.job.is_none());
        app.cancel_pending_analysis();
        assert!(app.core.scoped.pending_unset_analysis.is_none());
        assert!(app.core.scoped.job.is_none());
        assert!(app.core.scoped.unset_regions_ack.is_none());
        assert_eq!(
            before,
            bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap()
        );
        assert_eq!(
            cases,
            (
                app.core.model.vibration_cases.clone(),
                app.core.model.lumped_vibration_cases.clone()
            )
        );
        assert_eq!(
            views,
            (
                app.core.scoped.view_vibration_case,
                app.core.scoped.view_lumped_vibration_case
            )
        );
        assert_eq!(
            fresh,
            (
                app.core.scoped.staleness.results_stale,
                app.core.scoped.staleness.design_stale,
                app.core.scoped.staleness.last_run
            )
        );
    }
}

#[cfg(feature = "gui")]
#[test]
fn time_history_ownership_existing_file_handlers_keep_read_payload_name_and_original_case() {
    use sepika_solver::dynamic::lumped_mass::{LumpedStiffnessSource, StickDim};
    for entry in 0..3 {
        let dir = test_tmp();
        let path = dir.join("B.csv");
        std::fs::write(&path, "0\n1\n-2\n").unwrap();
        let mut app = ownership_unset_portal();
        let lumped = entry == 2;
        app.core.analysis_cfg.th_dir = ThDir::Y;
        app.core.analysis_cfg.lumped_dim = StickDim::Planar;
        app.core.analysis_cfg.lumped_stiffness = LumpedStiffnessSource::StoryQd;
        app.core.analysis_cfg.lumped_dir = SeismicDir::X;
        app.core.analysis_cfg.lumped_nonlinear = false;
        if lumped {
            app.run_seismic(SeismicDir::X);
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
        }
        let a = if lumped {
            let source = app
                .lumped_time_history_input(vec![0.0, 77.0], "A.csv".into())
                .source;
            app.apply_lumped_mass_result(Ok(dummy_lumped(0.77, true)), Some(source));
            app.core.model.lumped_vibration_cases[0].id.0
        } else {
            let source = app
                .time_history_input(ownership_wave(77.0, 0.02), "A.csv".into())
                .source;
            app.apply_time_history_result(Ok(ownership_result(77.0)), source);
            app.core.model.vibration_cases[0].id.0
        };
        app.set_wave_library_selection(Some(if entry == 0 { "A.csv" } else { "B.csv" }.into()));
        app.set_lumped_wave_library_selection(Some("B.csv".into()));
        match entry {
            0 => app.run_time_history_from_csv_path(&path),
            1 => app.run_time_history_from_library_in(&dir),
            _ => app.start_lumped_mass_library_th_job_in(&dir),
        }
        assert!(
            app.core.scoped.last_error.is_none(),
            "entry={entry}: {:?}",
            app.core.scoped.last_error
        );
        assert!(app.core.scoped.job.is_none());
        let identity = match app.core.scoped.pending_unset_analysis.as_ref().unwrap() {
            PendingAnalysis::TimeHistory(input) => {
                assert_eq!(input.source.wave_name, "B.csv");
                assert_eq!(input.data.dt, 0.02);
                assert_eq!(input.data.accel_x, vec![0.0; 3]);
                assert_eq!(
                    input.data.accel_y.as_deref(),
                    Some([0.0, 10.0, -20.0].as_slice())
                );
                assert_eq!(input.source.cfg.th_dir, ThDir::Y);
                input.source.identity.clone()
            }
            PendingAnalysis::LumpedMassTimeHistory(input) => {
                assert_eq!(input.source.wave_name, "B.csv");
                assert_eq!(input.data, vec![0.0, 10.0, -20.0]);
                assert_eq!(input.source.cfg.lumped_th_dt, 0.02);
                assert_eq!(input.source.cfg.lumped_dir, SeismicDir::X);
                input.source.identity.clone()
            }
            _ => panic!("時刻歴以外の確認待ち"),
        };
        app.set_wave_library_selection(Some("別選択.csv".into()));
        app.set_lumped_wave_library_selection(Some("別選択.csv".into()));
        std::fs::write(&path, "99\n98\n97\n").unwrap();
        app.resume_pending_analysis();
        assert!(
            app.core.scoped.job.is_some(),
            "entry={entry}: {:?}",
            app.core.scoped.last_error
        );
        wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "entry={entry}: {:?}",
            app.core.scoped.last_error
        );
        if lumped {
            assert_eq!(app.core.model.lumped_vibration_cases.len(), 2);
            let b = &app.core.model.lumped_vibration_cases[1];
            assert_eq!(b.wave_name, "B.csv");
            assert_eq!(b.input_identity.as_ref(), Some(&identity));
            assert_eq!(app.core.scoped.view_lumped_vibration_case, Some(b.id));
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .lumped_result_for(sepika_core::ids::LumpedVibrationCaseId(a))
                    .unwrap()
                    .modal
                    .period,
                vec![0.77]
            );
        } else {
            assert_eq!(app.core.model.vibration_cases.len(), 2);
            let b = &app.core.model.vibration_cases[1];
            assert_eq!(b.wave_name, "B.csv");
            assert_eq!(b.input_identity.as_ref(), Some(&identity));
            assert_eq!(app.core.scoped.view_vibration_case, Some(b.id));
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .time_history_for(VibrationCaseId(a))
                    .unwrap()
                    .peak_disp[0][0],
                77.0
            );
        }
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn time_history_ownership_fixed_results_follow_input_and_keep_one_slot() {
    let mut app = App::default();
    let input_a = app.time_history_input(ownership_wave(1.0, 0.02), "A.csv".into());
    app.apply_time_history_result(Ok(ownership_result(11.0)), input_a.source);
    let id_a = app.core.model.vibration_cases[0].id;
    app.core.scoped.wave_library_selection = Some("A.csv".into());
    let input_b = app.time_history_input(ownership_wave(2.0, 0.02), "B.csv".into());
    ownership_job(
        &mut app,
        Some(JobResult::TimeHistory(
            Box::new(Ok(ownership_result(22.0))),
            Box::new(input_b.source),
        )),
    );
    app.core.scoped.wave_library_selection = Some("C.csv".into());
    assert!(app.poll_job());
    assert!(app.core.scoped.last_error.is_none());
    let id_b = app.core.model.vibration_cases[1].id;
    assert_eq!(app.core.model.vibration_cases[1].wave_name, "B.csv");
    let bundle = app.core.scoped.results.as_ref().unwrap();
    assert_eq!(bundle.time_history_for(id_a).unwrap().peak_disp[0][0], 11.0);
    assert_eq!(bundle.time_history_for(id_b).unwrap().peak_disp[0][0], 22.0);
    let rerun = app.time_history_input(ownership_wave(1.0, 0.02), "A.csv".into());
    assert_eq!(rerun.source.target, Some(id_a.0));
    ownership_job(
        &mut app,
        Some(JobResult::TimeHistory(
            Box::new(Ok(ownership_result(33.0))),
            Box::new(rerun.source),
        )),
    );
    app.core.scoped.wave_library_selection = Some("B.csv".into());
    assert!(app.poll_job());
    assert_eq!(app.core.scoped.view_vibration_case, Some(id_a));
    let bundle = app.core.scoped.results.as_ref().unwrap();
    assert_eq!(bundle.time_histories.len(), 2);
    assert_eq!(bundle.time_history_for(id_a).unwrap().peak_disp[0][0], 33.0);
    assert_eq!(bundle.time_history_for(id_b).unwrap().peak_disp[0][0], 22.0);
}

#[test]
fn time_history_ownership_same_name_distinguishes_samples_dt_and_scale() {
    let mut app = App::default();
    for (value, dt) in [(1.0, 0.02), (2.0, 0.02), (1.0, 0.04), (10.0, 0.02)] {
        let input = app.time_history_input(ownership_wave(value, dt), "同名.csv".into());
        assert!(input.source.target.is_none());
        app.apply_time_history_result(Ok(ownership_result(value)), input.source);
    }
    assert_eq!(app.core.model.vibration_cases.len(), 4);
    let first = app.core.model.vibration_cases[0].id;
    let input = app.time_history_input(ownership_wave(1.0, 0.02), "同名.csv".into());
    assert_eq!(input.source.target, Some(first.0));
    app.apply_time_history_result(Ok(ownership_result(99.0)), input.source);
    assert_eq!(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .time_histories
            .len(),
        4
    );
    app.core.analysis_cfg.rc_damage_control = !app.core.analysis_cfg.rc_damage_control;
    let input = app.time_history_input(ownership_wave(1.0, 0.02), "同名.csv".into());
    assert_eq!(input.source.target, Some(first.0));
}

#[test]
fn lumped_time_history_ownership_tracks_converted_acceleration_and_settings() {
    let mut app = App::default();
    app.core.analysis_cfg.lumped_dir = SeismicDir::Y;
    let wave = sepika_job::build_ground_motion(0.02, ThDir::Y, vec![10.0, -20.0, 30.0]);
    let accel = sepika_job::lumped_accel_from_wave(&wave, SeismicDir::Y).unwrap();
    assert_eq!(accel, vec![10.0, -20.0, 30.0]);
    app.core.analysis_cfg.lumped_th_dt = wave.dt;
    let input = app.lumped_time_history_input(accel.clone(), "B.csv".into());
    let identity = input.source.identity.clone();
    assert_eq!(input.data, vec![10.0, -20.0, 30.0]);
    app.core.scoped.lumped_wave_library_selection = Some("A.csv".into());
    app.apply_lumped_mass_result(Ok(dummy_lumped(0.21, true)), Some(input.source));
    let id = app.core.model.lumped_vibration_cases[0].id;
    assert_eq!(app.core.model.lumped_vibration_cases[0].wave_name, "B.csv");
    let rerun = app.lumped_time_history_input(accel.clone(), "B.csv".into());
    assert_eq!(rerun.source.target, Some(id.0));
    ownership_job(
        &mut app,
        Some(JobResult::LumpedMass(
            Box::new(Ok(dummy_lumped(0.31, true))),
            Some(Box::new(rerun.source)),
        )),
    );
    app.core.scoped.lumped_wave_library_selection = Some("C.csv".into());
    assert!(app.poll_job());
    assert_eq!(app.core.model.lumped_vibration_cases.len(), 1);
    assert_eq!(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped_result_for(id)
            .unwrap()
            .modal
            .period,
        vec![0.31]
    );
    for variant in 0..3 {
        let mut next = accel.clone();
        if variant == 0 {
            next[1] = -21.0;
        }
        if variant == 1 {
            app.core.analysis_cfg.lumped_th_dt = 0.04;
        }
        if variant == 2 {
            app.core.analysis_cfg.lumped_th_dt = 0.02;
            next.iter_mut().for_each(|v| *v *= 2.0);
        }
        let input = app.lumped_time_history_input(next, "B.csv".into());
        assert_ne!(input.source.identity, identity);
        assert!(input.source.target.is_none());
        app.apply_lumped_mass_result(
            Ok(dummy_lumped(0.41 + variant as f64, true)),
            Some(input.source),
        );
    }
    assert_eq!(app.core.model.lumped_vibration_cases.len(), 4);
}

#[test]
fn time_history_ownership_rejection_failure_and_disconnection_preserve_results() {
    for lumped in [false, true] {
        for failure in 0..5 {
            let mut app = App::default();
            let source = if lumped {
                app.lumped_time_history_input(vec![0.0, 1.0], "A".into())
                    .source
            } else {
                app.time_history_input(ownership_wave(1.0, 0.02), "A".into())
                    .source
            };
            if lumped {
                app.apply_lumped_mass_result(Ok(dummy_lumped(0.21, true)), Some(source));
            } else {
                app.apply_time_history_result(Ok(ownership_result(11.0)), source);
            }
            app.core
                .scoped
                .results
                .as_mut()
                .unwrap()
                .record_input(ResultInputKey::Design, vec![9, 8, 7]);
            let source = if lumped {
                app.lumped_time_history_input(vec![0.0, 1.0], "A".into())
                    .source
            } else {
                app.time_history_input(ownership_wave(1.0, 0.02), "A".into())
                    .source
            };
            let result = if failure == 4 {
                None
            } else if lumped {
                Some(JobResult::LumpedMass(
                    Box::new(if failure == 3 {
                        Err("失敗fixture".into())
                    } else {
                        Ok(dummy_lumped(0.91, true))
                    }),
                    Some(Box::new(source)),
                ))
            } else {
                Some(JobResult::TimeHistory(
                    Box::new(if failure == 3 {
                        Err("失敗fixture".into())
                    } else {
                        Ok(ownership_result(99.0))
                    }),
                    Box::new(source),
                ))
            };
            ownership_job(&mut app, result);
            #[cfg(feature = "gui")]
            {
                app.ui.view.active_tab = Tab::Model;
                app.ui.view.results_view = ResultsView::Spatial;
                app.core.scoped.job.as_mut().unwrap().jump_on_success =
                    Some((Tab::Results, ResultsView::TimeHistory));
            }
            match failure {
                0 => {
                    app.core.model.slab_thickness += 1.0;
                }
                1 => {
                    if lumped {
                        app.core.analysis_cfg.lumped_th_damping = 0.12;
                    } else {
                        app.core.analysis_cfg.th_damping = 0.12;
                    }
                }
                2 => {
                    if lumped {
                        app.core.model.lumped_vibration_cases.clear();
                    } else {
                        app.core.model.vibration_cases.clear();
                    }
                }
                _ => {}
            }
            let before = bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap();
            let cases = (
                app.core.model.vibration_cases.clone(),
                app.core.model.lumped_vibration_cases.clone(),
            );
            let views = (
                app.core.scoped.view_vibration_case,
                app.core.scoped.view_lumped_vibration_case,
            );
            let fresh = (
                app.core.scoped.staleness.results_stale,
                app.core.scoped.staleness.design_stale,
                app.core.scoped.staleness.last_run,
            );
            assert!(app.poll_job());
            #[cfg(feature = "gui")]
            {
                assert_eq!(app.ui.view.active_tab, Tab::Model);
                assert_eq!(app.ui.view.results_view, ResultsView::Spatial);
            }
            assert!(app.core.scoped.last_error.is_some(), "{lumped}/{failure}");
            assert_eq!(
                before,
                bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap()
            );
            assert_eq!(
                cases,
                (
                    app.core.model.vibration_cases.clone(),
                    app.core.model.lumped_vibration_cases.clone()
                )
            );
            assert_eq!(
                views,
                (
                    app.core.scoped.view_vibration_case,
                    app.core.scoped.view_lumped_vibration_case
                )
            );
            assert_eq!(
                fresh,
                (
                    app.core.scoped.staleness.results_stale,
                    app.core.scoped.staleness.design_stale,
                    app.core.scoped.staleness.last_run
                )
            );
        }
    }
}

#[test]
fn time_history_ownership_save_reload_and_invalid_identity_are_not_recertified() {
    let path = test_tmp().join("ownership611.ovika");
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.generate_stories_action();
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.require_design_input().is_ok());
    let static_key = ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0)));
    assert!(app.require_result_input(static_key.clone()).is_ok());
    let original_inputs: Vec<_> = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .input_records
        .iter()
        .filter(|(key, _)| *key == static_key || *key == ResultInputKey::Design)
        .cloned()
        .collect();
    assert_eq!(original_inputs.len(), 2);
    let original_static =
        bincode::serialize(&app.core.scoped.results.as_ref().unwrap().statics).unwrap();
    let original_design =
        bincode::serialize(&app.core.scoped.results.as_ref().unwrap().member_checks).unwrap();
    let input = app.time_history_input(ownership_wave(1.0, 0.02), "A.csv".into());
    app.apply_time_history_result(Ok(ownership_result(11.0)), input.source);
    let lumped = app.lumped_time_history_input(vec![0.0, 2.0], "B.csv".into());
    app.apply_lumped_mass_result(Ok(dummy_lumped(0.21, true)), Some(lumped.source));
    for (value, dt) in [(1.0, 0.04), (10.0, 0.02)] {
        app.core.analysis_cfg.th_dt = dt;
        app.core.scoped.staleness.mark_edited();
        let input = app.time_history_input(ownership_wave(value, dt), "A.csv".into());
        app.apply_time_history_result(Ok(ownership_result(value * 10.0)), input.source);
        app.core.analysis_cfg.lumped_th_dt = dt;
        app.core.scoped.staleness.mark_edited();
        let input = app.lumped_time_history_input(vec![0.0, value], "B.csv".into());
        app.apply_lumped_mass_result(Ok(dummy_lumped(value, true)), Some(input.source));
    }
    let spatial = app.core.model.vibration_cases.clone();
    let lumped = app.core.model.lumped_vibration_cases.clone();
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    assert_eq!(spatial, loaded.core.model.vibration_cases);
    assert_eq!(lumped, loaded.core.model.lumped_vibration_cases);
    assert!(loaded.require_result_input(static_key.clone()).is_ok());
    assert!(loaded.require_design_input().is_ok());
    let loaded_bundle = loaded.core.scoped.results.as_ref().unwrap();
    for input in &original_inputs {
        assert!(loaded_bundle.input_records.contains(input));
    }
    assert_eq!(
        original_static,
        bincode::serialize(&loaded_bundle.statics).unwrap()
    );
    assert_eq!(
        original_design,
        bincode::serialize(&loaded_bundle.member_checks).unwrap()
    );
    assert_eq!(
        loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .time_histories
            .len(),
        3
    );
    assert_eq!(
        loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped_results
            .len(),
        3
    );
    assert_eq!(
        loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .time_history_for(spatial[0].id)
            .unwrap()
            .peak_disp[0][0],
        11.0
    );
    assert_eq!(
        loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped_result_for(lumped[0].id)
            .unwrap()
            .modal
            .period,
        vec![0.21]
    );
    for mismatch in [false, true] {
        let mut broken = App::default();
        broken.open_project_from(path.clone());
        broken.core.model.vibration_cases[0].input_identity = mismatch.then(|| "別入力".into());
        broken.core.model.lumped_vibration_cases[0].input_identity =
            mismatch.then(|| "別入力".into());
        assert!(broken
            .require_result_input(ResultInputKey::TimeHistory(spatial[0].id))
            .is_err());
        assert!(broken
            .require_result_input(ResultInputKey::LumpedTimeHistory(lumped[0].id))
            .is_err());
        let bad_path = test_tmp().join(format!("ownership611-invalid-{mismatch}.ovika"));
        broken.save_project_to(bad_path.clone());
        let mut reloaded = App::default();
        reloaded.open_project_from(bad_path.clone());
        assert_eq!(reloaded.core.model.vibration_cases.len(), 2);
        assert_eq!(reloaded.core.model.lumped_vibration_cases.len(), 2);
        assert!(reloaded
            .require_result_input(ResultInputKey::TimeHistory(spatial[0].id))
            .is_err());
        assert!(reloaded
            .require_result_input(ResultInputKey::LumpedTimeHistory(lumped[0].id))
            .is_err());
        let _ = std::fs::remove_file(bad_path);
    }
    for version in [
        None,
        Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION - 1),
    ] {
        let mut old = App::default();
        old.open_project_from(path.clone());
        old.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .analysis_contract_version = version;
        assert!(old
            .require_result_input(ResultInputKey::TimeHistory(spatial[0].id))
            .is_err());
        assert!(old
            .require_result_input(ResultInputKey::LumpedTimeHistory(lumped[0].id))
            .is_err());
        let old_path = test_tmp().join(format!("ownership611-old-{version:?}.ovika"));
        old.save_project_to(old_path.clone());
        let mut reloaded = App::default();
        reloaded.open_project_from(old_path.clone());
        reloaded.set_wave_library_selection(Some("表示だけ変更.csv".into()));
        assert!(reloaded.core.model.vibration_cases.is_empty());
        assert!(reloaded.core.model.lumped_vibration_cases.is_empty());
        assert!(reloaded
            .require_result_input(ResultInputKey::TimeHistory(spatial[0].id))
            .is_err());
        assert!(reloaded
            .require_result_input(ResultInputKey::LumpedTimeHistory(lumped[0].id))
            .is_err());
        assert!(reloaded
            .core
            .scoped
            .results
            .as_ref()
            .is_none_or(
                |bundle| bundle.time_histories.is_empty() && bundle.lumped_results.is_empty()
            ));
        let _ = std::fs::remove_file(old_path);
    }
    let mut model_changed = App::default();
    model_changed.open_project_from(path.clone());
    model_changed.core.model.materials[0].young *= 0.9;
    model_changed.core.scoped.staleness.mark_edited();
    assert!(model_changed
        .require_result_input(ResultInputKey::TimeHistory(spatial[0].id))
        .is_err());
    let changed_path = test_tmp().join("ownership611-model-changed.ovika");
    model_changed.save_project_to(changed_path.clone());
    let mut reloaded = App::default();
    reloaded.open_project_from(changed_path.clone());
    assert!(reloaded.core.model.vibration_cases.is_empty());
    assert!(reloaded.core.model.lumped_vibration_cases.is_empty());
    let _ = std::fs::remove_file(changed_path);
    let _ = std::fs::remove_file(path);
}

#[cfg(feature = "gui")]
#[test]
fn time_history_ownership_pending_freezes_input_and_rejects_setting_changes_or_cancel() {
    for lumped in [false, true] {
        let mut app = ownership_unset_portal();
        let input = app.time_history_input(ownership_wave(1.0, 0.02), "B.csv".into());
        let pending = if lumped {
            PendingAnalysis::LumpedMassTimeHistory(Box::new(
                app.lumped_time_history_input(vec![0.0, 1.0], "B.csv".into()),
            ))
        } else {
            PendingAnalysis::TimeHistory(Box::new(input))
        };
        app.request_analysis(pending);
        assert!(app.core.scoped.pending_unset_analysis.is_some());
        app.core.scoped.wave_library_selection = Some("A.csv".into());
        if lumped {
            app.core.analysis_cfg.lumped_th_dt = 0.04;
        } else {
            app.core.analysis_cfg.th_dt = 0.04;
        }
        app.resume_pending_analysis();
        assert!(app.core.scoped.job.is_none());
        assert!(app
            .core
            .scoped
            .last_error
            .as_deref()
            .unwrap()
            .contains("波形生成後"));
        assert!(app.core.model.vibration_cases.is_empty());
        assert!(app.core.model.lumped_vibration_cases.is_empty());
        let input = app.time_history_input(ownership_wave(1.0, 0.02), "B.csv".into());
        app.core.scoped.pending_unset_analysis =
            Some(PendingAnalysis::TimeHistory(Box::new(input)));
        app.cancel_pending_analysis();
        assert!(app.core.scoped.job.is_none());
        assert!(app.core.scoped.pending_unset_analysis.is_none());
        assert!(app.core.model.vibration_cases.is_empty());
    }
}

#[test]
fn time_history_ownership_sample_entry_ignores_selected_library_name() {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.th_duration = 0.04;
    app.generate_stories_action();
    let input = app.time_history_input(ownership_wave(1.0, 0.02), "A.csv".into());
    app.apply_time_history_result(Ok(ownership_result(11.0)), input.source);
    let id = app.core.model.vibration_cases[0].id;
    app.core.scoped.wave_library_selection = Some("A.csv".into());
    app.run_time_history_sample();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_eq!(app.core.model.vibration_cases.len(), 2);
    assert_eq!(app.core.model.vibration_cases[1].wave_name, "サンプル");
    assert_eq!(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .time_history_for(id)
            .unwrap()
            .peak_disp[0][0],
        11.0
    );
}

#[test]
fn time_history_ownership_asynchronous_sample_entries_preserve_selected_existing_case() {
    use sepika_solver::dynamic::lumped_mass::{LumpedStiffnessSource, StickDim};
    for lumped in [false, true] {
        let mut app = ownership_portal();
        app.core.analysis_cfg.th_dt = 0.02;
        app.core.analysis_cfg.th_duration = 0.05;
        app.core.analysis_cfg.lumped_th_dt = 0.02;
        app.core.analysis_cfg.lumped_th_duration = 0.05;
        app.core.analysis_cfg.ai_mode = AiMode::Approx;
        app.core.analysis_cfg.lumped_dim = StickDim::Planar;
        app.core.analysis_cfg.lumped_stiffness = LumpedStiffnessSource::StoryQd;
        app.core.analysis_cfg.lumped_dir = SeismicDir::X;
        app.core.analysis_cfg.lumped_nonlinear = false;
        app.run_seismic(SeismicDir::X);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        if lumped {
            let input = app.lumped_time_history_input(vec![0.0, 77.0], "A.csv".into());
            app.apply_lumped_mass_result(Ok(dummy_lumped(0.77, true)), Some(input.source));
            let a = app.core.model.lumped_vibration_cases[0].clone();
            app.core.scoped.lumped_wave_library_selection = Some("A.csv".into());
            app.start_lumped_mass_sample_th_job();
            #[cfg(feature = "gui")]
            if app.core.scoped.pending_unset_analysis.is_some() {
                app.resume_pending_analysis();
            }
            assert!(
                app.core.scoped.job.is_some(),
                "{:?}",
                app.core.scoped.last_error
            );
            app.core.scoped.lumped_wave_library_selection = Some("C.csv".into());
            wait_for_job(&mut app);
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            assert_eq!(app.core.model.lumped_vibration_cases.len(), 2);
            assert_eq!(app.core.model.lumped_vibration_cases[0], a);
            let current_id = app.core.model.lumped_vibration_cases[1].id;
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .lumped_result_for(current_id)
                    .unwrap()
                    .response
                    .as_ref()
                    .unwrap()
                    .time,
                vec![0.0, 0.02, 0.04, 0.06]
            );
            assert_eq!(
                app.core.model.lumped_vibration_cases[1].wave_name,
                "サンプル"
            );
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .lumped_result_for(a.id)
                    .unwrap()
                    .modal
                    .period,
                vec![0.77]
            );
        } else {
            let input = app.time_history_input(ownership_wave(77.0, 0.02), "A.csv".into());
            app.apply_time_history_result(Ok(ownership_result(77.0)), input.source);
            let a = app.core.model.vibration_cases[0].clone();
            app.core.scoped.wave_library_selection = Some("A.csv".into());
            let input = app.time_history_input(
                App::sample_wave(&app.core.analysis_cfg).unwrap(),
                "サンプル".into(),
            );
            #[cfg(feature = "gui")]
            {
                app.request_analysis(PendingAnalysis::TimeHistory(Box::new(input)));
                if app.core.scoped.pending_unset_analysis.is_some() {
                    app.resume_pending_analysis();
                }
            }
            #[cfg(not(feature = "gui"))]
            app.start_time_history_input_job(input);
            assert!(
                app.core.scoped.job.is_some(),
                "{:?}",
                app.core.scoped.last_error
            );
            app.core.scoped.wave_library_selection = Some("C.csv".into());
            wait_for_job(&mut app);
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            assert_eq!(app.core.model.vibration_cases.len(), 2);
            assert_eq!(app.core.model.vibration_cases[0], a);
            let current_id = app.core.model.vibration_cases[1].id;
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .time_history_for(current_id)
                    .unwrap()
                    .time,
                vec![0.0, 0.02, 0.04, 0.06]
            );
            assert_eq!(app.core.model.vibration_cases[1].wave_name, "サンプル");
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .time_history_for(a.id)
                    .unwrap()
                    .peak_disp[0][0],
                77.0
            );
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn time_history_ownership_csv_parser_keeps_effective_arrays_and_source_name() {
    let mut app = App::default();
    app.core.analysis_cfg.th_dt = 0.02;
    app.core.analysis_cfg.th_dir = ThDir::Y;
    app.core.scoped.wave_library_selection = Some("A.csv".into());
    let cfg = app.core.analysis_cfg;
    let wave = app.ground_motion_or_report(&cfg, "1\n-2\n3\n").unwrap();
    assert_eq!(wave.dt, 0.02);
    assert_eq!(wave.accel_x, vec![0.0; 3]);
    assert_eq!(
        wave.accel_y.as_deref(),
        Some([10.0, -20.0, 30.0].as_slice())
    );
    let input = app.time_history_input(wave, "B.csv".into());
    app.apply_time_history_result(Ok(ownership_result(22.0)), input.source);
    assert_eq!(app.core.model.vibration_cases[0].wave_name, "B.csv");
    app.core.analysis_cfg.lumped_dir = SeismicDir::Y;
    app.core.analysis_cfg.lumped_th_dt = 0.02;
    let wave = app.ground_motion_or_report(&cfg, "1\n-2\n3\n").unwrap();
    let accel = sepika_job::lumped_accel_from_wave(&wave, SeismicDir::Y).unwrap();
    assert_eq!(accel, vec![10.0, -20.0, 30.0]);
    let input = app.lumped_time_history_input(accel, "B.csv".into());
    assert_eq!(input.data, vec![10.0, -20.0, 30.0]);
    app.apply_lumped_mass_result(Ok(dummy_lumped(0.21, true)), Some(input.source));
    assert_eq!(app.core.model.lumped_vibration_cases[0].wave_name, "B.csv");
}

#[test]
fn lumped_model_type_separates_cases_and_survives_project_reload() {
    use sepika_solver::dynamic::lumped_mass::{LumpedMassType, StoryBending};
    let mut app = ownership_portal();
    let wave = vec![0.0, 1.0, -1.0];
    let shear = app.lumped_time_history_input(wave.clone(), "同じ波形".into());
    let shear_identity = shear.source.identity.clone();
    app.apply_lumped_mass_result(Ok(dummy_lumped(0.21, true)), Some(shear.source));
    let shear_id = app.core.model.lumped_vibration_cases[0].id;
    app.core.analysis_cfg.lumped_mass_type = LumpedMassType::EquivalentBendingShear;
    let bending = app.lumped_time_history_input(wave.clone(), "同じ波形".into());
    assert_ne!(shear_identity, bending.source.identity);
    assert!(bending.source.target.is_none());
    let mut result = dummy_lumped(0.31, true);
    result.model.model_type = LumpedMassType::EquivalentBendingShear;
    result.model.bending = vec![StoryBending {
        ei_x: 1e14,
        ei_y: 2e14,
        inertia: [1e6, 2e6, 3e5],
    }];
    result.modal.bending_rotations = vec![vec![[0.001, 0.002]]];
    result.response.as_mut().unwrap().bending_rotations = vec![vec![[0.003, 0.004]]];
    let serialized = bincode::serialize(&result).unwrap();
    app.apply_lumped_mass_result(Ok(result), Some(bending.source));
    assert_eq!(app.core.model.lumped_vibration_cases.len(), 2);
    let bending_id = app.core.model.lumped_vibration_cases[1].id;
    assert_ne!(shear_id, bending_id);
    app.core.analysis_cfg.lumped_mass_type = LumpedMassType::EquivalentShear;
    assert_eq!(
        app.lumped_time_history_input(wave.clone(), "表示名変更".into())
            .source
            .target,
        Some(shear_id.0)
    );
    app.core.analysis_cfg.lumped_mass_type = LumpedMassType::EquivalentBendingShear;
    assert_eq!(
        app.lumped_time_history_input(wave, "表示名変更".into())
            .source
            .target,
        Some(bending_id.0)
    );
    let path = test_tmp().join("issue834-bending-ownership.ovika");
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    assert_eq!(
        loaded.core.analysis_cfg.lumped_mass_type,
        LumpedMassType::EquivalentBendingShear
    );
    let bundle = loaded.core.scoped.results.as_ref().unwrap();
    assert_eq!(
        bundle.lumped_result_for(shear_id).unwrap().model.model_type,
        LumpedMassType::EquivalentShear
    );
    assert_eq!(
        serialized,
        bincode::serialize(bundle.lumped_result_for(bending_id).unwrap()).unwrap()
    );
    std::fs::remove_file(path).unwrap();
}
