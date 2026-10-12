//! 左右のアクティビティバー（VSCode 風の常時表示アイコン列）。
//!
//! 左列: ナビゲータ・作成パレット。右列: インスペクタ・準備計算・各解析パネル。
//! クリック挙動は [`super::toggle_dock_icon`] に委譲する。
//! アイコンは `assets/icons/` の SVG（gray-900 の黒）を `egui_extras` の SVG ローダで描く。

use super::*;

/// 外側アクセント線の幅（px）。非テキスト幾何のため固定値可。
pub(crate) const ACTIVITY_ACCENT_WIDTH: f32 = 3.0;

/// アイコン本体の一辺（px）。絵文字を本文サイズで描くと小さすぎるため、
/// VSCode の Activity Bar（アイコン約 24px）に合わせる。非テキスト幾何なので固定値。
const ACTIVITY_ICON_PX: f32 = 24.0;

/// アイコン上下の余白（px）。パネル内側余白 8px にヒット領域の余裕を足す。
const ACTIVITY_ICON_PAD: f32 = 10.0;

/// アクセント線を描く辺（左列は左端、右列は右端）。
#[derive(Clone, Copy)]
enum ActivityAccentEdge {
    Left,
    Right,
}

/// アクティビティバーの SVG アイコン。線色は gray-900（`#1A2332`）で統一する。
#[derive(Clone, Copy)]
enum ActivityGlyph {
    Navigator,
    DrawTools,
    Inspector,
    Preparation,
    Static,
    Eigen,
    Pushover,
    TimeHistory,
    LumpedMass,
}

impl ActivityGlyph {
    fn cache_key(self) -> &'static str {
        match self {
            Self::Navigator => "activity_icon_navigator",
            Self::DrawTools => "activity_icon_draw_tools",
            Self::Inspector => "activity_icon_inspector",
            Self::Preparation => "activity_icon_preparation",
            Self::Static => "activity_icon_static",
            Self::Eigen => "activity_icon_eigen",
            Self::Pushover => "activity_icon_pushover",
            Self::TimeHistory => "activity_icon_time_history",
            Self::LumpedMass => "activity_icon_lumped_mass",
        }
    }

    fn svg_bytes(self) -> &'static [u8] {
        match self {
            Self::Navigator => include_bytes!("../../../assets/icons/navigator.svg"),
            Self::DrawTools => include_bytes!("../../../assets/icons/draw_tools.svg"),
            Self::Inspector => include_bytes!("../../../assets/icons/inspector.svg"),
            Self::Preparation => include_bytes!("../../../assets/icons/preparation.svg"),
            Self::Static => include_bytes!("../../../assets/icons/static.svg"),
            Self::Eigen => include_bytes!("../../../assets/icons/eigen.svg"),
            Self::Pushover => include_bytes!("../../../assets/icons/pushover.svg"),
            Self::TimeHistory => include_bytes!("../../../assets/icons/time_history.svg"),
            Self::LumpedMass => include_bytes!("../../../assets/icons/lumped_mass.svg"),
        }
    }
}

/// SVG をテクスチャへラスタライズしてキャッシュする。
///
/// `egui::Image` + URI ローダだと、壊れたコメントや SizeHint 0 で赤い警告三角になる。
/// バイトから直接ラスタライズし、ハンドルをプロセス内で保持する。
fn activity_icon_texture(ctx: &egui::Context, glyph: ActivityGlyph) -> egui::TextureHandle {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<&'static str, egui::TextureHandle>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = cache.lock().expect("activity icon cache");
    if let Some(handle) = map.get(glyph.cache_key()) {
        return handle.clone();
    }
    let px = (ACTIVITY_ICON_PX * 2.0).round() as u32;
    let image = egui_extras::image::load_svg_bytes_with_size(
        glyph.svg_bytes(),
        egui::load::SizeHint::Size {
            width: px,
            height: px,
            maintain_aspect_ratio: true,
        },
        &Default::default(),
    )
    .unwrap_or_else(|err| panic!("{}: {err}", glyph.cache_key()));
    let handle = ctx.load_texture(glyph.cache_key(), image, egui::TextureOptions::LINEAR);
    map.insert(glyph.cache_key(), handle.clone());
    handle
}

/// アクティビティバーのパネル枠（ナビゲーション色）。
pub(crate) fn activity_bar_frame() -> egui::Frame {
    egui::Frame::new()
        .inner_margin(0)
        .fill(crate::theme::BLUE_200)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::BLUE_300))
}

/// 正方形スロット 1 辺の長さ（アイコン＋上下余白）。
fn activity_bar_slot_size() -> f32 {
    ACTIVITY_ICON_PX + ACTIVITY_ICON_PAD * 2.0
}

/// アイコン列全体の幅（スロット＋外側アクセント線）。
pub(crate) fn activity_bar_width() -> f32 {
    activity_bar_slot_size() + ACTIVITY_ACCENT_WIDTH
}

/// VSCode 風のアクティビティアイコン 1 個を描画する。
///
/// ヒット領域の幅はパネルの利用可能幅いっぱい（外側アクセントがウィンドウ端に
/// 付くようにする）。高さはスロット辺長。
fn activity_icon_button(
    ui: &mut egui::Ui,
    glyph: ActivityGlyph,
    is_active: bool,
    accent_edge: ActivityAccentEdge,
    hover_text: &str,
) -> egui::Response {
    let slot = activity_bar_slot_size();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), slot), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        {
            let painter = ui.painter();
            let bg = if is_active || response.hovered() {
                crate::theme::BLUE_300
            } else {
                egui::Color32::TRANSPARENT
            };
            if bg != egui::Color32::TRANSPARENT {
                painter.rect_filled(rect, 0.0, bg);
            }
            if is_active {
                let accent = match accent_edge {
                    ActivityAccentEdge::Left => egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.min.x + ACTIVITY_ACCENT_WIDTH, rect.max.y),
                    ),
                    ActivityAccentEdge::Right => egui::Rect::from_min_max(
                        egui::pos2(rect.max.x - ACTIVITY_ACCENT_WIDTH, rect.min.y),
                        rect.max,
                    ),
                };
                painter.rect_filled(accent, 0.0, crate::theme::BLUE_500);
            }
        }
        let icon_rect = egui::Rect::from_center_size(
            rect.center(),
            egui::vec2(ACTIVITY_ICON_PX, ACTIVITY_ICON_PX),
        );
        let tex = activity_icon_texture(ui.ctx(), glyph);
        ui.painter().image(
            tex.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    #[cfg(test)]
    tests::observe(ui, &response, hover_text);
    response.on_hover_text(hover_text)
}

impl App {
    /// 左アクティビティバー（ナビゲータ・作成パレット）。
    pub(crate) fn left_activity_bar(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            let is_nav_active =
                self.ui.view.left_dock_open && self.ui.view.left_panel == LeftPanel::Navigator;
            if activity_icon_button(
                ui,
                ActivityGlyph::Navigator,
                is_nav_active,
                ActivityAccentEdge::Left,
                "ナビゲータ",
            )
            .clicked()
                && toggle_dock_icon(&mut self.ui.view.left_dock_open, is_nav_active)
            {
                self.ui.view.left_panel = LeftPanel::Navigator;
            }
            let is_draw_active =
                self.ui.view.left_dock_open && self.ui.view.left_panel == LeftPanel::DrawTools;
            if activity_icon_button(
                ui,
                ActivityGlyph::DrawTools,
                is_draw_active,
                ActivityAccentEdge::Left,
                "作成パレット",
            )
            .clicked()
                && toggle_dock_icon(&mut self.ui.view.left_dock_open, is_draw_active)
            {
                self.ui.view.left_panel = LeftPanel::DrawTools;
            }
        });
    }

    /// 右アクティビティバー（インスペクタ・準備計算・各解析パネル）。
    pub(crate) fn right_activity_bar(&mut self, ui: &mut egui::Ui) {
        let selection_id = ui.make_persistent_id("right_activity_selection");
        let selected = self.ui.view.right_panel;
        let selection_changed = ui
            .ctx()
            .data(|data| data.get_temp::<RightPanel>(selection_id) != Some(selected));
        let parent_rect = ui.available_rect_before_wrap();
        let _scroll = egui::ScrollArea::vertical()
            .id_salt("right_activity_scroll")
            .max_height(parent_rect.height())
            .min_scrolled_height(0.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (panel, glyph, label) in [
                    (
                        RightPanel::Inspector,
                        ActivityGlyph::Inspector,
                        "インスペクタ",
                    ),
                    (
                        RightPanel::Preparation,
                        ActivityGlyph::Preparation,
                        "① 準備計算",
                    ),
                    (RightPanel::Static, ActivityGlyph::Static, "静的解析"),
                    (RightPanel::Eigen, ActivityGlyph::Eigen, "固有値"),
                    (RightPanel::Pushover, ActivityGlyph::Pushover, "増分解析"),
                    (
                        RightPanel::TimeHistory,
                        ActivityGlyph::TimeHistory,
                        "時刻歴応答",
                    ),
                    (RightPanel::LumpedMass, ActivityGlyph::LumpedMass, "質点系"),
                ] {
                    let active = self.ui.view.right_dock_open && self.ui.view.right_panel == panel;
                    let response =
                        activity_icon_button(ui, glyph, active, ActivityAccentEdge::Right, label);
                    if (selection_changed && selected == panel) || response.gained_focus() {
                        response.scroll_to_me(None);
                    }
                    if response.clicked()
                        && toggle_dock_icon(&mut self.ui.view.right_dock_open, active)
                    {
                        if selected != panel {
                            response.scroll_to_me(None);
                        }
                        self.ui.view.right_panel = panel;
                    }
                }
            });
        #[cfg(test)]
        tests::observe_scroll(ui.ctx(), parent_rect, &_scroll);
        ui.ctx()
            .data_mut(|data| data.insert_temp(selection_id, self.ui.view.right_panel));
    }
}

#[cfg(test)]
mod tests;
