use super::*;
use sepika_core::{ids::*, model::*};
#[path = "../../tests/support/nonnodal_quantity_model.rs"]
mod fixture;

fn panel_text(app: &mut App, grouping: crate::quantity_view::QuantityGrouping) -> String {
    app.ui.view.quantity_view.grouping = grouping;
    let context = egui::Context::default();
    context
        .run_ui(egui::RawInput::default(), |root| {
            egui::CentralPanel::default()
                .show_inside(root, |ui| crate::quantity_view::quantity_panel(ui, app));
        })
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn nonnodal557_gui_csv_and_common_entry_preserve_id_quantity_and_model() {
    use crate::quantity_view::QuantityGrouping::*;
    let mut app = App::default();
    app.core.model = fixture::placed();
    app.core.model.validate().unwrap();
    let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
    let revision = app.core.scoped.undo.revision();
    fixture::assert_quantity(&app.core.model, SecondaryMemberId(557), 0.54, 4.5);
    for grouping in [ByCategory, ByStory, SteelBySection, Detail] {
        let text = panel_text(&mut app, grouping);
        assert!(!text.contains("未算定"), "{text}");
        match grouping {
            Detail => assert!(
                text.contains("SM557")
                    && text.contains("非節点小梁")
                    && text.contains("0.540")
                    && text.contains("4.50"),
                "{text}"
            ),
            ByStory => assert!(
                text.contains("1F") && text.contains("0.54") && text.contains("4.50"),
                "{text}"
            ),
            ByCategory => assert!(
                text.contains("小梁") && text.contains("0.54") && text.contains("4.50"),
                "{text}"
            ),
            SteelBySection => assert!(text.contains("支持鋼材"), "{text}"),
            _ => unreachable!(),
        }
    }
    let csv = crate::summary::build_quantity_csv(&app.core.model);
    assert!(
        csv.contains("SM557") && csv.contains("\"非節点小梁\",0.540,4.50,"),
        "{csv}"
    );
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
    assert_eq!(app.core.scoped.undo.revision(), revision);
    assert!(!app.core.scoped.undo.can_undo());
    app.core.model.sections[0].material = Some(MaterialId(999));
    let text = panel_text(&mut app, Detail);
    assert!(
        text.contains("数量積算は未算定") && text.contains("SecondaryMemberId(557)"),
        "{text}"
    );
    let csv = crate::summary::build_quantity_csv(&app.core.model);
    assert!(
        csv.starts_with("[数量積算 未算定]") && csv.contains("SecondaryMemberId(557)"),
        "{csv}"
    );
    assert!(!csv.contains("[数量積算 明細]"));
}

#[test]
fn nonnodal557_gui_and_csv_keep_unknown_story_quantity_and_section_label() {
    let mut app = App::default();
    app.core.model = fixture::placed();
    for node in &mut app.core.model.nodes {
        node.story = None;
    }
    app.core.model.floor_regions[0].secondary_beams[0]
        .name
        .clear();
    let q = sepika_design_jp::quantity::try_compute_quantity_takeoff(
        &app.core.model,
        &app.core.model.quantity_cfg,
    )
    .unwrap();
    assert!((q.totals().concrete_m3 - 0.54).abs() < 1e-12);
    assert!(q
        .notes
        .iter()
        .any(|n| n == "SecondaryMemberId(557): 階帰属未確定（数量は階 - に計上）"));
    let text = panel_text(&mut app, crate::quantity_view::QuantityGrouping::ByStory);
    assert!(text.contains("0.54") && text.contains("4.50"), "{text}");
    let text = panel_text(&mut app, crate::quantity_view::QuantityGrouping::Detail);
    assert!(
        text.contains("SM557") && text.contains("B300x600"),
        "{text}"
    );
    let csv = crate::summary::build_quantity_csv(&app.core.model);
    assert!(
        csv.contains("SM557") && csv.contains("\"B300x600\",0.540,4.50,"),
        "{csv}"
    );
    assert!(csv.contains("階帰属未確定"), "{csv}");
}

#[test]
fn nonnodal557_app_supported_edits_undo_redo_and_ovika_preserve_quantities() {
    let mut app = App::default();
    app.core.model = fixture::edit_model();
    let before = app.core.model.clone();
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::PlaceSecondaryMember {
            parent: sepika_edit::SecondaryParent::Unassigned,
            kind: SecondaryMemberKind::Beam,
            ends: fixture::ends(0.5),
            section: Some(SectionId(0)),
            name: "編集小梁".into(),
        })),
        "{:?}",
        app.core.scoped.last_error
    );
    let id = app.core.model.beams().next().unwrap().id;
    fixture::assert_quantity(&app.core.model, id, 0.54, 4.5);
    let placed = app.core.model.clone();
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::SetSecondaryMemberEnds {
            member: id,
            ends: fixture::moved_ends()
        }))
    );
    fixture::assert_quantity(&app.core.model, id, 0.675, 5.625);
    let moved = app.core.model.clone();
    let text = panel_text(&mut app, crate::quantity_view::QuantityGrouping::Detail);
    assert!(text.contains("0.675") && text.contains("5.62"), "{text}");
    app.undo_action();
    assert!(app.core.model.eq_ignoring_dofmap(&placed));
    fixture::assert_quantity(&app.core.model, id, 0.54, 4.5);
    app.redo_action();
    assert!(app.core.model.eq_ignoring_dofmap(&moved));
    fixture::assert_quantity(&app.core.model, id, 0.675, 5.625);
    let revision = app.core.scoped.undo.revision();
    let path = std::env::temp_dir().join(format!("sepika557-app-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    let loaded = &reopened.core.model;
    assert!(loaded.eq_ignoring_dofmap(&moved));
    assert_eq!(
        loaded.secondary_member(id).unwrap().ends,
        fixture::moved_ends()
    );
    fixture::assert_quantity(loaded, id, 0.675, 5.625);
    assert_eq!(app.core.scoped.undo.revision(), revision);
    std::fs::remove_file(path).unwrap();
    app.undo_action();
    app.undo_action();
    assert!(app.core.model.eq_ignoring_dofmap(&before));
}

#[test]
fn nonnodal557_app_stb_member_fc30_survives_movement_undo_and_ovika() {
    let mut model = fixture::placed();
    let sm = model.floor_regions[0].secondary_beams.remove(0);
    model.floor_regions.clear();
    model.elements.truncate(2);
    model.unassigned_beams.push(sm);
    model = fixture::with_stb_strengths(model, SecondaryMemberId(557));
    model.validate().unwrap();
    let original_strengths = model.stb_strengths.clone();
    assert_eq!(
        model
            .secondary_material(model.secondary_member(SecondaryMemberId(557)).unwrap())
            .unwrap()
            .fc,
        Some(30.0)
    );
    assert_eq!(
        model
            .resolve_stb_concrete(&model.stb_strengths.members[0])
            .unwrap()
            .source,
        StrengthSource::Member
    );
    let mut control = model.clone();
    for (id, coord) in [(4, [1500.0, 0.0, 3000.0]), (5, [1500.0, 3000.0, 3000.0])] {
        let mut node = control.nodes[0].clone();
        node.id = NodeId(id);
        node.coord = coord;
        control.nodes.push(node);
        control.stories[0].node_ids.push(NodeId(id));
    }
    control.validate().unwrap();
    fixture::assert_quantity(&control, SecondaryMemberId(557), 0.54, 4.5);
    assert_eq!(
        control
            .secondary_material(control.secondary_member(SecondaryMemberId(557)).unwrap())
            .unwrap()
            .fc,
        Some(30.0)
    );
    assert_eq!(control.stb_strengths, original_strengths);
    let mut app = App::default();
    app.core.model = model;
    let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
    fixture::assert_quantity(&app.core.model, SecondaryMemberId(557), 0.54, 4.5);
    let text = panel_text(&mut app, crate::quantity_view::QuantityGrouping::Detail);
    assert!(text.contains("SM557") && text.contains("0.540"), "{text}");
    let csv = crate::summary::build_quantity_csv(&app.core.model);
    assert!(csv.contains("\"非節点小梁\",0.540,4.50,"), "{csv}");
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
    assert!(
        app.apply_model_edit(Box::new(sepika_edit::SetSecondaryMemberEnds {
            member: SecondaryMemberId(557),
            ends: fixture::moved_ends()
        }))
    );
    fixture::assert_quantity(&app.core.model, SecondaryMemberId(557), 0.675, 5.625);
    assert_eq!(app.core.model.stb_strengths, original_strengths);
    app.undo_action();
    fixture::assert_quantity(&app.core.model, SecondaryMemberId(557), 0.54, 4.5);
    assert_eq!(app.core.model.stb_strengths, original_strengths);
    app.redo_action();
    fixture::assert_quantity(&app.core.model, SecondaryMemberId(557), 0.675, 5.625);
    assert_eq!(app.core.model.stb_strengths, original_strengths);
    let path = std::env::temp_dir().join(format!("sepika557-stb-app-{}.ovika", std::process::id()));
    app.save_project_to(path.clone());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert!(
        reopened.core.scoped.last_error.is_none(),
        "{:?}",
        reopened.core.scoped.last_error
    );
    let loaded = &reopened.core.model;
    assert_eq!(loaded.stb_strengths, original_strengths);
    assert_eq!(
        loaded
            .secondary_material(loaded.secondary_member(SecondaryMemberId(557)).unwrap())
            .unwrap()
            .fc,
        Some(30.0)
    );
    fixture::assert_quantity(loaded, SecondaryMemberId(557), 0.675, 5.625);
    std::fs::remove_file(path).unwrap();
}
