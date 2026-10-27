use super::*;
#[path = "../../../../test_fixtures/rc_wall617.rs"]
pub(crate) mod fixture;

fn ready() -> App {
    let mut app = App::default();
    app.load_model(crate::app::tests::with_explicit_pushover_gravity(
        fixture::input_model(),
    ));
    app.core.analysis_cfg.push_steps = 2;
    app.core.analysis_cfg.push_use_max_disp = true;
    app.core.analysis_cfg.push_use_drift_angle = false;
    app.core.analysis_cfg.push_max_disp = 0.2;
    app.core.analysis_cfg.th_nonlinear = true;
    app.core.analysis_cfg.th_duration = 0.04;
    app.run_preparation();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}

fn ledger(app: &App) -> Vec<u8> {
    let r = app.core.scoped.results.as_ref().unwrap();
    serde_json::to_vec(&(&r.statics, &r.combos)).unwrap()
}

fn reject_wall_time_history_without_mutating_recorded_runs(app: &mut App) {
    let before = bincode::serialize(&app.core.scoped.results).unwrap();
    let cases = bincode::serialize(&app.core.model.vibration_cases).unwrap();
    let view = app.core.scoped.view_vibration_case;
    app.run_time_history_sample();
    let reason = app.core.scoped.last_error.as_deref().unwrap();
    for expected in [
        "非線形時刻歴解析",
        "部材 ID 0",
        "壁 ID [4]",
        "従属回転の慣性の連成は未定式化",
    ] {
        assert!(reason.contains(expected), "{reason}");
    }
    assert_eq!(
        bincode::serialize(&app.core.scoped.results).unwrap(),
        before
    );
    assert_eq!(
        bincode::serialize(&app.core.model.vibration_cases).unwrap(),
        cases
    );
    assert_eq!(app.core.scoped.view_vibration_case, view);
}

#[test]
fn pushover_preserves_static_ledger_and_wall_time_history_gate_keeps_recorded_capacity() {
    let mut app = ready();
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let before = ledger(&app);
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_eq!(ledger(&app), before);
    let po = app.displayed_pushover().unwrap();
    assert_eq!(po.wall_capacities.len(), 1);
    assert!(po.wall_capacities[0]
        .1
        .iter()
        .all(|q| q.is_finite() && *q > 0.0));
    let capacities = po.wall_capacities.clone();
    assert_eq!(po.wall_history.as_ref().unwrap().len(), po.steps.len());
    reject_wall_time_history_without_mutating_recorded_runs(&mut app);
    assert_eq!(ledger(&app), before);
    assert_eq!(
        app.displayed_pushover().unwrap().wall_capacities,
        capacities
    );
}

#[cfg(feature = "gui")]
#[test]
fn rejected_wall_time_history_keeps_adopted_pushover_capacity_and_input_across_save_and_model_changes(
) {
    use super::result_validity::ResultInputKey;
    use crate::viewer::ModelingAnalysis;
    let mut app = ready();
    app.run_linear_static(LoadCaseId(0));
    assert!(app.core.scoped.last_error.is_none());
    let static_ledger = ledger(&app);
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == sepika_core::model::ElementKind::Wall)
        .unwrap();
    assert!(app
        .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::TimeHistory)
        .is_err());
    let key = ResultInputKey::Pushover(SeismicDir::X);
    let mut capacities = vec![];
    for (name, fy) in [("caseA", 295.0), ("caseB", 345.0)] {
        app.core.model.materials[2].fy = Some(fy);
        app.core.scoped.wave_library_selection = Some(name.into());
        assert!(app.require_result_input(key.clone()).is_err());
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        capacities.push(
            app.adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
                .unwrap(),
        );
        reject_wall_time_history_without_mutating_recorded_runs(&mut app);
        assert!(app
            .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::TimeHistory)
            .is_err());
        assert!(app.require_result_input(key.clone()).is_ok());
        assert_eq!(ledger(&app), static_ledger);
    }
    assert!(capacities[1][0] > capacities[0][0]);
    let path = std::env::temp_dir().join(format!(
        "sepika617-wall-th-gate-{}.ovika",
        std::process::id()
    ));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    assert_eq!(
        loaded
            .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
            .unwrap(),
        capacities[1]
    );
    assert!(loaded
        .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::TimeHistory)
        .is_err());
    assert!(loaded
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .statics
        .is_empty());
    loaded.core.model.materials[2].fy = Some(390.0);
    assert!(loaded.require_result_input(key).is_err());
    assert!(loaded
        .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
        .is_err());
    assert_eq!(
        loaded.displayed_pushover().unwrap().wall_capacities[0].1,
        capacities[1]
    );
}

#[test]
fn background_nonlinear_jobs_reject_results_after_input_edit() {
    for th in [false, true] {
        let mut app = ready();
        if th {
            app.start_time_history_job(App::sample_wave(&app.core.analysis_cfg).unwrap());
        } else {
            app.start_pushover_job();
        }
        assert!(app.core.scoped.job.is_some());
        app.core.model.materials[2].fy = Some(345.0);
        super::tests::wait_for_job(&mut app);
        assert!(app.core.scoped.last_error.is_some());
        assert!(app
            .core
            .scoped
            .results
            .as_ref()
            .is_none_or(|r| r.pushover_x.is_none() && r.time_history.is_none()));
    }
}

#[test]
fn selected_static_case_and_combo_reach_common_face_and_reject_missing_or_old_results() {
    use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination, NodalLoad};
    use sepika_design_jp::{wall_check::WallCheckKind, CheckOutcome};
    let mut app = ready();
    if let Some(sepika_core::section_shape::SectionShape::RcWall { pwh_ratio, .. }) =
        &mut app.core.model.sections[2].shape
    {
        *pwh_ratio = Some(0.006);
    }
    app.core.model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(20),
        name: "独立水平P".into(),
        kind: LoadCaseKind::Live,
        nodal: vec![NodalLoad::manual(
            NodeId(2),
            [100_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )],
        member: Vec::new(),
    });
    app.core.model.combinations.push(LoadCombination {
        name: "617同一バッチ".into(),
        terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(20), 1.0)],
    });
    let index = app.core.model.combinations.len() - 1;
    for combination in [false, true] {
        if combination {
            app.run_combination(index);
        } else {
            app.run_linear_static(LoadCaseId(20));
        }
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
        let wall = expanded
            .elements
            .iter()
            .find(|e| e.kind == sepika_core::model::ElementKind::Wall)
            .unwrap();
        let result = app.core.scoped.results.as_ref().unwrap();
        let batch: Vec<_> = result
            .member_forces
            .iter()
            .map(|(id, f)| (*id, f.at.as_slice()))
            .collect();
        let face = sepika_element::wall::static_face::StaticWallFace::new(wall, &expanded)
            .unwrap()
            .recover(&batch)
            .unwrap();
        assert_eq!(face.physical_area_mm2, 1_332_000.0);
        assert!((face.shear_n.abs() - 100_000.0).abs() < 1e-6);
        let check = result
            .wall_checks
            .iter()
            .find(|w| w.kind == WallCheckKind::ReferenceSkeleton)
            .unwrap();
        assert!(check
            .case
            .contains(if combination { "combo:" } else { "case:20" }));
        let CheckOutcome::Checked(output) = &check.outcome else {
            panic!("{:?}", check)
        };
        let detail = &output.components[0].detail;
        for expected in [
            format!("Nface={:.1} kN", face.compression_n / 1000.0),
            format!("Mface={:.1} kNm", face.moment_nmm / 1e6),
            format!("Qface={:.1} kN", face.shear_n / 1000.0),
        ] {
            assert!(detail.contains(&expected), "{detail}");
        }
    }
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .member_forces
        .retain(|(id, _)| *id != ElemId(1));
    app.run_design_check();
    let checks = &app.core.scoped.results.as_ref().unwrap().wall_checks;
    assert!(checks.iter().any(|w| w.kind == WallCheckKind::ReferenceSkeleton && matches!(&w.outcome, CheckOutcome::Skipped {reason} if reason.contains("部材 ID 1"))));
    assert!(checks
        .iter()
        .any(|w| w.kind == WallCheckKind::AllowableShear
            && matches!(w.outcome, CheckOutcome::Checked(_))));
    app.core.model.materials[2].fy = Some(345.0);
    app.run_design_check();
    assert!(app.core.scoped.last_error.is_some());
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .is_empty());
}

#[cfg(feature = "gui")]
#[test]
fn incremental_modeling_reads_adopted_run_numbers_and_rejects_missing_invalid_or_old_capacity() {
    use crate::viewer::ModelingAnalysis;
    let mut app = ready();
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == sepika_core::model::ElementKind::Wall)
        .unwrap();
    assert!(app
        .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
        .is_err());
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let before = ledger(&app);
    let expected = app.displayed_pushover().unwrap().wall_capacities[0].1;
    assert_eq!(
        app.adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
            .unwrap(),
        expected
    );
    assert_eq!(ledger(&app), before);
    let bundle = app.core.scoped.results.as_mut().unwrap();
    let po = bundle.pushover_x.as_mut().unwrap();
    po.wall_capacities.clear();
    assert!(app
        .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
        .unwrap_err()
        .contains("未解決"));
    let bundle = app.core.scoped.results.as_mut().unwrap();
    let po = bundle.pushover_x.as_mut().unwrap();
    po.wall_capacities.push((wall.id, [f64::NAN, expected[1]]));
    assert!(app
        .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
        .unwrap_err()
        .contains("不正"));
    app.core.model.materials[2].fy = Some(345.0);
    assert!(app
        .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
        .is_err());
}

#[test]
fn holding_capacity_requires_same_run_wall_capacity_and_current_input() {
    for method in [
        sepika_core::model::RcWallShearMethod::ArakawaMean,
        sepika_core::model::RcWallShearMethod::ArakawaMin,
    ] {
        let mut app = ready();
        app.set_rc_wall_shear_method(method);
        use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
        let mut beam = SectionShape::RcBeamRect {
            b: 600.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia: 22.0,
                top: vec![4],
                bottom: vec![4],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: 10.0,
                    pitch: 100.0,
                    legs: 2,
                },
            },
        }
        .to_section(SectionId(1), "RC梁".into());
        beam.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
        beam.material = app.core.model.sections[1].material;
        beam.rebar_material = app.core.model.sections[1].rebar_material;
        beam.shear_rebar_material = app.core.model.sections[1].shear_rebar_material;
        app.core.model.sections[1] = beam;
        app.run_preparation();
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        super::tests::select_holding_points(&mut app);
        app.core.design_rank_auto = true;
        let valid = app.compute_unconfirmed_holding_capacity();
        assert!(valid.is_ok(), "{:?}", valid.err());
        let before = ledger(&app);
        let po = app.displayed_pushover().unwrap();
        assert_eq!(po.rc_wall_shear_method, method);
        let wall_id = po.wall_capacities[0].0;
        let qw = po
            .wall_response_at(wall_id, po.ds_evaluation.as_ref().unwrap().step)
            .unwrap()
            .qw_n;
        assert!(qw.abs() > 0.0);
        let branch = usize::from(qw < 0.0);
        let mut ranks = Vec::new();
        for demand_ratio in [0.98, 1.0] {
            let bundle = app.core.scoped.results.as_mut().unwrap();
            let po = bundle.pushover_x.as_mut().unwrap();
            let qu = &mut po
                .wall_capacities
                .iter_mut()
                .find(|(id, _)| *id == wall_id)
                .unwrap()
                .1;
            qu[branch] = qw.abs() / demand_ratio;
            let (result, _) = app
                .compute_unconfirmed_holding_capacity()
                .unwrap_or_else(|reason| panic!("{reason}"));
            ranks.push(
                result
                    .member_ranks
                    .iter()
                    .find(|(id, _)| *id == wall_id)
                    .unwrap()
                    .1,
            );
        }
        assert_eq!(
            ranks,
            vec![
                sepika_design_jp::secondary::holding_capacity::MemberRank::FA,
                sepika_design_jp::secondary::holding_capacity::MemberRank::FD
            ]
        );
        assert_eq!(ledger(&app), before);
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let po = bundle.pushover_x.as_mut().unwrap();
        po.wall_capacities.clear();
        let missing = app
            .compute_holding_capacity()
            .err()
            .expect("採用Qu欠落は未算定");
        assert!(missing.contains("初期Quが未解決"), "{missing}");
        assert_eq!(ledger(&app), before);
        app.core.model.materials[2].fy = Some(345.0);
        assert!(app.compute_holding_capacity().is_err());
    }
}

#[test]
fn wall_method_default_runs_save_undo_and_reject_old_results_and_jobs() {
    use super::result_validity::ResultInputKey;
    use sepika_core::model::RcWallShearMethod::{ArakawaMean, ArakawaMin};
    let mut app = ready();
    assert_eq!(app.core.model.stress_cfg.rc_wall_shear_method, ArakawaMean);
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mean = app.displayed_pushover().unwrap().wall_capacities.clone();
    assert_eq!(
        app.displayed_pushover().unwrap().rc_wall_shear_method,
        ArakawaMean
    );
    let key = ResultInputKey::Pushover(SeismicDir::X);
    assert!(app.require_result_input(key.clone()).is_ok());
    app.set_rc_wall_shear_method(ArakawaMin);
    assert!(app.core.scoped.staleness.results_stale);
    assert!(app.require_result_input(key.clone()).is_err());
    app.run_linear_static(LoadCaseId(0));
    assert!(app.require_result_input(key.clone()).is_err());
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let min = app.displayed_pushover().unwrap().wall_capacities.clone();
    assert_eq!(
        app.displayed_pushover().unwrap().rc_wall_shear_method,
        ArakawaMin
    );
    assert_ne!(mean, min);
    reject_wall_time_history_without_mutating_recorded_runs(&mut app);
    assert_eq!(
        app.displayed_pushover().unwrap().rc_wall_shear_method,
        ArakawaMin
    );
    assert_eq!(app.displayed_pushover().unwrap().wall_capacities, min);
    let path = std::env::temp_dir().join(format!("sepika572-method-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    assert_eq!(
        loaded.core.model.stress_cfg.rc_wall_shear_method,
        ArakawaMin
    );
    assert!(loaded.require_result_input(key.clone()).is_ok());
    loaded.set_rc_wall_shear_method(ArakawaMean);
    assert!(loaded.require_result_input(key.clone()).is_err());
    loaded.core.scoped.undo.undo(&mut loaded.core.model);
    loaded.core.scoped.staleness.mark_edited();
    assert_eq!(
        loaded.core.model.stress_cfg.rc_wall_shear_method,
        ArakawaMin
    );
    loaded.core.scoped.undo.redo(&mut loaded.core.model);
    loaded.core.scoped.staleness.mark_edited();
    assert_eq!(
        loaded.core.model.stress_cfg.rc_wall_shear_method,
        ArakawaMean
    );
    loaded.run_linear_static(LoadCaseId(0));
    assert!(loaded.require_result_input(key).is_err());
    loaded.run_design_check();
    assert!(loaded.core.scoped.last_error.is_none());
    loaded.set_rc_wall_shear_method(ArakawaMin);
    loaded.run_design_check();
    assert!(loaded.core.scoped.last_error.is_some());
    assert!(loaded
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .is_empty());
    for th in [false, true] {
        let mut app = ready();
        if th {
            app.start_time_history_job(App::sample_wave(&app.core.analysis_cfg).unwrap());
        } else {
            app.start_pushover_job();
        }
        assert!(app.core.scoped.job.is_some());
        app.set_rc_wall_shear_method(ArakawaMin);
        super::tests::wait_for_job(&mut app);
        assert!(app.core.scoped.last_error.is_some());
        assert!(app
            .core
            .scoped
            .results
            .as_ref()
            .is_none_or(|r| r.pushover_x.is_none() && r.time_history.is_none()));
    }
}

#[cfg(feature = "gui")]
#[test]
fn wall_method_modeling_uses_selected_run_and_no_display_recalculation() {
    use crate::viewer::ModelingAnalysis;
    use sepika_core::model::RcWallShearMethod::{ArakawaMean, ArakawaMin};
    let mut app = ready();
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
    let wall = expanded
        .elements
        .iter()
        .find(|e| e.kind == sepika_core::model::ElementKind::Wall)
        .unwrap();
    for method in [ArakawaMean, ArakawaMin] {
        app.set_rc_wall_shear_method(method);
        assert!(app
            .adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
            .is_err());
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let before = ledger(&app);
        let qu = app.displayed_pushover().unwrap().wall_capacities[0].1;
        assert_eq!(
            app.adopted_wall_capacity(wall, &expanded, ModelingAnalysis::Incremental)
                .unwrap(),
            qu
        );
        assert_eq!(ledger(&app), before);
    }
}
