use super::*;
use sepika_core::ids::*;
use sepika_core::model::*;

fn kinds() -> [ElementKind; 4] {
    [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
        ElementKind::Brace {
            tension_only: false,
        },
    ]
}

#[test]
fn frame_node_counts_public_solver_csv_and_app_open_preserve_state() {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.run_linear_static(LoadCaseId(0));
    assert!(
        app.core.scoped.results.is_some(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
        node: NodeId(0),
        coord: [-100.0, 0.0, 0.0]
    })));
    let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
    let results = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
    let preparation = rmp_serde::to_vec_named(&app.core.scoped.preparation).unwrap();
    let revision = app.core.scoped.undo.revision();
    let generation = app.result_input(&result_validity::ResultInputKey::Modal);
    for kind in kinds() {
        for nodes in [
            vec![],
            vec![NodeId(0)],
            vec![NodeId(0), NodeId(2), NodeId(1)],
            vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            vec![NodeId(0), NodeId(0)],
        ] {
            let mut model = crate::sample::portal_frame();
            model.elements[0].kind = kind;
            model.elements[0].nodes = nodes.into();
            let reason = model.validate_frame_node_counts().unwrap_err().to_string();
            let issues = sepika_solver::statics::analysis::precheck::model_issues(&model);
            assert_eq!(issues.len(), 1);
            assert_eq!(issues[0].message, reason);
            assert!(Analysis::prepare(&model)
                .err()
                .unwrap()
                .to_string()
                .contains(&reason));
            assert!(
                sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0))
                    .unwrap_err()
                    .to_string()
                    .contains(&reason)
            );
            let csv = crate::summary::build_quantity_csv(&model);
            assert!(csv.contains("[数量積算 未算定]") && csv.contains(&reason));
            let path = test_tmp().join(format!(
                "frame562-{kind:?}-{}.ovika",
                model.elements[0].nodes.len()
            ));
            super::secondary_cft_tests::write_invalid_container(&path, &model);
            for raw_open in [false, true] {
                if raw_open {
                    app.open_project_from(path.clone());
                } else {
                    assert!(!app.load_model(model.clone()));
                }
                assert!(app
                    .core
                    .scoped
                    .last_error
                    .as_ref()
                    .unwrap()
                    .contains(&reason));
                assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
                assert_eq!(
                    rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
                    results
                );
                assert_eq!(
                    rmp_serde::to_vec_named(&app.core.scoped.preparation).unwrap(),
                    preparation
                );
                assert_eq!(app.core.scoped.undo.revision(), revision);
                assert_eq!(
                    app.result_input(&result_validity::ResultInputKey::Modal),
                    generation
                );
            }
            std::fs::remove_file(path).unwrap();
        }
    }
    app.undo_action();
    app.redo_action();
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
}

#[test]
fn frame_node_counts_app_member_creation_reports_rejection_without_staling_results() {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.run_linear_static(LoadCaseId(0));
    let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
    let results = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
    let generation = app.result_input(&result_validity::ResultInputKey::Modal);
    let revision = app.core.scoped.undo.revision();
    for kind in kinds() {
        let mut elem = app.core.model.elements[0].clone();
        elem.id = ElemId(app.core.model.elements.len() as u32);
        elem.kind = kind;
        elem.nodes = vec![NodeId(0), NodeId(2), NodeId(1)].into();
        let reason = elem.validate_frame_node_count().unwrap_err().to_string();
        assert!(!app.apply_model_edit(Box::new(sepika_edit::AddMember { elem })));
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains(&reason));
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
        assert_eq!(
            rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
            results
        );
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(
            app.result_input(&result_validity::ResultInputKey::Modal),
            generation
        );
    }
    let mut elem = app.core.model.elements[0].clone();
    elem.id = ElemId(app.core.model.elements.len() as u32);
    elem.section = None;
    assert!(app.apply_model_edit(Box::new(sepika_edit::AddMember { elem })));
    app.undo_action();
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
    app.redo_action();
    assert_eq!(app.core.model.elements.len(), 4);
}

#[cfg(feature = "gui")]
#[test]
fn frame_node_counts_gui_quantity_renders_same_reason_without_totals() {
    for kind in kinds() {
        let mut app = App::default();
        app.core.model = crate::sample::portal_frame();
        app.core.model.elements[0].kind = kind;
        app.core.model.elements[0].nodes = vec![NodeId(0), NodeId(2), NodeId(1)].into();
        let reason = app
            .core
            .model
            .validate_frame_node_counts()
            .unwrap_err()
            .to_string();
        let output = egui::Context::default().run_ui(Default::default(), |ui| {
            crate::quantity_view::quantity_panel(ui, &mut app);
        });
        fn texts(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                egui::epaint::Shape::Vec(parts) => parts.iter().for_each(|part| texts(part, out)),
                _ => {}
            }
        }
        let mut rendered = vec![];
        for shape in output.shapes {
            texts(&shape.shape, &mut rendered);
        }
        let rendered = rendered.join("\n");
        assert!(
            rendered.contains("数量積算は未算定です") && rendered.contains(&reason),
            "{rendered}"
        );
        assert!(!rendered.contains("コンクリート[m³]"));
    }
}

#[test]
fn frame_node_counts_gui_member_button_creates_two_nodes_and_undo_redo_restores() {
    fn text_position(shape: &egui::epaint::Shape, needle: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == needle => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            egui::epaint::Shape::Vec(parts) => parts.iter().find_map(|p| text_position(p, needle)),
            _ => None,
        }
    }
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    let before = app.core.model.clone();
    let context = egui::Context::default();
    let rendered = context.run_ui(Default::default(), |ui| {
        crate::tables::members::members_table(ui, &mut app)
    });
    let button = rendered
        .shapes
        .iter()
        .find_map(|s| text_position(&s.shape, "+ 部材追加"))
        .unwrap();
    let _ = context.run_ui(
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(button),
                egui::Event::PointerButton {
                    pos: button,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        },
        |ui| crate::tables::members::members_table(ui, &mut app),
    );
    let _ = context.run_ui(
        egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: button,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            ..Default::default()
        },
        |ui| crate::tables::members::members_table(ui, &mut app),
    );
    assert_eq!(app.core.model.elements.len(), before.elements.len() + 1);
    assert_eq!(
        app.core.model.elements.last().unwrap().nodes.as_slice(),
        &[NodeId(0), NodeId(1)]
    );
    app.core.model.validate_frame_node_counts().unwrap();
    app.undo_action();
    assert!(before.eq_ignoring_dofmap(&app.core.model));
    app.redo_action();
    assert_eq!(app.core.model.elements.len(), before.elements.len() + 1);
    let path = test_tmp().join("frame562-gui-created.ovika");
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let again = sepika_io::ovika::load_ovika(&path).unwrap().model;
    assert_eq!(
        again.support_member_axis(SupportMemberId::Primary(ElemId(
            before.elements.len() as u32
        ))),
        Some((again.nodes[0].coord, again.nodes[1].coord))
    );
    std::fs::remove_file(path).unwrap();
}
