use super::*;
use crate::viewer::{mn_draw::MnView, project};

fn rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(40.0, 30.0), egui::vec2(600.0, 400.0))
}

fn zoom(
    cam: &mut CameraState,
    pointer: Option<egui::Pos2>,
    scroll: f32,
    pinch: f32,
    hovered: bool,
) {
    let ctx = egui::Context::default();
    let _ = ctx.run_ui(
        egui::RawInput {
            events: pointer
                .into_iter()
                .map(egui::Event::PointerMoved)
                .chain([egui::Event::Zoom(pinch)])
                .collect(),
            ..Default::default()
        },
        |ui| {
            let mut response = ui.interact(
                rect(),
                ui.id().with("camera"),
                egui::Sense::click_and_drag(),
            );
            response.flags.set(egui::response::Flags::HOVERED, hovered);
            ui.input_mut(|i| i.smooth_scroll_delta.y = scroll);
            cam.apply_pointer_input(ui, &response, true, true);
        },
    );
}

#[test]
fn wheel_and_pinch_keep_the_projected_point_after_pan() {
    for (scroll, pinch, expected_zoom) in [
        (25.0, 1.0, 3.75),
        (-25.0, 1.0, 2.25),
        (0.0, 1.5, 4.5),
        (0.0, 0.6, 1.8),
        (100.0, 2.0, 10.0),
    ] {
        let mut cam = CameraState {
            pan: [83.0, -47.0],
            ..Default::default()
        };
        let p = [0.8, -0.4, 0.3];
        let before = MnView::new(&rect(), &cam).project(p);
        zoom(&mut cam, Some(before), scroll, pinch, true);
        let after = MnView::new(&rect(), &cam).project(p);
        assert!(
            before.distance(after) < 0.0001,
            "{scroll}, {pinch}: {before:?} -> {after:?}"
        );
        assert!((cam.zoom - expected_zoom).abs() < 0.000001);
    }
}

#[test]
fn clamp_uses_the_final_combined_zoom_and_never_drifts_at_limits() {
    let pointer = egui::pos2(170.0, 120.0);
    for (initial, scroll, pinch, expected) in [
        (9.0, 100.0, 0.5, 9.0),
        (9.0, 100.0, 2.0, 10.0),
        (0.6, -80.0, 0.5, 0.5),
    ] {
        let mut cam = CameraState {
            zoom: initial,
            pan: [23.0, -17.0],
            ..Default::default()
        };
        let center = [rect().center().x, rect().center().y];
        let point = [
            (pointer.x - center[0] - cam.pan[0]) as f64 / initial as f64,
            -(pointer.y - center[1] - cam.pan[1]) as f64 / initial as f64,
            0.0,
        ];
        cam.snap_to_direction([0.0, 0.0, 1.0]);
        zoom(&mut cam, Some(pointer), scroll, pinch, true);
        assert_eq!(cam.zoom, expected);
        let projected = project(point, [0.0; 3], &cam, cam.zoom, center);
        assert!(egui::pos2(projected[0], projected[1]).distance(pointer) < 0.0001);
        if expected == initial {
            assert_eq!(cam.pan, [23.0, -17.0]);
        }
        if expected == 0.5 || expected == 10.0 {
            let pan = cam.pan;
            for _ in 0..10 {
                zoom(&mut cam, Some(pointer), scroll, pinch, true);
            }
            assert_eq!(cam.pan, pan);
        }
    }
}

#[test]
fn pinch_without_pointer_requires_hover_and_keeps_projection_center() {
    for hovered in [false, true] {
        let mut cam = CameraState {
            pan: [32.0, -20.0],
            ..Default::default()
        };
        zoom(&mut cam, None, 0.0, 2.0, hovered);
        assert_eq!(cam.zoom, if hovered { 6.0 } else { 3.0 });
        assert_eq!(cam.pan, [32.0, -20.0]);
    }
}

#[test]
fn zoom_does_not_reach_an_occluded_or_adjacent_view() {
    for foreground in [egui::Order::Middle, egui::Order::Foreground] {
        let ctx = egui::Context::default();
        let mut back = CameraState::default();
        let mut front = CameraState::default();
        let mut adjacent = CameraState::default();
        for step in 0..4 {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 700.0),
                    )),
                    events: if step == 3 {
                        vec![
                            egui::Event::PointerMoved(egui::pos2(200.0, 200.0)),
                            egui::Event::Zoom(1.2),
                        ]
                    } else {
                        vec![egui::Event::PointerMoved(egui::pos2(200.0, 200.0))]
                    },
                    ..Default::default()
                },
                |ui| {
                    let response =
                        ui.interact(rect(), ui.id().with("back"), egui::Sense::click_and_drag());
                    back.apply_pointer_input(ui, &response, true, true);
                    let response = ui.interact(
                        egui::Rect::from_min_size(egui::pos2(700.0, 0.0), egui::vec2(200.0, 500.0)),
                        ui.id().with("adjacent"),
                        egui::Sense::click_and_drag(),
                    );
                    adjacent.apply_pointer_input(ui, &response, true, true);
                    egui::Area::new(egui::Id::new("foreground"))
                        .order(foreground)
                        .fixed_pos(egui::pos2(100.0, 100.0))
                        .show(ui.ctx(), |ui| {
                            let (_, response) = ui.allocate_exact_size(
                                egui::vec2(300.0, 300.0),
                                egui::Sense::click_and_drag(),
                            );
                            front.apply_pointer_input(ui, &response, true, true);
                        });
                },
            );
        }
        assert_eq!(back.zoom, 3.0);
        assert_eq!(back.pan, [0.0; 2]);
        assert_eq!(adjacent.zoom, 3.0);
        assert!(front.zoom > 3.0);
    }
}
