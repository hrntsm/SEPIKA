use super::*;
use sepika_core::ids::SectionId;
use sepika_core::model::FrameSectionUse;
use sepika_core::section_shape::SectionShape;

fn shape(lip: f64) -> SectionShape {
    SectionShape::SteelLipChannel {
        height: 150.0,
        width: 75.0,
        lip,
        thick: 2.3,
    }
}

#[test]
fn lip_channel_geometry_app_load_and_edit_preserve_results_and_redo() {
    let mut app = App::default();
    assert!(app.load_model(crate::sample::portal_frame()));
    app.run_linear_static(LoadCaseId(0));
    assert!(app.core.scoped.results.is_some());
    let id = SectionId(app.core.model.sections.len() as u32);
    assert!(app.apply_model_edit(Box::new(sepika_edit::AddSectionShape {
        shape: shape(20.0),
        new_id: id,
        name: "LC544".into(),
        floor: None,
        frame_use: Some(FrameSectionUse::Girder),
    })));
    app.undo_action();
    let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
    let results = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
    let revision = app.core.scoped.undo.revision();
    let generation = app.result_input(&result_validity::ResultInputKey::Modal);
    for lip in [75.0, 80.0, 100.0] {
        assert!(
            !app.apply_model_edit(Box::new(sepika_edit::AddSectionShape {
                shape: shape(lip),
                new_id: id,
                name: "LC544".into(),
                floor: None,
                frame_use: Some(FrameSectionUse::Girder),
            }))
        );
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("H=150"));
        assert!(
            !app.apply_model_edit(Box::new(sepika_edit::EditSectionShape {
                section: app.core.model.sections[0].id,
                new_shape: shape(lip),
                frame_use: app.core.model.sections[0].frame_use,
            }))
        );
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("H=150"));
        let mut invalid = app.core.model.clone();
        let mut section = shape(20.0).to_section(id, "LC544".into());
        section.shape = Some(shape(lip));
        invalid.sections.push(section);
        assert!(!app.load_model(invalid));
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
        assert_eq!(
            rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
            results
        );
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert!(app.core.scoped.undo.can_redo());
        assert_eq!(
            app.result_input(&result_validity::ResultInputKey::Modal),
            generation
        );
    }
    app.redo_action();
    assert_eq!(
        app.core.model.sections.last().unwrap().shape,
        Some(shape(20.0))
    );
    let path = std::env::temp_dir().join(format!("lip544-app-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    let loaded = sepika_io::ovika::load_ovika(&path).unwrap().model;
    assert!(loaded.eq_ignoring_dofmap(&app.core.model));
    app.undo_action();
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
    app.redo_action();
    assert!(loaded.eq_ignoring_dofmap(&app.core.model));
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "gui")]
#[test]
fn lip_channel_geometry_egui_form_reports_reason_and_preserves_disabled_draft() {
    use crate::section_editor::{section_editor_panel, SectionEditorDraft, ShapeKind};
    fn text_position(shape: &egui::epaint::Shape, needle: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text().contains(needle) => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            egui::epaint::Shape::Vec(parts) => parts.iter().find_map(|p| text_position(p, needle)),
            _ => None,
        }
    }
    let mut app = App::default();
    app.ui.scoped.section_draft = SectionEditorDraft {
        kind: ShapeKind::SteelLipChannel,
        h: 150.0,
        b: 75.0,
        t: 2.3,
        lip: 20.0,
        name: "LC544".into(),
        ..Default::default()
    };
    let context = egui::Context::default();
    let rendered = context.run_ui(Default::default(), |ui| section_editor_panel(ui, &mut app));
    let button = rendered
        .shapes
        .iter()
        .find_map(|s| text_position(&s.shape, "+ 追加"))
        .unwrap();
    let click = |app: &mut App, button: egui::Pos2| {
        for pressed in [true, false] {
            let _ = context.run_ui(
                egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(button),
                        egui::Event::PointerButton {
                            pos: button,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                    ..Default::default()
                },
                |ui| section_editor_panel(ui, app),
            );
        }
    };
    click(&mut app, button);
    assert_eq!(app.core.model.sections.len(), 1);
    assert!(app.core.scoped.staleness.unsaved_changes);
    let original = app.core.model.sections[0].clone();
    app.ui.scoped.nav.focus_section = Some(SectionId(0));
    app.ui.scoped.section_draft.synced_focus = Some(SectionId(0));
    app.ui.scoped.section_draft.lip = 30.0;
    let rendered = context.run_ui(Default::default(), |ui| section_editor_panel(ui, &mut app));
    let apply_button = rendered
        .shapes
        .iter()
        .find_map(|s| text_position(&s.shape, "選択断面へ適用"))
        .unwrap();
    click(&mut app, apply_button);
    assert_eq!(app.core.model.sections[0].shape, Some(shape(30.0)));
    app.undo_action();
    assert_eq!(app.core.model.sections[0], original);
    let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
    app.ui.scoped.section_draft.lip = 100.0;
    let draft = format!("{:?}", app.ui.scoped.section_draft);
    let revision = app.core.scoped.undo.revision();
    let staleness = format!("{:?}", app.core.scoped.staleness);
    let rendered = context.run_ui(Default::default(), |ui| section_editor_panel(ui, &mut app));
    assert!(rendered
        .shapes
        .iter()
        .any(|s| text_position(&s.shape, "上下リップが重複").is_some()));
    for label in ["+ 追加", "選択断面へ適用"] {
        let button = rendered
            .shapes
            .iter()
            .find_map(|s| text_position(&s.shape, label))
            .unwrap();
        click(&mut app, button);
    }
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
    assert_eq!(format!("{:?}", app.ui.scoped.section_draft), draft);
    assert_eq!(format!("{:?}", app.core.scoped.staleness), staleness);
    assert_eq!(app.core.scoped.undo.revision(), revision);
    assert!(app.core.scoped.undo.can_redo());
}
