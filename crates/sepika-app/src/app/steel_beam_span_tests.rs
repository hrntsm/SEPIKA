use super::shear_area_acceptance_tests::h_column_input as combined_column;
use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{CheckKind, CheckOutcome};

#[path = "../../../sepika-design-jp/tests/fixtures/steel_beam_span_model.rs"]
pub(crate) mod input;

fn assert_short_bending_failure(app: &App) {
    let result = app
        .current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .find(|member| member.elem == input::BEAM)
        .unwrap();
    let peak = result
        .positions
        .iter()
        .find(|position| position.xi == 0.25)
        .unwrap();
    let CheckOutcome::Checked(checked) = &peak.outcome else {
        panic!("{:?}", peak.outcome);
    };
    let bending = checked
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Bending)
        .unwrap();
    assert!((bending.ratio - 1.195_894_596).abs() < 1e-8, "{bending:?}");
    assert!(!checked.ok());
}

#[test]
fn steel_beam_span_short_ng_survives_save_and_old_design_recheck_without_reanalysis() {
    let mut app = App::default();
    assert!(app.load_model(input::short_point_combination()));
    let combo_index = app
        .core
        .model
        .combinations
        .iter()
        .position(|combo| combo.name == "G+P+S")
        .unwrap();
    app.run_combination(combo_index);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert_short_bending_failure(&app);

    for version in [113, DESIGN_CONTRACT_VERSION] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("steel-beam-span-{version}"),
        );
        std::fs::remove_file(path).unwrap();
        let analysis = &loaded.core.scoped.results.as_ref().unwrap().member_forces;
        assert_eq!(
            *analysis,
            app.core.scoped.results.as_ref().unwrap().member_forces
        );
        if version != DESIGN_CONTRACT_VERSION {
            assert!(loaded.current_design_results().is_none());
            loaded.run_design_check();
            assert!(
                loaded.core.scoped.last_error.is_none(),
                "{:?}",
                loaded.core.scoped.last_error
            );
        }
        assert_short_bending_failure(&loaded);
        assert_eq!(
            loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
    }
}

#[test]
fn steel_beam_middle_brace_preserves_distinct_checks_at_the_same_position_after_old_design_recheck()
{
    let mut app = App::default();
    assert!(app.load_model(input::simply_supported_with_middle_brace(
        2_000.0, 150_000.0
    )));
    app.run_linear_static(input::DEAD);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let assert_bending = |app: &App| {
        let member = app
            .current_design_results()
            .unwrap()
            .member_checks
            .iter()
            .find(|member| member.elem == input::BEAM)
            .unwrap();
        let ratios = |xi: f64| {
            let mut ratios: Vec<_> = member
                .positions
                .iter()
                .filter(|position| position.xi == xi)
                .filter_map(|p| match &p.outcome {
                    CheckOutcome::Checked(check) => check
                        .components
                        .iter()
                        .find(|c| c.kind == CheckKind::Bending)
                        .map(|c| c.ratio),
                    _ => None,
                })
                .collect();
            ratios.sort_by(f64::total_cmp);
            ratios
        };
        let at_point = ratios(0.25);
        let at_brace = ratios(0.5);
        assert_eq!(at_point.len(), 2, "{member:?}");
        assert_eq!(at_brace.len(), 2, "{member:?}");
        assert!((at_point[0] - 1.743_795_729).abs() < 1e-8, "{at_point:?}");
        assert!((at_brace[0] - 0.903_524_681).abs() < 1e-8, "{at_brace:?}");
        assert!((at_brace[1] - 1.162_530_486).abs() < 1e-8, "{at_brace:?}");
    };
    assert_bending(&app);

    for version in [DESIGN_CONTRACT_VERSION - 1, DESIGN_CONTRACT_VERSION] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("steel-beam-middle-brace-{version}"),
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            loaded.core.scoped.results.as_ref().unwrap().member_forces,
            app.core.scoped.results.as_ref().unwrap().member_forces
        );
        if version != DESIGN_CONTRACT_VERSION {
            assert!(loaded.current_design_results().is_none());
            loaded.run_design_check();
            assert!(
                loaded.core.scoped.last_error.is_none(),
                "{:?}",
                loaded.core.scoped.last_error
            );
        }
        assert_bending(&loaded);
        assert_eq!(
            loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
    }
}
