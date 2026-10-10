use sepika_app::app::{App, StaticKey};
use sepika_core::ids::LoadCaseId;
use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination};

#[test]
fn load_state_csv_keeps_selected_target_terms_duration_and_floor_scope() {
    let mut model = sepika_app::sample::portal_frame();
    model.load_cases.push(LoadCase {
        id: LoadCaseId(2),
        name: "P,空".into(),
        kind: LoadCaseKind::Live,
        nodal: vec![],
        member: vec![],
    });
    model.combinations = vec![LoadCombination {
        name: "常時,\"任意名\"".into(),
        terms: vec![
            (LoadCaseId(0), 1.0),
            (LoadCaseId(2), 1.0),
            (LoadCaseId(1), -1.0),
        ],
    }];
    let mut app = App::default();
    app.core.model = model;
    app.run_combination(0);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.select_displayed_result(StaticKey::Combo(0));
    let csv = sepika_app::summary::build_report_csv(&app);
    for expected in [
        "[検定荷重状態]",
        "保存組合せ",
        "短期",
        "地震",
        "\"常時,\"\"任意名\"\"\"",
        "[検定荷重項]",
        "2,\"P,空\",Live,1",
        "1,\"地震X\",Seismic,-1",
        "[小梁・床検定範囲]",
        "長期略算",
        "選択短期等は未検定",
        "法的全条件,未確認",
    ] {
        assert!(csv.contains(expected), "{expected}: {csv}");
    }
    let path = std::env::temp_dir().join(format!("sepika487-csv-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    let mut reopened = App::default();
    reopened.open_project_from(path);
    assert!(reopened.core.scoped.last_error.is_none());
    assert_eq!(sepika_app::summary::build_report_csv(&reopened), csv);
    app.core.model.combinations[0].terms.pop();
    app.run_combination(0);
    assert!(app.core.scoped.last_error.is_none());
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(csv.contains("長期,常時"), "{csv}");
    app.core.model.combinations[0].terms[0].1 = 1.2;
    app.run_combination(0);
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(csv.contains("未判定"), "{csv}");
    assert!(csv.contains("1.2"), "{csv}");
    app.run_linear_static(LoadCaseId(1));
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(csv.contains("単独ケース"), "{csv}");
    assert!(csv.contains("法令組合せ未検定"), "{csv}");
}
