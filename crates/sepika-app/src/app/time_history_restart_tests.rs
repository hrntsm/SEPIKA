use super::tests::{test_tmp, wait_for_job, with_explicit_pushover_gravity};
use super::*;
use sepika_job::time_history_restart::{
    compute_checkpoint, resume_checkpoint, LinearTimeHistoryCheckpoint,
};
use sepika_solver::dynamic::timehistory::GroundMotion;

fn fixture() -> App {
    let mut app = App::default();
    app.load_model(with_explicit_pushover_gravity(crate::sample::portal_frame()));
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.th_record_every = 3;
    app.generate_stories_action();
    app.ensure_preparation();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}

fn wave() -> GroundMotion {
    GroundMotion {
        dt: 0.02,
        accel_x: vec![0.0, 1000.0, -500.0, 100.0, 0.0, 0.0],
        accel_y: Some(vec![0.0, 0.0, 0.0, 0.0, 2000.0, -2000.0]),
        accel_theta: None,
    }
}

#[test]
fn 途中保存を別appで読み込みジョブ再開すると連続実行の全記録と全ピークになる() {
    let mut app = fixture();
    let dir = test_tmp();
    let project = dir.join("restart.ovika");
    let checkpoint = dir.join("restart.sepika-th");
    app.run_time_history(wave());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let continuous = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history
        .as_ref()
        .unwrap()
        .clone();
    assert!(continuous.history.record_dir_y);
    app.core.scoped.checkpoint_stop_step = 2;
    app.start_time_history_job(wave());
    wait_for_job(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_eq!(
        app.core
            .scoped
            .linear_checkpoint
            .as_ref()
            .unwrap()
            .progress(),
        (2, 0.04)
    );
    app.save_linear_checkpoint(&checkpoint);
    app.core.model.upsert_vibration_case(
        "保存後に追加した表示ケース".into(),
        sepika_core::model::VibrationThDir::X,
        false,
        None,
    );
    app.save_project_to(project.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut restored = App::default();
    restored.open_project_from(project);
    restored.load_linear_checkpoint(&checkpoint);
    assert!(
        restored.core.scoped.last_error.is_none(),
        "{:?}",
        restored.core.scoped.last_error
    );
    restored.start_resume_linear_checkpoint();
    wait_for_job(&mut restored);
    assert!(
        restored.core.scoped.last_error.is_none(),
        "{:?}",
        restored.core.scoped.last_error
    );
    let resumed = restored
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history
        .as_ref()
        .unwrap();
    assert_eq!(resumed.time, vec![0.0, 0.02, 0.04, 0.06, 0.08, 0.1]);
    assert_eq!(
        resumed.recording.as_ref().unwrap().frame_time,
        vec![0.0, 0.06, 0.1]
    );
    assert_eq!(
        bincode::serialize(resumed).unwrap(),
        bincode::serialize(&continuous).unwrap()
    );
}

#[test]
fn 再開はモデル波形刻み設定契約自由度と破損の不一致を拒否する() {
    let app = fixture();
    let model = app.core.model;
    let cfg = app.core.analysis_cfg;
    let checkpoint = compute_checkpoint(model.clone(), cfg, wave(), 2).unwrap();
    let bytes = checkpoint.to_bytes().unwrap();
    let restored = LinearTimeHistoryCheckpoint::from_bytes(&bytes).unwrap();
    for (field, value, expected) in [
        ("contract", serde_json::json!(0), "計算契約"),
        (
            "state",
            serde_json::json!({"step":5,"time":0.1,"disp_red":[],"vel_red":[],"accel_red":[]}),
            "最終ステップ",
        ),
        (
            "state",
            serde_json::json!({"step":6,"time":0.12,"disp_red":[],"vel_red":[],"accel_red":[]}),
            "範囲外",
        ),
    ] {
        let mut saved = serde_json::to_value(&checkpoint).unwrap();
        saved[field] = value;
        let invalid: LinearTimeHistoryCheckpoint = serde_json::from_value(saved).unwrap();
        let error = resume_checkpoint(model.clone(), cfg, wave(), invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
    for field in ["time", "disp_red", "vel_red", "accel_red"] {
        let mut saved = serde_json::to_value(&checkpoint).unwrap();
        saved["state"][field] = if field == "time" {
            serde_json::json!(0.03)
        } else {
            serde_json::json!([])
        };
        let invalid: LinearTimeHistoryCheckpoint = serde_json::from_value(saved).unwrap();
        assert!(resume_checkpoint(model.clone(), cfg, wave(), invalid).is_err());
    }
    let mut saved = serde_json::to_value(&checkpoint).unwrap();
    saved["mapping"][0] = serde_json::json!(100000);
    let invalid: LinearTimeHistoryCheckpoint = serde_json::from_value(saved).unwrap();
    assert!(resume_checkpoint(model.clone(), cfg, wave(), invalid)
        .unwrap_err()
        .to_string()
        .contains("自由度"));
    let mut changed_model = model.clone();
    changed_model.materials[0].young *= 1.1;
    assert!(resume_checkpoint(changed_model, cfg, wave(), restored.clone()).is_err());
    let mut changed = wave();
    changed.accel_y.as_mut().unwrap()[4] *= 1.1;
    assert!(
        resume_checkpoint(model.clone(), cfg, changed, restored.clone())
            .unwrap_err()
            .to_string()
            .contains("波形")
    );
    let mut changed = wave();
    changed.dt *= 2.0;
    assert!(resume_checkpoint(model.clone(), cfg, changed, restored.clone()).is_err());
    for changed in [
        AnalysisSettings {
            th_damping: 0.03,
            ..cfg
        },
        AnalysisSettings {
            th_dt: cfg.th_dt * 2.0,
            ..cfg
        },
        AnalysisSettings {
            th_record_every: 2,
            ..cfg
        },
        AnalysisSettings {
            th_nonlinear: true,
            ..cfg
        },
    ] {
        assert!(resume_checkpoint(model.clone(), changed, wave(), restored.clone()).is_err());
    }
    for step in [0, 5, 6] {
        assert!(compute_checkpoint(model.clone(), cfg, wave(), step).is_err());
    }
    let mut corrupt = bytes;
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(LinearTimeHistoryCheckpoint::from_bytes(&corrupt).is_err());
}

#[test]
fn 間引き格子上の境界と自動間引きも連続記録に一致する() {
    let app = fixture();
    for (every, step, samples) in [(3, 3, 6), (0, 1201, 2205)] {
        let cfg = AnalysisSettings {
            th_record_every: every,
            ..app.core.analysis_cfg
        };
        let mut wave = wave();
        wave.accel_x.resize(samples, 0.0);
        wave.accel_y.as_mut().unwrap().resize(samples, 0.0);
        let continuous =
            sepika_job::compute::compute_time_history(app.core.model.clone(), cfg, wave.clone())
                .unwrap();
        let checkpoint =
            compute_checkpoint(app.core.model.clone(), cfg, wave.clone(), step).unwrap();
        let checkpoint =
            LinearTimeHistoryCheckpoint::from_bytes(&checkpoint.to_bytes().unwrap()).unwrap();
        let resumed = resume_checkpoint(app.core.model.clone(), cfg, wave, checkpoint).unwrap();
        assert_eq!(
            bincode::serialize(&resumed).unwrap(),
            bincode::serialize(&continuous).unwrap()
        );
    }
}

#[cfg(feature = "gui")]
#[test]
fn 本番時刻歴画面の再開ボタンをポインタイベントで押すと全区間結果を反映する() {
    fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 2400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.time_history_panel(ui),
        )
    }
    let ctx = egui::Context::default();
    let mut app = fixture();
    app.core.scoped.linear_checkpoint =
        Some(compute_checkpoint(app.core.model.clone(), app.core.analysis_cfg, wave(), 2).unwrap());
    let out = draw(&ctx, &mut app, vec![]);
    let pos = out
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "保存波形で照合・再開" => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .expect("本番画面に再開ボタンがある");
    for pressed in [true, false] {
        draw(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    assert!(app.core.scoped.job.is_some());
    wait_for_job(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let result = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history
        .as_ref()
        .unwrap();
    assert_eq!(result.time.len(), 6);
    assert_eq!(
        result.recording.as_ref().unwrap().frame_time,
        vec![0.0, 0.06, 0.1]
    );
}
