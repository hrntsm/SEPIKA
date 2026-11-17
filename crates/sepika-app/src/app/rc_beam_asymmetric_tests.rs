use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};

#[path = "../../../sepika-design-jp/tests/fixtures/rc_beam_asymmetric_model.rs"]
mod input;

#[test]
fn rc_beam_asymmetric_gui_rechecks_legacy_capacity_and_preserves_unchecked_reason() {
    let mut app = App::default();
    app.load_model(input::model(true));
    app.run_preparation();
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let forces = &app.current_static().unwrap().member_forces[0].1.at;
    assert!(
        forces.iter().all(|(_, f)| (f[5] - 45e6).abs() < 1e-6),
        "{forces:?}"
    );

    let expected = &app.current_design_results().unwrap().member_checks;
    assert_eq!(expected.len(), 1);
    assert_eq!(expected[0].positions.len(), 3);
    for position in &expected[0].positions {
        assert!(
            matches!(&position.outcome, CheckOutcome::Skipped { reason } if reason.contains("正曲げ") && reason.contains("重心までの距離が一致しません"))
        );
    }
    let expected = serde_json::to_value(expected).unwrap();

    let path =
        std::env::temp_dir().join(format!("sepika-1388-current-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
        expected
    );

    let bundle = app.core.scoped.results.as_mut().unwrap();
    assert_eq!(
        bundle.design_contract_version,
        Some(DESIGN_CONTRACT_VERSION)
    );
    let legacy = CheckResult {
        basis: "RC 規準13条".into(),
        detail: String::new(),
        components: vec![CheckComponent {
            kind: CheckKind::Bending,
            ratio: 45e6 / 45_198_823.354_266,
            detail: "MA=45198823.4 N·mm".into(),
        }],
    };
    assert!(legacy.ok());
    for position in &mut bundle.member_checks[0].positions {
        position.outcome = CheckOutcome::Checked(legacy.clone());
    }
    let (mut stale, path) = super::result_contract_tests::restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(57),
        "rc-beam-1388",
    );
    std::fs::remove_file(path).unwrap();
    assert!(stale.current_static().is_some());
    assert!(stale.current_design_results().is_none());
    stale.run_design_check();
    assert_eq!(
        serde_json::to_value(&stale.current_design_results().unwrap().member_checks).unwrap(),
        expected
    );
}
