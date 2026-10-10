use super::*;
use sepika_core as core_fixture_api;
#[path = "../../../sepika-core/tests/support/assignment_identity_model.rs"]
mod fixture;

#[test]
fn 準備計算は孤立版を表示し領域と入力荷重を未更新にする() {
    for wall in [false, true] {
        let mut app = App::default();
        app.core.model = fixture::with_plate_metadata(wall);
        if wall {
            app.core.model.unassigned_posts.push(fixture::divider(true));
        } else {
            app.core
                .model
                .unassigned_beams
                .push(fixture::divider(false));
        }
        let before = app.core.model.clone();
        app.sync_auto_load_cases_action();
        let reason = app.core.scoped.last_error.as_ref().unwrap();
        assert!(reason.contains("孤立版"));
        assert!(reason.contains("未更新"));
        assert!(reason.contains("0.0025"));
        fixture::assert_inputs_eq(&app.core.model, &before);
    }
}

#[cfg(feature = "gui")]
#[test]
fn guiは除去確認キャンセルで未更新とし確認後に一undoで全入力へ戻す() {
    use sepika_core::ids::{FloorRegionId, WallRegionId};
    use sepika_edit::{PlaceSecondaryMember, SecondaryParent};
    for wall in [false, true] {
        let mut app = App::default();
        app.core.model = fixture::with_plate(wall);
        app.core.model.assign_stb_node_ids().unwrap();
        let before = app.core.model.clone();
        let command = || {
            let sm = fixture::divider(wall);
            Box::new(PlaceSecondaryMember {
                parent: if wall {
                    SecondaryParent::Wall(WallRegionId(0))
                } else {
                    SecondaryParent::Floor(FloorRegionId(0))
                },
                kind: sm.kind,
                ends: sm.ends,
                section: None,
                name: String::new(),
            })
        };
        assert!(!app.apply_model_edit(command()));
        assert!(app
            .core
            .scoped
            .pending_plate_loss_edit
            .as_ref()
            .unwrap()
            .1
            .contains("0.0025"));
        fixture::assert_inputs_eq(&app.core.model, &before);
        assert_eq!(app.core.scoped.undo.revision(), 0);
        app.resolve_plate_loss_edit(false);
        fixture::assert_inputs_eq(&app.core.model, &before);
        assert!(!app.core.scoped.undo.can_undo());
        assert!(!app.apply_model_edit(command()));
        app.resolve_plate_loss_edit(true);
        assert_eq!(app.core.scoped.undo.revision(), 1);
        app.core.scoped.undo.undo(&mut app.core.model);
        fixture::assert_inputs_eq(&app.core.model, &before);
        app.core.scoped.undo.redo(&mut app.core.model);
        assert_eq!(app.core.model.next_secondary_member_id, 1);
    }
}

#[cfg(feature = "gui")]
#[test]
fn guiは境界衝突の拒否理由と未更新を表示し確認待ちにも履歴にも積まない() {
    use sepika_core::ids::{FloorPlateAssignmentRegionId, FloorRegionId};
    use sepika_edit::{PlaceSecondaryMember, SecondaryParent};
    let mut app = App::default();
    app.core.model = fixture::with_plate(false);
    let mut duplicate = app.core.model.floor_assignment_regions.regions[0].clone();
    duplicate.id = FloorPlateAssignmentRegionId(90);
    app.core
        .model
        .floor_assignment_regions
        .regions
        .push(duplicate);
    let before = app.core.model.clone();
    let sm = fixture::divider(false);
    assert!(!app.apply_model_edit(Box::new(PlaceSecondaryMember {
        parent: SecondaryParent::Floor(FloorRegionId(0)),
        kind: sm.kind,
        ends: sm.ends,
        section: None,
        name: String::new()
    })));
    let reason = app.core.scoped.last_error.as_ref().unwrap();
    assert!(reason.contains("境界キー"));
    assert!(reason.contains("未更新"));
    assert!(app.core.scoped.pending_plate_loss_edit.is_none());
    assert_eq!(app.core.scoped.undo.revision(), 0);
    assert!(!app.core.scoped.undo.can_undo());
    fixture::assert_inputs_eq(&app.core.model, &before);
}

#[cfg(feature = "gui")]
#[test]
fn 主架構一覧削除と節点移動は確認前とキャンセルで未更新とし確認後の一undoで荷重を戻す() {
    use sepika_core::ids::{ElemId, NodeId};
    for wall in [false, true] {
        for move_node in [false, true] {
            let mut app = App::default();
            app.core.model = fixture::with_plate(wall);
            app.core.model.assign_stb_node_ids().unwrap();
            let before = app.core.model.clone();
            let edit = |app: &mut App| {
                if move_node {
                    app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
                        node: NodeId(1),
                        coord: if wall {
                            [4000., 1000., 0.]
                        } else {
                            [4000., 0., 1000.]
                        },
                    }))
                } else {
                    crate::tables::members::delete_frame_member(app, ElemId(0))
                }
            };
            assert!(!edit(&mut app));
            let message = &app.core.scoped.pending_plate_loss_edit.as_ref().unwrap().1;
            assert!(message.contains("0.0025"));
            assert!(message.contains(if wall { "WallPlateId(0)" } else { "SlabId(0)" }));
            fixture::assert_inputs_eq(&app.core.model, &before);
            assert_eq!(app.core.scoped.undo.revision(), 0);
            app.resolve_plate_loss_edit(false);
            fixture::assert_inputs_eq(&app.core.model, &before);
            assert!(!app.core.scoped.undo.can_undo());
            assert!(!edit(&mut app));
            app.resolve_plate_loss_edit(true);
            assert_eq!(app.core.scoped.undo.revision(), 1);
            assert!(app.core.model.slabs.is_empty() && app.core.model.wall_plates.is_empty());
            let after = app.core.model.clone();
            app.core.scoped.undo.undo(&mut app.core.model);
            fixture::assert_inputs_eq(&app.core.model, &before);
            app.core.scoped.undo.redo(&mut app.core.model);
            fixture::assert_inputs_eq(&app.core.model, &after);
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn 節点グリッドの非平面貼付は旧版荷重を診断して未更新としguiにも理由を表示する() {
    use crate::grid::GridAdapter;
    for wall in [false, true] {
        let mut app = App::default();
        app.core.model = fixture::with_plate(wall);
        app.core.model.assign_stb_node_ids().unwrap();
        let before = app.core.model.clone();
        let mut adapter = node_grid::NodeGridAdapter {
            model: &mut app.core.model,
            undo: &mut app.core.scoped.undo,
            edited: false,
        };
        adapter.apply_block(&[(1, if wall { 1 } else { 2 }, "1000".into())], 0);
        assert!(!adapter.edited);
        let reason = app.core.scoped.undo.last_error().unwrap();
        assert!(reason.contains("0.0025"));
        assert!(reason.contains("未更新"));
        assert!(reason.contains("1 Undo"));
        fixture::assert_inputs_eq(&app.core.model, &before);
        assert_eq!(app.core.scoped.undo.revision(), 0);
        assert!(!app.core.scoped.undo.can_undo());
        let _ = egui::Context::default().run_ui(Default::default(), |ui| {
            crate::tables::nodes::nodes_table(ui, &mut app);
        });
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("0.0025"));
        fixture::assert_inputs_eq(&app.core.model, &before);
    }
}

#[test]
fn 一般準備の候補拒否は支持再推定もauto_ex生成記録強度入力も確定しない() {
    use sepika_core::model::SecondaryMemberEnds;
    for wall in [false, true] {
        let mut app = App::default();
        app.core.model = fixture::with_plate_metadata(wall);
        let mut member = fixture::divider(wall);
        member.ends = SecondaryMemberEnds::Detached(if wall {
            [[2000., 0., 0.], [2000., 0., 3000.]]
        } else {
            [[2000., 0., 0.], [2000., 3000., 0.]]
        });
        if wall {
            app.core.model.unassigned_posts.push(member);
        } else {
            app.core.model.unassigned_beams.push(member);
        }
        let before = app.core.model.clone();
        app.ensure_preparation();
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("孤立版"));
        fixture::assert_inputs_eq(&app.core.model, &before);
        assert_eq!(
            app.core.model.seismic_weight_generation,
            before.seismic_weight_generation
        );
        assert_eq!(app.core.model.stb_strengths, before.stb_strengths);
        assert_eq!(app.core.scoped.undo.revision(), 0);
        assert!(!app.core.scoped.undo.can_undo());
    }
}

#[cfg(feature = "gui")]
#[test]
fn gui_redo拒否は理由を表示しモデル履歴と選択を維持し正常再試行できる() {
    use sepika_core::ids::*;
    use sepika_edit::{PlaceSecondaryMember, SecondaryParent};
    for wall in [false, true] {
        for duplicate_plate in [false, true] {
            let mut app = App::default();
            app.core.model = fixture::with_plate_metadata(wall);
            app.core.model.assign_stb_node_ids().unwrap();
            let valid = app.core.model.clone();
            let sm = fixture::divider(wall);
            assert!(!app.apply_model_edit(Box::new(PlaceSecondaryMember {
                parent: if wall {
                    SecondaryParent::Wall(WallRegionId(0))
                } else {
                    SecondaryParent::Floor(FloorRegionId(0))
                },
                kind: sm.kind,
                ends: sm.ends,
                section: None,
                name: "分割".into(),
            })));
            app.resolve_plate_loss_edit(true);
            let divided = app.core.model.clone();
            app.undo_action();
            fixture::assert_inputs_eq(&app.core.model, &valid);
            if wall {
                let mut r = app.core.model.wall_assignment_regions.regions[0].clone();
                r.id = WallPlateAssignmentRegionId(90);
                if duplicate_plate {
                    r.boundary[0].span[1] = 0.5;
                }
                app.core.model.wall_assignment_regions.regions.push(r);
            } else {
                let mut r = app.core.model.floor_assignment_regions.regions[0].clone();
                r.id = FloorPlateAssignmentRegionId(90);
                if duplicate_plate {
                    r.boundary[0].span[1] = 0.5;
                }
                app.core.model.floor_assignment_regions.regions.push(r);
            }
            app.select_node(NodeId(0));
            let before = app.core.model.clone();
            let label = app.core.scoped.undo.redo_label().unwrap().to_owned();
            app.redo_action();
            fixture::assert_inputs_eq(&app.core.model, &before);
            assert_eq!(app.core.scoped.undo.revision(), 2);
            assert!(!app.core.scoped.undo.can_undo());
            assert!(app.core.scoped.undo.can_redo());
            assert_eq!(app.core.scoped.undo.redo_label(), Some(label.as_str()));
            assert_eq!(app.ui.scoped.selection.nodes(), &[NodeId(0)]);
            let reason = app.core.scoped.last_error.as_ref().unwrap();
            assert!(reason.contains("未更新"));
            assert!(reason.contains(if duplicate_plate {
                "重複割当"
            } else {
                "境界キー"
            }));
            assert_eq!(Some(reason.as_str()), app.core.scoped.undo.last_error());
            app.core.model = valid;
            app.redo_action();
            fixture::assert_inputs_eq(&app.core.model, &divided);
            assert_eq!(app.core.scoped.undo.revision(), 3);
            assert!(app.core.scoped.undo.last_error().is_none());
            assert!(app.core.scoped.last_error.is_none());
            app.undo_action();
            assert_eq!(app.core.scoped.undo.revision(), 4);
        }
    }
}

#[cfg(feature = "gui")]
#[test]
fn gui_noop_redoはモデル履歴と選択を更新しない() {
    use sepika_core::ids::NodeId;
    use sepika_edit::SetNodeRestraint;
    let mut app = App::default();
    app.core.model = fixture::rectangle(false);
    assert!(app.core.scoped.undo.run(
        &mut app.core.model,
        Box::new(SetNodeRestraint {
            node: NodeId(0),
            restraint: sepika_core::dof::Dof6Mask::FIXED,
        })
    ));
    app.undo_action();
    app.core.model.nodes.clear();
    app.select_node(NodeId(1));
    let before = app.core.model.clone();
    let label = app.core.scoped.undo.redo_label().unwrap().to_owned();
    app.redo_action();
    fixture::assert_inputs_eq(&app.core.model, &before);
    assert_eq!(app.core.scoped.undo.revision(), 2);
    assert!(!app.core.scoped.undo.can_undo() && app.core.scoped.undo.can_redo());
    assert_eq!(app.core.scoped.undo.redo_label(), Some(label.as_str()));
    assert_eq!(app.ui.scoped.selection.nodes(), &[NodeId(1)]);
    assert!(app.core.scoped.undo.last_error().is_none());
    assert!(app.core.scoped.last_error.is_none());
}
