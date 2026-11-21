use super::*;
use sepika_design_jp::{CheckKind, CheckOutcome};
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_model.rs"]
pub(crate) mod input;
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_reference.rs"]
pub(crate) mod reference;

#[test]
fn rc_column_boundary_gui_analysis_save_and_old_contract_preserve_unchecked_state() {
    for circle in [false, true] {
        let mut app = App::default();
        let mut model = input::column_model(circle, 1.01 * reference::axial_limit_n(circle));
        model.load_cases[0].nodal[0].values[0] = 1_000_000.0;
        assert!(app.load_model(model));
        app.run_preparation();
        app.run_linear_static(LoadCaseId(0));
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let expected = app.current_design_results().unwrap().member_checks.clone();
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].positions.len(), 6);
        for xi in [0.0, 0.5, 1.0] {
            let at_xi = || {
                expected[0]
                    .positions
                    .iter()
                    .filter(|position| position.xi == xi)
            };
            assert!(at_xi().any(|position| matches!(&position.outcome, CheckOutcome::Skipped { reason } if reason.contains("検定不能") && reason.contains("MA_y=0"))));
            assert!(at_xi().any(|position| matches!(&position.outcome, CheckOutcome::Checked(result) if !result.ok() && result.components.len() == 1 && result.components[0].kind == CheckKind::Shear)));
        }
        let path =
            std::env::temp_dir().join(format!("sepika1803-{circle}-{}.ovika", std::process::id()));
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
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        let (mut stale, path) = super::result_contract_tests::restore(
            &app,
            Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
            Some(100),
            &format!("rc-column1803-old-{circle}"),
        );
        std::fs::remove_file(path).unwrap();
        assert!(stale
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .has_analysis_results());
        assert!(stale.current_design_results().is_none());
        stale.run_design_check();
        assert_eq!(
            serde_json::to_value(&stale.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}

#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_tension_model.rs"]
mod tension_input;
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_tension_reference.rs"]
mod tension_reference;

#[test]
fn rc_column_tension_gui_preserves_ng_and_invalidates_previous_contract() {
    for circle in [false, true] {
        let mut app = App::default();
        app.load_model(tension_input::tension_model(
            input::column_model(circle, 0.0),
            1_000_000.0,
            [0.0, 0.0],
        ));
        app.run_preparation();
        app.run_linear_static(LoadCaseId(0));
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let expected = app.current_design_results().unwrap().member_checks.clone();
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].positions.len(), 3);
        for position in &expected[0].positions {
            let CheckOutcome::Checked(result) = &position.outcome else {
                panic!("{position:?}")
            };
            assert!(!result.ok());
            assert!(result.ratio() > 2.0);
            assert!(result.ratio() <= 1_000_000.0 / tension_reference::tension_limit_n(true));
        }
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
            Some(sepika_core::result_contract::DESIGN_CONTRACT_VERSION),
            &format!("rc-column1393-current-{circle}"),
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        loaded.run_design_check();
        let (mut stale, path) = super::result_contract_tests::restore(
            &app,
            Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
            Some(55),
            &format!("rc-column1393-old-{circle}"),
        );
        std::fs::remove_file(path).unwrap();
        assert!(stale
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .has_analysis_results());
        assert!(stale.current_design_results().is_none());
        stale.run_design_check();
        assert_eq!(
            serde_json::to_value(&stale.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}
