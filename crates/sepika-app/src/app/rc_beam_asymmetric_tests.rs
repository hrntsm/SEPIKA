use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};

#[path = "../../../sepika-design-jp/tests/fixtures/rc_beam_asymmetric_model.rs"]
pub(super) mod input;

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

#[test]
fn rc_rebar_sizes_are_retained_in_recorded_calculation_inputs() {
    use crate::app::references::{DisplayConditions, RecordedSettings, ReferenceTarget};
    use sepika_core::section_shape::{RebarSize, StandardRebar};
    use sepika_solver::dynamic::timehistory::GroundMotion;

    for expected in [
        (
            RebarSize::Standard(StandardRebar::D19),
            RebarSize::Standard(StandardRebar::D10),
        ),
        (RebarSize::DiameterMm(19.0), RebarSize::DiameterMm(10.0)),
    ] {
        let mut app = App::default();
        let mut model = input::model(false);
        let Some(sepika_core::section_shape::SectionShape::RcBeamRect { rebar, .. }) =
            model.sections[0].shape.as_mut()
        else {
            unreachable!()
        };
        (rebar.main_dia, rebar.stirrup.dia) = expected;
        assert!(app.load_model(model));
        app.run_preparation();
        app.run_linear_static(LoadCaseId(0));
        assert!(
            app.current_design_results().is_some(),
            "{:?}",
            app.core.scoped.last_error
        );
        let static_key = ResultInputKey::Static(StaticCaseKey::User(LoadCaseId(0)));
        let target = app
            .target_reference(ReferenceTarget::Member(ElemId(0)))
            .unwrap();
        let reference = app
            .result_reference(target, static_key.clone(), DisplayConditions::default())
            .unwrap();

        let input = app
            .time_history_input(
                GroundMotion {
                    dt: 0.02,
                    accel_x: vec![0.0, 10.0, -10.0],
                    accel_y: None,
                    accel_theta: None,
                },
                "入力波".into(),
            )
            .unwrap();
        app.apply_time_history_result(
            Ok(super::tests::dummy_th(vec![0.0, 0.02, 0.04])),
            input.source,
        );
        let vibration_key = ResultInputKey::TimeHistory(app.core.model.vibration_cases[0].id);

        let path =
            std::env::temp_dir().join(format!("sepika-1584-input-{}.ovika", uuid::Uuid::new_v4()));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(loaded.resolve_result_reference(&reference).is_ok());
        let assert_sizes = |model: &sepika_core::model::Model| {
            assert_eq!(
                model.sections[0].shape.as_ref().unwrap().rebar_sizes(),
                Some(expected)
            );
        };
        assert_sizes(&loaded.result_input_snapshot(&reference).unwrap());
        let (_, design) = loaded
            .recorded_calculation_input(&ResultInputKey::Design)
            .unwrap();
        assert_sizes(&design.model);
        let RecordedSettings::Design {
            source: Some((source_key, Some(source))),
            ..
        } = design.settings
        else {
            panic!("設計用の解析条件を記録から復元できません");
        };
        assert_eq!(source_key, static_key);
        assert_sizes(&source.model);

        let (_, vibration) = loaded.recorded_calculation_input(&vibration_key).unwrap();
        assert!(matches!(
            vibration.settings,
            RecordedSettings::Vibration { cfg: Some(_), .. }
        ));
        assert_sizes(&vibration.model);
    }
}
