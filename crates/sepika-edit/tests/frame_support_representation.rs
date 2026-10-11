#[path = "../../sepika-core/tests/support/frame_support_model.rs"]
mod fixture;
use sepika_core::ids::NodeId;
use sepika_edit::{SetNodeCoord, UndoStack};

#[test]
fn 明示解析表現の床壁は節点編集再構築undo_redoで版と支持区間を保持する() {
    for wall in [false, true] {
        for kinds in fixture::representations() {
            let mut model = fixture::with_plate(wall, kinds);
            model.assign_stb_node_ids().unwrap();
            let before = model.clone();
            let mut undo = UndoStack::new();
            assert!(
                undo.run_preserving_plate_assignments(
                    &mut model,
                    Box::new(SetNodeCoord {
                        node: NodeId(if wall { 2 } else { 1 }),
                        coord: if wall {
                            [4000., 0., 3100.]
                        } else {
                            [4100., 0., 0.]
                        },
                    })
                ),
                "{:?}",
                undo.last_error()
            );
            assert_eq!(
                model.floor_assignment_regions,
                before.floor_assignment_regions
            );
            assert_eq!(
                model.wall_assignment_regions,
                before.wall_assignment_regions
            );
            assert_eq!(model.slabs, before.slabs);
            assert_eq!(model.wall_plates, before.wall_plates);
            model.rebuild_assignment_regions().unwrap();
            let edited = model.clone();
            undo.undo(&mut model);
            assert!(model.eq_ignoring_dofmap(&before));
            fixture::assert_geometry(&model, wall);
            undo.redo(&mut model);
            assert!(model.eq_ignoring_dofmap(&edited));
        }
    }
}
