use super::*;
use sepika_core::model::PostLateralSupport;
#[cfg(feature = "gui")]
use sepika_core::model::SecondaryMemberKind;
mod fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sepika-core/tests/support/steel_secondary_post_model.rs"
    ));
}
fn calculated() -> App {
    let mut app = App::default();
    app.load_model(fixture::steel_post_model());
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.run_design_check();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let post = app
        .current_design_results()
        .expect("直接間柱検定")
        .beam_checks[0]
        .2
        .post
        .as_ref()
        .unwrap();
    assert!(post.complete(), "{post:?}");
    app
}
#[test]
fn post_inputs_results_roundtrip_and_design_edit_invalidates() {
    let app = calculated();
    let version = sepika_core::result_contract::DESIGN_CONTRACT_VERSION;
    let (mut loaded, path) =
        super::result_contract_tests::restore(&app, None, Some(version), "1097-post");
    assert_eq!(
        loaded.core.model.post_design_inputs,
        app.core.model.post_design_inputs
    );
    assert_eq!(
        loaded.current_design_results().unwrap().beam_checks,
        app.current_design_results().unwrap().beam_checks
    );
    let mut changed = loaded.core.model.post_design_inputs[0].clone();
    changed.lateral_support = PostLateralSupport::FixedStart;
    assert!(loaded.core.scoped.undo.run(
        &mut loaded.core.model,
        Box::new(sepika_edit::SetPostDesignInput {
            member: changed.member,
            input: Some(changed)
        })
    ));
    loaded.core.scoped.staleness.mark_edited();
    assert!(loaded.current_design_results().is_none());
    loaded.run_design_check();
    assert!(loaded.current_design_results().unwrap().beam_checks[0]
        .2
        .post
        .as_ref()
        .unwrap()
        .complete());
    let (old, old_path) =
        super::result_contract_tests::restore(&app, None, Some(version - 1), "1097-old");
    assert!(old.current_design_results().is_none());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(old_path).unwrap();
}
#[cfg(feature = "gui")]
#[test]
fn production_post_input_and_design_button_pointer_events() {
    let mut app = calculated();
    let ctx = egui::Context::default();
    fn draw(
        ctx: &egui::Context,
        app: &mut App,
        input: bool,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2600.0, 5000.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                if input {
                    crate::tables::secondary::secondary_member_list(
                        app,
                        ui,
                        SecondaryMemberKind::Post,
                    );
                } else {
                    crate::design_view::design_table(ui, app);
                }
            },
        )
    }
    fn pos(out: &egui::FullOutput, text: &str) -> egui::Pos2 {
        for exact in [true, false] {
            if let Some(p) = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if if exact {
                        t.galley.job.text == text
                    } else {
                        t.galley.job.text.contains(text)
                    } =>
                {
                    Some(t.pos + t.galley.size() / 2.0)
                }
                _ => None,
            }) {
                return p;
            }
        }
        panic!("表示なし: {text}")
    }
    fn click(ctx: &egui::Context, app: &mut App, input: bool, p: egui::Pos2) {
        for pressed in [true, false] {
            draw(
                ctx,
                app,
                input,
                vec![
                    egui::Event::PointerMoved(p),
                    egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "SM0 検定入力"));
    let out = draw(&ctx, &mut app, true, vec![]);
    click(&ctx, &mut app, true, pos(&out, "始端固定・終端自由"));
    assert_eq!(
        app.core.model.post_design_inputs[0].lateral_support,
        PostLateralSupport::FixedStart
    );
    assert!(app.current_design_results().is_none());
    draw(&ctx, &mut app, false, vec![]);
    let out = draw(&ctx, &mut app, false, vec![]);
    click(&ctx, &mut app, false, pos(&out, "検定を再実行"));
    assert!(
        app.current_design_results().is_some(),
        "error={:?}, validity={:?}",
        app.core.scoped.last_error,
        app.require_design_input()
    );
    let out = draw(&ctx, &mut app, false, vec![]);
    let p = pos(&out, "間柱 間柱P1");
    click(&ctx, &mut app, false, p);
    let out = draw(&ctx, &mut app, false, vec![]);
    assert!(out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text.contains("自動転送しません"))));
}

#[cfg(feature = "gui")]
#[test]
fn section_editor_shows_missing_corner_radius_before_commit() {
    let mut app = App::default();
    app.ui.scoped.section_draft.kind = crate::section_editor::ShapeKind::SteelBox;
    let ctx = egui::Context::default();
    let before = app.core.model.clone();
    let revision = app.core.scoped.undo.revision();
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2600.0, 4000.0),
            )),
            ..Default::default()
        },
        |ui| crate::section_editor::section_editor_panel(ui, &mut app),
    );
    assert!(out.shapes.iter().any(
        |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text.contains("角部外半径"))
    ));
    assert!(app.core.model.eq_ignoring_dofmap(&before));
    assert_eq!(app.core.scoped.undo.revision(), revision);
}
