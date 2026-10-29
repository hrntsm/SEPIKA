use super::*;
use references::{InputReference, ReferenceTarget, ReferenceView};

#[cfg(test)]
mod tests;

pub(super) const SHORTCUT_SEARCH: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
    egui::Key::P,
);

#[derive(Default)]
pub(crate) struct Search {
    pub open: bool,
    pub previous_focus: Option<egui::Id>,
    pub resume_input: bool,
    query: String,
    selected: Option<usize>,
    source: Option<InputReference>,
    pane: usize,
    composing: bool,
    focus_query: bool,
    reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Destination {
    Target(ReferenceTarget),
    Stage(Tab),
    Panel(RightPanel),
    Table(ModelTab),
}

struct Candidate {
    name: String,
    context: String,
    destination: Destination,
}

impl Candidate {
    fn kind(&self) -> &'static str {
        if matches!(self.destination, Destination::Target(_)) {
            "対象"
        } else {
            "操作"
        }
    }

    fn matches(&self, query: &str) -> bool {
        let text = format!("{} {} {}", self.kind(), self.name, self.context).to_lowercase();
        query
            .split_whitespace()
            .all(|word| text.contains(&word.to_lowercase()))
    }
}

impl App {
    pub(crate) fn open_search(&mut self, ui: &egui::Ui) {
        self.ui.scoped.search = Search {
            open: true,
            previous_focus: workspace::text_input_focus(ui),
            source: Some(self.input_reference()),
            pane: self.ui.view.workspace.placement.active,
            focus_query: true,
            ..Default::default()
        };
        crate::grid::retain_editing_for_layout(ui.ctx());
    }

    fn search_candidates(&self, query: &str) -> Vec<Candidate> {
        let model = &self.core.model;
        let mut candidates = Vec::new();
        for member in &model.elements {
            let section = member.section.and_then(|id| model.section(id));
            candidates.push(Candidate {
                name: section.map_or_else(|| "断面未指定".into(), |section| section.name.clone()),
                context: format!(
                    "部材 #{} / 階 {} / 節点 {:?}",
                    member.id.0,
                    section
                        .and_then(|section| section.floor.as_deref())
                        .unwrap_or("未指定"),
                    member.nodes
                ),
                destination: Destination::Target(ReferenceTarget::Member(member.id)),
            });
        }
        for section in &model.sections {
            candidates.push(Candidate {
                name: section.name.clone(),
                context: format!(
                    "断面 #{} / 階 {}",
                    section.id.0,
                    section.floor.as_deref().unwrap_or("未指定")
                ),
                destination: Destination::Target(ReferenceTarget::Section(section.id)),
            });
        }
        for story in &model.stories {
            candidates.push(Candidate {
                name: story.name.clone(),
                context: format!("階 #{}", story.id.0),
                destination: Destination::Target(ReferenceTarget::Story(story.id)),
            });
        }
        for (group, axes) in model.axes.iter().enumerate() {
            for (axis, item) in axes.axes.iter().enumerate() {
                candidates.push(Candidate {
                    name: item.name.clone(),
                    context: format!("通り / グループ {} #{} / 通り #{}", axes.name, group, axis),
                    destination: Destination::Target(ReferenceTarget::Axis { group, axis }),
                });
            }
        }
        for case in &model.load_cases {
            candidates.push(Candidate {
                name: case.name.clone(),
                context: format!("荷重ケース #{}", case.id.0),
                destination: Destination::Target(ReferenceTarget::LoadCase(case.id)),
            });
        }
        for (name, destination) in [
            ("モデル表示", Destination::Stage(Tab::Model)),
            ("荷重入力", Destination::Stage(Tab::Loads)),
            ("解析設定", Destination::Stage(Tab::Analysis)),
            ("結果表示", Destination::Stage(Tab::Results)),
            ("設計・検定", Destination::Stage(Tab::Design)),
            ("レポート", Destination::Stage(Tab::Report)),
            ("節点入力表", Destination::Table(ModelTab::Nodes)),
            ("部材入力表", Destination::Table(ModelTab::Members)),
            ("断面入力表", Destination::Table(ModelTab::Sections)),
            ("材料入力表", Destination::Table(ModelTab::Materials)),
            ("通り入力表", Destination::Table(ModelTab::Axes)),
            (
                "準備計算・階の設定",
                Destination::Panel(RightPanel::Preparation),
            ),
            ("静的解析設定", Destination::Panel(RightPanel::Static)),
            ("固有値解析設定", Destination::Panel(RightPanel::Eigen)),
            ("増分解析設定", Destination::Panel(RightPanel::Pushover)),
            (
                "時刻歴応答解析設定",
                Destination::Panel(RightPanel::TimeHistory),
            ),
            ("質点系解析設定", Destination::Panel(RightPanel::LumpedMass)),
        ] {
            candidates.push(Candidate {
                name: name.into(),
                context: "既存画面を開く".into(),
                destination,
            });
        }
        candidates.retain(|candidate| candidate.matches(query));
        candidates
    }

    fn search_unavailable(&self, state: &Search) -> Option<String> {
        if self.ui.scoped.reference_snapshot.is_some() {
            Some("読み取り専用の過去入力を閲覧中です。検索から本案へ移動できません。閲覧を閉じてから検索してください。".into())
        } else if state.source.as_ref() != Some(&self.input_reference()) {
            Some("検索元の案・入力版が変わりました。取消して検索を開き直してください。".into())
        } else if state.pane != self.ui.view.workspace.placement.active {
            Some("操作対象の領域が変わりました。検索を開き直してください。".into())
        } else {
            None
        }
    }

    fn confirm_search(
        &mut self,
        state: &Search,
        candidate: &Candidate,
        view: ReferenceView,
    ) -> Result<(), String> {
        if let Some(reason) = self.search_unavailable(state) {
            return Err(reason);
        }
        match candidate.destination {
            Destination::Target(target) => {
                let reference = self.target_reference(target)?;
                self.show_reference(&reference, view, matches!(view, ReferenceView::Diagram))
            }
            Destination::Stage(tab) => {
                self.ui.view.active_tab = tab;
                Ok(())
            }
            Destination::Panel(panel) => {
                self.enter_workspace();
                self.ui.view.right_panel = panel;
                self.ui.view.right_dock_open = true;
                self.ui.view.workspace.compact_dock = Some(false);
                Ok(())
            }
            Destination::Table(tab) => {
                self.enter_workspace();
                self.ui.view.model_tab = tab;
                self.ui.view.bottom_tab = BottomTab::Model;
                self.promote_table(workspace::Content::Model);
                Ok(())
            }
        }
    }

    pub(crate) fn search_ui(&mut self, root: &mut egui::Ui) {
        let mut state = std::mem::take(&mut self.ui.scoped.search);
        let ctx = root.ctx().clone();
        let ime_event = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Ime(_)))
        });
        let was_composing = state.composing;
        ctx.input(|input| {
            for event in &input.events {
                if let egui::Event::Ime(event) = event {
                    match event {
                        egui::ImeEvent::Enabled => {}
                        egui::ImeEvent::Preedit(text) => state.composing = !text.is_empty(),
                        egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled => {
                            state.composing = false
                        }
                    }
                }
            }
        });
        let keyboard = !ime_event && !was_composing && !state.composing;
        let mut cancel = keyboard
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut confirm = None;
        let unavailable = self.search_unavailable(&state);
        egui::CentralPanel::default().frame(crate::theme::content_panel_frame()).show_inside(root, |ui| {
            ui.heading("対象・操作を検索");
            ui.label(if self.ui.scoped.reference_snapshot.is_some() { "検索元: 過去入力（読み取り専用）・本案の候補は操作不可".into() } else {
                format!("検索元: 本案 / 領域{} / 編集世代 {}", state.pane + 1, state.source.as_ref().map_or(0, |source| source.generation))
            });
            if let Some(source) = &state.source {
                crate::theme::name_label(ui, &format!("案: {}", source.proposal));
            }
            ui.label("部材（断面符号・ID）・断面・階・通り・荷重ケース・画面名。上下キーで選択、Enterで詳細／画面へ、Escで取消。");
            let response = ui.add(egui::TextEdit::singleline(&mut state.query).hint_text("例: G1、2階、静的解析").desired_width(f32::INFINITY));
            if state.focus_query { response.request_focus(); state.focus_query = false; }
            crate::theme::focus_outline(ui, &response);
            if response.changed() { state.selected = None; state.reason = None; }
            let candidates = self.search_candidates(&state.query);
            let previous_selection = state.selected;
            if keyboard {
                for (key, forward) in [(egui::Key::ArrowDown, true), (egui::Key::ArrowUp, false)] {
                    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) && !candidates.is_empty() {
                        state.selected = Some(match state.selected { None => if forward { 0 } else { candidates.len() - 1 }, Some(index) => if forward { (index + 1) % candidates.len() } else { (index + candidates.len() - 1) % candidates.len() } });
                    }
                }
                if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    confirm = state.selected.map(|index| (index, ReferenceView::Detail));
                }
            }
            if let Some(reason) = &unavailable { ui.add(egui::Label::new(reason).wrap()); }
            if let Some(reason) = &state.reason { ui.add(egui::Label::new(reason).wrap()); }
            ui.horizontal_wrapped(|ui| {
                if let Some(candidate) = state.selected.and_then(|index| candidates.get(index)) {
                    if let Destination::Target(target) = candidate.destination {
                        for (label, view) in [("対象を図に表示", ReferenceView::Diagram), ("表へ", ReferenceView::Table), ("詳細へ", ReferenceView::Detail)] {
                            let reason = self.require_reference_view(target, view).err().or_else(||
                                (self.reference_hidden(target) && !matches!(view, ReferenceView::Diagram))
                                    .then(|| "対象は非表示です。先に「対象を図に表示」で表示してください。".into()));
                            if ui.add_enabled(unavailable.is_none() && reason.is_none(), egui::Button::new(label)).clicked() { confirm = state.selected.map(|index| (index, view)); }
                            if let Some(reason) = reason { ui.add(egui::Label::new(format!("{label}: {reason}")).wrap()); }
                        }
                        if self.reference_hidden(target) { ui.label("非表示: 図への確定で階・通りの絞込みを解除"); }
                    } else if ui.add_enabled(unavailable.is_none(), egui::Button::new("画面を開く")).clicked() { confirm = state.selected.map(|index| (index, ReferenceView::Detail)); }
                } else { ui.label("候補を選択してください（自動確定しません）"); }
                if ui.button("取消").clicked() { cancel = true; }
            });
            ui.separator();
            if candidates.is_empty() { ui.label("一致する対象・操作がありません"); }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (index, candidate) in candidates.iter().enumerate() {
                    let selected = state.selected == Some(index);
                    let response = crate::theme::selectable_name(ui, selected, &format!("[{}] {}", candidate.kind(), candidate.name));
                    if response.clicked() { state.selected = Some(index); state.reason = None; }
                    if selected && state.selected != previous_selection { response.scroll_to_me(Some(egui::Align::Center)); }
                    ui.add(egui::Label::new(&candidate.context).wrap());
                    ui.separator();
                }
            });
            if let Some((index, view)) = confirm.filter(|_| !cancel) {
                if let Some(candidate) = candidates.get(index) {
                    match self.confirm_search(&state, candidate, view) {
                        Ok(()) => state.open = false,
                        Err(reason) => state.reason = Some(reason),
                    }
                }
            }
        });
        if cancel {
            state.open = false;
        }
        if !state.open {
            state.resume_input = true;
            if !cancel {
                state.previous_focus = None;
            }
            ctx.request_repaint();
        }
        self.ui.scoped.search = state;
    }
}
