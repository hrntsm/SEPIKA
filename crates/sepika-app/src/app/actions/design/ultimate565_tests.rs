use super::*;
use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination};

fn analyzed() -> App {
    let mut app = App::default();
    let mut model = crate::sample::portal_frame();
    model.load_cases.push(LoadCase {
        id: LoadCaseId(2),
        name: "明示P0".into(),
        kind: LoadCaseKind::Live,
        nodal: vec![],
        member: vec![],
    });
    model.combinations = vec![LoadCombination {
        name: "保存長期架構".into(),
        terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(2), 1.0)],
    }];
    model.ultimate_gravity_combination = Some(0);
    app.load_model(model);
    app.core.analysis_cfg.threads = 1;
    for id in [LoadCaseId(0), LoadCaseId(2)] {
        app.run_linear_static(id);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
    }
    app
}

#[test]
fn static_ultimate_requires_all_selected_current_case_results_without_fallback() {
    let mut app = analyzed();
    assert!(app.ultimate_demand_by_elem().is_ok());
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .statics
        .retain(|(key, _)| *key != StaticCaseKey::User(LoadCaseId(2)));
    assert!(app
        .ultimate_demand_by_elem()
        .unwrap_err()
        .contains("case=2"));
    app.core.ultimate_use_pushover = true;
    assert!(app.ultimate_demand_by_elem().is_err());
    app.core.ultimate_use_pushover = false;
    app.core.model.ultimate_gravity_combination = None;
    assert!(app
        .ultimate_demand_by_elem()
        .unwrap_err()
        .contains("gravity_combination_missing"));
}

#[test]
fn selected_static_gravity_rejects_missing_identity_changed_terms_and_old_contract() {
    let mut app = analyzed();
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .input_records
        .clear();
    assert!(app
        .ultimate_demand_by_elem()
        .unwrap_err()
        .contains("入力識別情報"));
    app = analyzed();
    app.core.model.combinations[0]
        .terms
        .push((LoadCaseId(0), 1.0));
    assert!(app.ultimate_demand_by_elem().is_err());
    app = analyzed();
    app.core.model.sections[0].area *= 1.1;
    assert!(app
        .ultimate_demand_by_elem()
        .unwrap_err()
        .contains("一致しません"));
    app = analyzed();
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .analysis_contract_version = Some(0);
    assert!(app
        .ultimate_demand_by_elem()
        .unwrap_err()
        .contains("旧契約"));
}

#[test]
fn saved_gravity_selection_and_terms_edits_undo_redo_cannot_reuse_changed_inputs() {
    let app = analyzed();
    let path =
        std::env::temp_dir().join(format!("sepika565-selection-{}.ovika", std::process::id()));
    let mut app = app;
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.core.model.ultimate_gravity_combination, Some(0));
    assert!(loaded.ultimate_demand_by_elem().is_ok());
    loaded.core.scoped.undo.run(
        &mut loaded.core.model,
        Box::new(sepika_edit::SetUltimateGravityCombination { index: None }),
    );
    assert!(loaded.ultimate_demand_by_elem().is_err());
    loaded.core.scoped.undo.undo(&mut loaded.core.model);
    assert!(loaded.ultimate_demand_by_elem().is_ok());
    loaded.core.scoped.undo.redo(&mut loaded.core.model);
    assert!(loaded.ultimate_demand_by_elem().is_err());
    loaded.core.scoped.undo.undo(&mut loaded.core.model);
    loaded.core.model.combinations[0].terms[1].1 = 0.0;
    assert!(loaded.ultimate_demand_by_elem().is_err());
}

#[test]
fn pushover_ultimate_adopts_confirmed_capacity_step_and_never_static_fallback() {
    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    let mut app = analyzed();
    app.generate_stories_action();
    app.run_seismic(SeismicDir::X);
    app.core.analysis_cfg.push_steps = 3;
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.core.ultimate_use_pushover = true;
    app.core.ultimate_upper_factor = 1.0;
    assert!(app.ultimate_demand_by_elem().unwrap_err().contains("step"));
    let bundle = app.core.scoped.results.as_mut().unwrap();
    let po = bundle.pushover.as_mut().unwrap();
    let step = po.confirmed_history.as_ref().unwrap().last().unwrap().step;
    po.capacity_evaluation = Some(
        po.evaluation_point(
            EvaluationPurpose::HoldingCapacity,
            SeismicDir::X,
            step,
            "終局需要の明示採用".into(),
        )
        .unwrap(),
    );
    let copy = po.clone();
    bundle.pushover_x = Some(copy);
    let demand = app.ultimate_demand_by_elem().unwrap();
    assert!(demand.iter().all(|(_, d)| d.mode
        == sepika_design_jp::ultimate::UltimateDemandMode::ConfirmedTotal
        && d.q_long.is_none()));
    app.core.ultimate_upper_factor = 1.1;
    assert!(app.ultimate_demand_by_elem().unwrap_err().contains("倍率"));
    app.core.ultimate_upper_factor = 1.0;
    app.core.scoped.pushover_view_dir = SeismicDir::Y;
    assert!(app.ultimate_demand_by_elem().is_err());
}
