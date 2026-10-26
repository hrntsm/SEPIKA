use sepika_core::frame::FrameTarget;

use crate::app::App;

use super::super::{lumped, space_grid, ViewMode};

const MODES: [(ViewMode, &str); 11] = [
    (ViewMode::Shape, "形状"),
    (ViewMode::Deformed, "変形"),
    (ViewMode::Mode, "モード"),
    (ViewMode::Force, "応力図"),
    (ViewMode::Cmq, "CMQ図"),
    (ViewMode::CheckRatio, "検定比"),
    (ViewMode::Hinge, "ヒンジ"),
    (ViewMode::Modeling, "モデル化"),
    (ViewMode::TimeHistory, "時刻歴"),
    (ViewMode::LumpedMode, "質点モード"),
    (ViewMode::LumpedTimeHistory, "質点時刻歴"),
];

fn mode_available(mode: ViewMode, has_recording: bool, has_lumped: bool) -> bool {
    match mode {
        ViewMode::TimeHistory => has_recording,
        ViewMode::LumpedMode | ViewMode::LumpedTimeHistory => has_lumped,
        _ => true,
    }
}

struct FrameChoices {
    items: Vec<(FrameTarget, String)>,
    axis_count: usize,
}

impl FrameChoices {
    fn new(app: &App) -> Self {
        let mut items = Vec::new();
        for (group, axes) in app.core.model.axes.iter().enumerate() {
            for (axis, entry) in axes.axes.iter().enumerate() {
                items.push((
                    FrameTarget::Axis { group, axis },
                    format!("{} / {}", axes.name, entry.name),
                ));
            }
        }
        let axis_count = items.len();
        items.extend(
            app.core
                .model
                .stories
                .iter()
                .map(|story| (FrameTarget::Story(story.id), story.name.clone())),
        );
        Self { items, axis_count }
    }

    fn label(&self, target: Option<FrameTarget>) -> &str {
        target
            .and_then(|target| self.items.iter().find(|(entry, _)| *entry == target))
            .map(|(_, name)| name.as_str())
            .unwrap_or("全体（3D）")
    }

    fn step(&self, target: &mut Option<FrameTarget>, delta: i64) {
        let n = self.items.len() as i64;
        if n == 0 {
            return;
        }
        let current = target
            .and_then(|target| self.items.iter().position(|(entry, _)| *entry == target))
            .map(|index| index as i64)
            .unwrap_or(if delta > 0 { -1 } else { n });
        *target = Some(self.items[(current + delta).rem_euclid(n) as usize].0);
    }

    fn navigation(&self, ui: &mut egui::Ui, target: &mut Option<FrameTarget>) {
        ui.add_enabled_ui(!self.items.is_empty(), |ui| {
            if ui.button("◀").on_hover_text("前の通り／階").clicked() {
                self.step(target, -1);
            }
            if ui.button("▶").on_hover_text("次の通り／階").clicked() {
                self.step(target, 1);
            }
        });
    }
}

fn combo<R>(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    width: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::ComboBox::from_id_salt(id)
                .selected_text(label)
                .width(width)
                .truncate()
                .show_ui(ui, contents)
                .response
                .on_hover_text(label)
        },
    )
    .inner
}

fn section_toggles(ui: &mut egui::Ui, app: &mut App) {
    ui.toggle_value(&mut app.ui.view.show_sections, "断面表示");
    ui.toggle_value(&mut app.ui.view.show_floor_secondary, "床壁・二次部材")
        .on_hover_text(
            "床板・小梁・間柱と、解析要素にならない壁版（腰壁・垂壁・パラペット・\
             自立壁・間柱で分割された壁）の表示",
        );
}

pub(super) fn primary_controls(ui: &mut egui::Ui, app: &mut App) {
    let choices = FrameChoices::new(app);
    let mut target = app.ui.scoped.frame_target;
    let mode_label = MODES
        .iter()
        .find(|(mode, _)| *mode == app.ui.view.view_mode)
        .unwrap()
        .1;
    let range_label = choices.label(target);
    let spacing = ui.spacing().item_spacing.x;
    let mut probe = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("view_controls_measure")
            .layout(egui::Layout::left_to_right(egui::Align::Center))
            .max_rect(egui::Rect::from_min_size(
                ui.cursor().min,
                egui::vec2(f32::INFINITY, 100.0),
            ))
            .sizing_pass()
            .invisible(),
    );
    let mode_width = egui::ComboBox::from_id_salt("mode_measure")
        .width(0.0)
        .selected_text(mode_label)
        .wrap_mode(egui::TextWrapMode::Extend)
        .show_ui(&mut probe, |_| {})
        .response
        .rect
        .width();
    let range_width = egui::ComboBox::from_id_salt("range_measure")
        .width(0.0)
        .selected_text(range_label)
        .wrap_mode(egui::TextWrapMode::Extend)
        .show_ui(&mut probe, |_| {})
        .response
        .rect
        .width();
    let navigation_width =
        probe.button("◀").rect.width() + probe.button("▶").rect.width() + spacing;
    let mut sections = app.ui.view.show_sections;
    let mut secondary = app.ui.view.show_floor_secondary;
    let toggle_width = probe.toggle_value(&mut sections, "断面表示").rect.width()
        + probe
            .toggle_value(&mut secondary, "床壁・二次部材")
            .rect
            .width()
        + spacing;
    let settings_width = probe.button("表示設定").rect.width();
    let available = ui.available_width();
    let base_width = mode_width + range_width + settings_width + 2.0 * spacing;
    let navigation_inline =
        base_width + navigation_width + toggle_width + 2.0 * spacing <= available;
    let toggles_inline = base_width + toggle_width + spacing <= available;
    let compact = base_width > available;
    let (mode_width, range_width) = if compact {
        let budget = (available - settings_width - 2.0 * spacing).max(0.0);
        let mode_width = mode_width.min((budget * 0.4).max(64.0));
        (mode_width, range_width.min((budget - mode_width).max(80.0)))
    } else {
        (mode_width, range_width)
    };

    ui.scope(|ui| {
        // フローティングの横バーは、一行のボタンと当たり判定が重なる。
        ui.spacing_mut().scroll.floating = false;
        egui::ScrollArea::horizontal()
        .id_salt("view_controls_scroll")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let has_recording = app.core.scoped.results.as_ref()
                    .and_then(|results| results.time_history.as_ref())
                    .and_then(|result| result.recording.as_ref()).is_some();
                let has_lumped = lumped::has_lumped(app);
                let mut mode = app.ui.view.view_mode;
                combo(ui, "view_mode", mode_label, mode_width, |ui| {
                    for (candidate, label) in MODES {
                        if mode_available(candidate, has_recording, has_lumped) {
                            ui.selectable_value(&mut mode, candidate, label);
                        }
                    }
                }).on_hover_text("表示モード");
                app.ui.view.view_mode = mode;
                combo(ui, "frame_target", range_label, range_width, |ui| {
                    ui.set_max_width(400.0_f32.min(ui.ctx().content_rect().width() - 32.0));
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    ui.selectable_value(&mut target, None, "全体（3D）");
                    for (index, (entry, name)) in choices.items.iter().enumerate() {
                        if index == 0 && choices.axis_count > 0 {
                            ui.separator();
                            ui.label("通り（軸組図）");
                        }
                        if index == choices.axis_count {
                            ui.separator();
                            ui.label("階（伏図）");
                        }
                        ui.selectable_value(&mut target, Some(*entry), name);
                    }
                }).on_hover_text("表示範囲");
                if navigation_inline {
                    choices.navigation(ui, &mut target);
                }
                if toggles_inline {
                    section_toggles(ui, app);
                }
                ui.menu_button("表示設定", |ui| {
                    if !navigation_inline {
                        ui.horizontal(|ui| choices.navigation(ui, &mut target));
                    }
                    if !toggles_inline {
                        section_toggles(ui, app);
                    }
                    if !navigation_inline || !toggles_inline {
                        ui.separator();
                    }
                    if !lumped::is_lumped_view(app.ui.view.view_mode) {
                        ui.checkbox(&mut app.ui.view.show_supports, "支点")
                            .on_hover_text("拘束された節点の矢印・円弧、支点ばね、免震マーカー");
                    }
                    if app.core.model.constraints.iter().any(|constraint| matches!(
                        constraint, sepika_core::model::Constraint::RigidDiaphragm { .. }
                    )) {
                        ui.checkbox(&mut app.ui.view.show_diaphragm_master, "剛床代表点");
                    }
                    if space_grid::has_grid(&app.core.model) {
                        ui.checkbox(&mut app.ui.view.show_space_grid, "通り芯グリッド")
                            .on_hover_text(
                                "各階レベルに通り芯の平面格子を描きます。\
                                 梁作成モードでは格子点にスナップし、節点が無ければ梁とあわせて作ります",
                            );
                    }
                    ui.separator();
                    ui.menu_button("操作案内", |ui| {
                        ui.set_max_width(300.0);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        ui.label(if target.is_some() {
                            "左ドラッグ:移動 / スクロール:ズーム（回転なし）"
                        } else {
                            "左ドラッグ:回転 / 右ドラッグ:移動 / スクロール:ズーム"
                        });
                    });
                });
            });
        });
    });
    if app.ui.scoped.frame_target != target {
        app.ui.view.camera.pan = [0.0, 0.0];
        app.ui.view.camera.zoom = 3.0;
    }
    app.ui.scoped.frame_target = target;
}
