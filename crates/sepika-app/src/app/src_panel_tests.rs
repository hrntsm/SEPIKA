use super::*;
#[path = "../../../sepika-design-jp/tests/fixtures/src_panel_model.rs"]
mod fixture;
#[path = "../../../sepika-design-jp/tests/fixtures/src_panel_reference.rs"]
mod reference;

#[test]
fn gui_src_panel_reports_direction_ratio_and_transfer_diagnostics() {
    let mut app = App::default();
    app.load_model(fixture::input_model());
    app.run_preparation();
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let rows = &app.current_design_results().unwrap().joint_checks;
    let row = rows
        .iter()
        .find(|row| row.node == fixture::JOINT && row.label == fixture::PANEL_X)
        .expect("X構面の結果");
    let sepika_design_jp::CheckOutcome::Checked(result) = &row.outcome else {
        panic!("{:?}", row.outcome);
    };
    assert!(result.basis.contains("(20.1) L字形・長期"));
    assert!(
        (result.ratio() - reference::DEMAND_KN_M / reference::RC_LONG_ALLOWABLE_KN_M).abs() < 1e-10
    );
    for (label, expected) in [
        (fixture::PANEL_Y.to_owned(), "弱軸"),
        (format!("{} 応力伝達", fixture::PANEL_X), "別途確認"),
    ] {
        assert!(rows.iter().any(|row| row.node == fixture::JOINT
            && row.label == label
            && matches!(&row.outcome, sepika_design_jp::CheckOutcome::Skipped { reason }
                if reason.contains(expected))));
    }

    let mut assignments = fixture::short_model(fixture::ShortBeam::Rc).src_joint_hoops;
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::SetSrcJointHoops {
            node: fixture::JOINT,
            axis: [1.0, 0.0, 0.0],
            hoops: Some(assignments.remove(0).hoops),
        }))
    );
    assert!(app.current_design_results().is_some());

    let mut missing = App::default();
    missing.load_model(fixture::with_unavailable_column_face(fixture::input_model()));
    missing.run_preparation();
    missing.run_linear_static(LoadCaseId(0));
    assert!(
        missing.core.scoped.last_error.is_none(),
        "{:?}",
        missing.core.scoped.last_error
    );
    let rows = &missing.current_design_results().unwrap().joint_checks;
    assert!(rows.iter().any(|row| row.node == fixture::JOINT
        && row.label == fixture::PANEL_X
        && matches!(&row.outcome, sepika_design_jp::CheckOutcome::Skipped { reason }
            if reason.contains("実柱面") && reason.contains("切断側"))));
}

#[test]
fn gui_src_panel_uses_short_combination_and_explicit_hoops() {
    use fixture::ShortBeam;
    for (beam, expected) in [
        (
            ShortBeam::Src,
            Some(("(20.2)", reference::SRC_SHORT_ALLOWABLE_KN_M)),
        ),
        (
            ShortBeam::Steel,
            Some(("(解20.12)", reference::STEEL_SHORT_ALLOWABLE_KN_M)),
        ),
        (ShortBeam::Rc, None),
    ] {
        let mut app = App::default();
        app.load_model(fixture::short_model(beam));
        app.run_combination(0);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let rows = &app.current_design_results().unwrap().joint_checks;
        let row = rows
            .iter()
            .find(|row| row.node == fixture::JOINT && row.label == fixture::PANEL_X)
            .unwrap();
        match (expected, &row.outcome) {
            (Some((equation, allowable)), sepika_design_jp::CheckOutcome::Checked(result)) => {
                assert!(result.basis.contains(equation));
                assert!(result.basis.contains("L字形・短期"));
                assert!(
                    (result.ratio() - reference::DEMAND_KN_M / allowable).abs() < 1e-10,
                    "{result:?}"
                );
            }
            (None, sepika_design_jp::CheckOutcome::Skipped { reason }) => {
                assert!(reason.contains("RC梁の短期式(20.2)"), "{reason}");
                assert!(
                    reason.contains("有効高さと鉄骨ウェブへの応力伝達"),
                    "{reason}"
                );
            }
            (_, outcome) => panic!("X構面: {outcome:?}"),
        }
        assert!(rows.iter().any(|row| row.node == fixture::JOINT
            && row.label == format!("{} 応力伝達", fixture::PANEL_X)
            && matches!(&row.outcome, sepika_design_jp::CheckOutcome::Skipped { reason }
                if reason.contains("別途確認"))));
    }
}

#[test]
fn editing_short_panel_hoops_invalidates_only_the_design_and_rechecks_after_reopen() {
    use crate::app::references::ResultInputKey;
    use sepika_design_jp::CheckOutcome;
    use sepika_edit::SetSrcJointHoops;

    let mut app = App::default();
    app.load_model(fixture::short_model(fixture::ShortBeam::Src));
    app.run_combination(0);
    let analysis_source = app.selected_design_source().unwrap();
    let response = app
        .current_static()
        .unwrap()
        .girder_column_face_forces
        .clone();
    let before = reference::DEMAND_KN_M / reference::SRC_SHORT_ALLOWABLE_KN_M;
    let target = |row: &&JointCheck| row.node == fixture::JOINT && row.label == fixture::PANEL_X;
    let ratio = |app: &App| {
        let row = app
            .current_design_results()
            .unwrap()
            .joint_checks
            .iter()
            .find(target)
            .unwrap();
        let CheckOutcome::Checked(result) = &row.outcome else {
            panic!("{:?}", row.outcome)
        };
        result.ratio()
    };
    let ensure_analysis_is_current = |app: &App| {
        assert!(app.require_result_input(analysis_source.clone()).is_ok());
        assert_eq!(
            app.current_static().unwrap().girder_column_face_forces,
            response
        );
    };
    assert!((ratio(&app) - before).abs() < 1e-10);
    let (_, recorded) = app.calculation_snapshot(&ResultInputKey::Design).unwrap();
    assert_eq!(recorded.src_joint_hoops, app.core.model.src_joint_hoops);
    #[cfg(feature = "gui")]
    {
        let text = design_table_text(&mut app);
        assert!(text.contains("(20.2) L字形・短期"), "{text}");
        assert!(text.contains("応力伝達は別途確認が必要"), "{text}");
    }

    let mut changed = app.core.model.src_joint_hoops[0].hoops.clone();
    changed.spacing_mm = 350.0;
    assert!(app.apply_model_edit(Box::new(SetSrcJointHoops {
        node: fixture::JOINT,
        axis: [1.0, 0.0, 0.0],
        hoops: Some(changed),
    })));
    ensure_analysis_is_current(&app);
    assert!(app.current_design_results().is_none());
    #[cfg(feature = "gui")]
    assert!(!design_table_text(&mut app).contains("(20.2) L字形・短期"));

    app.undo_action();
    ensure_analysis_is_current(&app);
    assert!((ratio(&app) - before).abs() < 1e-10);
    app.redo_action();
    ensure_analysis_is_current(&app);
    assert!(app.current_design_results().is_none());
    app.run_design_check();
    let updated_ratio = ratio(&app);
    assert!((updated_ratio - before).abs() > 0.005);

    assert!(app.apply_model_edit(Box::new(SetSrcJointHoops {
        node: fixture::JOINT,
        axis: [1.0, 0.0, 0.0],
        hoops: None,
    })));
    ensure_analysis_is_current(&app);
    assert!(app.current_design_results().is_none());
    let path = std::env::temp_dir().join(format!(
        "sepika-src-panel-hoop-removal-{}.ovika",
        uuid::Uuid::new_v4()
    ));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    ensure_analysis_is_current(&reopened);
    assert!(reopened.core.model.src_joint_hoops.is_empty());
    assert!(reopened
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .joint_checks
        .is_empty());
    assert!(reopened.current_design_results().is_none());

    reopened.run_design_check();
    let rows = &reopened.current_design_results().unwrap().joint_checks;
    let outcome = &rows.iter().find(target).unwrap().outcome;
    assert!(matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("帯筋が未入力")));
    #[cfg(feature = "gui")]
    {
        let text = design_table_text(&mut reopened);
        assert!(text.contains("帯筋が未入力"), "{text}");
        assert!(text.contains("検定不能"), "{text}");
    }

    app.undo_action();
    ensure_analysis_is_current(&app);
    assert_eq!(ratio(&app), updated_ratio);
    app.redo_action();
    ensure_analysis_is_current(&app);
    assert!(app.current_design_results().is_none());
}

#[cfg(feature = "gui")]
fn design_table_text(app: &mut App) -> String {
    fn collect_text(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(value) => {
                text.push_str(value.galley.text());
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, text);
                }
            }
            _ => {}
        }
    }

    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(8000.0, 6000.0),
        )),
        ..Default::default()
    };
    let output =
        egui::Context::default().run_ui(input, |ui| crate::design_view::design_table(ui, app));
    let mut text = String::new();
    for shape in output.shapes {
        collect_text(&shape.shape, &mut text);
    }
    text
}
