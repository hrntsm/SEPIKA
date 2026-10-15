use super::*;
use sepika_core::model::{LoadSource, MemberLoad, MemberLoadKind, Model};
use sepika_design_jp::CheckOutcome;

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    rmp_serde::to_vec_named(value).unwrap()
}

fn src_app() -> App {
    let mut model: Model = serde_json::from_str(include_str!(
        "../../tests/fixtures/member_load_direction_src.json"
    ))
    .unwrap();
    model.load_cases[0].member[0].name = "手動P100kN".into();
    model.load_cases[0].member[0].dir = [0.0, 0.0, -2.0];
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.threads = 1;
    app
}

fn manual(app: &App) -> &MemberLoad {
    &app.core.model.load_cases[0].member[0]
}

fn set_direction(app: &mut App, dir: [f64; 3]) {
    let mut load = manual(app).clone();
    load.dir = dir;
    assert!(app.core.scoped.undo.run(
        &mut app.core.model,
        Box::new(sepika_edit::SetMemberLoad {
            lc: LoadCaseId(0),
            index: 0,
            load,
        }),
    ));
    app.core.scoped.staleness.mark_edited();
}

fn assert_calculated(app: &mut App, stage: &str) {
    let manual_before = bytes(manual(app));
    app.run_combination(0);
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let forces = &app.current_static().unwrap().member_forces[0].1.at;
    // 独立釣合い: P100 kN + 自重42 kN + 先端10 kN。
    assert!(
        (forces[0].1[1].abs() - 152_000.0).abs() < 1e-6,
        "{forces:?}"
    );
    assert!(app.require_selected_static_input().is_ok());
    assert_eq!(bytes(manual(app)), manual_before);
    assert_eq!(manual(app).dir, [0.0, 0.0, -2.0]);
    let checks = &app.core.scoped.results.as_ref().unwrap().member_checks;
    assert!(!checks.is_empty());
    assert!(checks
        .iter()
        .flat_map(|m| &m.positions)
        .all(|p| { matches!(&p.outcome, CheckOutcome::Checked(check) if check.ok()) }));
    let csv = crate::summary::build_report_csv(app);
    assert!(
        csv.lines()
            .any(|line| line.starts_with("0,") && line.contains(",OK,")),
        "{csv}"
    );
    eprintln!("{stage}: manual={:?}\n{csv}", manual(app));
}

fn assert_unavailable(app: &App, stage: &str) {
    assert!(app.require_selected_static_input().is_err());
    assert!(app.current_static().is_none());
    if let Some(bundle) = &app.core.scoped.results {
        assert!(bundle.member_forces.is_empty());
        assert!(bundle.member_checks.is_empty());
        assert!(!bundle.combos.iter().any(|(name, _)| name == "独立選択"));
    }
    let csv = crate::summary::build_report_csv(app);
    assert!(!csv.contains("全適合"), "{csv}");
    assert!(!csv.contains(",OK,"), "{csv}");
    eprintln!(
        "{stage}: error={:?} notice={:?} manual={:?}\n{csv}",
        app.core.scoped.last_error,
        app.core.scoped.last_notice,
        manual(app)
    );
}

fn assert_rejected(app: &mut App, stage: &str) {
    let before = bytes(&app.core.model);
    app.run_combination(0);
    let reason = app.core.scoped.last_error.as_deref().unwrap();
    assert!(
        reason.contains("DL")
            && reason.contains("member[0]")
            && reason.contains("手動")
            && reason.contains("部材 0")
            && reason.contains("正規化"),
        "{reason}"
    );
    assert_eq!(bytes(&app.core.model), before);
    assert_unavailable(app, stage);
    app.run_design_check();
    assert_unavailable(app, &format!("{stage}-design-only"));
    assert_eq!(bytes(&app.core.model), before);
}

#[test]
fn direction_gate_keeps_existing_auto_horizontal_loads_and_records_actual_notice() {
    let mut app = src_app();
    for (id, name, axis) in [(3, "EX", 0), (4, "EY", 1)] {
        let mut case = app.core.model.load_cases[2].clone();
        case.id = LoadCaseId(id);
        case.name = name.into();
        case.nodal[0].values = [0.0; 6];
        case.nodal[0].values[axis] = 1234.0;
        case.nodal[0].source = LoadSource::Auto;
        app.core.model.load_cases.push(case);
    }
    set_direction(&mut app, [0.0; 3]);
    let before = bytes(&app.core.model);
    app.sync_auto_load_cases_action();
    assert!(app
        .core
        .scoped
        .last_error
        .as_deref()
        .unwrap()
        .contains("正規化"));
    assert_eq!(bytes(&app.core.model), before);
    assert!(!app
        .core
        .scoped
        .last_notice
        .as_deref()
        .is_some_and(|notice| notice.contains("除去")));
    eprintln!(
        "auto-horizontal-observation: notice={:?}; EX={:?}; EY={:?}",
        app.core.scoped.last_notice,
        app.core.model.load_cases[3].nodal,
        app.core.model.load_cases[4].nodal
    );
}

#[test]
fn public_app_actions_reject_bad_direction_and_undo_redo_never_restore_old_ok() {
    let mut app = src_app();
    app.run_combination(1);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    // 重力だけの独立釣合い: P100 kN + 自重42 kN。
    let q_long_n = app.current_static().unwrap().member_forces[0].1.at[0].1[1].abs();
    assert!((q_long_n - 142_000.0).abs() < 1e-6, "{q_long_n}");
    assert_calculated(&mut app, "normal");
    let original = bytes(manual(&app));
    set_direction(&mut app, [0.0; 3]);
    let invalid = bytes(manual(&app));
    assert_rejected(&mut app, "invalid");
    assert_eq!(bytes(manual(&app)), invalid);
    assert!(app.core.scoped.undo.can_undo());
    app.core.scoped.undo.undo(&mut app.core.model);
    app.core.scoped.staleness.mark_edited();
    assert_eq!(bytes(manual(&app)), original);
    assert_unavailable(&app, "undo-before-reanalysis");
    assert_calculated(&mut app, "repaired");
    assert!(app.core.scoped.undo.can_redo());
    app.core.scoped.undo.redo(&mut app.core.model);
    app.core.scoped.staleness.mark_edited();
    assert_eq!(bytes(manual(&app)), invalid);
    assert_rejected(&mut app, "invalid-again");
}

#[test]
fn finite_zero_point_and_distributed_direction_save_open_and_repair_without_data_loss() {
    for (label, kind) in [
        (
            "point",
            MemberLoadKind::Point {
                a: 1500.0,
                p: 100_000.0,
            },
        ),
        (
            "distributed",
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 6000.0,
                w1: 100_000.0 / 6000.0,
                w2: 100_000.0 / 6000.0,
            },
        ),
    ] {
        let mut app = src_app();
        app.core.model.load_cases[0].member[0].kind = kind;
        assert_calculated(&mut app, &format!("{label}-normal"));
        set_direction(&mut app, [0.0; 3]);
        let expected = bytes(manual(&app));
        let path = std::env::temp_dir().join(format!(
            "sepika687-direction-{label}-{}.ovika",
            std::process::id()
        ));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let library = sepika_io::ovika::load_ovika(&path).unwrap();
        assert_eq!(bytes(&library.model.load_cases[0].member[0]), expected);
        assert_eq!(bytes(&library.model), bytes(&app.core.model));
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert_eq!(bytes(manual(&loaded)), expected);
        assert_eq!(bytes(&loaded.core.model), bytes(&app.core.model));
        assert_unavailable(&loaded, &format!("{label}-opened-incomplete"));
        assert_rejected(&mut loaded, &format!("{label}-rejected"));
        loaded.save_project_to(path.clone());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        assert_eq!(bytes(manual(&reopened)), expected);
        assert_unavailable(&reopened, &format!("{label}-resaved-incomplete"));
        set_direction(&mut reopened, [0.0, 0.0, -2.0]);
        assert_calculated(&mut reopened, &format!("{label}-repaired"));
        assert_eq!(manual(&reopened).source, LoadSource::Manual);
        assert_eq!(manual(&reopened).kind, manual(&app).kind);
        assert_eq!(manual(&reopened).extent, manual(&app).extent);
        assert_eq!(manual(&reopened).name, manual(&app).name);
        std::fs::remove_file(path).unwrap();
    }
}
