use super::{result_validity::ResultInputKey, App};
use crate::app::{ResultsBundle, StaticKey};
use crate::load_editor::{LoadDraft, LoadEditor};
use crate::theme;
use crate::viewer::{space_grid, viewer_panel, ViewMode};
use sepika_core::frame_gen::{frame_model, FrameSpec};
use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::*;

fn app() -> App {
    let axes = frame_model(&FrameSpec {
        x_spans: vec![6000.0],
        y_spans: vec![6000.0],
        with_slabs: false,
        ..Default::default()
    })
    .unwrap()
    .axes;
    let mut model = Model {
        axes,
        ..Default::default()
    };
    for (i, coord) in [
        [0.0, 0.0, 0.0],
        [6000.0, 0.0, 0.0],
        [0.0, 6000.0, 0.0],
        [6000.0, 6000.0, 0.0],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord,
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, nodes) in [[NodeId(0), NodeId(1)], [NodeId(2), NodeId(3)]]
        .into_iter()
        .enumerate()
    {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: ElementKind::Beam,
            nodes: nodes.into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    let mut app = App::default();
    app.load_model(model);
    app.ui.view.camera.snap_to_direction([0.0, 0.0, 1.0]);
    app.ui.view.show_supports = false;
    app.ui.view.show_beam_interpolation = false;
    let result = |sign: f64| sepika_solver::statics::linear::StaticOnce {
        // 自動倍率10・手動係数10でN1がN0の原位置に重なる。N0自身は別の位置へ移る。
        disp: vec![
            [0.0, 10.0 * sign, 0.0, 0.0, 0.0, 0.0],
            [-60.0 * sign, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0; 6],
            [0.0, 0.0, f64::sqrt(72.0) * 10.0, 0.0, 0.0, 0.0],
        ],
        member_forces: Vec::new(),
        panel_moments: Vec::new(),
    };
    app.core.scoped.results = Some(ResultsBundle {
        combos: vec![("正".into(), result(1.0)), ("逆".into(), result(-1.0))],
        ..Default::default()
    });
    app.core.scoped.last_static = Some(StaticKey::Combo(0));
    let key = ResultInputKey::Combo("正".into());
    let generation = app.result_input(&key);
    app.core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .record_input(key, generation);
    app
}

fn raw(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 1000.0),
        )),
        events,
        ..Default::default()
    }
}

fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_ui(raw(events), |ui| viewer_panel(ui, app))
}

fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn nodes(output: &egui::FullOutput) -> Vec<egui::Pos2> {
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Circle(c) if c.radius == 3.0 && c.fill == theme::DATA_BLUE => {
                Some(c.center)
            }
            _ => None,
        })
        .collect()
}

fn click(ctx: &egui::Context, app: &mut App, pos: egui::Pos2) {
    draw(ctx, app, pointer(pos, true));
    draw(ctx, app, pointer(pos, false));
}

fn persisted(app: &App) -> (Vec<u8>, u64, Vec<u8>, Vec<u8>) {
    (
        rmp_serde::to_vec_named(&app.core.model).unwrap(),
        app.core.scoped.undo.revision(),
        rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
        app.result_input(&ResultInputKey::Combo("正".into())),
    )
}

#[test]
fn beam_first_frame_uses_model_node_at_every_display_factor_and_case() {
    for factor in [0.0, 100.0, -100.0] {
        for case in [0, 1] {
            for stale in [false, true] {
                let mut app = app();
                assert!(space_grid::has_grid(&app.core.model));
                assert!(sepika_core::frame_gen::space_grid(&app.core.model)
                    .points()
                    .any(|p| p == [0.0, 0.0, 0.0]));
                let ctx = egui::Context::default();
                draw(&ctx, &mut app, vec![]);
                let rest = nodes(&draw(&ctx, &mut app, vec![]));
                assert_eq!(rest.len(), 4);
                app.ui.view.view_mode = ViewMode::Deformed;
                app.ui.view.deform_scale_factor = 10.0;
                app.ui.scoped.nav.focus_result = Some(StaticKey::Combo(case));
                app.core.scoped.staleness.results_stale = stale;
                let before = persisted(&app);
                let deformed = nodes(&draw(&ctx, &mut app, vec![]));
                if case == 0 {
                    let origin =
                        egui::pos2(deformed[2].x, deformed[2].y + deformed[3].x - deformed[2].x);
                    assert!(deformed[1].distance(origin) < 0.01);
                    assert!(deformed[0].distance(origin) > 10.0);
                }
                app.ui.view.deform_scale_factor = factor;
                app.ui.scoped.beam_draw_mode = true;
                let mut events = pointer(rest[0], true);
                events.extend(pointer(rest[0], false));
                let first_output = draw(&ctx, &mut app, events);
                assert_eq!(
                    app.ui.scoped.beam_draw_first,
                    Some(space_grid::SnapPoint::Node(NodeId(0))),
                    "factor={factor}, case={case}, stale={stale}, rest={rest:?}, first={:?}",
                    nodes(&first_output)
                );
                assert_eq!(app.core.model.nodes[0].coord, [0.0, 0.0, 0.0]);
                assert_eq!(app.ui.view.view_mode, ViewMode::Shape);
                assert_eq!(app.ui.view.deform_scale_factor, factor);
                assert!(!app.ui.view.overlay_deform);
                assert_eq!(persisted(&app), before);
                click(&ctx, &mut app, rest[0]);
                assert!(app.ui.scoped.beam_draw_first.is_none());
                assert_eq!(persisted(&app), before);
                app.ui.scoped.beam_draw_mode = false;
                draw(&ctx, &mut app, vec![]);
                assert_eq!(app.ui.view.view_mode, ViewMode::Shape);
                assert_eq!(app.core.scoped.staleness.results_stale, stale);
            }
        }
    }
}

#[test]
fn completed_beam_keeps_model_coordinates_and_marks_existing_results_stale() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.ui.view.view_mode = ViewMode::Force;
    draw(&ctx, &mut app, vec![]);
    let rest = nodes(&draw(&ctx, &mut app, vec![]));
    let result = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
    let generation = app.result_input(&ResultInputKey::Combo("正".into()));
    app.ui.view.view_mode = ViewMode::Force;
    app.ui.view.overlay_deform = true;
    app.ui.view.deform_scale_factor = 100.0;
    app.ui.scoped.beam_draw_mode = true;
    click(&ctx, &mut app, rest[0]);
    click(&ctx, &mut app, rest[3]);
    assert_eq!(app.core.model.elements.len(), 3);
    assert_eq!(
        app.core.model.elements[2].nodes.as_slice(),
        &[NodeId(0), NodeId(3)]
    );
    assert_eq!(
        app.core
            .model
            .nodes
            .iter()
            .map(|n| n.coord)
            .collect::<Vec<_>>(),
        vec![
            [0.0, 0.0, 0.0],
            [6000.0, 0.0, 0.0],
            [0.0, 6000.0, 0.0],
            [6000.0, 6000.0, 0.0]
        ]
    );
    assert_eq!(app.core.scoped.undo.revision(), 1);
    assert!(app.core.scoped.staleness.results_stale);
    assert_ne!(
        app.result_input(&ResultInputKey::Combo("正".into())),
        generation
    );
    assert_eq!(
        rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
        result
    );
    assert_eq!(app.ui.view.view_mode, ViewMode::Force);
    assert!(!app.ui.view.overlay_deform);
    app.ui.scoped.beam_draw_mode = false;
    draw(&ctx, &mut app, vec![]);
    assert!(!app.ui.view.overlay_deform);
}

#[test]
fn result_browsing_still_selects_member_at_deformed_position() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.ui.view.view_mode = ViewMode::Deformed;
    app.ui.view.deform_scale_factor = 2.0;
    draw(&ctx, &mut app, vec![]);
    let deformed = nodes(&draw(&ctx, &mut app, vec![]));
    let before = persisted(&app);
    click(&ctx, &mut app, deformed[0].lerp(deformed[1], 0.5));
    assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(0)));
    assert_eq!(
        app.core.model.elements[0].nodes.as_slice(),
        &[NodeId(0), NodeId(1)]
    );
    assert_eq!(app.ui.view.view_mode, ViewMode::Deformed);
    assert_eq!(persisted(&app), before);
}

fn text_pos(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == label => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("操作が見つかりません: {label}"))
}

#[test]
fn every_creation_palette_entry_clears_deformation_when_started() {
    for label in [
        "梁作成モード",
        "壁版割当モード",
        "床板割当モード",
        "小梁配置モード",
        "間柱配置モード",
    ] {
        for mode in [
            ViewMode::Deformed,
            ViewMode::Mode,
            ViewMode::TimeHistory,
            ViewMode::LumpedMode,
            ViewMode::LumpedTimeHistory,
            ViewMode::Force,
        ] {
            let mut app = app();
            let before = persisted(&app);
            let ctx = egui::Context::default();
            let _ = ctx.run_ui(raw(vec![]), |ui| app.draw_tools_panel(ui));
            let output = ctx.run_ui(raw(vec![]), |ui| app.draw_tools_panel(ui));
            let pos = text_pos(&output, label);
            app.ui.view.view_mode = mode;
            app.ui.view.overlay_deform = true;
            for pressed in [true, false] {
                let _ = ctx.run_ui(raw(pointer(pos, pressed)), |ui| app.draw_tools_panel(ui));
            }
            assert_eq!(
                app.ui.view.view_mode,
                if mode == ViewMode::Force {
                    ViewMode::Force
                } else {
                    ViewMode::Shape
                },
                "{label}: {mode:?}"
            );
            assert!(!app.ui.view.overlay_deform, "{label}: {mode:?}");
            assert_eq!(persisted(&app), before);
            assert!(!app.core.scoped.undo.can_undo());
            assert!(!app.core.scoped.staleness.results_stale);
        }
    }
}

#[test]
fn load_modal_begin_pick_and_escape_keep_model_history_and_results() {
    for nodal in [true, false] {
        let mut app = app();
        let ctx = egui::Context::default();
        draw(&ctx, &mut app, vec![]);
        let rest = nodes(&draw(&ctx, &mut app, vec![]));
        let before = persisted(&app);
        app.ui.view.view_mode = ViewMode::Deformed;
        app.ui.view.overlay_deform = true;
        app.ui.view.deform_scale_factor = 100.0;
        app.ui.scoped.load_editor = Some(if nodal {
            LoadEditor::new_nodal(LoadCaseId(0), None)
        } else {
            LoadEditor::new_member(LoadCaseId(0), None)
        });
        let load_frame = |app: &mut App, events| {
            ctx.run_ui(raw(events), |ui| {
                viewer_panel(ui, app);
                app.load_editor_ui(ui.ctx());
            })
        };
        load_frame(&mut app, vec![]);
        let output = load_frame(&mut app, vec![]);
        let begin = text_pos(&output, "3D で選択");
        load_frame(&mut app, pointer(begin, true));
        load_frame(&mut app, pointer(begin, false));
        assert!(app.load_pick_active());
        assert_eq!(app.ui.view.view_mode, ViewMode::Shape);
        assert!(!app.ui.view.overlay_deform);
        assert_eq!(persisted(&app), before);
        load_frame(&mut app, vec![]);
        let pos = if nodal {
            rest[0]
        } else {
            rest[0].lerp(rest[1], 0.5)
        };
        load_frame(&mut app, pointer(pos, true));
        load_frame(&mut app, pointer(pos, false));
        let editor = app.ui.scoped.load_editor.as_ref().unwrap();
        match &editor.draft {
            LoadDraft::Nodal(d) => {
                assert_eq!(d.node, Some(NodeId(0)));
                assert_eq!(
                    app.core.model.nodes[d.node.unwrap().index()].coord,
                    [0.0, 0.0, 0.0]
                );
            }
            LoadDraft::Member(d) => assert_eq!(d.elem, Some(ElemId(0))),
        }
        assert_eq!(persisted(&app), before);
        load_frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.load_pick_active());
        match &app.ui.scoped.load_editor.as_ref().unwrap().draft {
            LoadDraft::Nodal(d) => assert_eq!(d.node, None),
            LoadDraft::Member(d) => assert_eq!(d.elem, None),
        }
        assert_eq!(app.ui.view.view_mode, ViewMode::Shape);
        assert_eq!(persisted(&app), before);
        assert!(!app.core.scoped.undo.can_undo());
        assert!(!app.core.scoped.staleness.results_stale);
    }
}

#[test]
fn switching_to_deformation_during_edit_is_cleared_before_the_next_pick() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.ui.scoped.beam_draw_mode = true;
    draw(&ctx, &mut app, vec![]);
    let output = draw(&ctx, &mut app, vec![]);
    let pos = text_pos(&output, "変形");
    click(&ctx, &mut app, pos);
    assert_eq!(app.ui.view.view_mode, ViewMode::Shape);
    let rest = nodes(&draw(&ctx, &mut app, vec![]));
    click(&ctx, &mut app, rest[0]);
    assert_eq!(
        app.ui.scoped.beam_draw_first,
        Some(space_grid::SnapPoint::Node(NodeId(0)))
    );
}

#[test]
fn secondary_placement_starts_with_undeformed_parent_and_support_axis() {
    use super::WorkScope;
    use sepika_core::ids::{FloorRegionId, WallRegionId};
    for post in [false, true] {
        let mut app = app();
        if post {
            for n in &mut app.core.model.nodes {
                n.coord = [n.coord[0], 0.0, n.coord[1]];
            }
            app.core.model.wall_regions.push(WallRegion::new(
                WallRegionId(0),
                vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)],
            ));
            app.ui.view.camera.snap_to_direction([0.0, -1.0, 0.0]);
        } else {
            app.core.model.floor_regions.push(FloorRegion::new(
                FloorRegionId(0),
                vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)],
            ));
        }
        let ctx = egui::Context::default();
        draw(&ctx, &mut app, vec![]);
        let rest = nodes(&draw(&ctx, &mut app, vec![]));
        let before = persisted(&app);
        app.ui.view.view_mode = ViewMode::Deformed;
        app.ui.view.overlay_deform = true;
        app.ui.view.deform_scale_factor = 100.0;
        app.ui.scoped.beam_place_mode = !post;
        app.ui.scoped.post_place_mode = post;
        click(&ctx, &mut app, rest[0].lerp(rest[3], 0.5));
        assert_eq!(
            app.ui.scoped.work_scope,
            Some(if post {
                WorkScope::Wall(WallRegionId(0))
            } else {
                WorkScope::Floor(FloorRegionId(0))
            })
        );
        click(&ctx, &mut app, rest[0].lerp(rest[1], 0.5));
        let anchor = app.ui.scoped.member_place_first.unwrap();
        assert_eq!(anchor.support, SupportMemberId::Primary(ElemId(0)));
        assert!((anchor.position - 0.5).abs() < 1e-6);
        let coord = app.core.model.anchor_point(anchor).unwrap();
        assert!((coord[0] - 3000.0).abs() < 0.001);
        assert_eq!([coord[1], coord[2]], [0.0, 0.0]);
        assert_eq!(app.ui.view.view_mode, ViewMode::Shape);
        assert!(!app.ui.view.overlay_deform);
        assert_eq!(persisted(&app), before);
    }
}
