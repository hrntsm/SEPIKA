use super::*;
use sepika_design_jp::{CheckKind, CheckOutcome};
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_model.rs"]
pub(crate) mod input;
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_reference.rs"]
pub(crate) mod reference;

#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_long_term_shear_model.rs"]
mod long_term_shear_input;

fn rc_column_long_term_shear_model(circle: bool) -> sepika_core::model::Model {
    long_term_shear_input::model(
        input::column_model(circle, 0.0),
        super::rc_beam_asymmetric_tests::input::section(300.0, 600.0, 19.0, &[3], &[3]),
        true,
    )
}

fn rc_column_long_term_shear_app(model: sepika_core::model::Model) -> App {
    let mut app = App::default();
    assert!(app.load_model(model));
    app.run_combination(0);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}

#[test]
fn rc_column_long_term_shear_preserves_current_analysis_and_rechecks_old_ovika() {
    use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};

    for circle in [false, true] {
        let app = rc_column_long_term_shear_app(rc_column_long_term_shear_model(circle));
        let result = app.current_design_results().unwrap();
        let column = result
            .member_checks
            .iter()
            .find(|record| record.elem == long_term_shear_input::COLUMN)
            .unwrap();
        assert_eq!(column.positions.len(), 3);
        for position in &column.positions {
            assert!(
                matches!(&position.outcome, CheckOutcome::Checked(result) if !result.ok()
                && result.components.iter().any(|component| component.kind == CheckKind::Shear
                    && component.ratio > 1.0
                    && component.detail.contains("局所Qy最大 |Qy|=313900.000 N（i端から x=1100.000000 mm")))
            );
        }
        let expected = serde_json::to_value(&result.member_checks).unwrap();
        let expected_forces = result.member_forces.clone();
        for version in [102, 103, 104, 105, DESIGN_CONTRACT_VERSION] {
            let (mut reopened, path) = super::result_contract_tests::restore(
                &app,
                Some(ANALYSIS_CONTRACT_VERSION),
                Some(version),
                &format!("rc-column-long-term-shear-{circle}-{version}"),
            );
            std::fs::remove_file(path).unwrap();
            assert!(reopened.require_selected_static_input().is_ok());
            let bundle = reopened.core.scoped.results.as_ref().unwrap();
            assert_eq!(bundle.member_forces, expected_forces);
            assert_eq!(
                reopened.current_design_results().is_some(),
                version == DESIGN_CONTRACT_VERSION
            );
            if version != DESIGN_CONTRACT_VERSION {
                assert!(bundle.member_checks.is_empty());
            }
            reopened.run_design_check();
            assert!(
                reopened.core.scoped.last_error.is_none(),
                "{:?}",
                reopened.core.scoped.last_error
            );
            let rechecked = reopened.current_design_results().unwrap();
            assert_eq!(
                rechecked.design_contract_version,
                Some(DESIGN_CONTRACT_VERSION)
            );
            assert_eq!(rechecked.member_forces, expected_forces);
            assert_eq!(
                serde_json::to_value(&rechecked.member_checks).unwrap(),
                expected
            );
            println!("円形={circle} 解析版={ANALYSIS_CONTRACT_VERSION} 保存検定版={version} 再検定版={DESIGN_CONTRACT_VERSION}");
        }
    }
}

#[test]
fn rc_column_long_term_shear_keeps_independent_failure_with_invalid_saved_axial_force() {
    let mut app = rc_column_long_term_shear_app(rc_column_long_term_shear_model(false));
    let bundle = app.core.scoped.results.as_mut().unwrap();
    for forces in [
        &mut bundle.member_forces,
        &mut bundle.combos[0].1.member_forces,
    ] {
        let (_, response) = forces
            .iter_mut()
            .find(|(id, _)| *id == long_term_shear_input::COLUMN)
            .unwrap();
        let (_, station) = response.at.iter_mut().find(|(xi, _)| *xi == 0.5).unwrap();
        station[0] = f64::NAN;
    }
    assert!(app.require_selected_static_input().is_ok());
    app.run_design_check();
    let result = app.current_design_results().unwrap();
    let column = result
        .member_checks
        .iter()
        .find(|record| record.elem == long_term_shear_input::COLUMN)
        .unwrap();
    let middle = || {
        column
            .positions
            .iter()
            .filter(|position| position.xi == 0.5)
    };
    assert!(middle().any(|position| matches!(&position.outcome,
        CheckOutcome::Skipped { reason } if reason.contains("軸力Nが非有限")
            && !reason.contains("αの断面力参照") && !reason.contains("生成入力が未確認")
    )));
    assert!(middle().any(|position| matches!(&position.outcome,
        CheckOutcome::Checked(result) if !result.ok() && result.components.len() == 1
            && result.components[0].kind == CheckKind::Shear
            && result.components[0].detail.contains("局所Qy最大 |Qy|=313900.000 N")
    )));
}

#[test]
fn rc_column_long_term_shear_localizes_invalid_saved_section_position() {
    let mut model = rc_column_long_term_shear_model(false);
    let mut other_base = model.nodes[0].clone();
    other_base.id = NodeId(3);
    other_base.coord[0] = 6000.0;
    model.nodes.push(other_base);
    let mut other_tip = model.nodes[1].clone();
    other_tip.id = NodeId(4);
    other_tip.coord = [6000.0, 0.0, 2400.0];
    model.nodes.push(other_tip);
    let mut other = model.elements[0].clone();
    other.id = ElemId(2);
    other.nodes = [NodeId(3), NodeId(4)].into_iter().collect();
    let other_column = other.id;
    model.elements.push(other);
    let mut app = rc_column_long_term_shear_app(model);
    assert!(!app.core.model.node_on_rigid_diaphragm(NodeId(4)));
    let expected = app
        .current_design_results()
        .unwrap()
        .member_checks
        .iter()
        .find(|member| member.elem == long_term_shear_input::COLUMN)
        .unwrap();
    assert_eq!(expected.positions.len(), 3);
    assert!(
        expected
            .positions
            .iter()
            .all(|position| matches!(&position.outcome,
                CheckOutcome::Checked(result) if result.components.iter().any(|component| {
                    component.kind == CheckKind::Shear && component.ratio > 1.0
                        && component.detail.contains("局所Qy最大 |Qy|=313900.000 N")
                        && component.detail.contains("i端から x=1100.000000 mm")
                })
            )),
        "{expected:?}"
    );
    let expected = serde_json::to_value(&expected.positions).unwrap();

    let bundle = app.core.scoped.results.as_mut().unwrap();
    for forces in [
        &mut bundle.member_forces,
        &mut bundle.combos[0].1.member_forces,
    ] {
        let (_, response) = forces
            .iter_mut()
            .find(|(id, _)| *id == other_column)
            .unwrap();
        response.at.iter_mut().find(|(xi, _)| *xi == 0.5).unwrap().0 = f64::NAN;
    }
    app.run_design_check();
    let result = app.current_design_results().unwrap();
    let affected = result
        .member_checks
        .iter()
        .find(|member| member.elem == other_column)
        .unwrap();
    assert!(affected
        .positions
        .iter()
        .any(|position| matches!(&position.outcome,
            CheckOutcome::Skipped { reason } if reason.contains("断面力の評価位置が不足・不正")
        )));
    let unaffected = result
        .member_checks
        .iter()
        .find(|member| member.elem == long_term_shear_input::COLUMN)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&unaffected.positions).unwrap(),
        expected
    );

    let bundle = app.core.scoped.results.as_mut().unwrap();
    let (_, response) = bundle.combos[0]
        .1
        .member_forces
        .iter_mut()
        .find(|(id, _)| *id == other_column)
        .unwrap();
    let position = &mut response
        .at
        .iter_mut()
        .find(|(xi, _)| xi.is_nan())
        .unwrap()
        .0;
    *position = f64::from_bits(position.to_bits() ^ 1);
    app.run_design_check();
    let result = app.current_design_results().unwrap();
    let unverified = result
        .member_checks
        .iter()
        .find(|member| member.elem == long_term_shear_input::COLUMN)
        .unwrap();
    assert!(unverified
        .positions
        .iter()
        .any(|position| matches!(&position.outcome,
            CheckOutcome::Skipped { reason } if reason.contains("生成入力が未確認")
        )));
    assert!(!unverified.positions.iter().any(|position| matches!(&position.outcome,
        CheckOutcome::Checked(result) if result.components.iter().any(|component| component.kind == CheckKind::Shear)
    )));
}

#[test]
fn rc_column_boundary_gui_analysis_save_and_old_contract_preserve_unchecked_state() {
    for circle in [false, true] {
        let mut app = App::default();
        let mut model = input::column_model(circle, 1.01 * reference::axial_limit_n(circle));
        model.load_cases[0].nodal[0].values[0] = 1_000_000.0;
        assert!(app.load_model(model));
        app.run_preparation();
        app.run_linear_static(LoadCaseId(0));
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let expected = app.current_design_results().unwrap().member_checks.clone();
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].positions.len(), 6);
        for xi in [0.0, 0.5, 1.0] {
            let at_xi = || {
                expected[0]
                    .positions
                    .iter()
                    .filter(|position| position.xi == xi)
            };
            assert!(at_xi().any(|position| matches!(&position.outcome, CheckOutcome::Skipped { reason } if reason.contains("検定不能") && reason.contains("MA_y=0"))));
            assert!(at_xi().any(|position| matches!(&position.outcome, CheckOutcome::Checked(result) if !result.ok() && result.components.len() == 1 && result.components[0].kind == CheckKind::Shear)));
        }
        let path =
            std::env::temp_dir().join(format!("sepika1803-{circle}-{}.ovika", std::process::id()));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        let (mut stale, path) = super::result_contract_tests::restore(
            &app,
            Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
            Some(100),
            &format!("rc-column1803-old-{circle}"),
        );
        std::fs::remove_file(path).unwrap();
        assert!(stale
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .has_analysis_results());
        assert!(stale.current_design_results().is_none());
        stale.run_design_check();
        assert_eq!(
            serde_json::to_value(&stale.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}

#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_tension_model.rs"]
mod tension_input;
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_tension_reference.rs"]
mod tension_reference;

#[test]
fn rc_column_tension_gui_preserves_ng_and_invalidates_previous_contract() {
    for circle in [false, true] {
        let mut app = App::default();
        app.load_model(tension_input::tension_model(
            input::column_model(circle, 0.0),
            1_000_000.0,
            [0.0, 0.0],
        ));
        app.run_preparation();
        app.run_linear_static(LoadCaseId(0));
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let expected = app.current_design_results().unwrap().member_checks.clone();
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].positions.len(), 3);
        for position in &expected[0].positions {
            let CheckOutcome::Checked(result) = &position.outcome else {
                panic!("{position:?}")
            };
            assert!(!result.ok());
            assert!(result.ratio() > 2.0);
            assert!(result.ratio() <= 1_000_000.0 / tension_reference::tension_limit_n(true));
        }
        let (mut loaded, path) = super::result_contract_tests::restore(
            &app,
            Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
            Some(sepika_core::result_contract::DESIGN_CONTRACT_VERSION),
            &format!("rc-column1393-current-{circle}"),
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            serde_json::to_value(&loaded.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        loaded.run_design_check();
        let (mut stale, path) = super::result_contract_tests::restore(
            &app,
            Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
            Some(55),
            &format!("rc-column1393-old-{circle}"),
        );
        std::fs::remove_file(path).unwrap();
        assert!(stale
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .has_analysis_results());
        assert!(stale.current_design_results().is_none());
        stale.run_design_check();
        assert_eq!(
            serde_json::to_value(&stale.current_design_results().unwrap().member_checks).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}
