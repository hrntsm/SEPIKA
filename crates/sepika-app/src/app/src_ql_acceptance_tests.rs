use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::{CheckOutcome, QdMethod};

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    rmp_serde::to_vec_named(value).unwrap()
}

pub(crate) fn src_beam_app(empty_gravity: bool, method: QdMethod) -> App {
    let mut model: sepika_core::model::Model =
        serde_json::from_str(include_str!("../../tests/fixtures/src_ql_contract.json")).unwrap();
    if empty_gravity {
        model.load_cases[0].member.clear();
    }
    let mut app = App::default();
    assert!(app.load_model(model));
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.qd_method = method;
    app
}

fn assert_checked(app: &App) {
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.require_selected_static_input().is_ok());
    let bundle = app.current_design_results().expect("現行検定");
    let positions = &bundle.member_checks[0].positions;
    assert_eq!(positions.len(), 3);
    assert!(positions
        .iter()
        .all(|p| matches!(&p.outcome, CheckOutcome::Checked(c) if c.ok())));
    let csv = crate::summary::build_report_csv(app);
    assert!(
        csv.lines()
            .any(|line| line.starts_with("0,") && line.contains(",OK,")),
        "{csv}"
    );
}

fn src_pure_bending_app(method: QdMethod) -> App {
    let mut app = src_beam_app(true, method);
    app.core.model.load_cases[2].kind = sepika_core::model::LoadCaseKind::Wind;
    app.core.model.load_cases[2].nodal[0].values = [0.0, 0.0, 0.0, 0.0, -10_000.0, 0.0];
    app
}

#[test]
fn public_src_beam_ordinary_short_bending_does_not_use_the_qd_setting() {
    let mut baseline = src_pure_bending_app(QdMethod::Qd1);
    baseline.run_combination(0);
    baseline.run_design_check();
    assert_checked(&baseline);
    let csv = crate::summary::build_report_csv(&baseline);
    assert!(csv.contains("強軸曲げのみ"));
    let checks = bytes(&baseline.current_design_results().unwrap().member_checks);
    for method in [QdMethod::Qd2, QdMethod::Min] {
        let mut app = src_pure_bending_app(method);
        app.run_combination(0);
        app.run_design_check();
        assert_checked(&app);
        assert_eq!(
            bytes(&app.current_design_results().unwrap().member_checks),
            checks
        );
    }
}

#[test]
fn public_src_beam_does_not_pass_when_a_nonzero_shear_has_no_bending_split() {
    let mut app = src_beam_app(false, QdMethod::Qd1);
    app.run_combination(0);
    app.run_design_check();
    let bundle = app.current_design_results().expect("現行検定");
    let positions = &bundle.member_checks[0].positions;
    assert!(positions.iter().any(|row| {
        matches!(&row.outcome, CheckOutcome::Skipped { reason } if reason.contains("地震時の設計用せん断力"))
    }));
}

#[test]
fn src_ovika_design_contracts_keep_current_analysis_and_require_explicit_recheck() {
    let mut original = src_pure_bending_app(QdMethod::Qd1);
    original.run_combination(0);
    assert_checked(&original);
    let input = bytes(&original.core.model);
    let original_bundle = original.core.scoped.results.as_ref().unwrap();
    for (name, version, current) in [
        ("src-missing", None, false),
        ("src-old14", Some(14), false),
        ("src-old15", Some(15), false),
        ("src-old16", Some(16), false),
        ("src-old17", Some(17), false),
        ("src-old18", Some(18), false),
        ("src-old35", Some(35), false),
        ("src-old36", Some(36), false),
        ("src-old89", Some(89), false),
        ("src-current", Some(DESIGN_CONTRACT_VERSION), true),
        ("src-future", Some(DESIGN_CONTRACT_VERSION + 1), false),
    ] {
        let (mut loaded, path) = super::result_contract_tests::restore(
            &original,
            Some(ANALYSIS_CONTRACT_VERSION),
            version,
            name,
        );
        assert_eq!(bytes(&loaded.core.model), input);
        assert!(loaded.require_selected_static_input().is_ok());
        assert!(!loaded.core.scoped.staleness.results_stale);
        let bundle = loaded.core.scoped.results.as_ref().unwrap();
        assert_eq!(
            bundle.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(bytes(&bundle.combos), bytes(&original_bundle.combos));
        assert_eq!(bundle.member_forces, original_bundle.member_forces);
        assert_eq!(loaded.current_design_results().is_some(), current);
        assert_eq!(bundle.has_design_results(), current);
        assert_eq!(
            crate::summary::build_report_csv(&loaded).contains(",OK,"),
            current
        );
        if !current {
            assert!(bundle.member_checks.is_empty());
            loaded.select_displayed_result(original.core.scoped.last_static.unwrap());
            loaded.core.scoped.staleness.mark_fresh();
        }
        loaded.save_project_to(path.clone());
        assert!(loaded.core.scoped.last_error.is_none());
        let mut reopened = App::default();
        reopened.open_project_from(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        assert_eq!(bytes(&reopened.core.model), input);
        assert!(reopened.require_selected_static_input().is_ok());
        let saved = reopened.core.scoped.results.as_ref().unwrap();
        assert_eq!(
            saved.analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(saved.member_forces, original_bundle.member_forces);
        assert_eq!(reopened.current_design_results().is_some(), current);
        assert_eq!(
            crate::summary::build_report_csv(&reopened).contains(",OK,"),
            current
        );
        if !current {
            assert!(saved.member_checks.is_empty());
            assert_eq!(saved.design_contract_version, None);
        }
        reopened.run_design_check();
        assert_checked(&reopened);
        assert_eq!(
            reopened
                .current_design_results()
                .unwrap()
                .design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(
            bytes(&reopened.current_design_results().unwrap().member_checks),
            bytes(&original_bundle.member_checks)
        );
        assert_eq!(bytes(&reopened.core.model), input);
        reopened.save_project_to(path.clone());
        assert!(reopened.core.scoped.last_error.is_none());
        let mut recovered = App::default();
        recovered.open_project_from(path.clone());
        assert_checked(&recovered);
        assert_eq!(bytes(&recovered.core.model), input);
        assert_eq!(
            recovered
                .current_design_results()
                .unwrap()
                .analysis_contract_version,
            Some(ANALYSIS_CONTRACT_VERSION)
        );
        assert_eq!(
            recovered
                .current_design_results()
                .unwrap()
                .design_contract_version,
            Some(DESIGN_CONTRACT_VERSION)
        );
        assert_eq!(
            recovered.current_static().unwrap().member_forces,
            original.current_static().unwrap().member_forces
        );
        eprintln!(
            "{name}: analysis{ANALYSIS_CONTRACT_VERSION} kept; saved design={version:?}; explicit recheck design{DESIGN_CONTRACT_VERSION}"
        );
        std::fs::remove_file(path).unwrap();
    }
}

pub(crate) fn src_column_app(method: QdMethod) -> App {
    use sepika_core::ids::SectionId;
    use sepika_core::model::FrameSectionUse;
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};

    let mut app = src_beam_app(false, method);
    let old = app.core.model.sections[0].clone();
    let mut sec = SectionShape::SrcColumnRect {
        b: 500.0,
        d: 700.0,
        rebar: RcRectColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            x: vec![3],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
        steel_height: 500.0,
        steel_width: 200.0,
        steel_web_thick: 9.0,
        steel_flange_thick: 14.0,
    }
    .to_section(SectionId(0), "SRC柱912".into());
    sec.material = old.material;
    sec.steel_material = old.steel_material;
    sec.rebar_material = old.rebar_material;
    sec.shear_rebar_material = old.shear_rebar_material;
    sec.frame_use = Some(FrameSectionUse::Column);
    app.core.model.sections[0] = sec;
    app.core.model.steel_design_attrs = vec![sepika_core::model::SteelDesignAttr {
        elem: ElemId(0),
        joint_flange_loss: 0.0,
        joint_web_loss: 0.0,
        scallop_web_loss: 0.0,
        lb_direct: None,
        lateral_brace_count: None,
        lk_y_direct: Some(6000.0),
        lk_z_direct: Some(6000.0),
        c_direct: None,
        column_lateral_buckling_end_supports: None,
    }];
    app.core.model.nodes[1].coord = [0.0, 0.0, 6000.0];
    app.core.model.elements[0].local_axis.ref_vector = [1.0, 0.0, 0.0];
    app.core.model.stories = super::tests::explicit_story_inputs(&[0.0, 6000.0]);
    app
}

fn assert_src912_skipped(app: &App) {
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let bundle = app.current_design_results().expect("理由付き現行検定");
    assert_eq!(bundle.member_checks.len(), 1);
    assert!(!bundle.member_checks[0].positions.is_empty());
    assert!(bundle.member_checks[0].positions.iter().all(|p|
        matches!(&p.outcome, CheckOutcome::Skipped { reason } if reason.contains("適用根拠が未確定"))));
    assert!(!crate::summary::build_report_csv(app).contains(",OK,"));
}

#[test]
fn src912_material_edit_preparation_design_save_preserve_beam_and_column_skips() {
    use sepika_core::ids::MaterialId;
    use sepika_edit::{MaterialField, SetMaterialField, SetMaterialName};

    for column in [false, true] {
        for fy in [100.0, 235.0, 345.0] {
            let mut app = if column {
                src_column_app(QdMethod::Qd1)
            } else {
                src_pure_bending_app(QdMethod::Qd1)
            };
            app.run_combination(0);
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            assert!(app.current_design_results().unwrap().member_checks[0]
                .positions
                .iter()
                .all(|p| if column {
                    matches!(&p.outcome, CheckOutcome::Skipped { reason } if reason.contains("柱頭・柱脚の軸力"))
                } else {
                    matches!(&p.outcome, CheckOutcome::Checked(_))
                }));
            let known_steel_checks = app.current_design_results().unwrap().member_checks.clone();
            assert!(app.core.scoped.undo.run(
                &mut app.core.model,
                Box::new(SetMaterialName {
                    id: MaterialId(2),
                    name: "直接入力鋼材912".into(),
                })
            ));
            assert!(
                app.core.scoped.undo.run(
                    &mut app.core.model,
                    Box::new(SetMaterialField {
                        id: MaterialId(2),
                        field: MaterialField::Fy,
                        value: Some(fy),
                    })
                ) || fy == 235.0
            );
            app.core.scoped.staleness.mark_edited();
            app.sync_auto_load_cases_action();
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            app.run_combination(0);
            app.run_design_check();
            assert_src912_skipped(&app);
            let original = app.current_design_results().unwrap().clone();
            let checks = bytes(&original.member_checks);
            for version in [
                Some(DESIGN_CONTRACT_VERSION),
                Some(DESIGN_CONTRACT_VERSION - 1),
            ] {
                app.core.scoped.results.as_mut().unwrap().member_checks =
                    if version == Some(DESIGN_CONTRACT_VERSION) {
                        original.member_checks.clone()
                    } else {
                        known_steel_checks.clone()
                    };
                let (mut loaded, path) = super::result_contract_tests::restore(
                    &app,
                    Some(ANALYSIS_CONTRACT_VERSION),
                    version,
                    &format!("src912-{column}-{fy}-{version:?}"),
                );
                assert!(loaded.require_selected_static_input().is_ok());
                assert_eq!(
                    loaded.core.scoped.results.as_ref().unwrap().member_forces,
                    original.member_forces
                );
                if version == Some(DESIGN_CONTRACT_VERSION) {
                    assert_src912_skipped(&loaded);
                } else {
                    assert!(loaded.current_design_results().is_none());
                    assert!(loaded
                        .core
                        .scoped
                        .results
                        .as_ref()
                        .unwrap()
                        .member_checks
                        .is_empty());
                    loaded.run_design_check();
                    assert_src912_skipped(&loaded);
                }
                assert_eq!(
                    bytes(&loaded.current_design_results().unwrap().member_checks),
                    checks
                );
                loaded.save_project_to(path.clone());
                assert!(loaded.core.scoped.last_error.is_none());
                let mut reopened = App::default();
                reopened.open_project_from(path.clone());
                assert_src912_skipped(&reopened);
                assert_eq!(reopened.core.model.materials[2].name, "直接入力鋼材912");
                assert_eq!(reopened.core.model.materials[2].fy, Some(fy));
                assert_eq!(
                    bytes(&reopened.current_design_results().unwrap().member_checks),
                    checks
                );
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}

#[path = "../../../sepika-design-jp/tests/fixtures/src_h_steel_product_model.rs"]
mod steel_product_input;
#[path = "../../../sepika-design-jp/tests/fixtures/src_h_steel_product_reference.rs"]
mod steel_product_reference;

#[test]
fn src_h_steel_product_results_survive_project_save_and_old_contract_requires_recheck() {
    for column in [false, true] {
        let (n, mz) = steel_product_reference::demand(column);
        let run = |grade| {
            let mut app = App::default();
            assert!(app.load_model(steel_product_input::model(column, grade, n, mz)));
            app.core.analysis_cfg.threads = 1;
            app.run_combination(0);
            app.run_design_check();
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            app
        };
        let checked = run("SN400B");
        let checked_bundle = checked
            .current_design_results()
            .expect("適用できる材種の検定結果");
        let positions = &checked_bundle.member_checks[0].positions;
        let checked_positions: Vec<_> = positions
            .iter()
            .filter_map(|position| match &position.outcome {
                CheckOutcome::Checked(result) => Some(result),
                CheckOutcome::Skipped { reason } => {
                    assert!(!column && reason.contains("材料別の負担曲げ"), "{reason}");
                    None
                }
            })
            .collect();
        assert_eq!(checked_positions.len(), 3);
        for result in checked_positions {
            assert!(
                (result.ratio() - steel_product_reference::expected_ratio(column)).abs() < 1e-10,
                "{result:?}"
            );
            assert!(!result.ok());
        }
        let assert_skipped = |app: &App| {
            let bundle = app
                .current_design_results()
                .expect("理由付きの現行検定結果");
            assert_eq!(
                bundle.design_contract_version,
                Some(DESIGN_CONTRACT_VERSION)
            );
            assert_eq!(bundle.member_checks.len(), 1);
            let positions = &bundle.member_checks[0].positions;
            assert_eq!(positions.len(), 3);
            for position in positions {
                assert!(
                    matches!(
                        &position.outcome, CheckOutcome::Skipped { reason }
                            if reason.contains("STK400") && reason.contains("鋼管")
                            && reason.contains(if column { "SRC柱" } else { "SRC梁" })
                    ),
                    "{:?}",
                    position.outcome
                );
            }
            assert!(crate::summary::build_report_csv(app).contains("STK400"));
        };
        let mut skipped = run("STK400");
        assert_skipped(&skipped);
        let expected_checks = bytes(&skipped.current_design_results().unwrap().member_checks);
        let expected_forces = bytes(&skipped.current_static().unwrap().member_forces);

        for old in [false, true] {
            if old {
                skipped.core.scoped.results.as_mut().unwrap().member_checks =
                    checked_bundle.member_checks.clone();
            }
            let version = DESIGN_CONTRACT_VERSION - u32::from(old);
            let (mut loaded, path) = super::result_contract_tests::restore(
                &skipped,
                Some(ANALYSIS_CONTRACT_VERSION),
                Some(version),
                &format!("src-h-steel-product-{column}-{old}"),
            );
            assert!(loaded.require_selected_static_input().is_ok());
            assert_eq!(
                bytes(&loaded.current_static().unwrap().member_forces),
                expected_forces
            );
            if old {
                assert!(loaded.current_design_results().is_none());
                loaded.run_design_check();
            }
            assert_skipped(&loaded);
            assert_eq!(
                bytes(&loaded.current_design_results().unwrap().member_checks),
                expected_checks
            );
            loaded.save_project_to(path.clone());
            assert!(
                loaded.core.scoped.last_error.is_none(),
                "{:?}",
                loaded.core.scoped.last_error
            );
            let mut reopened = App::default();
            reopened.open_project_from(path.clone());
            assert_skipped(&reopened);
            assert_eq!(
                bytes(&reopened.current_design_results().unwrap().member_checks),
                expected_checks
            );
            std::fs::remove_file(path).unwrap();
        }
    }
}
