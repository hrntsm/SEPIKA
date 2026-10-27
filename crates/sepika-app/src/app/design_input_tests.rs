use super::*;
use result_validity::ResultInputKey;

/// 独立 oracle: 中実矩形 B=100,D=200 の Z=BD²/6。
/// 純曲げ σ=0.8*235/1.5、端モーメント M=σZ。自重は密度0で抑止。
pub(crate) fn pure_bending() -> App {
    let mut model = crate::sample::portal_frame();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    model.stories.clear();
    model.nodes.truncate(2);
    model.nodes[0].coord = [0.0, 0.0, 0.0];
    model.nodes[1].coord = [1000.0, 0.0, 0.0];
    model.nodes[1].restraint = sepika_core::dof::Dof6Mask::FREE;
    model.nodes[1].mass = Some([1.0, 1.0, 1.0, 0.0, 0.0, 0.0]);
    let mut beam = model.elements[2].clone();
    beam.id = ElemId(0);
    beam.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    beam.section = Some(SectionId(0));
    model.elements = vec![beam];
    let mut section = sepika_core::section_shape::SectionShape::SteelFlatBar {
        width: 100.0,
        thick: 200.0,
    }
    .to_section(SectionId(0), "独立矩形大梁".into());
    section.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
    section.material = Some(sepika_core::ids::MaterialId(0));
    model.sections = vec![section];
    model.materials[0].name = "独立カスタム鋼".into();
    model.materials[0].density = 0.0;
    model.load_cases.truncate(1);
    model.load_cases[0].member.clear();
    let moment = (0.8 * 235.0 / 1.5) * (100.0 * 200.0_f64.powi(2) / 6.0);
    model.load_cases[0].nodal = vec![sepika_core::model::NodalLoad::manual(
        NodeId(1),
        [0.0, 0.0, 0.0, 0.0, moment, 0.0],
    )];
    let mut app = App::default();
    app.load_model(model);
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.th_duration = 0.02;
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.require_design_input().is_ok());
    app
}

fn bending(app: &App) -> f64 {
    app.current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .flat_map(|m| &m.positions)
        .filter_map(|p| match &p.outcome {
            sepika_design_jp::CheckOutcome::Checked(c) => Some(c),
            _ => None,
        })
        .flat_map(|c| &c.components)
        .filter(|c| c.kind == sepika_design_jp::CheckKind::Bending)
        .map(|c| c.ratio)
        .fold(0.0, f64::max)
}

pub(crate) fn edit_fy(app: &mut App) {
    #[cfg(feature = "gui")]
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::SetMaterialField {
            id: sepika_core::ids::MaterialId(0),
            field: sepika_edit::MaterialField::Fy,
            value: Some(117.5),
        }))
    );
    #[cfg(not(feature = "gui"))]
    {
        app.core.model.materials[0].fy = Some(117.5);
        app.core.scoped.staleness.mark_edited();
    }
}

#[test]
fn design602_pure_bending_th_save_undo_and_explicit_recheck() {
    let mut app = pure_bending();
    assert!((bending(&app) - 0.8).abs() < 1e-10);
    let forces = &app.core.scoped.results.as_ref().unwrap().member_forces[0].1;
    let moment = (0.8 * 235.0 / 1.5) * (100.0 * 200.0_f64.powi(2) / 6.0);
    for (_, force) in &forces.at {
        assert!(force[..4].iter().all(|v| v.abs() < 1e-6), "{force:?}");
        assert!(force[4].abs() < 1e-6, "{force:?}");
        assert!((force[5].abs() - moment).abs() < 1e-6, "{force:?}");
    }
    edit_fy(&mut app);
    assert!(app.require_design_input().is_err());
    app.run_time_history_sample();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(!app.core.scoped.staleness.results_stale);
    assert!(app.require_design_input().is_err());
    let csv = crate::summary::build_report_csv(&app);
    assert!(csv.contains("[検定結果状態]"));
    assert!(!csv.contains("[部材検定]"));
    let path = std::env::temp_dir().join(format!("sepika602-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(app.core.scoped.last_error.is_none());
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(reopened.core.scoped.last_error.is_none());
    assert!(reopened
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history
        .is_some());
    assert!(reopened.current_design_results().is_none());
    assert!(!reopened.core.scoped.staleness.results_stale);
    #[cfg(feature = "gui")]
    {
        app.undo_action();
        assert!(app.require_design_input().is_ok());
        assert!((bending(&app) - 0.8).abs() < 1e-10);
        app.redo_action();
        assert!(app.require_design_input().is_err());
    }
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.require_design_input().is_ok(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!((bending(&app) - 1.6).abs() < 1e-10);
    assert!(app
        .current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .flat_map(|m| &m.positions)
        .any(|p| matches!(&p.outcome, sepika_design_jp::CheckOutcome::Checked(c) if !c.ok())));
    let csv = crate::summary::build_report_csv(&app);
    assert!(csv.contains("1.6000,NG"), "{csv}");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn design602_same_input_metadata_conditions_missing_and_failure() {
    let mut app = pure_bending();
    app.run_time_history_sample();
    assert!(app.require_design_input().is_ok());
    app.core.analysis_cfg.th_record_every += 1;
    app.generate_axes_action();
    #[cfg(feature = "gui")]
    {
        app.ui.view.check_ratio_label_all = true;
    }
    assert!(app.require_design_input().is_ok());
    app.core.analysis_cfg.rc_damage_control = !app.core.analysis_cfg.rc_damage_control;
    assert!(app.require_design_input().is_err());
    app.core.analysis_cfg.rc_damage_control = !app.core.analysis_cfg.rc_damage_control;
    assert!(app.require_design_input().is_ok());
    app.core.analysis_cfg.bond_method = sepika_design_jp::BondMethod::Rc1991;
    assert!(app.require_design_input().is_err());
    app.core.analysis_cfg.bond_method = sepika_design_jp::BondMethod::Rc1999;
    app.core.analysis_cfg.qd_method = sepika_design_jp::QdMethod::Qd1;
    assert!(app.require_design_input().is_err());
    app.core.analysis_cfg.qd_method = sepika_design_jp::QdMethod::Min;
    assert!(app.require_design_input().is_ok());
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .input_records
        .retain(|(key, _)| *key != ResultInputKey::Design);
    assert!(app.require_design_input().is_err());
    app.run_design_check();
    assert!(app.require_design_input().is_ok());
    edit_fy(&mut app);
    app.run_design_check(); // 旧静的入力を拒否する失敗。
    assert!(app.current_design_results().is_none());
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .member_checks
        .is_empty());
    app.core.scoped.staleness.mark_fresh();
    assert!(app.require_design_input().is_err());
}

#[test]
fn design602_partial_analysis_never_certifies_old_design() {
    for run in 0..5 {
        for edited in [false, true] {
            let mut model = crate::sample::portal_frame();
            model.panel_zone = sepika_core::model::PanelZoneMode::None;
            let mut app = App::default();
            app.load_model(crate::app::tests::with_explicit_pushover_gravity(model));
            app.generate_stories_action();
            app.core.analysis_cfg.threads = 1;
            app.core.analysis_cfg.push_steps = 2;
            app.core.analysis_cfg.push_max_disp = 1.0;
            if run >= 3 {
                app.run_seismic(SeismicDir::X);
                app.run_seismic(SeismicDir::Y);
            }
            app.run_linear_static(LoadCaseId(0));
            assert!(app.require_design_input().is_ok());
            let input = app
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .input_records
                .iter()
                .find(|(key, _)| *key == ResultInputKey::Design)
                .unwrap()
                .1
                .clone();
            if edited {
                edit_fy(&mut app);
            }
            match run {
                0 => app.run_eigen(1),
                1 | 2 => {
                    app.core.analysis_cfg.push_dir = if run == 1 {
                        SeismicDir::X
                    } else {
                        SeismicDir::Y
                    };
                    app.run_pushover();
                }
                3 => {
                    app.start_lumped_mass_eigen_job();
                    if app.core.scoped.job.is_some() {
                        super::tests::wait_for_job(&mut app);
                    }
                }
                _ => {
                    app.start_lumped_mass_th_job(vec![0.0; 3]);
                    if app.core.scoped.job.is_some() {
                        super::tests::wait_for_job(&mut app);
                    }
                }
            }
            assert!(
                app.core.scoped.last_error.is_none() == !(run >= 3 && edited),
                "run={run} edited={edited}: {:?}",
                app.core.scoped.last_error
            );
            if run >= 3 && edited {
                assert!(app.core.scoped.job.is_none());
                assert!(app
                    .core
                    .scoped
                    .last_error
                    .as_deref()
                    .unwrap()
                    .contains("再実行"));
            }
            assert_eq!(app.require_design_input().is_ok(), !edited, "run={run}");
            assert_eq!(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .input_records
                    .iter()
                    .find(|(key, _)| *key == ResultInputKey::Design)
                    .unwrap()
                    .1,
                input
            );
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn design602_actual_table_hides_old_ratios_after_th() {
    let mut app = pure_bending();
    let ctx = egui::Context::default();
    let labels = |app: &mut App| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(3000.0, 4000.0),
                )),
                ..Default::default()
            },
            |ui| crate::design_view::design_table(ui, app),
        )
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
    };
    assert!(labels(&mut app).iter().any(|text| text == "0.8000"));
    edit_fy(&mut app);
    app.run_time_history_sample();
    let text = labels(&mut app);
    assert!(!text.iter().any(|text| text == "0.8000"));
    assert!(text.iter().any(|text| text.contains("要再計算")));
}

#[test]
fn design602_combo_source_and_independent_selected_display() {
    let mut app = pure_bending();
    let live = LoadCaseId(
        app.core
            .model
            .load_cases
            .iter()
            .map(|case| case.id.0)
            .max()
            .unwrap()
            + 1,
    );
    app.core
        .model
        .load_cases
        .push(sepika_core::model::LoadCase {
            origin: Default::default(),
            id: live,
            name: "独立P0".into(),
            kind: sepika_core::model::LoadCaseKind::Live,
            nodal: vec![],
            member: vec![],
        });
    app.core
        .model
        .combinations
        .push(sepika_core::model::LoadCombination {
            name: "独立長期".into(),
            terms: vec![(LoadCaseId(0), 1.0), (live, 1.0)],
        });
    app.core.scoped.staleness.mark_edited();
    app.run_combination(0);
    assert!((bending(&app) - 0.8).abs() < 1e-10);
    assert_eq!(
        app.current_design_results().unwrap().design_source,
        Some(ResultInputKey::Combo("独立長期".into()))
    );
    // 表示フォーカスを変えても、生成証明は採用した組合せを照合する。
    app.ui.scoped.nav.focus_result = Some(StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
    assert!(app.require_design_input().is_ok());
    edit_fy(&mut app);
    app.run_time_history_sample();
    assert!(app.require_design_input().is_err());
    app.run_combination(0);
    assert!((bending(&app) - 1.6).abs() < 1e-10);
    assert!(crate::summary::build_report_csv(&app).contains("1.6000,NG"));
}

#[test]
fn design602_failed_analysis_and_rejected_job_do_not_make_design_current() {
    let mut app = pure_bending();
    let record = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .input_records
        .iter()
        .find(|(key, _)| *key == ResultInputKey::Design)
        .unwrap()
        .1
        .clone();
    app.start_eigen_job(1);
    edit_fy(&mut app);
    super::tests::wait_for_job(&mut app);
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("旧入力の解析結果を破棄"));
    assert!(app.require_design_input().is_err());
    app.run_time_history(sepika_solver::dynamic::timehistory::GroundMotion {
        dt: 0.0,
        accel_x: vec![0.0; 3],
        accel_y: None,
        accel_theta: None,
    });
    assert!(app.core.scoped.last_error.is_some());
    assert!(app.require_design_input().is_err());
    // 通信路切断は製品のpoll_job異常終了処理を直接通す。
    let (sender, receiver) = std::sync::mpsc::channel();
    drop(sender);
    app.core.scoped.job = Some(AnalysisJob {
        reference_source: app.input_reference(),
        label: "異常終了fixture",
        started: SystemTime::now(),
        input: app.job_input(),
        rx: receiver,
        #[cfg(feature = "gui")]
        jump_on_success: None,
    });
    assert!(app.poll_job());
    assert!(app.core.scoped.last_error.is_some());
    assert!(app.require_design_input().is_err());
    let missing = std::env::temp_dir().join(format!(
        "sepika602-missing-{}/save.ovika",
        std::process::id()
    ));
    app.save_project_to(missing);
    assert!(app.core.scoped.last_error.is_some());
    assert!(app.require_design_input().is_err());
    assert_eq!(
        app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .input_records
            .iter()
            .find(|(key, _)| *key == ResultInputKey::Design)
            .unwrap()
            .1,
        record
    );
}

#[cfg(feature = "gui")]
#[test]
fn design602_pending_analysis_and_save_cancel_keep_old_input_unavailable() {
    let mut app = pure_bending();
    edit_fy(&mut app);
    let records = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .input_records
        .clone();
    app.core.scoped.pending_unset_analysis = Some(PendingAnalysis::Eigen(1));
    app.cancel_pending_analysis();
    assert!(app.core.scoped.job.is_none());
    assert!(app.require_design_input().is_err());
    app.core.scoped.pending_save_recording =
        Some((std::env::temp_dir().join("602-cancel.ovika"), 513));
    app.resolve_pending_save_recording(None);
    assert!(app.core.scoped.pending_save_recording.is_none());
    assert!(app.require_design_input().is_err());
    assert_eq!(
        app.core.scoped.results.as_ref().unwrap().input_records,
        records
    );
}

#[test]
fn design602_missing_or_mismatched_design_and_source_proof_stays_unavailable() {
    for mismatch in 0..5 {
        let mut app = pure_bending();
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let key = if mismatch % 2 == 0 {
            ResultInputKey::Design
        } else {
            ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0)))
        };
        match mismatch {
            0 | 1 => bundle.input_records.retain(|(saved, _)| *saved != key),
            2 | 3 => bundle
                .input_records
                .iter_mut()
                .find(|(saved, _)| *saved == key)
                .unwrap()
                .1
                .push(0),
            _ => bundle.design_source = None,
        }
        app.core.scoped.staleness.mark_fresh();
        assert!(app.require_design_input().is_err(), "mismatch={mismatch}");
        assert!(!crate::summary::build_report_csv(&app).contains("[部材検定]"));
    }
}
