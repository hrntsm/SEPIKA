use super::*;
use sepika_core::ids::{ElemId, StoryId};

fn frame(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| viewer_panel(ui, app),
    )
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

fn highlighted(output: &egui::FullOutput) -> bool {
    output.shapes.iter().any(|shape| match &shape.shape {
        egui::Shape::LineSegment { stroke, .. } => {
            stroke.color == theme::PARETO_RED && stroke.width == 4.0
        }
        egui::Shape::Path(path) => {
            path.stroke.color == egui::epaint::ColorMode::Solid(theme::PARETO_RED)
                && path.stroke.width == 4.0
        }
        _ => false,
    })
}

#[test]
fn hover_and_click_share_visible_solid_face_and_leave_model_history_unchanged() {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.ui.view.show_sections = true;
    app.ui.view.camera.zoom = 12.0;
    let ctx = egui::Context::default();
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    let face = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Path(path)
                if path.points.len() == 4 && path.fill != egui::Color32::TRANSPARENT =>
            {
                let center = path
                    .points
                    .iter()
                    .map(|p| p.to_vec2())
                    .fold(egui::Vec2::ZERO, |a, b| a + b)
                    / 4.0;
                (shape.clip_rect.contains(center.to_pos2()) && center.x > 400.0 && center.x < 850.0)
                    .then_some(center.to_pos2())
            }
            _ => None,
        })
        .expect("描画された梁の面");
    let hover = frame(&ctx, &mut app, vec![egui::Event::PointerMoved(face)]);
    assert!(highlighted(&hover));
    assert!(app.ui.scoped.selection.members().is_empty());
    let model = serde_json::to_value(&app.core.model).unwrap();
    let undo = (
        app.core.scoped.undo.can_undo(),
        app.core.scoped.undo.revision(),
    );
    let staleness = format!("{:?}", app.core.scoped.staleness);
    click(&ctx, &mut app, face);
    assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(2)));
    assert_eq!(serde_json::to_value(&app.core.model).unwrap(), model);
    assert_eq!(
        (
            app.core.scoped.undo.can_undo(),
            app.core.scoped.undo.revision()
        ),
        undo
    );
    assert_eq!(format!("{:?}", app.core.scoped.staleness), staleness);
    click(&ctx, &mut app, egui::pos2(1200.0, 740.0));
    assert!(app.ui.scoped.selection.members().is_empty());
}

#[test]
fn hidden_selected_members_keep_shared_selection_and_regain_highlight_when_shown() {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    app.select_members(vec![ElemId(0), ElemId(2)], Some(ElemId(2)));
    let ctx = egui::Context::default();
    frame(&ctx, &mut app, vec![]);
    assert!(highlighted(&frame(&ctx, &mut app, vec![])));
    app.ui.scoped.frame_target = Some(sepika_core::frame::FrameTarget::Story(StoryId(0)));
    assert!(!highlighted(&frame(&ctx, &mut app, vec![])));
    assert_eq!(app.ui.scoped.selection.members(), &[ElemId(0), ElemId(2)]);
    app.ui.scoped.frame_target = None;
    assert!(highlighted(&frame(&ctx, &mut app, vec![])));
    app.ui.view.view_mode = ViewMode::LumpedMode;
    app.ui.view.lumped_show_frame = false;
    assert!(!highlighted(&frame(&ctx, &mut app, vec![])));
    assert_eq!(app.ui.scoped.selection.members(), &[ElemId(0), ElemId(2)]);
}

fn beam_point(output: &egui::FullOutput) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, stroke }
                if stroke.color == theme::GRAY_700 && stroke.width == 2.0 =>
            {
                Some(*points)
            }
            _ => None,
        })
        .max_by(|a, b| a[0].distance(a[1]).total_cmp(&b[0].distance(b[1])))
        .map(|ends| ends[0].lerp(ends[1], 0.5))
        .expect("通常の梁本体線")
}

#[test]
fn dedicated_creation_does_not_receive_normal_member_hover_or_click() {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    let ctx = egui::Context::default();
    frame(&ctx, &mut app, vec![]);
    let point = beam_point(&frame(&ctx, &mut app, vec![]));
    assert!(highlighted(&frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(point)]
    )));
    app.ui.scoped.beam_draw_mode = true;
    assert!(!highlighted(&frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(point)]
    )));
    click(&ctx, &mut app, point);
    assert!(app.ui.scoped.selection.members().is_empty());
    assert!(app.ui.scoped.beam_draw_first.is_none());
}

#[test]
fn viewcube_has_input_priority_over_a_member_behind_it() {
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    let ctx = egui::Context::default();
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    let point = beam_point(&output);
    let rect = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(r) if r.fill == theme::VIEW_BG => Some(r.rect),
            _ => None,
        })
        .unwrap();
    let cube_point = egui::pos2(rect.max.x - 55.0, rect.min.y + 40.0);
    app.ui.view.camera.pan = [(cube_point - point).x, (cube_point - point).y];
    let out = frame(&ctx, &mut app, vec![egui::Event::PointerMoved(cube_point)]);
    assert!(!highlighted(&out));
    let before = app.ui.view.camera.rot;
    click(&ctx, &mut app, cube_point);
    assert!(app.ui.scoped.selection.members().is_empty());
    assert_ne!(app.ui.view.camera.rot, before);
}

#[test]
fn foreground_window_and_popup_block_member_hover_and_click_behind_them() {
    for order in [egui::Order::Middle, egui::Order::Foreground] {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![]);
        let point = beam_point(&frame(&ctx, &mut app, vec![]));
        let mut front_clicked = false;
        for pressed in [None, None, None, Some(true), Some(false)] {
            let mut events = vec![egui::Event::PointerMoved(point)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    viewer_panel(ui, &mut app);
                    egui::Area::new(egui::Id::new("front"))
                        .order(order)
                        .fixed_pos(point - egui::vec2(60.0, 60.0))
                        .show(ui.ctx(), |ui| {
                            let (_, response) = ui.allocate_exact_size(
                                egui::vec2(200.0, 150.0),
                                egui::Sense::click(),
                            );
                            front_clicked |= response.clicked();
                        });
                },
            );
            if pressed.is_some() {
                assert!(!highlighted(&output));
            }
        }
        assert!(front_clicked);
        assert!(app.ui.scoped.selection.members().is_empty());
    }
}

#[test]
fn hinge_and_time_history_clicks_update_detail_targets_without_editing_the_model() {
    for mode in [ViewMode::Hinge, ViewMode::TimeHistory] {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.ui.view.view_mode = mode;
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![]);
        let point = beam_point(&frame(&ctx, &mut app, vec![]));
        let revision = app.core.scoped.undo.revision();
        click(&ctx, &mut app, point);
        assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(2)));
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(app.ui.view.view_mode, mode);
        if mode == ViewMode::Hinge {
            assert_eq!(app.ui.scoped.hinge_detail_elem, Some(ElemId(2)));
        } else {
            assert_eq!(app.ui.scoped.th_detail_elem, Some(ElemId(2)));
        }
    }
}

#[test]
fn deformed_result_selection_keeps_results_and_only_editing_switches_to_rest_shape() {
    use crate::app::{ResultsBundle, StaticCaseKey, StaticKey};
    use sepika_core::ids::LoadCaseId;
    let mut app = App::default();
    app.load_model(crate::sample::portal_frame());
    let mut disp = vec![[0.0; 6]; 4];
    disp[2][0] = 50.0;
    disp[3][0] = 50.0;
    app.core.scoped.results = Some(ResultsBundle {
        statics: vec![(
            StaticCaseKey::User(LoadCaseId(0)),
            sepika_solver::statics::linear::StaticOnce {
                disp,
                member_forces: vec![],
                panel_moments: vec![],
            },
        )],
        ..Default::default()
    });
    app.core.scoped.last_static = Some(StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
    app.ui.view.view_mode = ViewMode::Deformed;
    app.ui.view.show_beam_interpolation = false;
    let saved = bincode::serialize(&app.core.scoped.results).unwrap();
    let revision = app.core.scoped.undo.revision();
    let ctx = egui::Context::default();
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    let point = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, stroke }
                if stroke.color == theme::HILITE_PURPLE && stroke.width == 2.0 =>
            {
                Some(*points)
            }
            _ => None,
        })
        .max_by(|a, b| a[0].distance(a[1]).total_cmp(&b[0].distance(b[1])))
        .map(|ends| ends[0].lerp(ends[1], 0.5))
        .unwrap();
    click(&ctx, &mut app, point);
    assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(2)));
    assert_eq!(app.ui.view.view_mode, ViewMode::Deformed);
    assert_eq!(bincode::serialize(&app.core.scoped.results).unwrap(), saved);
    assert_eq!(app.core.scoped.undo.revision(), revision);
    app.ui.scoped.beam_draw_mode = true;
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.ui.view.view_mode, ViewMode::Shape);
    assert_eq!(bincode::serialize(&app.core.scoped.results).unwrap(), saved);
}
