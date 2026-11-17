use super::*;
use sepika_design_jp::CheckOutcome;
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_model.rs"]
mod input;
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_reference.rs"]
mod reference;

#[test]
fn rc_column_boundary_gui_analysis_save_and_old_contract_preserve_unchecked_state() {
    for circle in [false, true] {
        let mut app = App::default();
        app.load_model(input::column_model(
            circle,
            1.01 * reference::axial_limit_n(circle),
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
        assert!(expected[0].positions.iter().all(|position| matches!(&position.outcome, CheckOutcome::Skipped { reason } if reason.contains("検定不能") && reason.contains("MA_y=0"))));
        let json = serde_json::to_string(&expected).unwrap();
        assert!(!json.contains("Checked"));
        assert!(json.contains("Skipped"));
        let path =
            std::env::temp_dir().join(format!("sepika1374-{circle}-{}.ovika", std::process::id()));
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
            Some(50),
            &format!("rc-column1374-old-{circle}"),
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
