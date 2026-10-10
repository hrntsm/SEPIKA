use super::*;
use sepika_core as core_fixture_api;
#[path = "../../../sepika-core/tests/support/assignment_identity_model.rs"]
mod fixture;

#[test]
fn 準備計算は孤立版を表示し領域と入力荷重を未更新にする() {
    for wall in [false, true] {
        let mut app = App::default();
        app.core.model = fixture::with_plate(wall);
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
