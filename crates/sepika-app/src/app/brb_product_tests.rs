use super::*;
use sepika_core::model::{FrameSectionUse, Model};
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};

#[path = "../../../sepika-design-jp/tests/fixtures/brb_model.rs"]
mod input;

pub(crate) fn analyzed_app(length_reduction: Option<f64>) -> App {
    let mut model = input::model();
    model.brb_attrs[0].length_reduction = length_reduction;
    model.brb_attrs[0].critical_length = Some(5000.0);
    analyze(model)
}

pub(crate) fn analyzed_pipe_app(frame_use: FrameSectionUse) -> App {
    analyze(input::pipe_with_brb_attr(frame_use))
}

fn analyze(model: Model) -> App {
    let mut app = App::default();
    app.load_model(model);
    app.run_preparation();
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.current_static().is_some());
    app
}

#[test]
fn brb_product_model_and_check_survive_project_reload() {
    for (name, reduction, reason) in [
        ("missing", None, Some("端部低減距離 L1が未入力")),
        ("zero", Some(0.0), None),
        ("nan", Some(f64::NAN), Some("端部低減距離 L1には有限")),
        ("over", Some(3000.0), Some("控除後の座屈長さ L−2L1")),
    ] {
        let mut app = analyzed_app(reduction);
        let expected = &app.current_design_results().unwrap().member_checks;
        assert_eq!(expected.len(), 1, "{name}");
        assert_eq!(expected[0].elem, input::BRACE);
        assert_eq!(expected[0].positions.len(), 2, "{name}");
        for position in &expected[0].positions {
            match (&position.outcome, reason) {
                (CheckOutcome::Skipped { reason }, Some(expected)) => {
                    assert!(reason.contains(expected), "{name}: {reason}");
                }
                (CheckOutcome::Checked(result), None) => {
                    assert_eq!(result.ratio(), 1.0);
                    assert!(result.ok());
                }
                (outcome, _) => panic!("{name}: {outcome:?}"),
            }
        }
        let expected = serde_json::to_value(expected).unwrap();
        let path =
            std::env::temp_dir().join(format!("sepika-brb-{name}-{}.ovika", std::process::id()));
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
            loaded.core.model.brb_attrs[0]
                .length_reduction
                .map(f64::to_bits),
            reduction.map(f64::to_bits),
            "{name}"
        );
        assert!(loaded.current_static().is_some(), "{name}");
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            expected,
            "{name}"
        );
        loaded.run_design_check();
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            expected,
            "{name}"
        );
    }
}

#[test]
fn brb_target_mismatch_survives_reload_and_invalidates_legacy_pass() {
    for (frame_use, name) in [
        (FrameSectionUse::Column, "brb-on-column"),
        (FrameSectionUse::Girder, "brb-on-girder"),
    ] {
        let mut app = analyzed_pipe_app(frame_use);
        let elem = app.core.model.elements[0].id;
        let bundle = app.current_design_results().unwrap();
        assert_eq!(bundle.member_checks.len(), 1, "{name}");
        let checks = &bundle.member_checks[0];
        assert_eq!(checks.elem, elem);
        assert_eq!(checks.positions.len(), 3, "{name}");
        for position in &checks.positions {
            assert!(
                matches!(&position.outcome, CheckOutcome::Skipped { reason }
                    if reason.contains("BRB属性の参照先") && reason.contains("断面用途がブレース")),
                "{name}: {:?}",
                position.outcome
            );
        }
        let expected = serde_json::to_value(&bundle.member_checks).unwrap();
        let path = std::env::temp_dir().join(format!("sepika-{name}-{}.ovika", std::process::id()));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(loaded.current_static().is_some(), "{name}");
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            expected,
            "{name}"
        );

        let bundle = app.core.scoped.results.as_mut().unwrap();
        for position in &mut bundle.member_checks[0].positions {
            position.outcome = CheckOutcome::Checked(CheckResult {
                basis: "座屈拘束ブレースの製品検定".into(),
                detail: String::new(),
                components: vec![CheckComponent {
                    kind: CheckKind::Axial,
                    ratio: 0.5,
                    detail: String::new(),
                }],
            });
        }
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(69),
            &format!("{name}-old-design"),
        );
        std::fs::remove_file(path).unwrap();
        assert!(loaded.current_static().is_some(), "{name}");
        assert!(loaded.current_design_results().is_none(), "{name}");
        loaded.run_design_check();
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            expected,
            "{name}"
        );
    }
}

#[test]
fn brb_product_old_or_unversioned_checks_require_explicit_recheck() {
    let mut app = analyzed_app(None);
    let bundle = app.core.scoped.results.as_mut().unwrap();
    let expected = serde_json::to_value(&bundle.member_checks).unwrap();
    for position in &mut bundle.member_checks[0].positions {
        position.outcome = CheckOutcome::Checked(CheckResult {
            basis: "座屈補剛ブレース（メーカー許容値による検定）".into(),
            detail: String::new(),
            components: vec![CheckComponent {
                kind: CheckKind::Axial,
                ratio: 1.0,
                detail: String::new(),
            }],
        });
    }
    for (name, version) in [("brb-legacy", Some(67)), ("brb-unversioned", None)] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            version,
            name,
        );
        std::fs::remove_file(path).unwrap();
        assert!(loaded.current_static().is_some(), "{name}");
        assert!(loaded.current_design_results().is_none(), "{name}");
        loaded.run_design_check();
        let results = loaded.current_design_results().unwrap();
        assert_eq!(
            results.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(
            serde_json::to_value(&results.member_checks).unwrap(),
            expected
        );
    }
}

#[test]
fn brb_product_legacy_analysis_input_requires_new_analysis_even_at_current_version() {
    let mut app = analyzed_app(Some(0.0));
    let attr = &app.core.model.brb_attrs[0];
    let current_attr = bincode::serialize(attr).unwrap();
    let legacy_attr = bincode::serialize(&(
        attr.elem,
        attr.allowable_axial_short.unwrap(),
        attr.critical_length.unwrap(),
        attr.length_reduction.unwrap(),
    ))
    .unwrap();
    let bundle = app.core.scoped.results.as_mut().unwrap();
    let (_, recorded) = bundle
        .input_records
        .iter_mut()
        .find(|(key, _)| matches!(key, ResultInputKey::Static(_)))
        .unwrap();
    let starts: Vec<_> = recorded
        .windows(current_attr.len())
        .enumerate()
        .filter_map(|(i, bytes)| (bytes == current_attr).then_some(i))
        .collect();
    let [start] = starts.as_slice() else {
        panic!("BRBの生成入力を一意に特定できません: {starts:?}");
    };
    recorded.splice(*start..*start + current_attr.len(), legacy_attr);

    let (mut loaded, path) = super::result_contract_tests::restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(DESIGN_CONTRACT_VERSION),
        "brb-legacy-input",
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.core.model.brb_attrs, app.core.model.brb_attrs);
    assert!(loaded.current_static().is_none());
    assert!(loaded.current_design_results().is_none());
    loaded.run_linear_static(LoadCaseId(0));
    assert!(loaded.current_static().is_some());
    assert!(loaded.current_design_results().is_some());
}
