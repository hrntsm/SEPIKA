use super::*;
use egui::{Event, FullOutput, Pos2};

struct Harness {
    app: App,
    ctx: egui::Context,
    size: egui::Vec2,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::default();
        app.load_model(sepika_core::frame_gen::frame_model(&Default::default()).unwrap());
        app.enter_workspace();
        Self {
            app,
            ctx: Default::default(),
            size: egui::vec2(1500.0, 1000.0),
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> FullOutput {
        self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, self.size)),
                events,
                ..Default::default()
            },
            |ui| self.app.workspace_panel(ui),
        )
    }

    fn settled(&mut self) -> FullOutput {
        self.frame(vec![]);
        self.frame(vec![]);
        self.frame(vec![])
    }

    fn click(&mut self, output: &FullOutput, text: &str) -> FullOutput {
        let position = text_rect(output, text)
            .unwrap_or_else(|| panic!("操作なし: {text}"))
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
        self.settled()
    }
}

fn text_rect(output: &FullOutput, text: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text_shape) if text_shape.galley.job.text == text => {
            Some(text_shape.visual_bounding_rect())
        }
        _ => None,
    })
}

fn document_state(app: &App) -> (Vec<u8>, Vec<u8>, u64) {
    (
        rmp_serde::to_vec_named(&app.core.model).unwrap(),
        rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
        app.core.scoped.undo.revision(),
    )
}

#[test]
fn requested_pairs_render_in_both_directions_without_changing_document_or_selection() {
    let mut h = Harness::new();
    h.app.select_member(sepika_core::ids::ElemId(0));
    let before = document_state(&h.app);
    for arrangement in [Arrangement::Horizontal, Arrangement::Vertical] {
        for content in [Content::Model, Content::Drawing, Content::Results] {
            h.app.ui.view.workspace.placement.arrangement = arrangement;
            h.app.ui.view.workspace.placement.contents = [Content::Drawing, content];
            h.app.ui.view.results_view = ResultsView::TimeHistory;
            let output = h.settled();
            assert!(text_rect(&output, "▸ 領域1 · 本案").is_some());
            assert!(text_rect(&output, "領域2 · 本案").is_some());
            assert_eq!(document_state(&h.app), before);
            assert_eq!(
                h.app.ui.scoped.selection.active_member(),
                Some(sepika_core::ids::ElemId(0))
            );
        }
    }
}

#[test]
fn drawing_controls_and_pointer_zoom_belong_to_each_view_and_sync_is_explicit() {
    let mut h = Harness::new();
    h.app.ui.view.workspace.placement.arrangement = Arrangement::Horizontal;
    h.app.ui.view.workspace.placement.contents = [Content::Drawing; 2];
    h.app.ui.view.workspace.secondary.frame_target =
        Some(sepika_core::frame::FrameTarget::Axis { group: 0, axis: 0 });
    h.app.ui.view.workspace.secondary.show_sections = true;
    h.app.ui.view.workspace.secondary.deform_scale_factor = 2.5;
    let output = h.settled();
    assert_eq!(h.app.ui.view.frame_target, None);
    assert!(!h.app.ui.view.show_sections);
    assert_eq!(h.app.ui.view.workspace.secondary.deform_scale_factor, 2.5);
    let left = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == crate::theme::VIEW_3D_BG && rect.rect.width() > 100.0 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .unwrap();
    let second_zoom = h.app.ui.view.workspace.secondary.camera.zoom;
    let first_zoom = h.app.ui.view.camera.zoom;
    h.frame(vec![
        Event::PointerMoved(left.center()),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0.0, 80.0),
            modifiers: Default::default(),
        },
    ]);
    assert_ne!(h.app.ui.view.camera.zoom, first_zoom);
    assert_eq!(h.app.ui.view.workspace.secondary.camera.zoom, second_zoom);
    let output = h.settled();
    h.click(&output, "表示を同期 1→2");
    assert_eq!(h.app.ui.view.workspace.secondary.frame_target, None);
    assert!(!h.app.ui.view.workspace.secondary.show_sections);
    assert_eq!(
        h.app.ui.view.workspace.secondary.camera.zoom,
        h.app.ui.view.camera.zoom
    );
    assert_eq!(h.app.ui.view.workspace.secondary.deform_scale_factor, 1.0);
}

#[test]
fn stage_roundtrip_and_preference_reload_restore_layout_without_discarding_drafts() {
    let mut h = Harness::new();
    h.app.ui.view.workspace.placement.arrangement = Arrangement::Vertical;
    h.app.ui.view.workspace.placement.contents = [Content::Drawing, Content::Loads];
    h.app.ui.view.workspace.placement.widths = [240.0, 360.0, 170.0];
    h.app.ui.view.left_dock_open = false;
    h.app.ui.scoped.node_draft[0] = "未確定".into();
    h.app.ui.view.camera.zoom = 4.5;
    h.app.ui.view.active_tab = Tab::Analysis;
    h.app.enter_workspace();
    h.app.ui.view.active_tab = Tab::Model;
    h.app.enter_workspace();
    assert_eq!(
        h.app.ui.view.workspace.placement.arrangement,
        Arrangement::Vertical
    );
    assert_eq!(
        h.app.ui.view.workspace.placement.contents,
        [Content::Drawing, Content::Loads]
    );
    assert_eq!(
        h.app.ui.view.workspace.placement.widths,
        [240.0, 360.0, 170.0]
    );
    assert!(!h.app.ui.view.left_dock_open);
    assert_eq!(h.app.ui.scoped.node_draft[0], "未確定");
    assert_eq!(h.app.ui.view.camera.zoom, 4.5);
    h.app.capture_workspace();
    let encoded = rmp_serde::to_vec_named(&h.app.ui.view.workspace.preferences).unwrap();
    let mut reopened = App::default();
    reopened.ui.view.workspace.preferences = rmp_serde::from_slice(&encoded).unwrap();
    reopened.enter_workspace();
    assert_eq!(
        reopened.ui.view.workspace.placement.arrangement,
        Arrangement::Vertical
    );
    assert_eq!(
        reopened.ui.view.workspace.placement.widths,
        [240.0, 360.0, 170.0]
    );
    assert!(!reopened.ui.view.left_dock_open);
}

#[test]
fn table_expansion_returns_to_its_previous_pair_and_preserves_pending_input() {
    let mut h = Harness::new();
    h.app.ui.view.workspace.placement.arrangement = Arrangement::Horizontal;
    h.app.ui.view.workspace.placement.contents = [Content::Drawing, Content::Design];
    h.app.ui.scoped.node_draft[0] = "入力中".into();
    h.app.promote_table(Content::Model);
    h.settled();
    assert!(h.app.table_in_workspace(Content::Model));
    h.app.restore_bottom_table();
    assert_eq!(
        h.app.ui.view.workspace.placement.arrangement,
        Arrangement::Horizontal
    );
    assert_eq!(
        h.app.ui.view.workspace.placement.contents,
        [Content::Drawing, Content::Design]
    );
    assert_eq!(h.app.ui.scoped.node_draft[0], "入力中");
}

#[test]
fn narrow_switch_and_reset_keep_both_cameras_and_selection() {
    let mut h = Harness::new();
    h.app.ui.view.workspace.placement.arrangement = Arrangement::Horizontal;
    h.app.ui.view.workspace.placement.contents = [Content::Drawing; 2];
    h.app.ui.view.camera.zoom = 4.0;
    h.app.ui.view.workspace.secondary.camera.zoom = 6.0;
    h.app.select_member(sepika_core::ids::ElemId(0));
    h.settled();
    h.size.x = 600.0;
    let output = h.settled();
    assert!(text_rect(&output, "推奨配置へ戻す").is_some());
    let output = h.click(&output, "領域2");
    assert!(text_rect(&output, "▸ 領域2 · 本案").is_some());
    assert_eq!(h.app.ui.view.camera.zoom, 4.0);
    assert_eq!(h.app.ui.view.workspace.secondary.camera.zoom, 6.0);
    h.click(&output, "推奨配置へ戻す");
    assert_eq!(
        h.app.ui.view.workspace.placement.arrangement,
        Arrangement::Single
    );
    assert_eq!(
        h.app.ui.scoped.selection.active_member(),
        Some(sepika_core::ids::ElemId(0))
    );
    assert_eq!(h.app.ui.view.camera.zoom, 4.0);
}

#[test]
fn changing_layout_keeps_invalid_node_cell_input_open_without_committing() {
    let mut h = Harness::new();
    h.app.ui.view.workspace.placement.contents = [Content::Model, Content::Drawing];
    h.settled();
    h.app
        .ui
        .scoped
        .node_grid
        .grid
        .click(crate::grid::CellRef { row: 0, col: 0 }, false);
    h.frame(vec![Event::Key {
        key: egui::Key::F2,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    h.frame(vec![Event::Text("未確定".into())]);
    let output = h.settled();
    assert!(h.app.ui.scoped.node_grid.grid.editing.is_some());
    let before = document_state(&h.app);
    h.click(&output, "左右二分割");
    assert!(h.app.ui.scoped.node_grid.grid.editing.is_some());
    assert_eq!(document_state(&h.app), before);
}

#[test]
fn switching_away_from_drawing_cancels_hidden_creation_without_editing() {
    let mut h = Harness::new();
    h.app.ui.scoped.beam_draw_mode = true;
    h.app.ui.scoped.beam_draw_first = Some(crate::viewer::space_grid::SnapPoint::Node(
        sepika_core::ids::NodeId(0),
    ));
    h.app.ui.view.workspace.placement.contents[0] = Content::Model;
    let before = document_state(&h.app);
    h.settled();
    assert!(!h.app.ui.scoped.beam_draw_mode);
    assert_eq!(h.app.ui.scoped.beam_draw_first, None);
    assert_eq!(document_state(&h.app), before);
}

#[test]
fn each_drawing_keeps_its_static_result_when_the_other_result_changes() {
    let mut h = Harness::new();
    let first = StaticKey::Case(StaticCaseKey::User(sepika_core::ids::LoadCaseId(0)));
    let second = StaticKey::Case(StaticCaseKey::User(sepika_core::ids::LoadCaseId(1)));
    let result = |displacement| sepika_solver::statics::linear::StaticOnce {
        stiffness_ratio_disp: None,
        disp: vec![[displacement; 6]; h.app.core.model.nodes.len()],
        member_forces: Vec::new(),
        panel_moments: Vec::new(),
    };
    h.app.core.scoped.results = Some(ResultsBundle {
        statics: vec![
            (
                StaticCaseKey::User(sepika_core::ids::LoadCaseId(0)),
                result(1.0),
            ),
            (
                StaticCaseKey::User(sepika_core::ids::LoadCaseId(1)),
                result(2.0),
            ),
        ],
        ..Default::default()
    });
    h.app.ui.view.workspace.placement.arrangement = Arrangement::Horizontal;
    h.app.ui.view.workspace.placement.contents = [Content::Drawing; 2];
    h.app.ui.view.viewport.static_key = Some(first);
    h.app.ui.view.workspace.secondary.static_key = Some(second);
    h.app.ui.view.view_mode = crate::viewer::ViewMode::Deformed;
    h.app.ui.view.workspace.secondary.view_mode = crate::viewer::ViewMode::Deformed;
    let before = document_state(&h.app);
    h.settled();
    assert_eq!(
        h.app.ui.view.viewport.current_static(&h.app).unwrap().disp[0][0],
        1.0
    );
    assert_eq!(
        h.app
            .ui
            .view
            .workspace
            .secondary
            .current_static(&h.app)
            .unwrap()
            .disp[0][0],
        2.0
    );
    h.app.ui.view.workspace.secondary.static_key = Some(first);
    h.settled();
    assert_eq!(h.app.ui.view.viewport.static_key, Some(first));
    assert_eq!(document_state(&h.app), before);
}

#[test]
fn issue975_pending_section_assignment_rejects_reindexed_candidates_and_undo_generations() {
    let mut app = App::default();
    let mut model = crate::sample::portal_frame();
    let template = model.sections[0].clone();
    model.sections = ["A", "B", "C"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let mut section = template.clone();
            section.id = SectionId(index as u32);
            section.name = name.into();
            section
        })
        .collect();
    for member in &mut model.elements {
        member.section = Some(SectionId(2));
    }
    app.load_model(model);
    app.stage_section_assignment(ElemId(0), Some(SectionId(1)))
        .unwrap();
    assert!(app.apply_model_edit(Box::new(sepika_edit::DeleteSection { id: SectionId(0) })));
    let before = document_state(&app);
    assert!(!app.apply_pending_section_assignment());
    assert_eq!(document_state(&app), before);
    assert_eq!(
        app.core.model.element(ElemId(0)).unwrap().section,
        Some(SectionId(1))
    );
    app.undo_action();
    assert!(!app.apply_pending_section_assignment());
    app.stage_section_assignment(ElemId(0), Some(SectionId(1)))
        .unwrap();
    assert!(app.apply_pending_section_assignment());
    assert_eq!(
        app.core.model.element(ElemId(0)).unwrap().section,
        Some(SectionId(1))
    );
    app.undo_action();
    assert_eq!(
        app.core.model.element(ElemId(0)).unwrap().section,
        Some(SectionId(2))
    );
}

#[test]
fn issue975_reference_conditions_come_from_the_active_drawing_and_graphs_are_rejected_in_every_stage(
) {
    let mut h = Harness::new();
    h.app.ui.view.workspace.placement.arrangement = Arrangement::Horizontal;
    h.app.ui.view.workspace.placement.contents = [Content::Drawing; 2];
    h.app.ui.view.workspace.placement.active = 1;
    h.app.ui.view.workspace.secondary.frame_target =
        Some(sepika_core::frame::FrameTarget::Axis { group: 0, axis: 0 });
    h.app.ui.view.workspace.secondary.view_mode = crate::viewer::ViewMode::Force;
    h.app.ui.view.workspace.secondary.force_components = crate::viewer::ForceComponents::default();
    let display = h.app.current_display_conditions().unwrap();
    assert_eq!(display.diagram, Some(references::ReferenceDiagram::Force));
    assert_eq!(
        display.frame,
        Some(references::ReferenceTarget::Axis { group: 0, axis: 0 })
    );
    h.app.ui.view.workspace.placement.active = 0;
    let display = h.app.current_display_conditions().unwrap();
    assert_eq!(display.diagram, Some(references::ReferenceDiagram::Shape));
    assert_eq!(display.frame, None);
    h.app.ui.view.workspace.placement.contents[0] = Content::Results;
    h.app.ui.view.results_view = ResultsView::Pushover;
    assert_eq!(h.app.ui.view.active_tab, Tab::Model);
    assert!(h.app.current_display_conditions().is_err());
}

#[test]
fn issue975_spatial_and_lumped_cases_remain_independent_through_navigation_and_playback() {
    use sepika_core::model::{
        LumpedVibrationCase, LumpedVibrationDim, LumpedVibrationDir, VibrationCase, VibrationThDir,
    };
    let mut h = Harness::new();
    h.app.core.model.vibration_cases = (0..2)
        .map(|index| VibrationCase {
            id: VibrationCaseId(index),
            name: format!("立体{index}"),
            wave_name: format!("波形{index}"),
            input_identity: Some(format!("入力{index}")),
            dir: VibrationThDir::X,
            nonlinear: false,
        })
        .collect();
    h.app.core.model.lumped_vibration_cases = (0..2)
        .map(|index| LumpedVibrationCase {
            id: LumpedVibrationCaseId(index),
            name: format!("質点{index}"),
            wave_name: format!("波形{index}"),
            input_identity: Some(format!("入力{index}")),
            dir: LumpedVibrationDir::X,
            nonlinear: false,
            dim: LumpedVibrationDim::Planar,
        })
        .collect();
    let spatial = |value| {
        let mut result = crate::app::tests::dummy_th(vec![0.0, value]);
        result.peak_disp = vec![[value; 6]; h.app.core.model.nodes.len()];
        result.recording = Some(sepika_solver::dynamic::timehistory::ThRecording {
            frame_time: vec![0.0, value],
            node_disp: vec![vec![[value; 6]; h.app.core.model.nodes.len()]; 2],
            ..Default::default()
        });
        result
    };
    let a = spatial(1.0);
    let b = spatial(3.0);
    let mut la = crate::app::tests::dummy_lumped(1.0, true);
    la.response.as_mut().unwrap().time = vec![0.0, 1.0];
    let mut lb = crate::app::tests::dummy_lumped(3.0, true);
    lb.response.as_mut().unwrap().time = vec![0.0, 3.0];
    h.app.core.scoped.results = Some(ResultsBundle {
        time_history: Some(a.clone()),
        time_histories: vec![(VibrationCaseId(0), a.clone()), (VibrationCaseId(1), b)],
        lumped: Some(la.clone()),
        lumped_results: vec![
            (LumpedVibrationCaseId(0), la),
            (LumpedVibrationCaseId(1), lb),
        ],
        ..Default::default()
    });
    h.app.core.scoped.view_vibration_case = Some(VibrationCaseId(0));
    h.app.core.scoped.view_lumped_vibration_case = Some(LumpedVibrationCaseId(0));
    h.app.core.scoped.staleness.results_stale = false;
    h.app.ui.view.active_tab = Tab::Results;
    h.app.enter_workspace();
    h.app.ui.view.workspace.placement.arrangement = Arrangement::Horizontal;
    h.app.ui.view.workspace.placement.contents = [Content::Drawing; 2];
    h.app.ui.view.view_mode = crate::viewer::ViewMode::TimeHistory;
    h.app.ui.view.workspace.secondary.view_mode = crate::viewer::ViewMode::TimeHistory;
    h.settled();
    h.app.ui.view.workspace.placement.active = 1;
    h.app.focus_spatial_vibration_case(VibrationCaseId(1));
    h.app.ui.view.workspace.placement.contents[1] = Content::Drawing;
    h.settled();
    assert_eq!(
        h.app
            .ui
            .view
            .viewport
            .spatial_result(&h.app)
            .unwrap()
            .peak_disp[0][0],
        1.0
    );
    assert_eq!(
        h.app
            .ui
            .view
            .workspace
            .secondary
            .spatial_result(&h.app)
            .unwrap()
            .peak_disp[0][0],
        3.0
    );
    h.app.focus_lumped_vibration_case(LumpedVibrationCaseId(1));
    h.app.ui.view.workspace.placement.contents[1] = Content::Drawing;
    h.app.ui.view.view_mode = crate::viewer::ViewMode::LumpedTimeHistory;
    h.app.ui.view.workspace.secondary.view_mode = crate::viewer::ViewMode::LumpedTimeHistory;
    h.app.ui.view.th_playing = true;
    h.app.ui.view.workspace.secondary.th_playing = false;
    let secondary_time = h.app.ui.view.workspace.secondary.th_play_time;
    h.settled();
    assert_eq!(
        h.app
            .ui
            .view
            .viewport
            .lumped_result(&h.app)
            .unwrap()
            .modal
            .period[0],
        1.0
    );
    assert_eq!(
        h.app
            .ui
            .view
            .workspace
            .secondary
            .lumped_result(&h.app)
            .unwrap()
            .modal
            .period[0],
        3.0
    );
    assert_eq!(
        h.app.ui.view.workspace.secondary.th_play_time,
        secondary_time
    );
    assert!(h.app.ui.view.th_play_time > 0.0);
}

#[test]
fn issue975_paste_into_another_focused_input_does_not_edit_the_selected_node_grid() {
    let mut h = Harness::new();
    h.app.ui.view.workspace.placement.contents = [Content::Model, Content::Drawing];
    let mut other_input = String::new();
    let input_id = egui::Id::new("another_region_input");
    let mut frame = |events| {
        h.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, h.size)),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut other_input).id(input_id));
                h.app.workspace_panel(ui);
            },
        )
    };
    frame(vec![]);
    frame(vec![]);
    h.app
        .ui
        .scoped
        .node_grid
        .grid
        .click(crate::grid::CellRef { row: 0, col: 0 }, false);
    h.ctx.memory_mut(|memory| memory.request_focus(input_id));
    let before = document_state(&h.app);
    let _ = h.ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, h.size)),
            events: vec![Event::Paste("999".into())],
            ..Default::default()
        },
        |ui| {
            ui.add(egui::TextEdit::singleline(&mut other_input).id(input_id));
            h.app.workspace_panel(ui);
        },
    );
    assert_eq!(other_input, "999");
    assert_eq!(document_state(&h.app), before);
    assert!(h.app.ui.scoped.node_grid.grid.editing.is_none());
}

#[test]
fn issue975_hinge_direction_and_step_do_not_follow_the_adjacent_performance_curve() {
    let mut h = Harness::new();
    let mut x = crate::app::tests::dummy_pushover(1.0);
    let mut y = crate::app::tests::dummy_pushover(2.0);
    x.hinges = vec![
        sepika_solver::nonlinear::pushover::HingeEvent {
            step: 0,
            elem: ElemId(0),
            pos: 0.0,
            level: sepika_solver::nonlinear::pushover::HingeLevel::Yield,
            ductility: 1.0,
        },
        sepika_solver::nonlinear::pushover::HingeEvent {
            step: 1,
            elem: ElemId(0),
            pos: 0.0,
            level: sepika_solver::nonlinear::pushover::HingeLevel::Ultimate,
            ductility: 2.0,
        },
    ];
    y.hinges = vec![sepika_solver::nonlinear::pushover::HingeEvent {
        step: 0,
        elem: ElemId(1),
        pos: 0.0,
        level: sepika_solver::nonlinear::pushover::HingeLevel::Crack,
        ductility: 0.1,
    }];
    h.app.core.scoped.results = Some(ResultsBundle {
        pushover_x: Some(x),
        pushover_y: Some(y),
        ..Default::default()
    });
    h.app.ui.view.workspace.placement.arrangement = Arrangement::Horizontal;
    h.app.ui.view.workspace.placement.contents = [Content::Drawing, Content::Results];
    h.app.ui.view.view_mode = crate::viewer::ViewMode::Hinge;
    h.app.ui.view.viewport.pushover_dir = Some(SeismicDir::X);
    h.app.ui.view.viewport.hinge_step = Some(0);
    h.app.ui.view.results_view = ResultsView::Pushover;
    h.settled();
    h.app.set_pushover_view_dir(SeismicDir::Y);
    let output = h.settled();
    assert_eq!(
        h.app.ui.view.viewport.pushover_result(&h.app).unwrap().qu,
        1.0
    );
    assert_eq!(h.app.ui.view.viewport.hinge_step, Some(0));
    let display = h.app.current_display_conditions().unwrap();
    assert_eq!(display.direction.as_deref(), Some("X"));
    assert_eq!(display.step, Some(0));
    assert!(text_rect(&output, "降伏 (1件)").is_some());
    assert!(text_rect(&output, "終局 (0件)").is_some());
}

#[test]
fn issue975_pinned_panels_restore_from_preferences_before_pinning_across_stages() {
    let mut original = Harness::new().app;
    original.ui.view.left_dock_open = false;
    original.ui.view.left_panel = LeftPanel::DrawTools;
    original.ui.view.right_dock_open = true;
    original.ui.view.right_panel = RightPanel::TimeHistory;
    original.ui.view.workspace.preferences.left_pinned = true;
    original.ui.view.workspace.preferences.right_pinned = true;
    original.capture_workspace();
    let bytes = rmp_serde::to_vec_named(&original.ui.view.workspace.preferences).unwrap();
    let mut reopened = App::default();
    reopened.ui.view.workspace.preferences = rmp_serde::from_slice(&bytes).unwrap();
    reopened.enter_workspace();
    assert!(!reopened.ui.view.left_dock_open);
    assert_eq!(reopened.ui.view.left_panel, LeftPanel::DrawTools);
    assert!(reopened.ui.view.right_dock_open);
    assert_eq!(reopened.ui.view.right_panel, RightPanel::TimeHistory);
    assert!(reopened.ui.view.workspace.preferences.left_pinned);
    assert!(reopened.ui.view.workspace.preferences.right_pinned);
    reopened.ui.view.active_tab = Tab::Analysis;
    reopened.enter_workspace();
    assert!(!reopened.ui.view.left_dock_open);
    assert_eq!(reopened.ui.view.left_panel, LeftPanel::DrawTools);
    assert_eq!(reopened.ui.view.right_panel, RightPanel::TimeHistory);
}
