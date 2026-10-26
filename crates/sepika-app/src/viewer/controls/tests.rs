use super::*;
use crate::app::ResultsBundle;
use egui::{Event, FullOutput, Pos2};
use sepika_core::frame::FrameTarget;
use sepika_core::frame_gen::{frame_model, FrameSpec};

struct Harness {
    ctx: egui::Context,
    app: App,
    width: f32,
    viewer: bool,
    full_app: bool,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::default();
        app.core.model = frame_model(&FrameSpec::default()).unwrap();
        Self {
            ctx: egui::Context::default(),
            app,
            width: 1400.0,
            viewer: false,
            full_app: false,
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> FullOutput {
        self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(self.width, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                if self.full_app {
                    eframe::App::ui(&mut self.app, ui, &mut eframe::Frame::_new_kittest());
                } else if self.viewer {
                    super::super::viewer_panel(ui, &mut self.app);
                } else {
                    view_controls(ui, &mut self.app);
                }
            },
        )
    }

    fn settled(&mut self) -> FullOutput {
        self.frame(vec![]);
        self.frame(vec![]);
        self.frame(vec![])
    }

    fn click(&mut self, pos: Pos2) -> FullOutput {
        self.frame(vec![Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
        self.settled()
    }

    fn click_text(&mut self, output: &FullOutput, text: &str) -> FullOutput {
        self.click(
            text_rect(output, text)
                .unwrap_or_else(|| panic!("操作なし: {text}"))
                .center(),
        )
    }
}

fn text_rect(output: &FullOutput, text: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(shape) if shape.galley.job.text == text => {
            Some(shape.visual_bounding_rect())
        }
        _ => None,
    })
}

fn state(app: &App) -> (Vec<u8>, u64, Vec<u8>, bool) {
    (
        rmp_serde::to_vec_named(&app.core.model).unwrap(),
        app.core.scoped.undo.revision(),
        rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
        app.core.scoped.staleness.results_stale,
    )
}

#[test]
fn primary_row_moves_controls_in_order_and_shares_settings_after_resizing() {
    let mut h = Harness::new();
    let before = state(&h.app);
    let output = h.settled();
    let labels = [
        "形状",
        "全体（3D）",
        "◀",
        "▶",
        "断面表示",
        "床壁・二次部材",
        "表示設定",
    ];
    let rects: Vec<_> = labels
        .iter()
        .map(|text| text_rect(&output, text).unwrap())
        .collect();
    assert!(rects
        .windows(2)
        .all(|pair| pair[0].right() <= pair[1].left()));
    assert!(rects
        .iter()
        .all(|rect| (rect.center().y - rects[0].center().y).abs() < 2.0));
    assert!(text_rect(&output, "支点").is_none());
    let mut navigation_moved = false;
    let mut toggles_moved = false;
    for width in (180..1400).rev().step_by(5) {
        h.width = width as f32;
        let output = h.settled();
        let navigation = text_rect(&output, "◀").is_some();
        let toggles = text_rect(&output, "断面表示").is_some();
        if !navigation && toggles {
            navigation_moved = true;
            let output = h.click_text(&output, "表示設定");
            assert!(text_rect(&output, "◀").is_some());
            h.frame(vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }]);
        }
        if !toggles {
            assert!(!navigation);
            toggles_moved = true;
            let output = h.click_text(&output, "表示設定");
            assert!(text_rect(&output, "◀").is_some());
            let value = h.app.ui.view.show_sections;
            h.click_text(&output, "断面表示");
            assert_eq!(h.app.ui.view.show_sections, !value);
            h.click(egui::pos2(10.0, 700.0));
            h.width = 1400.0;
            let output = h.settled();
            assert!(text_rect(&output, "断面表示").is_some());
            assert_eq!(h.app.ui.view.show_sections, !value);
            assert!(text_rect(&output, "表示設定").is_some());
            break;
        }
    }
    assert!(navigation_moved && toggles_moved);
    assert_eq!(state(&h.app), before);
}

#[test]
fn range_menu_keeps_categories_cyclic_navigation_and_camera_reset() {
    let mut h = Harness::new();
    let before = state(&h.app);
    let output = h.settled();
    let output = h.click_text(&output, "全体（3D）");
    assert!(text_rect(&output, "通り（軸組図）").is_some());
    assert!(text_rect(&output, "階（伏図）").is_some());
    h.click(egui::pos2(1000.0, 700.0));
    let output = h.settled();
    h.app.ui.view.camera.pan = [30.0, 40.0];
    h.app.ui.view.camera.zoom = 7.0;
    h.click_text(&output, "▶");
    assert_eq!(
        h.app.ui.scoped.frame_target,
        Some(FrameTarget::Axis { group: 0, axis: 0 })
    );
    assert_eq!(h.app.ui.view.camera.pan, [0.0, 0.0]);
    assert_eq!(h.app.ui.view.camera.zoom, 3.0);
    let output = h.settled();
    h.click_text(&output, "◀");
    assert_eq!(
        h.app.ui.scoped.frame_target,
        h.app
            .core
            .model
            .stories
            .last()
            .map(|story| FrameTarget::Story(story.id))
    );
    assert_eq!(state(&h.app), before);
}

#[test]
fn empty_model_has_only_whole_model_range_and_keeps_mode_controls() {
    let mut h = Harness::new();
    h.app = App::default();
    h.width = 220.0;
    let output = h.settled();
    let output = h.click_text(&output, "全体（3D）");
    assert!(text_rect(&output, "通り（軸組図）").is_none());
    assert!(text_rect(&output, "階（伏図）").is_none());
    h.click(egui::pos2(10.0, 700.0));
    let output = h.settled();
    let output = h.click_text(&output, "形状");
    assert!(text_rect(&output, "時刻歴").is_none());
    assert!(text_rect(&output, "質点モード").is_none());
    let output = h.click_text(&output, "CMQ図");
    assert_eq!(h.app.ui.view.view_mode, ViewMode::Cmq);
    assert!(text_rect(&output, "成分:").is_some());
    assert!(text_rect(&output, "軸:").is_some());
    assert!(text_rect(&output, "C(モーメント)").is_some());
}

#[test]
fn long_range_names_are_truncated_without_wrapping_and_available_in_menu() {
    let mut h = Harness::new();
    h.width = 320.0;
    h.app.core.model.axes[0].axes[0].name = "長い通り名称".repeat(30);
    h.app.ui.scoped.frame_target = Some(FrameTarget::Axis { group: 0, axis: 0 });
    let full_name = format!(
        "{} / {}",
        h.app.core.model.axes[0].name, h.app.core.model.axes[0].axes[0].name
    );
    let before = state(&h.app);
    for zoom in [1.0, 1.5, 2.0] {
        h.ctx.set_zoom_factor(zoom);
        let output = h.settled();
        let rect = text_rect(&output, &full_name).unwrap();
        let mode = text_rect(&output, "形状").unwrap();
        let settings = text_rect(&output, "表示設定").unwrap();
        assert!(rect.height() < 30.0);
        assert!((mode.center().y - settings.center().y).abs() < 2.0);
        let output = h.click(rect.center());
        assert!(text_rect(&output, "通り（軸組図）").is_some());
        assert!(text_rect(&output, &full_name).is_some());
        h.click(egui::pos2(10.0, 700.0 / zoom));
    }
    assert_eq!(state(&h.app), before);
}

#[test]
fn stale_time_history_guidance_stays_below_primary_row_after_resizing() {
    let mut h = Harness::new();
    h.app.core.scoped.results = Some(ResultsBundle::default());
    h.app.ui.view.view_mode = ViewMode::TimeHistory;
    h.app.core.scoped.staleness.results_stale = true;
    let before = state(&h.app);
    for width in [1400.0, 260.0, 1400.0] {
        h.width = width;
        let output = h.settled();
        let message = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(shape)
                    if shape.galley.job.text.contains("モデルが編集されています") =>
                {
                    Some(shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap();
        assert!(message.top() > text_rect(&output, "時刻歴").unwrap().bottom());
        assert_eq!(h.app.ui.view.view_mode, ViewMode::TimeHistory);
    }
    assert_eq!(state(&h.app), before);
}

#[test]
fn viewcube_and_settings_clicks_do_not_reach_the_model_or_move_camera() {
    let mut h = Harness::new();
    h.viewer = true;
    h.app.select_node(sepika_core::ids::NodeId(0));
    let selected = h.app.ui.scoped.selection.nodes().to_vec();
    let before = state(&h.app);
    let output = h.settled();
    let top = text_rect(&output, "Top").unwrap();
    assert!(top.center().x > h.width - 110.0);
    assert!(top.top() > text_rect(&output, "表示設定").unwrap().bottom());
    h.click(top.center());
    assert_eq!(h.app.ui.scoped.selection.nodes(), selected);
    let output = h.settled();
    let output = h.click_text(&output, "表示設定");
    let pan = h.app.ui.view.camera.pan;
    let rot = h.app.ui.view.camera.rot;
    let zoom = h.app.ui.view.camera.zoom;
    h.click_text(&output, "支点");
    assert_eq!(h.app.ui.scoped.selection.nodes(), selected);
    assert_eq!(h.app.ui.view.camera.pan, pan);
    assert_eq!(h.app.ui.view.camera.rot, rot);
    assert_eq!(h.app.ui.view.camera.zoom, zoom);
    let output = h.settled();
    let output = h.click_text(&output, "表示設定");
    h.frame(vec![
        Event::PointerMoved(text_rect(&output, "操作案内").unwrap().center()),
        Event::MouseWheel {
            phase: egui::TouchPhase::Move,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 100.0),
            modifiers: Default::default(),
        },
    ]);
    assert_eq!(h.app.ui.view.camera.zoom, zoom);
    assert_eq!(state(&h.app), before);
}

#[test]
fn mode_menu_includes_conditional_results_and_keeps_time_history_playback() {
    use sepika_solver::dynamic::lumped_mass::{
        LumpedMassModal, LumpedMassModel, LumpedMassResult, LumpedMassType,
    };
    use sepika_solver::dynamic::timehistory::{ResponseResult, ThRecording};
    let mut h = Harness::new();
    h.app.core.scoped.results = Some(ResultsBundle {
        time_history: Some(ResponseResult {
            wall_capacities: vec![],
            rc_wall_shear_method: Default::default(),
            time: vec![0.0, 1.0],
            peak_disp: vec![],
            story_drift_angle: vec![],
            cumulative_ductility: vec![],
            history: Default::default(),
            recording: Some(ThRecording {
                frame_time: vec![0.0, 1.0],
                ..Default::default()
            }),
            nonlinear: false,
            applied_long_term: false,
            non_converged_steps: 0,
        }),
        lumped: Some(LumpedMassResult {
            model: LumpedMassModel::from_stories(LumpedMassType::EquivalentShear, vec![]),
            modal: LumpedMassModal::default(),
            response: None,
        }),
        ..Default::default()
    });
    let before = state(&h.app);
    let output = h.settled();
    let output = h.click_text(&output, "形状");
    for label in ["時刻歴", "質点モード"] {
        assert!(text_rect(&output, label).is_some(), "{label}");
    }
    let pos = text_rect(&output, "質点モード").unwrap().center();
    h.frame(vec![
        Event::PointerMoved(pos),
        Event::MouseWheel {
            phase: egui::TouchPhase::Move,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -200.0),
            modifiers: Default::default(),
        },
    ]);
    for _ in 0..30 {
        h.frame(vec![]);
    }
    let output = h.settled();
    assert!(text_rect(&output, "質点時刻歴").is_some());
    h.click_text(&output, "時刻歴");
    h.width = 260.0;
    let output = h.settled();
    assert_eq!(h.app.ui.view.view_mode, ViewMode::TimeHistory);
    assert!(text_rect(&output, "速度:").is_some());
    h.click_text(&output, "▶");
    assert!(h.app.ui.scoped.th_playing);
    assert!(h.app.ui.scoped.th_play_time > 0.0);
    assert_eq!(state(&h.app), before);
}

#[test]
fn very_narrow_primary_row_scrolls_to_settings_without_wrapping() {
    let mut h = Harness::new();
    h.width = 120.0;
    let output = h.settled();
    let mode = text_rect(&output, "形状").unwrap();
    assert!(text_rect(&output, "表示設定").is_none_or(|settings| settings.center().x > h.width));
    h.frame(vec![
        Event::PointerMoved(egui::pos2(60.0, mode.center().y)),
        Event::MouseWheel {
            phase: egui::TouchPhase::Move,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(-500.0, 0.0),
            modifiers: Default::default(),
        },
    ]);
    let mut output = h.settled();
    for _ in 0..30 {
        output = h.frame(vec![]);
    }
    let settings = text_rect(&output, "表示設定").unwrap();
    assert!(settings.center().x < h.width);
    assert!((mode.center().y - settings.center().y).abs() < 2.0);
    let output = h.click(settings.center());
    assert!(
        text_rect(&output, "断面表示").is_some(),
        "settings={settings:?}, shapes={:?}",
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((&text.galley.job.text, text.pos, shape.clip_rect)),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert!(text_rect(&output, "床壁・二次部材").is_some());
}

#[test]
fn panel_changes_and_display_zoom_keep_primary_row_and_view_settings() {
    let mut h = Harness::new();
    h.full_app = true;
    h.app.ui.view.show_sections = true;
    h.app.ui.view.show_floor_secondary = false;
    h.app.ui.view.view_mode = ViewMode::Force;
    let before = state(&h.app);
    for zoom in [1.0, 1.5] {
        h.ctx.set_zoom_factor(zoom);
        for (left, right, bottom) in [
            (false, false, false),
            (true, true, true),
            (true, false, true),
            (false, true, false),
        ] {
            h.app.ui.view.left_dock_open = left;
            h.app.ui.view.right_dock_open = right;
            h.app.ui.view.bottom_dock_open = bottom;
            let output = h.settled();
            let mode = text_rect(&output, "応力図").unwrap();
            let settings = text_rect(&output, "表示設定").unwrap();
            assert!((mode.center().y - settings.center().y).abs() < 2.0);
            assert!(text_rect(&output, "成分:").unwrap().top() > mode.bottom());
            assert!(h.app.ui.view.show_sections);
            assert!(!h.app.ui.view.show_floor_secondary);
            assert_eq!(h.app.ui.view.view_mode, ViewMode::Force);
            assert_eq!(state(&h.app), before);
        }
    }
}

#[test]
fn zooming_model_and_frame_views_preserves_model_undo_and_result_state() {
    for empty in [false, true] {
        let mut h = Harness::new();
        h.viewer = true;
        if empty {
            h.app.core.model = Default::default();
        }
        for in_frame in [false, true] {
            if in_frame && !empty {
                h.app.ui.scoped.frame_target = Some(FrameTarget::Axis { group: 0, axis: 0 });
            }
            h.settled();
            h.app.ui.view.camera.pan = [23.0, -17.0];
            let before = state(&h.app);
            let camera = h.app.ui.view.camera.clone();
            let pointer = egui::pos2(400.0, 400.0);
            h.frame(vec![Event::PointerMoved(pointer)]);
            h.frame(vec![Event::Zoom(1.5)]);
            assert!(h.app.ui.view.camera.zoom > camera.zoom);
            assert_ne!(h.app.ui.view.camera.pan, camera.pan);
            assert_eq!(h.app.ui.view.camera.rot, camera.rot);
            assert_eq!(state(&h.app), before);
            assert!(h.app.ui.view.camera.pan.iter().all(|x| x.is_finite()));
        }
    }
}
