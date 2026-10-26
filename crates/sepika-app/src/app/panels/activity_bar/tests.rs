use super::*;

#[derive(Clone)]
struct Icon {
    response: egui::Response,
    clip: egui::Rect,
    label: String,
}

pub(super) fn observe(ui: &egui::Ui, response: &egui::Response, label: &str) {
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Vec<Icon>>(egui::Id::new("activity_test_icons"))
            .push(Icon {
                response: response.clone(),
                clip: ui.clip_rect(),
                label: label.into(),
            });
    });
}

struct Harness {
    ctx: egui::Context,
    app: App,
    height: f32,
    ppp: f32,
    time: f64,
    icons: Vec<Icon>,
    bar: egui::Rect,
    center: egui::Rect,
    full_app: bool,
    screen_height: f32,
}

impl Harness {
    fn new(height: f32, ppp: f32) -> Self {
        Self {
            ctx: egui::Context::default(),
            app: App::default(),
            height,
            ppp,
            time: 0.0,
            icons: Vec::new(),
            bar: egui::Rect::NOTHING,
            center: egui::Rect::NOTHING,
            full_app: false,
            screen_height: height + 60.0,
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 1.0 / 60.0;
        self.ctx
            .data_mut(|data| data.remove::<Vec<Icon>>(egui::Id::new("activity_test_icons")));
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(
                    if self.full_app { 1200.0 } else { 800.0 },
                    if self.full_app {
                        self.screen_height
                    } else {
                        self.height + 60.0
                    },
                ),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(self.ppp);
        let output = self.ctx.run_ui(input, |ui| {
            if self.full_app {
                eframe::App::ui(&mut self.app, ui, &mut eframe::Frame::_new_kittest());
                return;
            }
            egui::Panel::top("test_toolbar")
                .exact_size(40.0)
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    ui.label("toolbar");
                });
            egui::Panel::bottom("test_status")
                .exact_size(20.0)
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    ui.label("status");
                });
            // 下ドックは本番同様に左右アイコン列より内側へ配置する。
            egui::Panel::right("right_activity_bar")
                .exact_size(activity_bar_width())
                .resizable(false)
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    ui.set_max_height(self.height);
                    self.bar = ui.available_rect_before_wrap();
                    self.app.right_activity_bar(ui);
                });
            egui::Panel::bottom("test_bottom")
                .exact_size(100.0)
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    ui.label("bottom dock");
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());
                    self.center = rect;
                    self.app
                        .ui
                        .view
                        .camera
                        .apply_pointer_input(ui, &response, true, true);
                });
        });
        self.icons = self
            .ctx
            .data(|data| data.get_temp::<Vec<Icon>>(egui::Id::new("activity_test_icons")))
            .unwrap();
        self.icons
            .retain(|icon| LABELS.contains(&icon.label.as_str()));
        if self.full_app {
            self.bar = self.scroll().parent;
            self.center = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect)
                        if rect.fill == crate::theme::VIEW_BG
                            && rect.stroke.width == 0.0
                            && rect.rect.width() > 200.0 =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .unwrap();
        }
        assert_eq!(self.ctx.pixels_per_point(), self.ppp);
        assert_eq!(self.ctx.zoom_factor(), 1.0);
        assert_eq!(self.icons.len(), 7);
        output
    }

    fn settle(&mut self) {
        for _ in 0..90 {
            self.frame(Vec::new());
        }
    }

    fn wheel(&mut self, pos: egui::Pos2, y: f32) {
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, y),
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        self.settle();
    }

    fn visible(&self, index: usize) -> bool {
        self.icons[index]
            .clip
            .contains_rect(self.icons[index].response.rect)
    }
}

#[test]
fn right_activity_bar_low_window_reaches_last_icon_with_wheel() {
    for height in [240.0, 360.0, 720.0] {
        for ppp in [1.0, 2.0] {
            let mut h = Harness::new(height, ppp);
            h.frame(Vec::new());
            h.settle();
            h.wheel(h.bar.center(), -500.0);
            assert!(
                h.visible(6),
                "height={height}, ppp={ppp}: {:?} clip={:?}",
                h.icons[6].response.rect,
                h.icons[6].clip
            );
        }
    }
}

#[derive(Clone)]
struct Scroll {
    parent: egui::Rect,
    clip: egui::Rect,
    offset: f32,
    id: egui::Id,
    content: egui::Vec2,
}

pub(super) fn observe_scroll(
    ctx: &egui::Context,
    parent: egui::Rect,
    scroll: &egui::containers::scroll_area::ScrollAreaOutput<()>,
) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("activity_test_scroll"),
            Scroll {
                parent,
                clip: scroll.inner_rect,
                offset: scroll.state.offset.y,
                id: scroll.id,
                content: scroll.content_size,
            },
        )
    });
}

const PANELS: [RightPanel; 7] = [
    RightPanel::Inspector,
    RightPanel::Preparation,
    RightPanel::Static,
    RightPanel::Eigen,
    RightPanel::Pushover,
    RightPanel::TimeHistory,
    RightPanel::LumpedMass,
];
const LABELS: [&str; 7] = [
    "インスペクタ",
    "① 準備計算",
    "静的解析",
    "固有値",
    "増分解析",
    "時刻歴応答",
    "質点系",
];

impl Harness {
    fn scroll(&self) -> Scroll {
        self.ctx
            .data(|data| data.get_temp::<Scroll>(egui::Id::new("activity_test_scroll")))
            .unwrap()
    }

    fn click(&mut self, index: usize) {
        assert!(self.visible(index));
        let pos = self.icons[index].response.rect.center();
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        self.frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        self.settle();
    }

    fn key(&mut self, key: egui::Key, shift: bool) {
        let modifiers = egui::Modifiers {
            shift,
            ..egui::Modifiers::NONE
        };
        for pressed in [true, false] {
            self.frame(vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            }]);
        }
        self.settle();
    }

    fn tooltip(&mut self, index: usize) {
        let pos = self.icons[index].response.rect.center();
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        self.settle();
        assert!(self.icons[index].response.hovered());
        let out = self.frame(Vec::new());
        assert!(
            out.shapes
                .iter()
                .any(|shape| has_text(&shape.shape, LABELS[index])),
            "tooltip {}",
            LABELS[index]
        );
    }
}

fn has_text(shape: &egui::epaint::Shape, text: &str) -> bool {
    match shape {
        egui::epaint::Shape::Text(s) => s.galley.job.text.contains(text),
        egui::epaint::Shape::Vec(shapes) => shapes.iter().any(|s| has_text(s, text)),
        _ => false,
    }
}

#[test]
fn right_activity_bar_all_icons_pointer_tooltip_and_keyboard_at_each_scale() {
    for height in [240.0, 360.0, 720.0] {
        for ppp in [1.0, 2.0] {
            let mut h = Harness::new(height, ppp);
            h.frame(Vec::new());
            h.settle();
            assert_eq!(h.bar.height(), height);
            assert_eq!(h.bar.width(), 47.0);
            let model = h.app.core.model.clone();
            let revision = h.app.core.scoped.undo.revision();
            let stale = format!("{:?}", h.app.core.scoped.staleness);
            let unsaved = h.app.core.scoped.staleness.unsaved_changes;
            let settings = generation_inputs(&h.app);
            let results = rmp_serde::to_vec_named(&h.app.core.scoped.results).unwrap();
            for (index, panel) in PANELS.into_iter().enumerate() {
                h.wheel(h.bar.center(), if index < 4 { 500.0 } else { -500.0 });
                assert_eq!(h.icons[index].label, LABELS[index]);
                assert_eq!(h.icons[index].response.rect.height(), 44.0);
                assert!(h.visible(index), "index={index} height={height}");
                h.tooltip(index);
                h.click(index);
                if !h.app.ui.view.right_dock_open {
                    h.click(index);
                }
                assert_eq!(h.app.ui.view.right_panel, panel);
                assert!(h.app.ui.view.right_dock_open);
                h.click(index);
                assert!(!h.app.ui.view.right_dock_open);
                h.click(index);
                assert!(h.app.ui.view.right_dock_open);
            }
            h.ctx
                .memory_mut(|memory| memory.surrender_focus(h.icons[6].response.id));
            h.frame(vec![egui::Event::PointerGone]);
            for (index, panel) in PANELS.into_iter().enumerate() {
                h.key(egui::Key::Tab, false);
                assert!(
                    h.icons[index].response.has_focus(),
                    "Tab index={index} height={height}"
                );
                assert!(h.visible(index), "focus index={index} height={height}");
                let was_open = h.app.ui.view.right_dock_open && h.app.ui.view.right_panel == panel;
                h.key(egui::Key::Enter, false);
                assert_eq!(h.app.ui.view.right_panel, panel);
                assert_eq!(h.app.ui.view.right_dock_open, !was_open);
                h.key(egui::Key::Space, false);
                assert_eq!(h.app.ui.view.right_dock_open, was_open);
            }
            for index in (0..6).rev() {
                h.key(egui::Key::Tab, true);
                assert!(h.icons[index].response.has_focus());
                assert!(h.visible(index));
            }
            assert!(h.app.core.model.eq_ignoring_dofmap(&model));
            assert_eq!(h.app.core.scoped.undo.revision(), revision);
            assert_eq!(format!("{:?}", h.app.core.scoped.staleness), stale);
            assert_eq!(h.app.core.scoped.staleness.unsaved_changes, unsaved);
            assert_eq!(generation_inputs(&h.app), settings);
            assert_eq!(
                rmp_serde::to_vec_named(&h.app.core.scoped.results).unwrap(),
                results
            );
            assert!(h.app.core.scoped.job.is_none());
        }
    }
}

#[test]
fn right_activity_bar_selection_focus_resize_and_manual_position_are_independent() {
    for ppp in [1.0, 2.0] {
        let mut h = Harness::new(240.0, ppp);
        h.frame(Vec::new());
        h.settle();
        h.app.ui.view.right_panel = RightPanel::LumpedMass;
        h.frame(Vec::new());
        h.settle();
        assert!(h.visible(6));
        for _ in 0..7 {
            h.key(egui::Key::Tab, false);
        }
        assert!(h.icons[6].response.has_focus());
        h.wheel(h.bar.center(), 500.0);
        assert!(h.visible(0));
        assert!(!h.visible(6));
        assert!(h.icons[6].response.has_focus());
        let offset = h.scroll().offset;
        h.settle();
        assert_eq!(h.scroll().offset, offset);
        h.app.apply_tab_preset(Tab::Analysis);
        h.frame(Vec::new());
        h.settle();
        assert_eq!(h.app.ui.view.right_panel, RightPanel::Preparation);
        assert!(h.visible(1));
        h.key(egui::Key::Tab, true);
        assert!(h.icons[5].response.has_focus());
        assert!(h.visible(5));
        h.wheel(h.bar.center(), 500.0);
        h.settle();
        assert_eq!(h.scroll().offset, 0.0);
        assert!(h.icons[5].response.has_focus());
        let id = h.scroll().id;
        h.height = 720.0;
        h.frame(Vec::new());
        h.settle();
        assert_eq!(h.scroll().offset, 0.0);
        assert!(h.visible(6));
        h.height = 240.0;
        h.frame(Vec::new());
        h.settle();
        assert_eq!(h.scroll().id, id);
        assert_eq!(h.scroll().clip.height(), 240.0);
        assert_eq!(h.scroll().offset, 0.0);
        h.wheel(h.bar.center(), -500.0);
        assert!(h.visible(6));
        h.height = 360.0;
        h.frame(Vec::new());
        h.settle();
        assert_eq!(h.scroll().offset, 0.0);
        assert!(h.visible(0) && h.visible(6));
    }
}

#[test]
fn right_activity_bar_full_app_layout_routes_wheel_without_editing_inputs() {
    for height in [240.0, 360.0, 720.0] {
        for ppp in [1.0, 2.0] {
            let mut h = Harness::new(height, ppp);
            h.full_app = true;
            h.screen_height = height + 100.0;
            h.app.load_model(crate::sample::portal_frame());
            h.app.ui.view.bottom_tab = BottomTab::Log;
            h.ctx.data_mut(|data| {
                data.insert_persisted(
                    egui::Id::new("bottom_dock"),
                    egui::containers::panel::PanelState {
                        rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 80.0)),
                    },
                )
            });
            h.frame(Vec::new());
            h.settle();
            h.screen_height += height - h.scroll().parent.height();
            h.frame(Vec::new());
            h.settle();
            assert!(
                (h.scroll().parent.height() - height).abs() < 0.01,
                "height={height}, actual={:?}",
                h.scroll().parent
            );
            let model = h.app.core.model.clone();
            let revision = h.app.core.scoped.undo.revision();
            let stale = format!("{:?}", h.app.core.scoped.staleness);
            let unsaved = h.app.core.scoped.staleness.unsaved_changes;
            let hash = h.app.core.scoped.auto_load_sync_hash;
            let results = rmp_serde::to_vec_named(&h.app.core.scoped.results).unwrap();
            let settings = generation_inputs(&h.app);
            for name in ["top_toolbar", "status_bar", "bottom_dock"] {
                let state =
                    egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(name)).unwrap();
                assert!(
                    !h.scroll().clip.intersects(state.rect),
                    "overlap {name}: {:?} {:?}",
                    h.scroll().clip,
                    state.rect
                );
            }
            assert_eq!(h.scroll().parent.width(), 47.0);
            assert!(h.scroll().clip.height() <= h.scroll().parent.height());
            assert_eq!(
                h.scroll().content.y,
                308.0 + 6.0 * h.ctx.global_style().spacing.item_spacing.y
            );
            let zoom = h.app.ui.view.camera.zoom;
            let offset = h.scroll().offset;
            h.wheel(h.bar.center(), -120.0);
            assert_eq!(h.app.ui.view.camera.zoom, zoom);
            if height == 240.0 {
                assert!(h.scroll().offset > offset);
            } else {
                assert_eq!(h.scroll().offset, offset);
            }
            h.wheel(h.bar.center(), 500.0);
            assert_eq!(h.scroll().offset, 0.0);
            let top = egui::pos2(h.bar.center().x, h.bar.top() + 1.0);
            h.wheel(top, 40.0);
            assert_eq!(h.app.ui.view.camera.zoom, zoom);
            h.wheel(h.bar.center(), -500.0);
            let max_offset = h.scroll().offset;
            let bottom = egui::pos2(h.bar.center().x, h.bar.bottom() - 1.0);
            h.wheel(bottom, -40.0);
            assert_eq!(h.scroll().offset, max_offset);
            assert_eq!(h.app.ui.view.camera.zoom, zoom);
            assert!(
                h.center.width() > 0.0 && h.center.height() > 0.0,
                "viewport {:?}",
                h.center
            );
            h.wheel(h.center.center(), 20.0);
            assert!(
                h.app.ui.view.camera.zoom > zoom,
                "central wheel {:?}",
                h.center
            );
            assert_eq!(h.scroll().offset, max_offset);
            let zoom = h.app.ui.view.camera.zoom;
            h.wheel(h.bar.center(), 500.0);
            assert_eq!(h.app.ui.view.camera.zoom, zoom);
            assert!(h.app.core.model.eq_ignoring_dofmap(&model));
            assert_eq!(h.app.core.scoped.undo.revision(), revision);
            assert_eq!(format!("{:?}", h.app.core.scoped.staleness), stale);
            assert_eq!(h.app.core.scoped.staleness.unsaved_changes, unsaved);
            assert_eq!(h.app.core.scoped.auto_load_sync_hash, hash);
            assert_eq!(
                rmp_serde::to_vec_named(&h.app.core.scoped.results).unwrap(),
                results
            );
            assert_eq!(generation_inputs(&h.app), settings);
            assert!(h.app.core.scoped.job.is_none());
        }
    }
}

fn generation_inputs(app: &App) -> Vec<u8> {
    rmp_serde::to_vec_named(&(
        app.core.analysis_cfg,
        &app.core.scoped.wave_library_selection,
        &app.core.scoped.wave_library_selected_sha256,
        &app.core.scoped.lumped_wave_library_selection,
        &app.core.scoped.lumped_wave_library_selected_sha256,
        app.core.scoped.last_static,
        app.core.scoped.view_vibration_case,
        app.core.scoped.view_lumped_vibration_case,
    ))
    .unwrap()
}

#[test]
fn right_activity_bar_clicking_partly_clipped_icon_reveals_entire_selection() {
    for ppp in [1.0, 2.0] {
        let mut h = Harness::new(240.0, ppp);
        h.frame(Vec::new());
        h.settle();
        assert!(!h.visible(5));
        let visible = h.icons[5].response.rect.intersect(h.icons[5].clip);
        assert!(visible.height() > 0.0 && visible.height() < 44.0);
        let pos = visible.center();
        for pressed in [true, false] {
            h.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        h.settle();
        assert_eq!(h.app.ui.view.right_panel, RightPanel::TimeHistory);
        assert!(h.app.ui.view.right_dock_open);
        assert!(h.visible(5));
        h.wheel(h.bar.center(), 500.0);
        assert_eq!(h.scroll().offset, 0.0);
        h.settle();
        assert_eq!(h.scroll().offset, 0.0);
    }
}
