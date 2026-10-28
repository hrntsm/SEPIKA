use super::*;
use egui::{Event, FullOutput, Key, Modifiers, Pos2};

struct Harness {
    app: App,
    ctx: egui::Context,
    size: egui::Vec2,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::default();
        app.load_model(sepika_core::frame_gen::frame_model(&Default::default()).unwrap());
        let mut section = app.core.model.sections[0].clone();
        section.id = SectionId(1);
        section.name = "追加断面".into();
        app.core.model.sections.push(section);
        Self {
            app,
            ctx: Default::default(),
            size: egui::vec2(1300.0, 950.0),
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> FullOutput {
        self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, self.size)),
                modifiers: if events
                    .iter()
                    .any(|event| matches!(event, Event::Key { modifiers, .. } if modifiers.command))
                {
                    SHORTCUT_SEARCH.modifiers
                } else {
                    Default::default()
                },
                events,
                ..Default::default()
            },
            |ui| self.app.app_ui(ui),
        )
    }

    fn settled(&mut self) -> FullOutput {
        self.frame(vec![]);
        self.frame(vec![])
    }

    fn key(&mut self, key: Key) {
        self.frame(vec![key_event(key, Modifiers::NONE)]);
    }

    fn open(&mut self) {
        self.frame(vec![key_event(Key::P, SHORTCUT_SEARCH.modifiers)]);
        self.settled();
        assert!(self.app.ui.scoped.search.open);
    }

    fn click(&mut self, output: &FullOutput, label: &str) {
        let position = text_rect(output, label)
            .unwrap_or_else(|| panic!("操作なし: {label}"))
            .center();
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(position),
                Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
    }
}

fn key_event(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn text_rect(output: &FullOutput, label: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == label => {
            Some(text.visual_bounding_rect())
        }
        _ => None,
    })
}

fn document(app: &App) -> (Vec<u8>, u64) {
    (
        rmp_serde::to_vec_named(&app.core.model).unwrap(),
        app.core.scoped.undo.revision(),
    )
}

#[test]
fn search_names_distinguish_member_section_story_axis_and_operation_without_selecting() {
    let mut h = Harness::new();
    h.app.core.model.sections[0].name = "G1".into();
    h.app.core.model.sections[1].name = "G1".into();
    h.app.core.model.sections[0].floor = Some("日本語階".into());
    h.app.core.model.elements[0].section = Some(SectionId(0));
    h.app.core.model.stories[0].name = "日本語階".into();
    h.app.core.model.axes[0].axes[0].name = "日本語通り".into();
    let members = h.app.search_candidates("g1 部材");
    assert!(!members.is_empty());
    assert!(members.iter().all(|item| matches!(
        item.destination,
        Destination::Target(ReferenceTarget::Member(_))
    )));
    let sections = h.app.search_candidates("G1 断面");
    assert!(sections.iter().any(|item| item.context.contains("断面 #0")));
    assert!(sections.iter().any(|item| item.context.contains("断面 #1")));
    assert!(!h.app.search_candidates("日本語階").is_empty());
    assert_eq!(h.app.search_candidates("日本語通り").len(), 1);
    assert_eq!(
        h.app.search_candidates("静的解析")[0].destination,
        Destination::Panel(RightPanel::Static)
    );
    assert!(h.app.search_candidates("見つからない文字列").is_empty());
    let before = document(&h.app);
    h.open();
    h.frame(vec![Event::Text("静的解析".into())]);
    h.key(Key::Enter);
    assert!(h.app.ui.scoped.search.open);
    assert!(h.app.ui.scoped.search.selected.is_none());
    assert_eq!(document(&h.app), before);
    h.key(Key::ArrowDown);
    assert_eq!(h.app.ui.scoped.search.selected, Some(0));
    h.key(Key::Enter);
    assert!(!h.app.ui.scoped.search.open);
    assert_eq!(h.app.ui.view.right_panel, RightPanel::Static);
    assert!(h.app.core.scoped.job.is_none());
    assert_eq!(document(&h.app), before);
}

#[test]
fn search_button_and_shortcut_preserve_invalid_grid_draft_and_selection_on_cancel() {
    for button in [false, true] {
        for draft in ["未確定", "123.0"] {
            let mut h = Harness::new();
            h.settled();
            h.app.ui.view.workspace.placement.contents[0] = workspace::Content::Model;
            h.settled();
            h.app
                .ui
                .scoped
                .node_grid
                .grid
                .click(crate::grid::CellRef { row: 0, col: 0 }, false);
            h.key(Key::F2);
            h.frame(vec![Event::Text(draft.into())]);
            let output = h.settled();
            assert!(h.app.ui.scoped.node_grid.grid.editing.is_some());
            let focus = h.ctx.memory(|memory| memory.focused());
            let before = document(&h.app);
            let selection = (
                h.app.ui.scoped.selection.nodes().to_vec(),
                h.app.ui.scoped.selection.members().to_vec(),
            );
            if button {
                h.click(&output, "検索…");
                h.settled();
            } else {
                h.open();
            }
            assert!(h.app.ui.scoped.search.open);
            h.frame(vec![Event::Text("検索だけ".into())]);
            h.key(Key::Escape);
            h.settled();
            assert!(!h.app.ui.scoped.search.open);
            assert_eq!(h.ctx.memory(|memory| memory.focused()), focus);
            assert!(h.app.ui.scoped.node_grid.grid.editing.is_some());
            assert_eq!(document(&h.app), before);
            assert_eq!(
                (
                    h.app.ui.scoped.selection.nodes().to_vec(),
                    h.app.ui.scoped.selection.members().to_vec()
                ),
                selection
            );
            h.open();
            h.frame(vec![Event::Text("断面入力表".into())]);
            h.key(Key::ArrowDown);
            h.key(Key::Enter);
            h.settled();
            assert_eq!(h.app.ui.view.model_tab, ModelTab::Sections);
            assert_eq!(document(&h.app), before);
            h.app.ui.view.model_tab = ModelTab::Nodes;
            h.settled();
            assert!(h.app.ui.scoped.node_grid.grid.editing.is_some());
            assert_eq!(document(&h.app), before);
        }
    }
}

#[test]
fn ime_commit_enter_does_not_navigate_but_next_explicit_enter_does() {
    let mut h = Harness::new();
    h.open();
    h.key(Key::ArrowDown);
    h.frame(vec![
        Event::Ime(egui::ImeEvent::Enabled),
        Event::Ime(egui::ImeEvent::Preedit("固有値解析".into())),
    ]);
    h.key(Key::Enter);
    assert!(h.app.ui.scoped.search.open);
    h.frame(vec![
        Event::Ime(egui::ImeEvent::Commit("固有値解析".into())),
        key_event(Key::Enter, Modifiers::NONE),
    ]);
    assert!(h.app.ui.scoped.search.open);
    h.key(Key::ArrowDown);
    h.key(Key::Enter);
    assert!(
        !h.app.ui.scoped.search.open,
        "query={} reason={:?}",
        h.app.ui.scoped.search.query, h.app.ui.scoped.search.reason
    );
    assert_eq!(h.app.ui.view.right_panel, RightPanel::Eigen);
}

#[test]
fn narrow_search_reports_unavailable_views_and_read_only_source_without_navigation() {
    let mut h = Harness::new();
    h.size = egui::vec2(460.0, 650.0);
    h.open();
    h.app.ui.scoped.search.query = "荷重ケース".into();
    h.key(Key::ArrowDown);
    let output = h.settled();
    assert!(text_rect(
        &output,
        "対象を図に表示: この対象の図表示には未対応です。表または詳細を使用してください。"
    )
    .is_some());
    h.app.ui.scoped.reference_snapshot = Some((
        "過去入力".into(),
        references::RecordedCalculationInput {
            model: h.app.core.model.clone(),
            settings: references::RecordedSettings::Analysis {
                kind: references::ResultInputKey::Modal,
                cfg: Box::new(h.app.core.analysis_cfg),
                period_s: None,
            },
        },
    ));
    let before = document(&h.app);
    let output = h.settled();
    assert!(text_rect(
        &output,
        "検索元: 過去入力（読み取り専用）・本案の候補は操作不可"
    )
    .is_some());
    h.key(Key::Enter);
    assert!(h.app.ui.scoped.search.open);
    assert_eq!(document(&h.app), before);
}

#[test]
fn search_target_reuses_reference_visibility_and_rejects_changed_input() {
    let mut h = Harness::new();
    h.open();
    let state = std::mem::take(&mut h.app.ui.scoped.search);
    let candidate = h.app.search_candidates("部材").remove(0);
    let Destination::Target(target) = candidate.destination else {
        panic!()
    };
    let reference = h.app.target_reference(target).unwrap();
    let mut direct = App::default();
    direct.load_model(h.app.core.model.clone());
    let direct_reference = direct.target_reference(target).unwrap();
    h.app
        .confirm_search(&state, &candidate, ReferenceView::Diagram)
        .unwrap();
    direct
        .show_reference(&direct_reference, ReferenceView::Diagram, true)
        .unwrap();
    assert_eq!(h.app.ui.scoped.selection, direct.ui.scoped.selection);
    assert_eq!(h.app.ui.scoped.opened_reference, Some(reference));
    assert_eq!(h.app.ui.view.active_tab, direct.ui.view.active_tab);
    h.app
        .apply_model_edit(Box::new(sepika_edit::SetSectionName {
            id: SectionId(0),
            name: "変更".into(),
            floor: None,
        }));
    assert!(h
        .app
        .confirm_search(&state, &candidate, ReferenceView::Detail)
        .unwrap_err()
        .contains("入力版"));
}

#[test]
fn both_panes_open_the_existing_table_and_share_validation_and_undo() {
    for pane in [0, 1] {
        let mut h = Harness::new();
        h.settled();
        h.app.ui.view.workspace.placement.arrangement = workspace::Arrangement::Horizontal;
        h.app.ui.view.workspace.placement.active = pane;
        h.open();
        let state = std::mem::take(&mut h.app.ui.scoped.search);
        let candidate = h.app.search_candidates("断面入力表").remove(0);
        let before = document(&h.app);
        h.app
            .confirm_search(&state, &candidate, ReferenceView::Detail)
            .unwrap();
        assert_eq!(h.app.ui.view.workspace.placement.active, pane);
        assert_eq!(h.app.active_workspace_content(), workspace::Content::Model);
        assert_eq!(h.app.ui.view.model_tab, ModelTab::Sections);
        assert_eq!(document(&h.app), before);
        let other = h.app.core.model.sections[1].clone();
        let original = h.app.core.model.sections[0].clone();
        h.app
            .apply_model_edit(Box::new(sepika_edit::SetSectionName {
                id: SectionId(0),
                name: other.name,
                floor: other.floor,
            }));
        assert_eq!(h.app.core.model.sections[0].name, original.name);
        assert_eq!(h.app.core.model.sections[0].floor, original.floor);
        h.app
            .apply_model_edit(Box::new(sepika_edit::SetSectionName {
                id: SectionId(0),
                name: "検索から編集".into(),
                floor: None,
            }));
        assert_eq!(h.app.core.model.sections[0].name, "検索から編集");
        h.app.undo_action();
        assert_eq!(h.app.core.model.sections[0].name, original.name);
        h.app.redo_action();
        assert_eq!(h.app.core.model.sections[0].name, "検索から編集");
    }
}

#[test]
fn hidden_member_requires_explicit_reveal_before_detail_navigation() {
    let mut h = Harness::new();
    h.settled();
    h.app.ui.view.frame_target = Some(sepika_core::frame::FrameTarget::Story(
        h.app.core.model.stories[0].id,
    ));
    h.open();
    h.app.ui.scoped.search.query = "部材".into();
    let candidates = h.app.search_candidates("部材");
    let index = candidates
        .iter()
        .position(|candidate| {
            matches!(candidate.destination,
        Destination::Target(target) if h.app.reference_hidden(target))
        })
        .expect("非表示の部材");
    h.app.ui.scoped.search.selected = Some(index);
    let output = h.settled();
    assert!(text_rect(&output, "非表示: 図への確定で階・通りの絞込みを解除").is_some());
    h.key(Key::Enter);
    assert!(h.app.ui.scoped.search.open);
    assert!(h
        .app
        .ui
        .scoped
        .search
        .reason
        .as_ref()
        .unwrap()
        .contains("非表示"));
    let output = h.settled();
    let before = document(&h.app);
    h.click(&output, "対象を図に表示");
    assert!(!h.app.ui.scoped.search.open);
    assert_eq!(h.app.ui.view.frame_target, None);
    assert_eq!(document(&h.app), before);
}

#[test]
fn narrow_detail_confirmation_reveals_the_shared_reference_inspector() {
    for dock in [None, Some(true)] {
        for search in [false, true] {
            let mut h = Harness::new();
            h.size = egui::vec2(460.0, 650.0);
            h.app.core.model.sections[0].name = "詳細対象断面".into();
            h.settled();
            h.app.ui.view.workspace.compact_dock = dock;
            if search {
                h.open();
                h.frame(vec![Event::Text("詳細対象断面".into())]);
                h.key(Key::ArrowDown);
                h.key(Key::Enter);
                assert!(!h.app.ui.scoped.search.open);
            } else {
                let reference = h
                    .app
                    .target_reference(ReferenceTarget::Section(SectionId(0)))
                    .unwrap();
                h.app
                    .show_reference(&reference, ReferenceView::Detail, false)
                    .unwrap();
            }
            h.settled();
            assert_eq!(h.app.ui.view.right_panel, RightPanel::Inspector);
            assert!(h.app.right_dock_visible());
            assert_eq!(h.app.ui.scoped.nav.focus_section, Some(SectionId(0)));
        }
    }
}
