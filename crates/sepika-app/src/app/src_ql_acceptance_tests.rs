use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{CheckKind, CheckOutcome, QdMethod};

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    rmp_serde::to_vec_named(value).unwrap()
}

fn app(empty_gravity: bool, method: QdMethod) -> App {
    let mut model: sepika_core::model::Model =
        serde_json::from_str(include_str!("../../tests/fixtures/src_ql_contract.json")).unwrap();
    if empty_gravity {
        model.load_cases[0].member.clear();
    }
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.qd_method = method;
    app
}

fn assert_checked(app: &App) {
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.require_selected_static_input().is_ok());
    let bundle = app.current_design_results().expect("現行検定");
    let positions = &bundle.member_checks[0].positions;
    assert_eq!(positions.len(), 3);
    assert!(positions
        .iter()
        .all(|p| matches!(&p.outcome, CheckOutcome::Checked(c) if c.ok())));
    let csv = crate::summary::build_report_csv(app);
    assert!(
        csv.lines()
            .any(|line| line.starts_with("0,") && line.contains(",OK,")),
        "{csv}"
    );
}

#[test]
fn public_src_actions_check_real_ql_and_valid_empty_gravity_without_q0() {
    for method in [QdMethod::Qd1, QdMethod::Qd2, QdMethod::Min] {
        for empty in [false, true] {
            let mut app = app(empty, method);
            let manual = bytes(&app.core.model.load_cases[0].member);
            app.run_combination(1);
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            let ql_n = app.current_static().unwrap().member_forces[0].1.at[0].1[1];
            assert!((ql_n - if empty { 0.0 } else { 100_000.0 }).abs() < 1e-6);
            let terms = sepika_job::design_gravity_terms(
                &app.core.model,
                &app.core.model.combinations[0].terms,
            );
            assert_eq!(terms, vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)]);
            let q0 = sepika_job::simple_beam_q0_by_terms(&app.core.model, &terms).unwrap();
            if empty {
                assert!(q0.is_empty());
            } else {
                assert!((q0[&ElemId(0)] - 75_000.0).abs() < 1e-6);
            }
            app.run_combination(0);
            app.run_design_check();
            assert_checked(&app);
            let q_n = app.current_static().unwrap().member_forces[0].1.at[0].1[1];
            assert!((q_n - if empty { 10_000.0 } else { 110_000.0 }).abs() < 1e-6);
            assert_eq!(bytes(&app.core.model.load_cases[0].member), manual);
            assert!(app.core.model.load_cases[0]
                .member
                .iter()
                .all(|load| load.source == sepika_core::model::LoadSource::Manual));
            assert_eq!(app.core.model.load_cases[3].name, "DL");
            assert!(!app.core.model.load_cases[3].member.is_empty());
            if !empty {
                let CheckOutcome::Checked(check) =
                    &app.current_design_results().unwrap().member_checks[0].positions[0].outcome
                else {
                    panic!("支持端検定");
                };
                let shear = check
                    .components
                    .iter()
                    .find(|c| c.kind == CheckKind::Shear)
                    .unwrap();
                assert!(
                    (shear.ratio - 0.34759322126721853).abs() < 1e-12,
                    "{shear:?}"
                );
            }
            eprintln!(
                "method={method:?} empty={empty} QL={ql_n} Q={q_n} q0={q0:?}\n{}",
                crate::summary::build_report_csv(&app)
            );
        }
    }
}

#[test]
fn src_ovika_design_contracts_keep_current_analysis_and_require_explicit_recheck() {
    let mut original = app(false, QdMethod::Qd1);
    original.run_combination(0);
    assert_checked(&original);
    let input = bytes(&original.core.model);
    let original_bundle = original.core.scoped.results.as_ref().unwrap();
    for (name, version) in [
        ("src-missing", None),
        ("src-old14", Some(14)),
        ("src-old15", Some(15)),
        ("src-old16", Some(16)),
        ("src-old17", Some(17)),
        ("src-old18", Some(18)),
        ("src-current19", Some(19)),
        ("src-future20", Some(20)),
    ] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &original,
            Some(ANALYSIS_CONTRACT_VERSION),
            version,
            name,
        );
        assert_eq!(bytes(&loaded.core.model), input);
        assert!(loaded.require_selected_static_input().is_ok());
        assert!(!loaded.core.scoped.staleness.results_stale);
        let bundle = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(
            bundle.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(bytes(&bundle.combos), bytes(&original_bundle.combos));
        assert_eq!(bundle.member_forces, original_bundle.member_forces);
        let current = version == Some(DESIGN_CONTRACT_VERSION);
        assert_eq!(loaded.current_design_results().is_some(), current);
        assert_eq!(bundle.has_design_results(), current);
        assert_eq!(
            crate::summary::build_report_csv(&loaded).contains(",OK,"),
            current
        );
        if !current {
            assert!(bundle.member_checks.is_empty());
            loaded.select_displayed_result(original.core.scoped.last_static.unwrap());
            loaded.core.scoped.staleness.mark_fresh();
        }
        loaded.save_project_to(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        assert_eq!(bytes(&reopened.core.model), input);
        assert!(reopened.require_selected_static_input().is_ok());
        let saved = reopened.core.scoped.results.as_ref().unwrap();
        assert_eq!(
            saved.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(saved.member_forces, original_bundle.member_forces);
        assert_eq!(reopened.current_design_results().is_some(), current);
        assert_eq!(
            crate::summary::build_report_csv(&reopened).contains(",OK,"),
            current
        );
        if !current {
            assert!(saved.member_checks.is_empty());
            assert_eq!(saved.design_contract_version, None);
        }
        reopened.run_design_check();
        assert_checked(&reopened);
        assert_eq!(
            reopened
                .current_design_results()
                .unwrap()
                .design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(
            bytes(&reopened.current_design_results().unwrap().member_checks),
            bytes(&original_bundle.member_checks)
        );
        assert_eq!(bytes(&reopened.core.model), input);
        reopened.save_project_to(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        let mut recovered = App::default();
        recovered.open_project_from(path.clone());
        assert_checked(&recovered);
        assert_eq!(bytes(&recovered.core.model), input);
        assert_eq!(
            recovered
                .current_design_results()
                .unwrap()
                .analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(
            recovered
                .current_design_results()
                .unwrap()
                .design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(
            recovered.current_static().unwrap().member_forces,
            original.current_static().unwrap().member_forces
        );
        eprintln!(
            "{name}: analysis{ANALYSIS_CONTRACT_VERSION} kept; saved design={version:?}; explicit recheck design{DESIGN_CONTRACT_VERSION}"
        );
        std::fs::remove_file(path).unwrap();
    }
}
