use super::{load_case_name_id, loads_table};
use crate::app::App;

fn frame(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1800.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| loads_table(ui, app),
    )
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn click(ctx: &egui::Context, app: &mut App, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
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

fn begin(ctx: &egui::Context, app: &mut App, text: &str) {
    begin_row(ctx, app, 0, text);
}

fn begin_row(ctx: &egui::Context, app: &mut App, row: usize, text: &str) {
    frame(ctx, app, vec![]);
    frame(ctx, app, vec![]);
    let id = load_case_name_id(app.core.model.load_cases[row].id);
    let pos = ctx
        .read_response(id)
        .expect("荷重ケース名称欄")
        .rect
        .center();
    click(ctx, app, pos);
    let mut select_all = key(egui::Key::A);
    if let egui::Event::Key { modifiers, .. } = &mut select_all {
        modifiers.ctrl = true;
        modifiers.command = true;
    }
    frame(ctx, app, vec![select_all, key(egui::Key::Backspace)]);
    if !text.is_empty() {
        frame(ctx, app, vec![egui::Event::Text(text.into())]);
    }
}

fn assert_displayed(ctx: &egui::Context, app: &mut App, expected: &str) {
    let out = frame(ctx, app, vec![]);
    let id = load_case_name_id(app.core.model.load_cases[0].id);
    let rect = ctx.read_response(id).unwrap().rect;
    assert!(
        out.shapes.iter().any(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) =>
                text.galley.job.text == expected && rect.intersects(text.visual_bounding_rect()),
            _ => false,
        }),
        "名称欄の表示: {expected}"
    );
}

#[test]
fn typed_load_case_name_survives_frames_and_commits_once_on_enter_or_blur() {
    for (first, second, enter) in [("DL_", "edited", true), ("固定", "荷重", false)] {
        let ctx = egui::Context::default();
        let mut app = App::default();
        assert!(app.load_model(sepika_core::Model::with_default_load_cases()));
        let original = app.core.model.load_cases[0].name.clone();
        let revision = app.core.scoped.undo.revision();
        begin(&ctx, &mut app, first);
        assert_displayed(&ctx, &mut app, first);
        assert_eq!(app.core.model.load_cases[0].name, original);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        frame(&ctx, &mut app, vec![egui::Event::Text(second.into())]);
        let expected = format!("{first}{second}");
        assert_displayed(&ctx, &mut app, &expected);
        if enter {
            frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        } else {
            click(&ctx, &mut app, egui::pos2(1700.0, 850.0));
        }
        assert_eq!(app.core.model.load_cases[0].name, expected);
        assert_eq!(app.core.scoped.undo.revision(), revision + 1);
        assert!(app.core.scoped.staleness.unsaved_changes);
        assert_displayed(&ctx, &mut app, &expected);
        app.undo_action();
        assert_eq!(app.core.model.load_cases[0].name, original);
        assert_displayed(&ctx, &mut app, &original);
        app.redo_action();
        assert_eq!(app.core.model.load_cases[0].name, expected);
        assert_displayed(&ctx, &mut app, &expected);

        let path =
            std::env::temp_dir().join(format!("sepika-load-name-{}.ovika", uuid::Uuid::new_v4()));
        app.save_project_to(path.clone());
        assert!(path.is_file(), "保存結果: {:?}", app.core.scoped.last_error);
        let restored = sepika_io::ovika::load_ovika(&path).unwrap().model;
        std::fs::remove_file(path).unwrap();
        assert_eq!(restored.load_cases[0].name, expected);
        assert!(app.load_model(restored));
        assert_displayed(&ctx, &mut app, &expected);
    }
}

#[test]
fn empty_cancelled_and_unchanged_names_preserve_model_and_redo() {
    let ctx = egui::Context::default();
    let mut app = App::default();
    assert!(app.load_model(sepika_core::Model::with_default_load_cases()));
    let original = app.core.model.load_cases[0].name.clone();
    begin(&ctx, &mut app, "renamed");
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    app.undo_action();
    let revision = app.core.scoped.undo.revision();
    for (text, finish) in [
        ("", egui::Key::Enter),
        ("   ", egui::Key::Enter),
        ("cancelled", egui::Key::Escape),
        (original.as_str(), egui::Key::Enter),
    ] {
        begin(&ctx, &mut app, text);
        frame(&ctx, &mut app, vec![key(finish)]);
        assert_eq!(app.core.model.load_cases[0].name, original);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert!(app.core.scoped.undo.can_redo());
        assert_displayed(&ctx, &mut app, &original);
    }
    begin(&ctx, &mut app, "  fixed load  ");
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.core.model.load_cases[0].name, "fixed load");
    assert_displayed(&ctx, &mut app, "fixed load");
}

#[test]
fn model_replacement_discards_active_name_for_the_same_load_case_id() {
    let ctx = egui::Context::default();
    let mut app = App::default();
    assert!(app.load_model(sepika_core::Model::with_default_load_cases()));
    begin(&ctx, &mut app, "未確定の名称");
    let mut replacement = sepika_core::Model::with_default_load_cases();
    assert_eq!(
        app.core.model.load_cases[0].id,
        replacement.load_cases[0].id
    );
    replacement.load_cases[0].name = "読込後".into();
    assert!(app.load_model(replacement));
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.core.model.load_cases[0].name, "読込後");
    assert!(!app.core.scoped.undo.can_undo());
    assert_displayed(&ctx, &mut app, "読込後");
}

#[test]
fn switching_to_an_earlier_row_commits_the_previous_name() {
    let ctx = egui::Context::default();
    let mut app = App::default();
    assert!(app.load_model(sepika_core::Model::with_default_load_cases()));
    begin_row(&ctx, &mut app, 1, "second row");
    frame(&ctx, &mut app, vec![]);
    let id = load_case_name_id(app.core.model.load_cases[0].id);
    let pos = ctx.read_response(id).unwrap().rect.center();
    click(&ctx, &mut app, pos);
    assert_eq!(app.core.model.load_cases[1].name, "second row");
    assert_eq!(app.core.scoped.undo.revision(), 1);
}

#[test]
fn study_switch_keeps_main_name_draft_out_of_the_other_model() {
    use crate::app::study::Location;
    let ctx = egui::Context::default();
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    let s = app
        .create_study("名称入力の分離".into(), vec![sepika_core::ids::ElemId(2)])
        .unwrap();
    let original = app.core.model.load_cases[0].name.clone();
    begin(&ctx, &mut app, "本案で入力中");
    app.switch_study(Location::Proposal(s, 0)).unwrap();
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.core.model.load_cases[0].name, original);
    assert_displayed(&ctx, &mut app, &original);
    app.switch_study(Location::Main).unwrap();
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.core.model.load_cases[0].name, "本案で入力中");
    assert_displayed(&ctx, &mut app, "本案で入力中");
}

#[test]
fn scrolling_the_active_name_out_of_view_preserves_the_edit() {
    let ctx = egui::Context::default();
    let mut app = App::default();
    assert!(app.load_model(sepika_core::Model::with_default_load_cases()));
    let template = app.core.model.load_cases[0].clone();
    for id in 100..200 {
        let mut case = template.clone();
        case.id = sepika_core::ids::LoadCaseId(id);
        case.name = format!("LC{id}");
        app.core.model.load_cases.push(case);
    }
    begin(&ctx, &mut app, "スクロール中の名称");
    let pos = ctx
        .read_response(load_case_name_id(template.id))
        .unwrap()
        .rect
        .center();
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, -1800.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let mut hidden = false;
    for _ in 0..20 {
        let out = frame(&ctx, &mut app, vec![]);
        hidden |= !out.shapes.iter().any(|shape| {
            matches!(&shape.shape,
            egui::epaint::Shape::Text(text) if text.galley.job.text == "スクロール中の名称")
        });
    }
    assert!(hidden, "編集中の行を可視範囲外へスクロールする");
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0.0, 10000.0),
            modifiers: egui::Modifiers::NONE,
        }],
    );
    for _ in 0..30 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_displayed(&ctx, &mut app, "スクロール中の名称");
    let pos = ctx
        .read_response(load_case_name_id(template.id))
        .unwrap()
        .rect
        .center();
    click(&ctx, &mut app, pos);
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(app.core.model.load_cases[0].name, "スクロール中の名称");
}
