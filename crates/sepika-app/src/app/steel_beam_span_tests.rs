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
