//! ビューアの視点操作（ドラッグ・ズーム・構面正対・ViewCube）。
//!
//! 描画領域のポインタ入力を、カメラ状態とモデルの選択・編集へ反映する部分を
//! ここへ集める。視点（[`interact_camera`]・[`interact_viewcube`]）は投影より前に
//! 確定させる必要があり、クリック処理（[`handle_click`]）は投影後の点列が要る。
//! ホバー時の強調表示は描画と一体のため、[`super::viewer_panel`] 側に残している。

use sepika_core::frame::Frame;

use crate::app::App;

use super::camera::CameraState;
use super::pick::{
    member_load_pickable, pick_assignment_region, pick_nearest_member, pick_nearest_node,
    pick_parent_region, pick_support_anchor, ParentRegionPick, RegionPick,
};
use super::wall_expanded_view_model;
use super::{frame_view, space_grid, viewcube, FrameFilter, Projector, ViewMode};

/// ViewCube（右上に描く方位キューブ）の当たり判定結果。
pub(super) struct ViewCubeState {
    pub layout: viewcube::Layout,
    /// 構面表示中は視点が固定のため出さない。
    pub visible: bool,
    pub hover: Option<viewcube::Hit>,
    /// キューブ上のクリックはピック処理へ流さないため、呼び出し側が参照する。
    pub clicked: bool,
}

/// ポインタ入力（[`CameraState::apply_pointer_input`]）と構面正対を反映した
/// カメラを返す。
///
/// 構面表示中は回転を禁じたうえで、その構面の法線方向へ毎フレーム正対させる。
/// 正対はこのビュー固有の扱いのため、操作の共通部分とは分けてここに置く。
pub(super) fn interact_camera(
    ui: &egui::Ui,
    response: &egui::Response,
    frame: Option<&Frame>,
    base: &CameraState,
) -> CameraState {
    let mut cam = base.clone();
    cam.apply_pointer_input(ui, response, frame.is_none());

    if let Some(f) = frame {
        cam.snap_to_direction(frame_view::view_direction(f.normal));
    }
    cam
}

/// ViewCube の当たり判定と、クリックによる視点スナップ。
///
/// 面クリック＝標準ビュー／コーナークリック＝アイソメへ即時スナップする。
/// モデルより手前の固定 UI のため、当たり判定を部材ピックより先に行い、
/// キューブ上のクリックはピック処理へ流さない。
pub(super) fn interact_viewcube(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    visible: bool,
    cam: &mut CameraState,
) -> ViewCubeState {
    let layout = viewcube::Layout {
        center: egui::pos2(rect.max.x - 55.0, rect.min.y + 55.0),
        scale: 22.0,
    };
    let hover = visible
        .then(|| {
            response
                .hover_pos()
                .and_then(|p| viewcube::hit_test(cam, &layout, p))
        })
        .flatten();
    if hover.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut clicked = false;
    if visible && response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if let Some(hit) = viewcube::hit_test(cam, &layout, pos) {
                cam.snap_to_direction(viewcube::hit_direction(hit));
                clicked = true;
            }
        }
    }
    ViewCubeState {
        layout,
        visible,
        hover,
        clicked,
    }
}

/// クリック処理が要る描画側の文脈（投影結果と絞り込み条件）。
///
/// `pts` は全節点をこのフレームの投影で写した画面座標、`node_visible` はその
/// 表示可否（解析対象外の節点を作成モードのピック対象から外す）。
pub(super) struct ClickContext<'a> {
    pub pts: &'a [egui::Pos2],
    pub node_visible: &'a [bool],
    pub filter: FrameFilter<'a>,
    pub proj: &'a Projector<'a>,
    pub frame: Option<&'a Frame>,
    pub mode: ViewMode,
}

/// 描画領域のクリックを処理する（荷重の対象ピック・作成モードの節点選び・
/// 通常の選択）。ViewCube 上のクリックは呼び出し側で除外済み。
///
/// 呼び出し側の描画用モデル（`wall_expanded_view_model` の結果）を作る前に呼ぶ。
/// 梁・壁作成モードが `app.core.model` を可変借用するためである。
/// 通常モードの部材ピックだけは壁を展開したモデルが要るため、この中で作り直す。
pub(super) fn handle_click(app: &mut App, response: &egui::Response, ctx: ClickContext<'_>) {
    let ClickContext {
        pts,
        node_visible,
        filter,
        proj,
        frame,
        mode,
    } = ctx;
    if let Some(click_pos) = response.interact_pointer_pos() {
        if app.load_pick_active() {
            let picks_node = app
                .ui
                .scoped
                .load_editor
                .as_ref()
                .is_some_and(|e| e.picks_node());
            if picks_node {
                const NODE_PICK_THRESHOLD: f32 = 10.0;
                if let Some((i, d)) = pick_nearest_node(pts, node_visible, click_pos) {
                    if d <= NODE_PICK_THRESHOLD {
                        let node_id = app.core.model.nodes[i].id;
                        if let Some(editor) = app.ui.scoped.load_editor.as_mut() {
                            editor.set_picked_node(node_id);
                        }
                        app.select_node(node_id);
                    }
                }
            } else {
                const PICK_THRESHOLD: f32 = 8.0;
                if let Some((id, d)) = pick_nearest_member(&app.core.model, pts, click_pos, filter)
                {
                    if d <= PICK_THRESHOLD && member_load_pickable(&app.core.model, id) {
                        let is_brace = crate::load_editor::is_brace(&app.core.model, id);
                        if let Some(editor) = app.ui.scoped.load_editor.as_mut() {
                            editor.set_picked_member(id, is_brace);
                        }
                        app.select_member(id);
                    }
                }
            }
        } else if app.ui.scoped.beam_draw_mode {
            let picked = if app.ui.view.show_space_grid && frame.is_none() {
                space_grid::pick(&app.core.model, proj, pts, node_visible, click_pos)
            } else {
                const NODE_PICK_THRESHOLD: f32 = 10.0;
                pick_nearest_node(pts, node_visible, click_pos)
                    .filter(|(_, d)| *d <= NODE_PICK_THRESHOLD)
                    .map(|(i, _)| space_grid::SnapPoint::Node(app.core.model.nodes[i].id))
            };
            if let Some(point) = picked {
                match app.ui.scoped.beam_draw_first {
                    None => {
                        app.ui.scoped.beam_draw_first = Some(point);
                    }
                    Some(first) => {
                        if let Some((cmd, new_id)) =
                            space_grid::beam_command(&app.core.model, first, point)
                        {
                            if app.apply_model_edit(Box::new(cmd)) {
                                app.select_member(new_id);
                            }
                        }
                        app.ui.scoped.beam_draw_first = None;
                    }
                }
            }
        } else if app.ui.scoped.wall_draw_mode {
            if let Some(RegionPick::Wall(id)) =
                pick_assignment_region(&app.core.model, proj, click_pos, false, true)
            {
                app.ui.scoped.region_assign_dialog = Some(crate::app::RegionAssignTarget::Wall(id));
            }
        } else if app.ui.scoped.slab_draw_mode {
            if let Some(RegionPick::Floor(id)) =
                pick_assignment_region(&app.core.model, proj, click_pos, true, false)
            {
                app.ui.scoped.region_assign_dialog =
                    Some(crate::app::RegionAssignTarget::Floor(id));
            }
        } else if app.ui.scoped.beam_place_mode || app.ui.scoped.post_place_mode {
            use crate::app::WorkScope;
            use sepika_core::model::{SecondaryMemberEnds, SecondaryMemberKind};
            use sepika_edit::{PlaceSecondaryMember, SecondaryParent};
            let kind = if app.ui.scoped.beam_place_mode {
                SecondaryMemberKind::Beam
            } else {
                SecondaryMemberKind::Post
            };
            if app.ui.scoped.work_scope.is_none() {
                let include_floor = kind == SecondaryMemberKind::Beam;
                let include_wall = kind == SecondaryMemberKind::Post;
                if let Some(pick) = pick_parent_region(
                    &app.core.model,
                    proj,
                    click_pos,
                    include_floor,
                    include_wall,
                ) {
                    app.ui.scoped.work_scope = Some(match pick {
                        ParentRegionPick::Floor(id) => WorkScope::Floor(id),
                        ParentRegionPick::Wall(id) => WorkScope::Wall(id),
                    });
                    app.ui.scoped.member_place_first = None;
                }
            } else if let Some(anchor) =
                pick_support_anchor(&app.core.model, proj, click_pos, kind, 12.0)
            {
                match app.ui.scoped.member_place_first {
                    None => app.ui.scoped.member_place_first = Some(anchor),
                    Some(first) => {
                        if first != anchor {
                            let parent = match app.ui.scoped.work_scope {
                                Some(WorkScope::Floor(id)) => Some(SecondaryParent::Floor(id)),
                                Some(WorkScope::Wall(id)) => Some(SecondaryParent::Wall(id)),
                                None => None,
                            };
                            if let Some(parent) = parent {
                                let inside = match (
                                    app.ui.scoped.work_scope,
                                    app.core.model.anchor_point(first),
                                    app.core.model.anchor_point(anchor),
                                ) {
                                    (Some(WorkScope::Floor(id)), Some(a), Some(b)) => {
                                        app.core.model.floor_region_contains_point(
                                            id,
                                            [
                                                (a[0] + b[0]) * 0.5,
                                                (a[1] + b[1]) * 0.5,
                                                (a[2] + b[2]) * 0.5,
                                            ],
                                        )
                                    }
                                    (Some(WorkScope::Wall(id)), Some(a), Some(b)) => {
                                        app.core.model.wall_region_contains_point(
                                            id,
                                            [
                                                (a[0] + b[0]) * 0.5,
                                                (a[1] + b[1]) * 0.5,
                                                (a[2] + b[2]) * 0.5,
                                            ],
                                        )
                                    }
                                    _ => false,
                                };
                                let applied = inside
                                    && app.core.scoped.undo.run(
                                        &mut app.core.model,
                                        Box::new(PlaceSecondaryMember {
                                            parent,
                                            kind,
                                            ends: SecondaryMemberEnds::Supported([first, anchor]),
                                            section: app.ui.scoped.secondary_draft.section,
                                            name: app.ui.scoped.secondary_draft.name.clone(),
                                        }),
                                    );
                                if applied {
                                    app.clear_generated_member_selection();
                                    app.core.scoped.staleness.mark_edited();
                                } else {
                                    app.core.scoped.last_notice = Some(
                                        "両端が作業範囲の外側をつなぐため配置しませんでした。\
                                         作業範囲（親領域）の内側で支持部材を選んでください。"
                                            .to_string(),
                                    );
                                }
                            }
                        }
                        app.ui.scoped.member_place_first = None;
                    }
                }
            }
        } else {
            const PICK_THRESHOLD: f32 = 8.0;
            let display_model = wall_expanded_view_model(&app.core.model);
            let frame_for_pick = app
                .ui
                .scoped
                .frame_target
                .and_then(|t| sepika_core::frame::build_frame(display_model.as_ref(), t));
            let filter_pick = FrameFilter::new(frame_for_pick.as_ref());
            match pick_nearest_member(display_model.as_ref(), pts, click_pos, filter_pick) {
                Some((id, d)) if d <= PICK_THRESHOLD => {
                    app.select_member(id);
                    if mode == ViewMode::Hinge {
                        if app.ui.scoped.hinge_detail_elem != Some(id) {
                            app.ui.scoped.hinge_step = None;
                            app.ui.scoped.hinge_view_cache = None;
                        }
                        app.ui.scoped.hinge_detail_elem = Some(id);
                    }
                    if mode == ViewMode::TimeHistory && !app.core.scoped.staleness.results_stale {
                        app.ui.scoped.th_detail_elem = Some(id);
                    }
                }
                _ => {
                    app.clear_geometry_selection();
                }
            }
        }
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::app::{GeometrySelection, WorkScope};
    use sepika_core::ids::*;
    use sepika_core::model::*;
    use sepika_core::section_shape::SectionShape;

    fn wall_app() -> App {
        let mut app = App::default();
        app.core.model.sections.push(
            SectionShape::RcWall {
                thickness: 180.0,
                ps: 0.0025,
            }
            .to_section(SectionId(0), "壁".into()),
        );
        for wall in 0..2u32 {
            let offset = wall * 4;
            for (i, (x, z)) in [(0.0, 0.0), (4000.0, 0.0), (4000.0, 3000.0), (0.0, 3000.0)]
                .into_iter()
                .enumerate()
            {
                app.core.model.nodes.push(Node {
                    id: NodeId(offset + i as u32),
                    coord: [x + f64::from(wall) * 8000.0, 0.0, z],
                    restraint: Default::default(),
                    mass: None,
                    story: None,
                    support_spring: None,
                });
            }
            for (i, (a, b)) in [(0, 3), (1, 2), (3, 2), (0, 1)].into_iter().enumerate() {
                app.core.model.elements.push(ElementData {
                    id: ElemId(offset + i as u32),
                    kind: ElementKind::Beam,
                    nodes: [NodeId(offset + a), NodeId(offset + b)]
                        .into_iter()
                        .collect(),
                    section: None,
                    local_axis: LocalAxis {
                        ref_vector: [0.0, 1.0, 0.0],
                    },
                    end_cond: [EndCondition::Fixed; 2],
                    force_regime: ForceRegime::Auto,
                    rigid_zone: Default::default(),
                    plastic_zone: None,
                    spring: None,
                });
            }
            let boundary = (offset..offset + 4).map(NodeId).collect::<Vec<_>>();
            app.core
                .model
                .wall_regions
                .push(WallRegion::new(WallRegionId(wall), boundary.clone()));
            app.core.model.add_enclosed_wall_plate_from_nodes(
                &boundary,
                WallPlate {
                    id: WallPlateId(wall),
                    shape: WallPlateShape::Enclosed,
                    section: Some(SectionId(0)),
                    self_weight_shares: Vec::new(),
                    opening_area: 0.0,
                    opening_weight: 0.0,
                    openings: Vec::new(),
                    loads: Vec::new(),
                    slit: Default::default(),
                },
            );
            app.core.model.wall_regions[wall as usize]
                .wall_plate_ids
                .push(WallPlateId(wall));
        }
        app.core.model.rebuild_wall_assignment_regions();
        app
    }

    fn raw_input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            ..Default::default()
        }
    }

    fn click(ctx: &egui::Context, pos: egui::Pos2, mut draw: impl FnMut(&mut egui::Ui)) {
        for pressed in [true, false] {
            let mut raw = raw_input();
            raw.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let _ = ctx.run_ui(raw, |ui| draw(ui));
        }
    }

    fn text_pos(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("ボタン・候補が見つかりません: {label}"))
    }

    fn place_post(app: &mut App, viewer: bool) {
        let ctx = egui::Context::default();
        if viewer {
            app.ui.scoped.post_place_mode = true;
            app.ui.scoped.work_scope = Some(WorkScope::Wall(WallRegionId(0)));
            app.ui.scoped.member_place_first = Some(SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(3)),
                position: 0.5,
            });
            let mut cam = CameraState::default();
            cam.snap_to_direction([0.0, -1.0, 0.0]);
            let proj = Projector::new([2000.0, 0.0, 1500.0], &cam, 0.1, [400.0, 400.0]);
            let pos = proj.project([2000.0, 0.0, 3000.0]);
            let mut draw = |ui: &mut egui::Ui| {
                let response = ui.allocate_response(egui::vec2(800.0, 800.0), egui::Sense::click());
                if response.clicked() {
                    handle_click(
                        app,
                        &response,
                        ClickContext {
                            pts: &[],
                            node_visible: &[],
                            filter: FrameFilter::new(None),
                            proj: &proj,
                            frame: None,
                            mode: ViewMode::Shape,
                        },
                    );
                }
            };
            let _ = ctx.run_ui(raw_input(), &mut draw);
            click(&ctx, pos, draw);
        } else {
            app.ui.scoped.secondary_draft.parent = Some(0);
            app.ui.scoped.secondary_draft.support_a = Some(SupportMemberId::Primary(ElemId(3)));
            app.ui.scoped.secondary_draft.support_b = Some(SupportMemberId::Primary(ElemId(2)));
            let mut draw = |ui: &mut egui::Ui| {
                crate::tables::secondary::secondary_member_placement_form(
                    app,
                    ui,
                    SecondaryMemberKind::Post,
                )
            };
            let _ = ctx.run_ui(raw_input(), &mut draw);
            let output = ctx.run_ui(raw_input(), &mut draw);
            click(&ctx, text_pos(&output, "配置"), draw);
        }
        assert_eq!(app.core.model.posts().count(), 1);
    }

    #[test]
    fn secondary_placement_clears_wall_selection_before_generated_id_reuse() {
        for viewer in [false, true] {
            let mut app = wall_app();
            let before = wall_expanded_view_model(&app.core.model);
            let walls = before
                .elements
                .iter()
                .filter(|e| e.kind == ElementKind::Wall)
                .collect::<Vec<_>>();
            assert_eq!(walls.len(), 2);
            let old_id = walls[0].id;
            let b_nodes = walls[1].nodes.clone();
            drop(before);
            app.select_member(old_id);
            place_post(&mut app, viewer);
            assert_eq!(app.core.model.wall_plates.len(), 1, "間柱で壁Aが孤児化する");
            let after = wall_expanded_view_model(&app.core.model);
            let reused = after
                .element(old_id)
                .expect("壁Bが壁Aの旧生成IDを再利用する");
            assert_eq!(reused.nodes, b_nodes);
            assert_eq!(
                app.ui.scoped.selection,
                GeometrySelection::None,
                "viewer={viewer}"
            );
        }
    }

    #[test]
    fn secondary_placement_preserves_primary_member_and_node_selection() {
        for viewer in [false, true] {
            for node in [false, true] {
                let mut app = wall_app();
                if node {
                    app.select_nodes(vec![NodeId(0), NodeId(1)], Some(NodeId(1)));
                } else {
                    app.select_members(vec![ElemId(0), ElemId(1)], Some(ElemId(1)));
                }
                place_post(&mut app, viewer);
                if node {
                    assert_eq!(app.ui.scoped.selection.nodes(), &[NodeId(0), NodeId(1)]);
                    assert_eq!(app.ui.scoped.selection.active_node(), Some(NodeId(1)));
                } else {
                    assert_eq!(app.ui.scoped.selection.members(), &[ElemId(0), ElemId(1)]);
                    assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(1)));
                }
            }
        }
    }

    #[test]
    fn node_coordinate_edit_clears_only_generated_selection_before_id_reuse() {
        for selected in 0..3 {
            let mut app = wall_app();
            let before = wall_expanded_view_model(&app.core.model);
            let walls = before
                .elements
                .iter()
                .filter(|e| e.kind == ElementKind::Wall)
                .collect::<Vec<_>>();
            let old_id = walls[0].id;
            let b_nodes = walls[1].nodes.clone();
            drop(before);
            match selected {
                0 => app.select_member(old_id),
                1 => app.select_members(vec![ElemId(0), ElemId(1)], Some(ElemId(1))),
                _ => app.select_nodes(vec![NodeId(0), NodeId(1)], Some(NodeId(1))),
            }
            let ctx = egui::Context::default();
            let _ = ctx.run_ui(raw_input(), |ui| {
                crate::tables::nodes::nodes_table(ui, &mut app);
            });
            app.ui
                .scoped
                .node_grid
                .grid
                .click(crate::grid::CellRef { row: 0, col: 0 }, false);
            let mut raw = raw_input();
            raw.events = vec![egui::Event::Paste("4000".into())];
            let _ = ctx.run_ui(raw, |ui| {
                crate::tables::nodes::nodes_table(ui, &mut app);
            });
            assert_eq!(app.core.model.nodes[0].coord[0], 4000.0);
            assert_eq!(app.core.model.wall_plates.len(), 1);
            let after = wall_expanded_view_model(&app.core.model);
            assert_eq!(after.element(old_id).unwrap().nodes, b_nodes);
            match selected {
                0 => assert_eq!(app.ui.scoped.selection, GeometrySelection::None),
                1 => {
                    assert_eq!(app.ui.scoped.selection.members(), &[ElemId(0), ElemId(1)]);
                    assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(1)));
                }
                _ => {
                    assert_eq!(app.ui.scoped.selection.nodes(), &[NodeId(0), NodeId(1)]);
                    assert_eq!(app.ui.scoped.selection.active_node(), Some(NodeId(1)));
                }
            }
        }
    }

    #[test]
    fn secondary_list_edits_clear_only_generated_member_selection() {
        for delete in [false, true] {
            for selected in 0..3 {
                let mut app = wall_app();
                place_post(&mut app, false);
                match selected {
                    0 => {
                        let view = wall_expanded_view_model(&app.core.model);
                        let id = view
                            .elements
                            .iter()
                            .find(|e| e.kind == ElementKind::Wall)
                            .unwrap()
                            .id;
                        app.select_member(id);
                    }
                    1 => app.select_members(vec![ElemId(0), ElemId(1)], Some(ElemId(1))),
                    _ => app.select_nodes(vec![NodeId(0), NodeId(1)], Some(NodeId(1))),
                }
                let ctx = egui::Context::default();
                let mut draw = |ui: &mut egui::Ui| {
                    crate::tables::secondary::secondary_member_list(
                        &mut app,
                        ui,
                        SecondaryMemberKind::Post,
                    )
                };
                let _ = ctx.run_ui(raw_input(), &mut draw);
                let output = ctx.run_ui(raw_input(), &mut draw);
                let label = if delete { "削除" } else { "支持-支持" };
                click(&ctx, text_pos(&output, label), &mut draw);
                if !delete {
                    let output = ctx.run_ui(raw_input(), &mut draw);
                    click(&ctx, text_pos(&output, "支持-自由（片持ち）"), draw);
                    assert!(matches!(
                        app.core.model.posts().next().unwrap().ends,
                        SecondaryMemberEnds::Cantilever { .. }
                    ));
                } else {
                    assert_eq!(app.core.model.posts().count(), 0);
                }
                match selected {
                    0 => assert_eq!(app.ui.scoped.selection, GeometrySelection::None),
                    1 => {
                        assert_eq!(app.ui.scoped.selection.members(), &[ElemId(0), ElemId(1)]);
                        assert_eq!(app.ui.scoped.selection.active_member(), Some(ElemId(1)));
                    }
                    _ => {
                        assert_eq!(app.ui.scoped.selection.nodes(), &[NodeId(0), NodeId(1)]);
                        assert_eq!(app.ui.scoped.selection.active_node(), Some(NodeId(1)));
                    }
                }
            }
        }
    }
}
