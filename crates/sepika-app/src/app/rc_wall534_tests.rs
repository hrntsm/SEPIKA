use super::*;
#[path = "../../../../test_fixtures/rc_wall534.rs"]
mod crack_fixture;

fn crack_ready() -> App {
    let mut app = App::default();
    app.load_model(crack_fixture::input_model(
        super::rc_wall617_tests::fixture::input_model(),
    ));
    app.run_preparation();
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app
}

#[test]
fn gui_design_crack_points_match_independent_fixture_and_saved_component() {
    let mut app = crack_ready();
    let reference = app
        .current_design_results()
        .unwrap()
        .wall_checks
        .iter()
        .find(|w| w.kind == sepika_design_jp::wall_check::WallCheckKind::ReferenceSkeleton)
        .unwrap();
    assert!(
        matches!(
            reference.outcome,
            sepika_design_jp::CheckOutcome::Checked(_)
        ),
        "{:?}",
        reference
    );
    crack_fixture::assert_reference(reference.reference_skeleton.as_ref().unwrap());
    let expected = reference.reference_skeleton.clone();
    let path =
        std::env::temp_dir().join(format!("sepika534-reference-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    assert!(
        loaded.core.scoped.last_error.is_none(),
        "{:?}",
        loaded.core.scoped.last_error
    );
    let saved = loaded
        .current_design_results()
        .unwrap()
        .wall_checks
        .iter()
        .find(|w| w.kind == sepika_design_jp::wall_check::WallCheckKind::ReferenceSkeleton)
        .unwrap();
    assert_eq!(saved.reference_skeleton, expected);
}

#[cfg(feature = "gui")]
#[test]
fn gui_wall_check_draws_crack_force_and_both_reference_axes() {
    let app = crack_ready();
    let context = egui::Context::default();
    let mut text = String::new();
    for _ in 0..3 {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(3000.0, 3000.0),
                )),
                ..Default::default()
            },
            |ui| {
                crate::design_view::wall_checks_ui(
                    ui,
                    &app.current_design_results().unwrap().wall_checks,
                )
            },
        );
        text.push_str(
            &output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    for expected in [
        "Qc=2016.0 kN",
        "Qc用Aw=840000.0 mm²",
        "81244.598 N/mm",
        "純せん断変位 δ [mm]",
        "純せん断角 γ [rad]",
        "せん断力 Q [kN]",
    ] {
        assert!(text.contains(expected), "missing={expected}, text={text}");
    }
}

#[test]
fn crack_reference_input_edits_undo_redo_and_save_never_reauthenticate_old_results() {
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_edit::{EditSectionShape, MaterialField, SetMaterialField, SetNodeCoord};
    for edit in 0..4 {
        let mut app = crack_ready();
        let before = app.core.model.clone();
        let command: Box<dyn sepika_edit::EditCommand> = match edit {
            0 => Box::new(SetMaterialField {
                id: MaterialId(0),
                field: MaterialField::Fc,
                value: Some(22.0),
            }),
            1 | 3 => {
                let section = if edit == 1 {
                    SectionId(2)
                } else {
                    SectionId(0)
                };
                let mut shape = app.core.model.sections[section.index()]
                    .shape
                    .clone()
                    .unwrap();
                match &mut shape {
                    sepika_core::section_shape::SectionShape::RcWall { thickness, .. } => {
                        *thickness = 180.0
                    }
                    sepika_core::section_shape::SectionShape::RcColumnRect { b, d, .. } => {
                        *b = 800.0;
                        *d = 800.0;
                    }
                    _ => panic!(),
                }
                Box::new(EditSectionShape {
                    section,
                    new_shape: shape,
                    frame_use: app.core.model.sections[section.index()].frame_use,
                })
            }
            _ => Box::new(sepika_edit::CompositeCommand {
                label: "柱心距離編集".into(),
                children: vec![
                    Box::new(SetNodeCoord {
                        node: NodeId(1),
                        coord: [2100.0, 0.0, 0.0],
                    }),
                    Box::new(SetNodeCoord {
                        node: NodeId(3),
                        coord: [2100.0, 0.0, 3000.0],
                    }),
                ],
            }),
        };
        assert!(
            app.core.scoped.undo.run(&mut app.core.model, command),
            "edit={edit}, {:?}",
            app.core.scoped.undo.last_error()
        );
        app.core.scoped.staleness.mark_edited();
        assert!(app.current_design_results().is_none(), "edit={edit}");
        app.core.scoped.undo.undo(&mut app.core.model);
        app.core.scoped.staleness.mark_edited();
        assert_eq!(
            serde_json::to_value(&app.core.model).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        // 同じ生成入力へUndoした結果を使う場合も、再認証は入力・計算契約の完全一致に限る。
        if let Some(results) = app.current_design_results() {
            crack_fixture::assert_reference(
                results
                    .wall_checks
                    .iter()
                    .find_map(|w| w.reference_skeleton.as_ref())
                    .unwrap(),
            );
        }
        app.core.scoped.undo.redo(&mut app.core.model);
        app.core.scoped.staleness.mark_edited();
        assert!(app.current_design_results().is_none());
        let path = std::env::temp_dir().join(format!(
            "sepika534-edited-{edit}-{}.ovika",
            std::process::id()
        ));
        app.save_project_to(path.clone());
        if edit == 2 {
            assert!(app.core.model.validate_member_load_extents().is_err());
            assert!(app.current_design_results().is_none());
        }
        assert!(app.core.scoped.pending_save_recording.is_none());
        assert!(
            app.core.scoped.last_error.is_none(),
            "edit={edit}, {:?}",
            app.core.scoped.last_error
        );
        assert!(
            path.is_file(),
            "edit={edit}, save_error={:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "edit={edit}, {:?}",
            loaded.core.scoped.last_error
        );
        std::fs::remove_file(path).unwrap();
        assert!(loaded.current_design_results().is_none(), "edit={edit}");
        loaded.run_design_check();
        assert!(loaded
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .wall_checks
            .iter()
            .all(|w| w.reference_skeleton.is_none()));
        app.run_design_check();
        assert!(app.current_design_results().is_none());
        if edit == 0 {
            app.run_linear_static(LoadCaseId(0));
            let results = app.current_design_results().unwrap();
            let skeleton = results
                .wall_checks
                .iter()
                .find_map(|w| w.reference_skeleton.as_ref())
                .unwrap();
            assert!((skeleton.displacement_points[1].1 - 1_848_000.0).abs() < 1e-6);
        }
    }
}

#[test]
fn saved_crack_reference_with_previous_design_contract_is_unavailable() {
    let app = crack_ready();
    let (loaded, path) =
        super::result_contract_tests::restore(&app, Some(18), Some(9), "qc-before-534");
    std::fs::remove_file(path).unwrap();
    assert!(loaded.current_design_results().is_none());
    assert!(loaded
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .wall_checks
        .is_empty());
}

#[test]
fn opening_edit_crack_invalidates_saved_reference_and_recalculation_keeps_reasoned_skip() {
    use sepika_design_jp::{
        wall_check::{WallCheckKind, WallSkipKind},
        CheckOutcome,
    };
    let mut app = crack_ready();
    let plate = &app.core.model.wall_plates[0];
    let edit = sepika_edit::SetWallPlateAttrs {
        id: plate.id,
        dl_support: plate.dl_support,
        self_weight_shares: plate.self_weight_shares.clone(),
        opening_area: 0.0,
        opening_weight: 0.0,
        openings: vec![sepika_core::model::WallOpening {
            width: 200.0,
            height: 200.0,
            offset: None,
        }],
        loads: plate.loads.clone(),
        slit: plate.slit,
    };
    assert!(app
        .core
        .scoped
        .undo
        .run(&mut app.core.model, Box::new(edit)));
    app.core.scoped.staleness.mark_edited();
    assert!(app.current_design_results().is_none());
    app.core.scoped.undo.undo(&mut app.core.model);
    assert!(app.core.model.wall_plates[0].openings.is_empty());
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(app.core.model.wall_plates[0].openings.len(), 1);
    assert!(app.current_design_results().is_none());
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let reference = app
        .current_design_results()
        .unwrap()
        .wall_checks
        .iter()
        .find(|w| w.kind == WallCheckKind::ReferenceSkeleton)
        .unwrap();
    assert!(
        matches!(&reference.outcome, CheckOutcome::Skipped { reason } if reason.contains("Qc用開口低減率が未確定")),
        "{:?}",
        reference
    );
    assert_eq!(reference.skip_kind, Some(WallSkipKind::NotImplemented));
    assert!(reference.reference_skeleton.is_none());
    let path = std::env::temp_dir().join(format!("sepika534-opening-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    let mut loaded = App::default();
    loaded.open_project_from(path.clone());
    std::fs::remove_file(path).unwrap();
    let saved = loaded
        .current_design_results()
        .unwrap()
        .wall_checks
        .iter()
        .find(|w| w.kind == WallCheckKind::ReferenceSkeleton)
        .unwrap();
    assert!(
        matches!(&saved.outcome, CheckOutcome::Skipped { reason } if reason.contains("Qc用開口低減率が未確定"))
    );
    assert!(saved.reference_skeleton.is_none());
}

#[test]
fn crack_fc_edit_retains_axial_applicability_limit_after_reanalysis() {
    let mut app = crack_ready();
    assert!(app.core.scoped.undo.run(
        &mut app.core.model,
        Box::new(sepika_edit::SetMaterialField {
            id: sepika_core::ids::MaterialId(0),
            field: sepika_edit::MaterialField::Fc,
            value: Some(20.0),
        })
    ));
    app.core.scoped.staleness.mark_edited();
    assert!(app.current_design_results().is_none());
    app.run_linear_static(LoadCaseId(0));
    let check = app
        .current_design_results()
        .unwrap()
        .wall_checks
        .iter()
        .find(|w| w.kind == sepika_design_jp::wall_check::WallCheckKind::ReferenceSkeleton)
        .unwrap();
    assert!(
        matches!(&check.outcome, sepika_design_jp::CheckOutcome::Skipped { reason } if reason.contains("圧縮正N/A") && reason.contains("適用範囲外"))
    );
    assert!(check.reference_skeleton.is_none());
}
