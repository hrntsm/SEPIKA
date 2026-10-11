use super::{materials::materials_table, nodes::nodes_table};
use crate::app::App;

fn frame(
    ctx: &egui::Context,
    app: &mut App,
    materials: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1800.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            if materials {
                materials_table(ui, app);
            } else {
                nodes_table(ui, app);
            }
        },
    )
}
fn click_text(ctx: &egui::Context, app: &mut App, materials: bool, text: &str, last: bool) {
    frame(ctx, app, materials, vec![]);
    let out = frame(ctx, app, materials, vec![]);
    let positions: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text == text => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .collect();
    let pos = if last {
        *positions.last().unwrap()
    } else {
        positions[0]
    };
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            materials,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

const INVALID: [&str; 12] = [
    "NaN", "nan", "inf", "-inf", "Infinity", "∞", "1e309", "-1e309", "abc", "=1+2", "1,234",
    "10 mm",
];
fn material_draft(ctx: &egui::Context, app: &mut App, values: [&str; 6]) {
    frame(ctx, app, true, vec![]);
    let mut draft: [String; 7] = Default::default();
    draft[0] = "direct".into();
    for (i, v) in values.iter().enumerate() {
        draft[i + 1] = (*v).into();
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("material_custom_draft"), draft));
}
fn snapshot(
    app: &App,
) -> (
    Vec<u8>,
    u64,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
) {
    (
        rmp_serde::to_vec_named(&app.core.model).unwrap(),
        app.core.scoped.undo.revision(),
        app.core.scoped.undo.undo_label().map(str::to_owned),
        app.core.scoped.undo.redo_label().map(str::to_owned),
        format!("{:?}", app.core.scoped.staleness),
        format!("{:?}", app.core.scoped.preparation),
        format!("{:?}", app.core.scoped.results),
    )
}
fn ready_app() -> App {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.run_preparation();
    assert!(app.core.scoped.preparation.is_some());
    let lc = app.core.model.load_cases[0].id;
    app.run_linear_static(lc);
    assert!(!app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .input_records
        .is_empty());
    app
}
fn material_value(app: &App, field: sepika_edit::MaterialField) -> Option<f64> {
    use sepika_edit::MaterialField::*;
    let m = &app.core.model.materials[0];
    match field {
        Young => Some(m.young),
        Poisson => Some(m.poisson),
        Density => Some(m.density),
        Fc => m.fc,
        Fy => m.fy,
        StrengthFactor => m.strength_factor,
    }
}
fn assert_edited(app: &App) {
    let s = &app.core.scoped.staleness;
    assert!(
        s.results_stale
            && s.design_stale
            && s.preparation_stale
            && s.diagnostics_stale
            && s.unsaved_changes
    );
}
fn fresh(app: &mut App) {
    let s = &mut app.core.scoped.staleness;
    s.results_stale = false;
    s.design_stale = false;
    s.preparation_stale = false;
    s.diagnostics_stale = false;
    s.unsaved_changes = false;
}
fn lose_focus(ctx: &egui::Context, app: &mut App) {
    let pos = egui::pos2(1700.0, 850.0);
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            true,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
fn begin_cell_edit(
    ctx: &egui::Context,
    app: &mut App,
    field: sepika_edit::MaterialField,
    text: &str,
) {
    begin_material_cell_edit(ctx, app, sepika_core::ids::MaterialId(0), field, text);
}
fn begin_material_cell_edit(
    ctx: &egui::Context,
    app: &mut App,
    material_id: sepika_core::ids::MaterialId,
    field: sepika_edit::MaterialField,
    text: &str,
) {
    let id =
        super::materials::material_cell_id(material_id, field, app.ui.scoped.material_id_revision);
    frame(ctx, app, true, vec![]);
    let resp = ctx.read_response(id).expect("実材料TextEdit");
    let pos = resp.rect.center();
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            true,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    frame(
        ctx,
        app,
        true,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            },
            egui::Event::Text(text.to_string()),
        ],
    );
    if text.is_empty() {
        frame(
            ctx,
            app,
            true,
            vec![egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}
fn edit_cell(ctx: &egui::Context, app: &mut App, field: sepika_edit::MaterialField, text: &str) {
    begin_cell_edit(ctx, app, field, text);
    lose_focus(ctx, app);
}
#[test]
fn issue546_real_node_add_matrix_preserves_redo_results_and_draft() {
    let ctx = egui::Context::default();
    let mut app = ready_app();
    app.ui.scoped.node_draft = [" 1e3 ".into(), "-2.5e2".into(), "".into()];
    click_text(&ctx, &mut app, false, "+ 追加", false);
    assert_eq!(
        app.core.model.nodes.last().unwrap().coord,
        [1000.0, -250.0, 0.0]
    );
    assert_edited(&app);
    let accepted = rmp_serde::to_vec_named(&app.core.model).unwrap();
    app.core.scoped.undo.undo(&mut app.core.model);
    fresh(&mut app);
    assert!(app.core.scoped.undo.can_redo());
    let before = snapshot(&app);
    for k in 0..3 {
        for invalid in INVALID {
            let mut values = ["1".to_string(), "2".to_string(), "3".to_string()];
            values[k] = invalid.into();
            app.ui.scoped.node_draft = values.clone();
            click_text(&ctx, &mut app, false, "+ 追加", false);
            assert_eq!(snapshot(&app), before, "{k}: {invalid}");
            assert_eq!(app.ui.scoped.node_draft, values);
            assert!(app.ui.scoped.pending_duplicate_node_coord.is_none());
        }
    }
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), accepted);
}
#[test]
fn issue546_real_material_add_matrix_is_atomic_and_preserves_redo() {
    let ctx = egui::Context::default();
    let mut app = ready_app();
    material_draft(
        &ctx,
        &mut app,
        [" 2.05e5 ", "0.3", "7.85e-9", "24", "235", "1.1"],
    );
    click_text(&ctx, &mut app, true, "+ 追加", true);
    let m = app.core.model.materials.last().unwrap();
    assert_eq!(
        (m.young, m.poisson, m.density, m.fc, m.fy, m.strength_factor),
        (205000.0, 0.3, 7.85e-9, Some(24.0), Some(235.0), Some(1.1))
    );
    assert_edited(&app);
    let accepted = rmp_serde::to_vec_named(&app.core.model).unwrap();
    app.core.scoped.undo.undo(&mut app.core.model);
    fresh(&mut app);
    let before = snapshot(&app);
    for k in 0..6 {
        for invalid in INVALID {
            let mut values = ["205000", "0.3", "7.85e-9", "24", "235", "1.1"];
            values[k] = invalid;
            material_draft(&ctx, &mut app, values);
            click_text(&ctx, &mut app, true, "+ 追加", true);
            assert_eq!(snapshot(&app), before, "{k}: {invalid}");
            assert_eq!(
                ctx.data(|d| d.get_temp::<[String; 7]>(egui::Id::new("material_custom_draft")))
                    .unwrap()[k + 1],
                invalid
            );
        }
    }
    for k in 0..3 {
        let mut values = ["205000", "0.3", "7.85e-9", "", "", ""];
        values[k] = "  ";
        material_draft(&ctx, &mut app, values);
        click_text(&ctx, &mut app, true, "+ 追加", true);
        assert_eq!(snapshot(&app), before);
    }
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), accepted);
}
#[test]
fn issue546_real_single_text_edit_matrix_retains_draft_and_redo() {
    use sepika_edit::MaterialField::*;
    let ctx = egui::Context::default();
    let mut app = ready_app();
    edit_cell(&ctx, &mut app, Young, "2.04e5");
    assert_eq!(app.core.model.materials[0].young, 204000.0);
    assert_edited(&app);
    let accepted = rmp_serde::to_vec_named(&app.core.model).unwrap();
    app.core.scoped.undo.undo(&mut app.core.model);
    fresh(&mut app);
    let before = snapshot(&app);
    for field in [Young, Poisson, Density, Fc, Fy, StrengthFactor] {
        for invalid in INVALID {
            edit_cell(&ctx, &mut app, field, invalid);
            assert_eq!(snapshot(&app), before, "{field:?}: {invalid}");
            assert_eq!(
                ctx.data(|d| d.get_temp::<String>(super::materials::material_cell_id(
                    sepika_core::ids::MaterialId(0),
                    field,
                    app.ui.scoped.material_id_revision
                )))
                .as_deref(),
                Some(invalid)
            );
            assert!(app
                .core
                .scoped
                .last_error
                .as_ref()
                .unwrap()
                .contains("材料 0"));
        }
    }
    for field in [Young, Poisson, Density] {
        edit_cell(&ctx, &mut app, field, "");
        assert_eq!(snapshot(&app), before);
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("必須空欄"));
    }
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), accepted);
}
#[test]
fn issue546_real_duplicate_confirmation_rechecks_finite_pending_coordinate() {
    let ctx = egui::Context::default();
    let mut app = App::default();
    app.ui.scoped.node_draft = ["0".into(), "0".into(), "0".into()];
    click_text(&ctx, &mut app, false, "+ 追加", false);
    click_text(&ctx, &mut app, false, "+ 追加", false);
    assert_eq!(app.ui.scoped.pending_duplicate_node_coord, Some([0.0; 3]));
    click_text(&ctx, &mut app, false, "追加する", false);
    assert_eq!(app.core.model.nodes.len(), 2);
    app.core.scoped.undo.undo(&mut app.core.model);
    fresh(&mut app);
    let before = snapshot(&app);
    for k in 0..3 {
        let mut coord = [0.0; 3];
        coord[k] = f64::NAN;
        app.ui.scoped.pending_duplicate_node_coord = Some(coord);
        click_text(&ctx, &mut app, false, "追加する", false);
        assert_eq!(snapshot(&app), before);
        assert!(app.ui.scoped.pending_duplicate_node_coord.unwrap()[k].is_nan());
    }
    app.ui.scoped.pending_duplicate_node_coord = None;
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(app.core.model.nodes.len(), 2);
}
#[test]
fn issue546_finite_controls_through_real_add_and_single_edit() {
    use sepika_edit::MaterialField::*;
    for text in [" 1e3 ", "0", "-0", "-2.5", "5e-324"] {
        let ctx = egui::Context::default();
        let mut app = App::default();
        material_draft(&ctx, &mut app, [text, text, text, text, text, text]);
        click_text(&ctx, &mut app, true, "+ 追加", true);
        let value = text.trim().parse::<f64>().unwrap();
        let m = &app.core.model.materials[0];
        assert_eq!(
            (m.young, m.poisson, m.density, m.fc, m.fy, m.strength_factor),
            (value, value, value, Some(value), Some(value), Some(value))
        );
        assert_eq!(m.young.to_bits(), value.to_bits());
        assert_eq!(m.fc.unwrap().to_bits(), value.to_bits());
        app.core.scoped.undo.undo(&mut app.core.model);
        assert!(app.core.model.materials.is_empty());
        app.core.scoped.undo.redo(&mut app.core.model);
        assert_eq!(app.core.model.materials.len(), 1);
        for field in [Young, Poisson, Density, Fc, Fy, StrengthFactor] {
            edit_cell(&ctx, &mut app, field, "2");
            assert_eq!(material_value(&app, field), Some(2.0));
            edit_cell(&ctx, &mut app, field, text);
            assert_eq!(
                material_value(&app, field).unwrap().to_bits(),
                value.to_bits()
            );
        }
        app.ui.scoped.node_draft = [text.into(), text.into(), text.into()];
        click_text(&ctx, &mut app, false, "+ 追加", false);
        assert_eq!(app.core.model.nodes[0].coord, [value; 3]);
        assert_eq!(app.core.model.nodes[0].coord[0].to_bits(), value.to_bits());
    }
    let ctx = egui::Context::default();
    let mut app = App::default();
    material_draft(&ctx, &mut app, ["205000", "0.3", "7.85e-9", "  ", "", ""]);
    click_text(&ctx, &mut app, true, "+ 追加", true);
    assert_eq!(app.core.model.materials[0].fc, None);
    for field in [Fc, Fy, StrengthFactor] {
        edit_cell(&ctx, &mut app, field, "24");
        edit_cell(&ctx, &mut app, field, "");
    }
    let m = &app.core.model.materials[0];
    assert_eq!((m.fc, m.fy, m.strength_factor), (None, None, None));
    fresh(&mut app);
    let before = snapshot(&app);
    edit_cell(&ctx, &mut app, Young, " 2.05e5 ");
    assert_eq!(snapshot(&app), before);
    app.ui.scoped.node_draft = ["".into(), "  ".into(), "".into()];
    click_text(&ctx, &mut app, false, "+ 追加", false);
    assert_eq!(app.core.model.nodes[0].coord, [0.0; 3]);
}

#[test]
fn issue546_real_stb_strength_loss_focus_and_standard_e_contract() {
    use sepika_core::model::{SourceStory, SourceStoryKind, StbCommonStrength};
    use sepika_edit::MaterialField::*;
    let ctx = egui::Context::default();
    let mut app = App::default();
    app.core.model.stb_strengths.common = Some(StbCommonStrength {
        project_name: "fixture".into(),
        app_name: "STB".into(),
        app_version: "2".into(),
        strength_concrete: Some("Fc24".into()),
        reinforcement: vec![],
    });
    app.core.model.source_stories.push(SourceStory {
        id: 1,
        guid: None,
        name: "1F".into(),
        height: 0.0,
        kind: SourceStoryKind::General,
        kind_from_native: false,
        id_dependence: None,
        strength_concrete: Some("Fc24".into()),
        node_ids: vec![],
    });
    app.core.model.prepare_stb_strength_materials();
    fresh(&mut app);
    let original = snapshot(&app);
    for text in ["NaN", "abc"] {
        edit_cell(&ctx, &mut app, Fc, text);
        assert_eq!(snapshot(&app), original);
    }
    edit_cell(&ctx, &mut app, Young, "20000");
    assert_eq!(snapshot(&app), original);
    edit_cell(&ctx, &mut app, Fc, "");
    assert_eq!(app.core.model.materials[0].fc, None);
    assert_eq!(app.core.model.stb_strengths.materials[0].grade, "未指定Fc");
    assert_eq!(
        app.core.model.source_stories[0]
            .strength_concrete
            .as_deref(),
        Some("未指定Fc")
    );
    let clear = rmp_serde::to_vec_named(&app.core.model).unwrap();
    app.core.scoped.undo.undo(&mut app.core.model);
    assert_eq!(
        rmp_serde::to_vec_named(&app.core.model).unwrap(),
        original.0
    );
    app.core.scoped.undo.redo(&mut app.core.model);
    assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), clear);
    app.core.model.materials[0].concrete_class = sepika_core::units::ConcreteClass::UserDefined;
    edit_cell(&ctx, &mut app, Young, "20000");
    assert_eq!(app.core.model.materials[0].young, 20000.0);
}

#[test]
fn issue546_real_pending_text_edit_rechecks_deletion_and_category_change() {
    use sepika_edit::MaterialField::Young;
    let ctx = egui::Context::default();
    let mut app = App::default();
    material_draft(&ctx, &mut app, ["205000", "0.3", "7.85e-9", "", "", ""]);
    click_text(&ctx, &mut app, true, "+ 追加", true);
    begin_cell_edit(&ctx, &mut app, Young, "203000");
    app.core.model.materials[0].category = sepika_core::model::MaterialCategory::Concrete;
    app.core.model.materials[0].concrete_class = sepika_core::units::ConcreteClass::Normal;
    fresh(&mut app);
    let before = snapshot(&app);
    lose_focus(&ctx, &mut app);
    frame(&ctx, &mut app, true, vec![]);
    assert_eq!(snapshot(&app), before);
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("編集不可"));
    assert_eq!(
        ctx.data(|d| d.get_temp::<String>(super::materials::material_cell_id(
            sepika_core::ids::MaterialId(0),
            Young,
            app.ui.scoped.material_id_revision
        )))
        .as_deref(),
        Some("203000")
    );
    app.core.model.materials[0].category = sepika_core::model::MaterialCategory::Steel;
    begin_cell_edit(&ctx, &mut app, Young, "202000");
    app.core.model.materials.clear();
    fresh(&mut app);
    let before = snapshot(&app);
    lose_focus(&ctx, &mut app);
    assert_eq!(snapshot(&app), before);
    assert!(app
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("参照不明"));
    assert_eq!(
        ctx.data(|d| d.get_temp::<String>(super::materials::material_cell_id(
            sepika_core::ids::MaterialId(0),
            Young,
            app.ui.scoped.material_id_revision
        )))
        .as_deref(),
        Some("202000")
    );
}

#[test]
fn issue546_real_signed_zero_changes_all_fields_with_undo_redo() {
    use sepika_edit::MaterialField::*;
    let ctx = egui::Context::default();
    let mut app = App::default();
    material_draft(&ctx, &mut app, ["0", "0", "0", "0", "0", "0"]);
    click_text(&ctx, &mut app, true, "+ 追加", true);
    for field in [Young, Poisson, Density, Fc, Fy, StrengthFactor] {
        for text in ["-0", "+0"] {
            let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
            let revision = app.core.scoped.undo.revision();
            edit_cell(&ctx, &mut app, field, text);
            let expected = if text == "-0" { -0.0f64 } else { 0.0 };
            assert_eq!(
                material_value(&app, field).unwrap().to_bits(),
                expected.to_bits()
            );
            assert_eq!(app.core.scoped.undo.revision(), revision + 1);
            let after = rmp_serde::to_vec_named(&app.core.model).unwrap();
            assert_ne!(after, before);
            app.undo_action();
            assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
            app.redo_action();
            assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), after);
        }
        fresh(&mut app);
        let before = snapshot(&app);
        edit_cell(&ctx, &mut app, field, " 0.0e0 ");
        assert_eq!(snapshot(&app), before);
    }
}

#[test]
fn issue546_real_id_reassignment_never_applies_an_old_draft_to_another_material() {
    use sepika_core::ids::MaterialId;
    use sepika_edit::{DeleteMaterial, MaterialField::Young};
    for (target, deleted) in [(0u32, 0u32), (1, 0)] {
        let ctx = egui::Context::default();
        let mut app = App::default();
        for (name, young) in [("A", "100"), ("B", "200"), ("C", "300")] {
            material_draft(&ctx, &mut app, [young, "0.3", "7.85e-9", "", "", ""]);
            ctx.data_mut(|d| {
                let id = egui::Id::new("material_custom_draft");
                let mut draft = d.get_temp::<[String; 7]>(id).unwrap();
                draft[0] = name.into();
                d.insert_temp(id, draft);
            });
            click_text(&ctx, &mut app, true, "+ 追加", true);
        }
        let old_cell = super::materials::material_cell_id(
            MaterialId(target),
            Young,
            app.ui.scoped.material_id_revision,
        );
        begin_material_cell_edit(&ctx, &mut app, MaterialId(target), Young, "150");
        assert!(app.apply_model_edit(Box::new(DeleteMaterial {
            id: MaterialId(deleted)
        })));
        fresh(&mut app);
        let before = snapshot(&app);
        lose_focus(&ctx, &mut app);
        frame(&ctx, &mut app, true, vec![]);
        assert_eq!(snapshot(&app), before);
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("参照不明"));
        assert_eq!(
            ctx.data(|d| d.get_temp::<String>(old_cell)).as_deref(),
            Some("150")
        );
        assert_eq!(
            ctx.data(|d| d.get_temp::<bool>(old_cell.with("changed"))),
            Some(true)
        );
        assert_eq!(app.core.model.materials[0].young, 200.0);
        assert_eq!(app.core.model.materials[1].young, 300.0);
        app.undo_action();
        frame(&ctx, &mut app, true, vec![]);
        fresh(&mut app);
        let before = snapshot(&app);
        lose_focus(&ctx, &mut app);
        assert_eq!(snapshot(&app), before);
        assert!(app.core.scoped.undo.can_redo());
        app.redo_action();
        frame(&ctx, &mut app, true, vec![]);
        assert_eq!(app.core.model.materials[0].young, 200.0);
        assert_eq!(app.core.model.materials[1].young, 300.0);
        begin_material_cell_edit(&ctx, &mut app, MaterialId(0), Young, "250");
        lose_focus(&ctx, &mut app);
        assert_eq!(app.core.model.materials[0].young, 250.0);
        assert_eq!(app.core.model.materials[1].young, 300.0);
    }
}

#[test]
fn issue546_real_model_switch_discards_material_inputs_only() {
    use sepika_core::ids::MaterialId;
    use sepika_edit::{DeleteMaterial, MaterialField::Young};
    for (new_model, id_change) in [(false, false), (false, true), (true, false), (true, true)] {
        for draft in ["abc", "150"] {
            let ctx = egui::Context::default();
            let mut app = App::default();
            let mut old = crate::sample::portal_frame();
            old.materials[0].young = 100.0;
            old.materials.push(old.materials[0].clone());
            old.materials.last_mut().unwrap().id = MaterialId(old.materials.len() as u32 - 1);
            assert!(app.load_model(old));
            // Keep a rejected old-revision draft as well as the active current cell.
            begin_cell_edit(&ctx, &mut app, Young, "abc");
            lose_focus(&ctx, &mut app);
            let original = super::materials::material_cell_id(MaterialId(0), Young, 0);
            if id_change {
                let last = MaterialId(app.core.model.materials.len() as u32 - 1);
                assert!(app.apply_model_edit(Box::new(DeleteMaterial { id: last })));
            }
            begin_cell_edit(&ctx, &mut app, Young, draft);
            if draft == "abc" {
                lose_focus(&ctx, &mut app);
            }
            let current = super::materials::material_cell_id(
                MaterialId(0),
                Young,
                app.ui.scoped.material_id_revision,
            );
            material_draft(&ctx, &mut app, ["123", "0.3", "1", "", "", ""]);
            let unrelated = egui::Id::new("unrelated_table_draft");
            ctx.data_mut(|d| d.insert_temp(unrelated, String::from("keep")));
            if new_model {
                assert!(app.load_model(sepika_core::Model::with_default_load_cases()));
                frame(&ctx, &mut app, true, vec![]);
                material_draft(&ctx, &mut app, ["200", "0.3", "1", "", "", ""]);
                click_text(&ctx, &mut app, true, "+ 追加", true);
            } else {
                let mut replacement = crate::sample::portal_frame();
                replacement.materials[0].young = 200.0;
                assert!(app.load_model(replacement));
            }
            let before = snapshot(&app);
            lose_focus(&ctx, &mut app);
            frame(&ctx, &mut app, true, vec![]);
            assert_eq!(snapshot(&app), before);
            assert_eq!(app.core.model.materials[0].young, 200.0);
            for cell in [original, current] {
                assert!(ctx.data(|d| d.get_temp::<String>(cell)).is_none());
                assert!(ctx
                    .data(|d| d.get_temp::<bool>(cell.with("changed")))
                    .is_none());
            }
            assert_eq!(
                ctx.data(|d| d.get_temp::<String>(unrelated)).as_deref(),
                Some("keep")
            );
            begin_cell_edit(&ctx, &mut app, Young, "250");
            lose_focus(&ctx, &mut app);
            assert_eq!(app.core.model.materials[0].young, 250.0);
        }
    }
}
