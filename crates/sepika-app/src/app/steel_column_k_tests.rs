use super::*;
use sepika_design_jp::CheckOutcome;
#[path = "../../../sepika-design-jp/tests/fixtures/steel_column_k_model.rs"]
mod input;

#[test]
fn column_k_gui_keeps_unchecked_reason_and_invalidates_saved_auto_k_contract() {
    let mut app = App::default();
    assert!(app.load_model(input::portal(1.0)));
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let expected = app.current_design_results().unwrap().member_checks.clone();
    let columns: Vec<_> = expected.iter().filter(|member| member.elem.0 < 2).collect();
    assert_eq!(columns.len(), 2);
    for column in columns {
        assert_eq!(column.positions.len(), 3);
        assert!(column.positions.iter().all(|position| matches!(&position.outcome, CheckOutcome::Skipped { reason } if reason.contains("直接指定"))));
    }
    for version in [56, sepika_core::result_contract::DESIGN_CONTRACT_VERSION] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("column-k1398-{version}"),
        );
        std::fs::remove_file(path).unwrap();
        assert!(loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .has_analysis_results());
        if version == 56 {
            assert!(loaded.current_design_results().is_none());
            loaded.run_design_check();
        }
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}
