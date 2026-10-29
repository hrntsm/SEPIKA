use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) enum Arrangement {
    #[default]
    Single,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) enum Content {
    #[default]
    Drawing,
    Model,
    Loads,
    Results,
    Design,
    Report,
}

impl Content {
    fn label(self) -> &'static str {
        match self {
            Self::Drawing => "図（3D・構面）",
            Self::Model => "モデル入力表",
            Self::Loads => "荷重入力表",
            Self::Results => "結果・グラフ",
            Self::Design => "検定表・数量",
            Self::Report => "レポート",
        }
    }
    fn editable(self) -> bool {
        self != Self::Report
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Placement {
    pub arrangement: Arrangement,
    pub contents: [Content; 2],
    pub active: usize,
    pub fraction: f32,
    pub left_open: bool,
    pub left: LeftPanel,
    pub right_open: bool,
    pub right: RightPanel,
    pub bottom_open: bool,
    pub bottom: BottomTab,
    pub widths: [f32; 3],
    #[serde(default)]
    pub restore_table: Option<(Arrangement, [Content; 2], usize)>,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            arrangement: Arrangement::Single,
            contents: [Content::Drawing, Content::Model],
            active: 0,
            fraction: 0.5,
            left_open: true,
            left: LeftPanel::Navigator,
            right_open: true,
            right: RightPanel::Inspector,
            bottom_open: true,
            bottom: BottomTab::Log,
            widths: [280.0, 320.0, 200.0],
            restore_table: None,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Preferences {
    pub stages: [Option<Placement>; 6],
    pub left_pinned: bool,
    pub right_pinned: bool,
}

pub struct Workspace {
    pub(crate) placement: Placement,
    pub(crate) secondary: crate::viewer::ViewportState,
    preferences: Preferences,
    stage: Tab,
    initialized: bool,
    saved: Vec<u8>,
    pub(crate) reset_generation: u64,
    pub(crate) compact: bool,
    pub(crate) compact_dock: Option<bool>,
}

pub(crate) struct EvidenceReturn {
    location: study::Location,
    readonly_view: Option<Box<study::Slot>>,
    tab: Tab,
    model_tab: ModelTab,
    placement: Placement,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            placement: Placement::default(),
            secondary: Default::default(),
            preferences: Default::default(),
            stage: Tab::Model,
            initialized: false,
            saved: Vec::new(),
            reset_generation: 0,
            compact: false,
            compact_dock: None,
        }
    }
}

fn stage_index(tab: Tab) -> usize {
    match tab {
        Tab::Model => 0,
        Tab::Loads => 1,
        Tab::Analysis => 2,
        Tab::Results => 3,
        Tab::Design => 4,
        Tab::Report => 5,
    }
}

fn preference_path() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(|root| {
            std::path::PathBuf::from(root)
                .join("SEPIKA")
                .join("workspace.msgpack")
        })
}

impl App {
    fn switch_evidence_location(
        &mut self,
        next: study::Location,
        restore: Option<study::Slot>,
    ) -> Result<Option<study::Slot>, String> {
        let evidence = self.ui.scoped.result_evidence.take();
        let snapshot = self.ui.scoped.reference_snapshot.take();
        let snapshot_return = self.ui.scoped.reference_snapshot_return.take();
        self.retain_study_view_references();
        let result = self.switch_study_with_view(next);
        if result.is_ok() {
            if let Some(mut slot) = restore {
                slot.core.scoped.references.fixed = self.core.scoped.references.fixed.clone();
                slot.core.scoped.references.fixed_results =
                    self.core.scoped.references.fixed_results.clone();
                slot.core.scoped.staleness.unsaved_changes = false;
                slot.core.scoped.project_path = self.core.scoped.project_path.clone();
                slot.core.scoped.pending_save_recording =
                    self.core.scoped.pending_save_recording.take();
                self.put_slot(slot);
            }
        }
        self.ui.scoped.result_evidence = evidence;
        self.ui.scoped.reference_snapshot = snapshot;
        self.ui.scoped.reference_snapshot_return = snapshot_return;
        result
    }
    pub(crate) fn enter_evidence_editor(
        &mut self,
        model_tab: ModelTab,
    ) -> Result<EvidenceReturn, String> {
        self.enter_workspace();
        self.capture_workspace();
        let mut return_to = EvidenceReturn {
            location: self.studies.active,
            readonly_view: None,
            tab: self.ui.view.active_tab,
            model_tab: self.ui.view.model_tab,
            placement: self.ui.view.workspace.placement.clone(),
        };
        return_to.readonly_view = self
            .switch_evidence_location(study::Location::Main, None)?
            .map(Box::new);
        self.ui.view.active_tab = Tab::Model;
        self.enter_workspace();
        self.ui.view.model_tab = model_tab;
        self.promote_table(Content::Model);
        Ok(return_to)
    }

    pub(crate) fn return_from_evidence_editor(&mut self, return_to: EvidenceReturn) {
        if let Err(reason) = self.switch_evidence_location(
            return_to.location,
            return_to.readonly_view.map(|slot| *slot),
        ) {
            self.report_error(reason);
            return;
        }
        self.ui.view.active_tab = return_to.tab;
        self.enter_workspace();
        self.ui.view.model_tab = return_to.model_tab;
        self.ui.view.workspace.placement = return_to.placement;
        self.apply_workspace_placement(true);
        self.capture_workspace();
    }
    pub(crate) fn workspace_dock_id(&self, name: &str) -> egui::Id {
        egui::Id::new((
            name,
            self.ui.view.active_tab as usize,
            self.ui.view.workspace.reset_generation,
        ))
    }
    pub(crate) fn active_viewport(&self) -> &crate::viewer::ViewportState {
        if self.ui.view.workspace.placement.active == 0 {
            &self.ui.view.viewport
        } else {
            &self.ui.view.workspace.secondary
        }
    }
    pub(crate) fn active_viewport_mut(&mut self) -> &mut crate::viewer::ViewportState {
        if self.ui.view.workspace.placement.active == 0 {
            &mut self.ui.view.viewport
        } else {
            &mut self.ui.view.workspace.secondary
        }
    }
    pub(crate) fn active_workspace_content(&self) -> Content {
        if self.ui.view.workspace.initialized {
            let p = &self.ui.view.workspace.placement;
            p.contents[p.active]
        } else {
            match self.ui.view.active_tab {
                Tab::Results => Content::Results,
                Tab::Design => Content::Design,
                Tab::Report => Content::Report,
                _ => Content::Drawing,
            }
        }
    }
    pub(crate) fn open_result_workspace(&mut self, content: ResultsView) {
        self.ui.view.active_tab = Tab::Results;
        self.enter_workspace();
        let active = self.ui.view.workspace.placement.active;
        self.ui.view.workspace.placement.contents[active] = Content::Results;
        if self.ui.view.workspace.placement.contents[1 - active] == Content::Results {
            self.ui.view.workspace.placement.contents[1 - active] = Content::Drawing;
        }
        self.ui.view.results_view = content;
    }
    pub(crate) fn bind_spatial_case_to_active_view(&mut self, id: VibrationCaseId) {
        if let Some(identity) = self
            .core
            .model
            .vibration_cases
            .iter()
            .find(|case| case.id == id)
            .map(super::vibration::vibration_identity)
        {
            self.active_viewport_mut().spatial_source =
                Some(crate::viewer::SpatialResultSource::Case(id, identity));
            self.active_viewport_mut().th_scale_cache = None;
        }
    }
    pub(crate) fn bind_lumped_case_to_active_view(&mut self, id: LumpedVibrationCaseId) {
        if let Some(identity) = self
            .core
            .model
            .lumped_vibration_cases
            .iter()
            .find(|case| case.id == id)
            .map(super::vibration::vibration_identity)
        {
            self.active_viewport_mut().lumped_source =
                Some(crate::viewer::LumpedResultSource::Case(id, identity));
        }
    }
    pub(crate) fn left_dock_visible(&self) -> bool {
        self.ui.view.left_dock_open
            && (!self.ui.view.workspace.compact
                || self.ui.view.workspace.compact_dock == Some(true))
    }
    pub(crate) fn right_dock_visible(&self) -> bool {
        self.ui.view.right_dock_open
            && (!self.ui.view.workspace.compact
                || self.ui.view.workspace.compact_dock == Some(false))
    }
    pub fn restore_workspace_preferences(&mut self) {
        if let Some(path) = preference_path() {
            if let Ok(bytes) = std::fs::read(path) {
                match rmp_serde::from_slice::<Preferences>(&bytes) {
                    Ok(preferences) => self.ui.view.workspace.preferences = preferences,
                    Err(error) => self.report_notice(format!("配置設定を読み込めません: {error}")),
                }
            }
        }
    }

    pub(crate) fn show_workspace_reference_diagram(&mut self) {
        self.enter_workspace();
        self.ui.view.workspace.placement.contents[0] = Content::Drawing;
        self.ui.view.workspace.placement.active = 0;
    }

    pub(crate) fn capture_workspace(&mut self) {
        let view = &mut self.ui.view;
        let p = &mut view.workspace.placement;
        p.left_open = view.left_dock_open;
        p.left = view.left_panel;
        p.right_open = view.right_dock_open;
        p.right = view.right_panel;
        p.bottom_open = view.bottom_dock_open;
        p.bottom = view.bottom_tab;
        view.workspace.preferences.stages[stage_index(view.workspace.stage)] = Some(p.clone());
    }

    fn apply_workspace_placement(&mut self, initial_restore: bool) {
        let view = &mut self.ui.view;
        let p = &view.workspace.placement;
        if initial_restore || !view.workspace.preferences.left_pinned {
            view.left_dock_open = p.left_open;
            view.left_panel = p.left;
        }
        if initial_restore || !view.workspace.preferences.right_pinned {
            view.right_dock_open = p.right_open;
            view.right_panel = p.right;
        }
        view.bottom_dock_open = p.bottom_open;
        view.bottom_tab = p.bottom;
    }

    pub(crate) fn enter_workspace(&mut self) {
        let tab = self.ui.view.active_tab;
        let ws = &self.ui.view.workspace;
        if ws.initialized && ws.stage == tab {
            return;
        }
        let initial_restore = !ws.initialized;
        if ws.initialized {
            self.capture_workspace();
        }
        let saved = self.ui.view.workspace.preferences.stages[stage_index(tab)].clone();
        if let Some(mut saved) = saved {
            saved.active = saved.active.min(1);
            for (index, width) in saved.widths.iter_mut().enumerate() {
                let limits = [(180.0, 520.0), (220.0, 560.0), (80.0, 520.0)][index];
                *width = if width.is_finite() {
                    width.clamp(limits.0, limits.1)
                } else {
                    [280.0, 320.0, 200.0][index]
                };
            }
            saved.fraction = if saved.fraction.is_finite() {
                saved.fraction.clamp(0.2, 0.8)
            } else {
                0.5
            };
            self.ui.view.workspace.placement = saved;
        } else {
            let pinned_left = (self.ui.view.left_dock_open, self.ui.view.left_panel);
            let pinned_right = (self.ui.view.right_dock_open, self.ui.view.right_panel);
            self.apply_tab_preset(tab);
            if self.ui.view.workspace.preferences.left_pinned {
                (self.ui.view.left_dock_open, self.ui.view.left_panel) = pinned_left;
            }
            if self.ui.view.workspace.preferences.right_pinned {
                (self.ui.view.right_dock_open, self.ui.view.right_panel) = pinned_right;
            }
            let content = match tab {
                Tab::Results => Content::Results,
                Tab::Design => Content::Design,
                Tab::Report => Content::Report,
                _ => Content::Drawing,
            };
            self.ui.view.workspace.placement = Placement {
                contents: [content, Content::Model],
                left_open: self.ui.view.left_dock_open,
                left: self.ui.view.left_panel,
                right_open: self.ui.view.right_dock_open,
                right: self.ui.view.right_panel,
                bottom_open: self.ui.view.bottom_dock_open,
                bottom: self.ui.view.bottom_tab,
                ..Default::default()
            };
        }
        self.ui.view.workspace.stage = tab;
        self.ui.view.workspace.initialized = true;
        self.apply_workspace_placement(initial_restore);
    }

    pub(crate) fn persist_workspace(&mut self) {
        self.capture_workspace();
        let ws = &mut self.ui.view.workspace;
        let Ok(bytes) = rmp_serde::to_vec_named(&ws.preferences) else {
            return;
        };
        if bytes == ws.saved {
            return;
        }
        ws.saved = bytes.clone();
        #[cfg(not(test))]
        if let Some(path) = preference_path() {
            let result = std::fs::create_dir_all(path.parent().unwrap())
                .and_then(|()| std::fs::write(path, bytes));
            if let Err(error) = result {
                self.report_notice(format!("配置設定を保存できません: {error}"));
            }
        }
    }

    pub(crate) fn promote_table(&mut self, content: Content) {
        let ws = &mut self.ui.view.workspace;
        if ws.placement.restore_table.is_none() {
            ws.placement.restore_table = Some((
                ws.placement.arrangement,
                ws.placement.contents,
                ws.placement.active,
            ));
        }
        ws.placement.arrangement = Arrangement::Single;
        ws.placement.contents[ws.placement.active] = content;
    }

    fn restore_bottom_table(&mut self) {
        let ws = &mut self.ui.view.workspace;
        if let Some((arrangement, contents, active)) = ws.placement.restore_table.take() {
            ws.placement.arrangement = arrangement;
            ws.placement.contents = contents;
            ws.placement.active = active;
        }
        self.ui.view.bottom_dock_open = true;
    }

    pub(crate) fn table_in_workspace(&self, content: Content) -> bool {
        let p = &self.ui.view.workspace.placement;
        if p.arrangement == Arrangement::Single {
            p.contents[p.active] == content
        } else {
            p.contents.contains(&content)
        }
    }

    fn workspace_controls(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let input_focus = text_input_focus(ui);
        let response = ui
            .horizontal_wrapped(|ui| {
                ui.label("主領域:");
                let p = &mut self.ui.view.workspace.placement;
                for (arrangement, label) in [
                    (Arrangement::Single, "単独"),
                    (Arrangement::Horizontal, "左右二分割"),
                    (Arrangement::Vertical, "上下二分割"),
                ] {
                    if crate::theme::selectable_name(ui, p.arrangement == arrangement, label)
                        .clicked()
                    {
                        p.arrangement = arrangement;
                    }
                }
                if ui.button("入替").clicked() {
                    p.contents.swap(0, 1);
                    p.active = 1 - p.active;
                    std::mem::swap(
                        &mut self.ui.view.viewport,
                        &mut self.ui.view.workspace.secondary,
                    );
                }
                if ui
                    .button(format!(
                        "表示を同期 {}→{}",
                        self.ui.view.workspace.placement.active + 1,
                        2 - self.ui.view.workspace.placement.active
                    ))
                    .clicked()
                {
                    let ws = &mut self.ui.view.workspace;
                    if ws.placement.active == 0 {
                        ws.secondary = self.ui.view.viewport.clone();
                    } else {
                        self.ui.view.viewport = ws.secondary.clone();
                    }
                }
                if ui.button("推奨配置へ戻す").clicked() {
                    let tab = self.ui.view.active_tab;
                    self.ui.view.workspace.preferences.stages[stage_index(tab)] = None;
                    self.ui.view.workspace.initialized = false;
                    self.ui.view.workspace.reset_generation += 1;
                    self.ui.view.workspace.placement.restore_table = None;
                    self.enter_workspace();
                }
                if self.ui.view.workspace.placement.restore_table.is_some()
                    && ui.button("表を下部へ戻す").clicked()
                {
                    self.restore_bottom_table();
                }
                ui.checkbox(
                    &mut self.ui.view.workspace.preferences.left_pinned,
                    "左を固定",
                );
                ui.checkbox(
                    &mut self.ui.view.workspace.preferences.right_pinned,
                    "右を固定",
                );
            })
            .response;
        retain_input_focus(ui, input_focus, response.rect);
        response
    }

    pub(crate) fn workspace_panel(&mut self, ui: &mut egui::Ui) {
        self.enter_workspace();
        let input_focus = text_input_focus(ui);
        let compact_height = ui.available_height() < 340.0;
        let controls = if compact_height {
            ui.menu_button("主領域・配置", |ui| self.workspace_controls(ui))
                .response
        } else {
            let input = self.input_reference();
            ui.label(format!(
                "{} · 編集世代 {} · プロジェクト {}",
                self.study_display_name(),
                input.generation,
                input.project
            ));
            let response = self.workspace_controls(ui);
            if self.study_is_main() {
                ui.label("属性・検索・Undo / Redo の対象: 本案（画面の閲覧だけでは編集先を変更しません）");
            } else if self.study_read_only() {
                ui.label(format!(
                    "属性・検索の対象: {}（入力編集・Undo / Redo は利用できません）",
                    self.study_display_name()
                ));
            } else {
                ui.label(format!(
                    "属性・検索・Undo / Redo の対象: {}（固定対象の断面割当のみ編集できます）",
                    self.study_display_name()
                ));
            }
            response
        };
        retain_input_focus(ui, input_focus, controls.rect);
        let p = self.ui.view.workspace.placement.clone();
        let narrow = p.arrangement == Arrangement::Horizontal && ui.available_width() < 720.0
            || p.arrangement == Arrangement::Vertical && ui.available_height() < 420.0;
        if narrow {
            ui.horizontal(|ui| {
                ui.label("狭幅表示:");
                for index in 0..2 {
                    let active = &mut self.ui.view.workspace.placement.active;
                    if crate::theme::selectable_name(
                        ui,
                        *active == index,
                        &format!("領域{}", index + 1),
                    )
                    .clicked()
                    {
                        *active = index;
                    }
                }
            });
        }
        let owner = self.ui.view.workspace.placement.active;
        if self.ui.view.workspace.placement.contents[owner] != Content::Drawing {
            self.reset_draw_modes();
        }
        let rect = ui.available_rect_before_wrap();
        let mut primary = std::mem::take(&mut self.ui.view.viewport);
        let mut secondary = std::mem::take(&mut self.ui.view.workspace.secondary);
        if p.arrangement == Arrangement::Single || narrow {
            let index = self.ui.view.workspace.placement.active;
            self.workspace_pane(
                ui,
                rect,
                index,
                if index == 0 {
                    &mut primary
                } else {
                    &mut secondary
                },
            );
        } else {
            let (first, second, divider) = split_rect(rect, p.arrangement, p.fraction);
            retain_input_focus(ui, input_focus, divider);
            let response = ui.interact(
                divider,
                ui.id().with("workspace_divider"),
                egui::Sense::drag(),
            );
            if response.dragged() {
                let delta = if p.arrangement == Arrangement::Horizontal {
                    response.drag_delta().x / rect.width()
                } else {
                    response.drag_delta().y / rect.height()
                };
                self.ui.view.workspace.placement.fraction = (p.fraction + delta).clamp(0.2, 0.8);
            }
            ui.painter()
                .rect_filled(divider, 0.0, crate::theme::GRAY_600);
            self.workspace_pane(ui, first, 0, &mut primary);
            self.workspace_pane(ui, second, 1, &mut secondary);
        }
        self.ui.view.viewport = primary;
        self.ui.view.workspace.secondary = secondary;
        crate::viewer::show_detail_windows(ui, self);
    }

    fn workspace_pane(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        index: usize,
        view: &mut crate::viewer::ViewportState,
    ) {
        let mut pane = ui.new_child(
            egui::UiBuilder::new()
                .id(egui::Id::new(("workspace_pane", index)))
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        pane.set_clip_rect(rect.intersect(ui.clip_rect()));
        let ui = &mut pane;
        view.pane_index = index;
        view.allow_creation =
            self.study_is_main() && self.ui.view.workspace.placement.active == index;
        let mut content = self.ui.view.workspace.placement.contents[index];
        let input_focus = text_input_focus(ui);
        let header = ui.horizontal_wrapped(|ui| {
            let active = self.ui.view.workspace.placement.active == index;
            if crate::theme::selectable_name(
                ui,
                active,
                &format!("領域{} · {}", index + 1, self.study_display_name()),
            )
            .clicked()
            {
                self.ui.view.workspace.placement.active = index;
            }
            egui::ComboBox::from_id_salt("content")
                .selected_text(content.label())
                .show_ui(ui, |ui| {
                    for choice in [
                        Content::Drawing,
                        Content::Model,
                        Content::Loads,
                        Content::Results,
                        Content::Design,
                        Content::Report,
                    ] {
                        ui.selectable_value(&mut content, choice, choice.label());
                    }
                });
            if rect.height() >= 300.0 && ui.small_button("単独に広げる").clicked() {
                self.ui.view.workspace.placement.active = index;
                self.ui.view.workspace.placement.arrangement = Arrangement::Single;
            }
        });
        retain_input_focus(ui, input_focus, header.response.rect);
        if content != Content::Drawing
            && self.ui.view.workspace.placement.contents[1 - index] == content
        {
            self.ui.view.workspace.placement.contents[1 - index] = Content::Drawing;
        }
        self.ui.view.workspace.placement.contents[index] = content;
        if self.ui.view.left_dock_open
            && self.ui.view.left_panel == LeftPanel::DrawTools
            && self.ui.view.workspace.placement.active == index
            && content == Content::Drawing
        {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(if self.study_is_main() { "作成・割当設定 · 対象: 本案" } else { "断面割当設定 · 対象: 表示中の検討案" });
                    if ui.button("取消して閉じる").clicked() || ui.button("取消").clicked() {
                        self.reset_draw_modes();
                        self.ui.scoped.draw_section_assignment = None;
                        self.ui.view.left_panel = LeftPanel::Navigator;
                    }
                });
                ui.label("適用: 梁・小梁・間柱は終点クリック、床板・壁版は領域選択後の適用で確定します。");
                egui::ScrollArea::vertical().id_salt("drawing_settings").max_height(170.0).show(ui, |ui| self.draw_tools_panel(ui));
            });
        }
        ui.label(if self.study_read_only() {
            "検討の固定入力 · 読取専用"
        } else if !self.study_is_main() {
            "検討案 · 比較案の固定対象断面のみ編集可 · Undo / Redo: 表示中の案"
        } else if rect.height() < 300.0 {
            if content.editable() {
                "本案 · 条件編集可 · 結果値は読取専用 · Undo: 本案"
            } else {
                "本案 · 読取専用 · Undo: 本案"
            }
        } else if content.editable() {
            "入力・条件の編集先: 本案 · Undo / Redo: 本案（結果値は読取専用）"
        } else {
            "読み取り専用 · 編集先 / Undo: 本案"
        });
        view.allow_creation =
            self.study_is_main() && self.ui.view.workspace.placement.active == index;
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .id(if content == Content::Drawing {
                    ui.id().with("drawing")
                } else {
                    egui::Id::new(("workspace_content", content.label()))
                })
                .max_rect(ui.available_rect_before_wrap())
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut content_ui;
        match content {
            Content::Drawing => crate::viewer::viewer_panel_with_view(view, ui, self),
            Content::Model => self.workspace_table(ui, Content::Model),
            Content::Loads => self.workspace_table(ui, Content::Loads),
            Content::Results => self.results_tab_panel_with_view(ui, view),
            Content::Design => self.design_tab_panel(ui),
            Content::Report => self.report_tab_panel(ui),
        }
    }

    pub(crate) fn workspace_table(&mut self, ui: &mut egui::Ui, content: Content) {
        let enabled = content != Content::Loads || self.study_is_main();
        let mut table = ui.new_child(
            egui::UiBuilder::new()
                .id(egui::Id::new(("shared_input_table", content.label())))
                .max_rect(ui.available_rect_before_wrap())
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        if !enabled {
            table.disable();
        }
        egui::ScrollArea::vertical()
            .id_salt("table_scroll")
            .auto_shrink([false, false])
            .show(&mut table, |ui| match content {
                Content::Model => self.model_tab_panel(ui),
                Content::Loads => crate::tables::loads::loads_table(ui, self),
                _ => {}
            });
    }
}

fn split_rect(
    rect: egui::Rect,
    arrangement: Arrangement,
    fraction: f32,
) -> (egui::Rect, egui::Rect, egui::Rect) {
    let mut first = rect;
    let mut second = rect;
    let mut divider = rect;
    if arrangement == Arrangement::Horizontal {
        let x = rect.left() + rect.width() * fraction;
        first.max.x = x - 3.0;
        second.min.x = x + 3.0;
        divider.min.x = x - 3.0;
        divider.max.x = x + 3.0;
    } else {
        let y = rect.top() + rect.height() * fraction;
        first.max.y = y - 3.0;
        second.min.y = y + 3.0;
        divider.min.y = y - 3.0;
        divider.max.y = y + 3.0;
    }
    (first, second, divider)
}

pub(crate) fn text_input_focus(ui: &egui::Ui) -> Option<egui::Id> {
    ui.ctx()
        .egui_wants_keyboard_input()
        .then(|| ui.ctx().memory(|memory| memory.focused()))
        .flatten()
}

pub(crate) fn retain_input_focus(ui: &egui::Ui, focus: Option<egui::Id>, controls: egui::Rect) {
    if let Some(focus) = focus {
        if ui.input(|input| {
            (input.pointer.any_pressed() || input.pointer.any_released())
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|position| controls.contains(position))
        }) {
            ui.ctx().memory_mut(|memory| memory.request_focus(focus));
            crate::grid::retain_editing_for_layout(ui.ctx());
        }
    }
}

#[cfg(test)]
mod tests;
