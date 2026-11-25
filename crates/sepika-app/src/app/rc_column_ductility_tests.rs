use super::*;
use sepika_core::model::{LoadCaseKind, LoadCombination};
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{RcColumnDuctilityApplicability, RcColumnDuctilityComparison};

fn short_column_app() -> App {
    let mut model = super::rc_column_boundary_tests::input::column_model(false, 700_000.0);
    let mut wind = model.load_cases[0].clone();
    wind.id = LoadCaseId(1);
    wind.name = "風".into();
    wind.kind = LoadCaseKind::Wind;
    wind.nodal[0].values = [500.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let mut live = wind.clone();
    live.id = LoadCaseId(2);
    live.name = "架構用積載".into();
    live.kind = LoadCaseKind::Live;
    live.nodal.clear();
    model.load_cases.extend([wind, live]);
    model.combinations.push(LoadCombination {
        name: "G+P+W".into(),
        terms: vec![
            (LoadCaseId(0), 1.0),
            (LoadCaseId(2), 1.0),
            (LoadCaseId(1), 1.0),
        ],
    });
    let mut app = App::default();
    assert!(app.load_model(model));
    app.run_combination(0);
    assert_eq!(
        app.selected_design_load_state().unwrap().duration,
        sepika_core::load_combo::LoadDuration::Short
    );
    assert!(
        app.current_design_results().is_some(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}

fn different_governing_positions(app: &mut App) {
    let bundle = app.core.scoped.results.as_mut().unwrap();
    for forces in [
        &mut bundle.member_forces,
        &mut bundle.combos[0].1.member_forces,
    ] {
        for (xi, station) in &mut forces[0].1.at {
            *station = if *xi == 0.0 {
                [-500_000.0, 0.0, 0.0, 0.0, 0.0, 600_000_000.0]
            } else if *xi == 1.0 {
                [-3_000_000.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            } else {
                [f64::NAN, 0.0, 0.0, 0.0, 0.0, 0.0]
            };
        }
    }
    app.run_design_check();
    assert!(
        app.current_design_results().is_some(),
        "{:?}",
        app.core.scoped.last_error
    );
}

#[test]
fn short_compression_reference_survives_ovika_without_becoming_the_governing_strength_check() {
    let mut app = short_column_app();
    different_governing_positions(&mut app);
    let bundle = app.current_design_results().unwrap();
    let expected_checks = serde_json::to_value(&bundle.member_checks).unwrap();
    let expected_forces = rmp_serde::to_vec_named(&bundle.member_forces).unwrap();
    let expected = bundle.rc_column_ductility_advisories.clone();
    let [column] = bundle.member_checks.as_slice() else {
        panic!("{bundle:?}")
    };
    let governing = column
        .positions
        .iter()
        .filter_map(|row| match &row.outcome {
            sepika_design_jp::CheckOutcome::Checked(result) => Some((row.xi, result.ratio())),
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    assert_eq!(governing.0, 0.0);
    let [advisory] = expected.as_slice() else {
        panic!("{expected:?}")
    };
    assert_eq!(advisory.elem, column.elem);
    assert_eq!(
        advisory.applicability,
        RcColumnDuctilityApplicability::Undetermined
    );
    assert_eq!(
        advisory.positions[1],
        (0.5, RcColumnDuctilityComparison::Unavailable)
    );
    assert_eq!(
        advisory.positions[2],
        (
            1.0,
            RcColumnDuctilityComparison::Compared {
                compression_n_per_mm2: 12.5,
                standard_n_per_mm2: 8.0,
                exceeds_standard: true,
            }
        )
    );

    let path =
        std::env::temp_dir().join(format!("sepika-1392-current-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    let bundle = loaded.current_design_results().unwrap();
    assert_eq!(bundle.rc_column_ductility_advisories, expected);
    assert_eq!(
        serde_json::to_value(&bundle.member_checks).unwrap(),
        expected_checks
    );

    let (mut old_results, path) = super::result_contract_tests::restore(
        &app,
        Some(ANALYSIS_CONTRACT_VERSION),
        Some(DESIGN_CONTRACT_VERSION - 1),
        "rc-column-ductility",
    );
    std::fs::remove_file(path).unwrap();
    assert!(old_results.require_selected_static_input().is_ok());
    assert!(old_results.current_design_results().is_none());
    assert_eq!(
        rmp_serde::to_vec_named(
            &old_results
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .member_forces
        )
        .unwrap(),
        expected_forces,
    );
    old_results.run_design_check();
    let rechecked = old_results.current_design_results().unwrap();
    assert_eq!(rechecked.rc_column_ductility_advisories, expected);
    assert_eq!(
        serde_json::to_value(&rechecked.member_checks).unwrap(),
        expected_checks
    );
}

#[cfg(feature = "gui")]
#[test]
fn normal_design_table_surfaces_the_reference_excess_and_missing_comparison() {
    let mut app = short_column_app();
    different_governing_positions(&mut app);
    let ctx = egui::Context::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        crate::design_view::design_table(ui, &mut app)
    });
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("標準値の数値超過 1 本")),
        "{labels:?}"
    );
    let result = &app
        .current_design_results()
        .unwrap()
        .rc_column_ductility_advisories[0];
    let text = crate::design_view::rc_column_ductility_summary(result);
    assert!(
        text.contains("適用判別不能") && text.contains("標準値との参考比較"),
        "{text}"
    );
    assert!(
        text.contains("12.500 N/mm²（位置 1.000000）") && text.contains("標準値超過"),
        "{text}"
    );
    assert!(text.contains("比較できない位置 1 箇所"), "{text}");
    assert!(!text.contains("OK") && !text.contains("NG"), "{text}");
}
