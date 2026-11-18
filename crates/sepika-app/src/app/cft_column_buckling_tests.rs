use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::CheckOutcome;

#[path = "../../../sepika-design-jp/tests/fixtures/cft_column_buckling_model.rs"]
pub(crate) mod input;

#[test]
fn cft_column_gui_reports_missing_axis_and_expires_older_saved_design() {
    let mut app = App::default();
    let mut model = input::cantilever(false, 3_700_000.0);
    model.steel_design_attrs = vec![input::direct_lengths(ElemId(0), Some(6000.0), None)];
    assert!(app.load_model(model));
    app.run_linear_static(LoadCaseId(1));
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let bundle = app.current_design_results().unwrap();
    let assert_missing = |outcome: &CheckOutcome| {
        assert!(
            matches!(outcome, CheckOutcome::Skipped { reason }
            if reason.contains("CFT柱 ID 0") && reason.contains("弱軸の座屈長さ")),
            "{outcome:?}"
        );
    };
    for position in &bundle.member_checks[0].positions {
        assert_missing(&position.outcome);
    }
    let original_member_forces = bundle.member_forces.clone();
    let err = app.compute_cft_ultimate_checks().unwrap_err();
    assert!(err.contains("弱軸の座屈長さ"), "{err}");

    for version in [58, 59, DESIGN_CONTRACT_VERSION] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("cft-column-buckling-{version}"),
        );
        std::fs::remove_file(path).unwrap();
        let restored = loaded.core.scoped.results.as_ref().unwrap();
        assert!(restored.has_analysis_results());
        assert_eq!(restored.member_forces, original_member_forces);
        if version != DESIGN_CONTRACT_VERSION {
            assert!(loaded.current_design_results().is_none());
            assert!(restored.member_checks.is_empty());
            loaded.run_design_check();
        }
        let current = loaded.current_design_results().unwrap();
        assert_eq!(
            current.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(current.member_checks[0].positions.len(), 3);
        for position in &current.member_checks[0].positions {
            assert_missing(&position.outcome);
        }
    }
}
