use super::shear_area_acceptance_tests::h_column_input as combined_column;
use super::*;
use sepika_core::ids::LoadCaseId;
use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination, LL_FRAME_CASE_NAME};
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{CheckKind, CheckOutcome};

#[path = "../../../sepika-design-jp/tests/fixtures/steel_column_span_model.rs"]
mod input;
#[path = "../../../sepika-design-jp/tests/fixtures/steel_column_lateral_buckling_model.rs"]
#[allow(dead_code)]
pub(crate) mod support_column;

fn assert_off_center_result(app: &App) {
    let result = app
        .current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .find(|row| row.elem == app.core.model.elements[0].id)
        .unwrap();
    let rows = result
        .positions
        .iter()
        .filter(|row| (row.xi - 1.0 / 3.0).abs() < 1e-12)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "{result:?}");
    for (row, side) in rows.into_iter().zip(["左側", "右側"]) {
        let CheckOutcome::Checked(result) = &row.outcome else {
            panic!("{row:?}");
        };
        assert!(!result.ok(), "{result:?}");
        assert!(result.detail.contains(side), "{result:?}");
        assert!(result.detail.contains("C=1.0000"), "{result:?}");
        let bending = result
            .components
            .iter()
            .find(|c| c.kind == CheckKind::AxialBending)
            .unwrap();
        assert!(
            (bending.ratio - 1.131_706_715_914_949_4).abs() < 1e-8,
            "{bending:?}"
        );
    }
}

#[test]
fn steel_column_span_short_combination_rechecks_old_saved_design_without_reanalysis() {
    let mut model = input::column(125_000.0, -135_000.0);
    let member_loads = std::mem::take(&mut model.load_cases[0].member);
    let snow = LoadCaseId(11);
    let live = LoadCaseId(12);
    model.load_cases.extend([
        LoadCase {
            id: snow,
            name: "積雪試験荷重".into(),
            kind: LoadCaseKind::Snow,
            origin: Default::default(),
            nodal: vec![],
            member: member_loads,
        },
        LoadCase {
            id: live,
            name: LL_FRAME_CASE_NAME.into(),
            kind: LoadCaseKind::Live,
            origin: Default::default(),
            nodal: vec![],
            member: vec![],
        },
    ]);
    model.combinations = vec![LoadCombination {
        name: "G+P+S".into(),
        terms: vec![(input::CASE, 1.0), (live, 1.0), (snow, 1.0)],
    }];

    let mut app = App::default();
    assert!(app.load_model(model));
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
    assert_off_center_result(&app);

    for version in [128, DESIGN_CONTRACT_VERSION] {
        let (mut restored, path) = super::result_contract_tests::restore(
            &app,
            Some(ANALYSIS_CONTRACT_VERSION),
            Some(version),
            &format!("steel-column-span-{version}"),
        );
        std::fs::remove_file(path).unwrap();
        let expected_analysis = &app.core.scoped.results.as_ref().unwrap().member_forces;
        assert_eq!(
            &restored.core.scoped.results.as_ref().unwrap().member_forces,
            expected_analysis
        );
        if version != DESIGN_CONTRACT_VERSION {
            assert!(restored.current_design_results().is_none());
            restored.run_design_check();
            assert!(
                restored.core.scoped.last_error.is_none(),
                "{:?}",
                restored.core.scoped.last_error
            );
        }
        assert_off_center_result(&restored);
        let bundle = restored.core.scoped.results.as_ref().unwrap();
        assert_eq!(&bundle.member_forces, expected_analysis);
        assert_eq!(
            bundle.design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
    }
}
