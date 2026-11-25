use super::src_column_route21::{Route21Axis, Route21Input, Route21Rc};
use super::*;
use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination, NodalLoad};
use sepika_design_jp::{CheckKind, CheckOutcome, QdMethod};

use super::src_ql_acceptance_tests::steel_product_input;

const COLUMN: ElemId = ElemId(0);

fn ready() -> App {
    let mut model = steel_product_input::model(true, "SN400B", 0.0, 0.0);
    model.load_cases.push(LoadCase {
        id: LoadCaseId(2),
        name: "地震X".into(),
        kind: LoadCaseKind::Seismic,
        origin: Default::default(),
        member: Vec::new(),
        nodal: vec![NodalLoad::manual(
            NodeId(1),
            [20_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )],
    });
    for (name, sign) in [("短期地震X+", 1.0), ("短期地震X-", -1.0)] {
        model.combinations.push(LoadCombination {
            name: name.into(),
            terms: vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), sign),
            ],
        });
    }

    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.threads = 1;
    app.run_combination(1);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}

fn confirmed() -> Route21Input {
    Route21Input {
        clear_height_mm: 6_000.0,
        axes: [
            Some(Route21Axis {
                positive: true,
                steel_n_mm: [120_000_000.0, 180_000_000.0],
                rc: Route21Rc::PrimaryEarthquake { shear_n: 75_000.0 },
                evidence: "外部計算書 A-1".into(),
            }),
            Some(Route21Axis {
                positive: false,
                steel_n_mm: [24_000_000.0, 36_000_000.0],
                rc: Route21Rc::Bending {
                    ultimate_n_mm: [90_000_000.0, 30_000_000.0],
                    connected_beams: None,
                },
                evidence: "外部計算書 A-2".into(),
            }),
        ],
    }
}

fn outcomes(app: &App) -> impl Iterator<Item = &CheckOutcome> {
    app.current_design_results()
        .expect("現行検定")
        .member_checks
        .iter()
        .filter(|member| member.elem == COLUMN)
        .flat_map(|member| member.positions.iter().map(|p| &p.outcome))
}

fn assert_confirmed(app: &App) {
    let checked: Vec<_> = outcomes(app)
        .filter_map(|row| match row {
            CheckOutcome::Checked(check) => Some(check),
            _ => None,
        })
        .collect();
    assert!(!checked.is_empty());
    for row in checked {
        let shear = row
            .components
            .iter()
            .find(|component| component.kind == CheckKind::Shear)
            .unwrap();
        for needle in [
            "Qy方向 鉄骨 sQD=5e4 N",
            "RC rQD=5e4 N",
            "採用方法=代替式（Qd2",
            "局所加力=正",
            "根拠=外部計算書 A-1",
            "Qz方向 鉄骨 sQD=1e4 N",
            "RC rQD=2e4 N",
            "採用方法=曲げ方式（Qd1",
            "局所加力=負",
            "根拠=外部計算書 A-2",
        ] {
            assert!(shear.detail.contains(needle), "{needle}: {}", shear.detail);
        }
        assert!(row.ok(), "{row:?}");
    }
}

fn assert_route21_not_confirmed(app: &App) {
    let rows: Vec<_> = outcomes(app).collect();
    assert!(!rows.is_empty());
    assert!(rows
        .iter()
        .all(|outcome| !matches!(outcome, CheckOutcome::Checked(r) if r.ok())));
}

#[test]
fn src_route21_saved_registered_demand_and_new_analysis_are_distinguished() {
    let mut app = ready();
    assert_route21_not_confirmed(&app);
    let source = app.selected_route21_source().unwrap();
    app.register_route21(source.clone(), COLUMN, confirmed())
        .unwrap();
    assert!(app.current_design_results().is_none());
    app.run_design_check();
    assert_confirmed(&app);

    let path =
        std::env::temp_dir().join(format!("sepika-src-route21-{}.ovika", uuid::Uuid::new_v4()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.selected_route21_source().unwrap(), source);
    assert_confirmed(&loaded);

    loaded.run_combination(1);
    assert_ne!(loaded.selected_route21_source().unwrap(), source);
    assert_route21_not_confirmed(&loaded);
    assert!(loaded
        .register_route21(source, COLUMN, confirmed())
        .is_err());
}

#[test]
fn src_route21_registration_does_not_cross_combination_or_changed_design_conditions() {
    let mut app = ready();
    let source = app.selected_route21_source().unwrap();
    app.register_route21(source.clone(), COLUMN, confirmed())
        .unwrap();
    app.run_design_check();
    assert_confirmed(&app);

    let mut cfg = app.core.analysis_cfg;
    cfg.qd_method = if cfg.qd_method == QdMethod::Min {
        QdMethod::Qd1
    } else {
        QdMethod::Min
    };
    assert!(app.commit_analysis_settings(cfg));
    assert!(app.current_design_results().is_none());
    assert!(app.register_route21(source, COLUMN, confirmed()).is_err());
    app.run_design_check();
    assert_route21_not_confirmed(&app);

    app.run_combination(2);
    assert_eq!(app.selected_route21_source().unwrap().combo, "短期地震X-");
    assert_route21_not_confirmed(&app);
}

#[test]
fn src_route21_missing_axis_keeps_explicit_incomplete_reason() {
    let mut app = ready();
    let mut input = confirmed();
    input.axes[1] = None;
    app.register_route21(app.selected_route21_source().unwrap(), COLUMN, input)
        .unwrap();
    app.run_design_check();
    assert_route21_not_confirmed(&app);
    for row in outcomes(&app) {
        let CheckOutcome::Skipped { reason } = row else {
            panic!("{row:?}");
        };
        for needle in [
            "Qz方向のルート2-1の確認済み入力がありません",
            "せん断の確認結果",
            "Qy方向 鉄骨 sQD=5e4 N, sQA=",
            "sQD/sQA=",
            "RC rQD=5e4 N, rQA=",
            "rQD/rQA=",
            "採用方法=代替式（Qd2",
            "局所加力=正",
            "根拠=外部計算書 A-1",
        ] {
            assert!(reason.contains(needle), "{needle}: {reason}");
        }
    }
}
