use sepika_core as core_fixture_api;
use sepika_core::ids::*;
use sepika_edit::*;
#[path = "../../sepika-core/tests/support/assignment_identity_model.rs"]
mod fixture;

fn place(wall: bool) -> PlaceSecondaryMember {
    let sm = fixture::divider(wall);
    PlaceSecondaryMember {
        parent: if wall {
            SecondaryParent::Wall(WallRegionId(0))
        } else {
            SecondaryParent::Floor(FloorRegionId(0))
        },
        kind: sm.kind,
        ends: sm.ends,
        section: None,
        name: "分割材".into(),
    }
}

#[test]
fn 明示分割の診断と旧版除去は一undoで全入力へ戻りredoで同じidになる() {
    for wall in [false, true] {
        let mut model = fixture::with_plate(wall);
        model.assign_stb_node_ids().unwrap();
        let before = model.clone();
        let mut undo = UndoStack::new();
        let loss = preview_plate_assignment_loss(&model, &place(wall)).unwrap();
        assert_eq!(
            (loss.slabs.len(), loss.wall_plates.len()),
            if wall { (0, 1) } else { (1, 0) }
        );
        assert!(loss.description().contains("0.0025"));
        fixture::assert_inputs_eq(&model, &before);
        assert_eq!(undo.revision(), 0);
        assert!(
            undo.run(&mut model, Box::new(place(wall))),
            "{:?}",
            undo.last_error()
        );
        assert_eq!(model.next_secondary_member_id, 1);
        if wall {
            assert!(model.wall_plates.is_empty());
            assert_eq!(
                model
                    .wall_assignment_regions
                    .regions
                    .iter()
                    .map(|r| r.id.0)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );
            assert!(model
                .wall_assignment_regions
                .regions
                .iter()
                .all(|r| r.assignment.is_unset()));
        } else {
            assert!(model.slabs.is_empty());
            assert_eq!(
                model
                    .floor_assignment_regions
                    .regions
                    .iter()
                    .map(|r| r.id.0)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );
            assert!(model
                .floor_assignment_regions
                .regions
                .iter()
                .all(|r| r.assignment.is_unset()));
        }
        let divided = model.clone();
        undo.undo(&mut model);
        fixture::assert_inputs_eq(&model, &before);
        assert!(!undo.can_undo());
        assert!(undo.can_redo());
        undo.redo(&mut model);
        fixture::assert_inputs_eq(&model, &divided);
        assert!(undo.run(
            &mut model,
            Box::new(DeleteSecondaryMember {
                member: SecondaryMemberId(0)
            })
        ));
        if wall {
            assert_eq!(model.wall_assignment_regions.regions[0].id.0, 3);
            assert!(model.wall_assignment_regions.regions[0]
                .assignment
                .is_unset());
        } else {
            assert_eq!(model.floor_assignment_regions.regions[0].id.0, 3);
            assert!(model.floor_assignment_regions.regions[0]
                .assignment
                .is_unset());
        }
    }
}

#[test]
fn 既存キー衝突と重複版は直接編集とundo入口で全状態と履歴を保持する() {
    for wall in [false, true] {
        for duplicate_plate in [false, true] {
            let mut model = fixture::with_plate(wall);
            model.assign_stb_node_ids().unwrap();
            let mut undo = UndoStack::new();
            assert!(undo.run(&mut model, Box::new(place(wall))));
            undo.undo(&mut model);
            if wall {
                let mut duplicate = model.wall_assignment_regions.regions[0].clone();
                duplicate.id = WallPlateAssignmentRegionId(90);
                if duplicate_plate {
                    duplicate.boundary[0].span[1] = 0.5;
                }
                model.wall_assignment_regions.regions.push(duplicate);
            } else {
                let mut duplicate = model.floor_assignment_regions.regions[0].clone();
                duplicate.id = FloorPlateAssignmentRegionId(90);
                if duplicate_plate {
                    duplicate.boundary[0].span[1] = 0.5;
                }
                model.floor_assignment_regions.regions.push(duplicate);
            }
            let before = model.clone();
            let revision = undo.revision();
            let redo_label = undo.redo_label().unwrap().to_owned();
            assert!(!undo.run(&mut model, Box::new(place(wall))));
            assert!(undo.last_error().unwrap().contains(if duplicate_plate {
                "重複割当"
            } else {
                "境界キー"
            }));
            fixture::assert_inputs_eq(&model, &before);
            assert_eq!(undo.revision(), revision);
            assert!(!undo.can_undo());
            assert!(undo.can_redo());
            assert_eq!(undo.redo_label(), Some(redo_label.as_str()));
            let inverse = place(wall).apply(&mut model);
            assert!(inverse.rejection().is_some());
            fixture::assert_inputs_eq(&model, &before);
        }
    }
}

#[test]
fn 版あり二領域の明示統合は二版の入力荷重を診断して一undoで復元する() {
    use sepika_core::model::PlateAssignment;
    for wall in [false, true] {
        let mut model = fixture::with_plate(wall);
        model.assign_stb_node_ids().unwrap();
        let mut undo = UndoStack::new();
        assert!(undo.run(&mut model, Box::new(place(wall))));
        let original = fixture::with_plate(wall);
        if wall {
            let first = original.wall_plates[0].clone();
            let mut second = first.clone();
            second.id = WallPlateId(1);
            second.loads[0].value = 0.0037;
            model.wall_plates = vec![first, second];
            model.wall_regions[0].wall_plate_ids = vec![WallPlateId(0), WallPlateId(1)];
            for (i, r) in model.wall_assignment_regions.regions.iter_mut().enumerate() {
                r.assignment = PlateAssignment::Plate(WallPlateId(i as u32));
            }
        } else {
            let first = original.slabs[0].clone();
            let mut second = first.clone();
            second.id = SlabId(1);
            second.plate.loads[0].value = 0.0037;
            model.slabs = vec![first, second];
            model.floor_regions[0].slab_ids = vec![SlabId(0), SlabId(1)];
            for (i, r) in model
                .floor_assignment_regions
                .regions
                .iter_mut()
                .enumerate()
            {
                r.assignment = PlateAssignment::Plate(SlabId(i as u32));
            }
        }
        let before = model.clone();
        let command = DeleteSecondaryMember {
            member: SecondaryMemberId(0),
        };
        let loss = preview_plate_assignment_loss(&model, &command).unwrap();
        assert_eq!(
            (loss.slabs.len(), loss.wall_plates.len()),
            if wall { (0, 2) } else { (2, 0) }
        );
        assert!(loss.description().contains("0.0025") && loss.description().contains("0.0037"));
        assert!(undo.run(&mut model, Box::new(command)));
        if wall {
            assert!(model.wall_plates.is_empty());
            assert_eq!(model.wall_assignment_regions.regions[0].id.0, 3);
            assert!(model.wall_assignment_regions.regions[0]
                .assignment
                .is_unset());
        } else {
            assert!(model.slabs.is_empty());
            assert_eq!(model.floor_assignment_regions.regions[0].id.0, 3);
            assert!(model.floor_assignment_regions.regions[0]
                .assignment
                .is_unset());
        }
        let merged = model.clone();
        undo.undo(&mut model);
        fixture::assert_inputs_eq(&model, &before);
        undo.redo(&mut model);
        fixture::assert_inputs_eq(&model, &merged);
    }
}

#[test]
fn 明示編集が取り除く既存孤立版も確認対象から落とさない() {
    for wall in [false, true] {
        let mut model = fixture::with_plate(wall);
        if wall {
            model.wall_assignment_regions.regions[0].assignment =
                sepika_core::model::PlateAssignment::Unset;
        } else {
            model.floor_assignment_regions.regions[0].assignment =
                sepika_core::model::PlateAssignment::Unset;
        }
        let before = model.clone();
        let loss = preview_plate_assignment_loss(&model, &place(wall)).unwrap();
        assert_eq!(
            (loss.slabs.len(), loss.wall_plates.len()),
            if wall { (0, 1) } else { (1, 0) }
        );
        assert!(loss.description().contains("0.0025"));
        fixture::assert_inputs_eq(&model, &before);
    }
}
