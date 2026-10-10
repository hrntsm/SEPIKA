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

fn frame_edit(wall: bool, operation: usize) -> Box<dyn EditCommand> {
    match operation {
        0 => Box::new(DeleteMember { id: ElemId(0) }),
        1 => Box::new(SetNodeCoord {
            node: NodeId(1),
            coord: if wall {
                [4000., 1000., 0.]
            } else {
                [4000., 0., 1000.]
            },
        }),
        _ => {
            let mut elem = fixture::rectangle(wall).elements[0].clone();
            elem.id = ElemId(4);
            elem.nodes = [NodeId(0), NodeId(2)].into_iter().collect();
            Box::new(AddMember { elem })
        }
    }
}

#[test]
fn 主架構追加削除と非平面節点移動は旧版荷重を診断し一undoとredoで全入力を復元する() {
    for wall in [false, true] {
        for operation in 0..3 {
            let mut model = fixture::with_plate(wall);
            model.assign_stb_node_ids().unwrap();
            let before = model.clone();
            let loss = preview_plate_assignment_loss(&model, frame_edit(wall, operation).as_ref())
                .unwrap();
            assert_eq!(
                (loss.slabs.len(), loss.wall_plates.len()),
                if wall { (0, 1) } else { (1, 0) },
                "wall={wall}, operation={operation}"
            );
            assert!(loss.description().contains("0.0025"));
            if wall {
                assert_eq!(loss.wall_plates, before.wall_plates);
            } else {
                assert_eq!(loss.slabs, before.slabs);
            }
            fixture::assert_inputs_eq(&model, &before);
            let mut undo = UndoStack::new();
            assert!(
                undo.run(&mut model, frame_edit(wall, operation)),
                "{:?}",
                undo.last_error()
            );
            assert!(model.slabs.is_empty() && model.wall_plates.is_empty());
            let after = model.clone();
            undo.undo(&mut model);
            fixture::assert_inputs_eq(&model, &before);
            undo.redo(&mut model);
            fixture::assert_inputs_eq(&model, &after);
        }
    }
}

#[test]
fn 主架構節点の直接編集も既存境界衝突を原子的に拒否する() {
    for operation in 0..3 {
        let mut model = fixture::with_plate(false);
        let mut duplicate = model.floor_assignment_regions.regions[0].clone();
        duplicate.id = FloorPlateAssignmentRegionId(90);
        model.floor_assignment_regions.regions.push(duplicate);
        let before = model.clone();
        let inverse = frame_edit(false, operation).apply(&mut model);
        assert!(inverse.rejection().unwrap().contains("境界キー"));
        fixture::assert_inputs_eq(&model, &before);
    }
}

#[test]
fn 確認できない入口は支持境界変更の版荷重消失を拒否しredo履歴も保持する() {
    for wall in [false, true] {
        for operation in 0..3 {
            let mut model = fixture::with_plate(wall);
            model.assign_stb_node_ids().unwrap();
            let before = model.clone();
            let mut undo = UndoStack::new();
            assert!(undo.run(
                &mut model,
                Box::new(SetNodeRestraint {
                    node: NodeId(0),
                    restraint: sepika_core::dof::Dof6Mask::FIXED,
                })
            ));
            undo.undo(&mut model);
            assert_eq!(undo.revision(), 2);
            assert!(!undo.run_preserving_plate_assignments(&mut model, frame_edit(wall, operation)));
            let reason = undo.last_error().unwrap();
            assert!(
                reason.contains("0.0025") && reason.contains("未更新") && reason.contains("1 Undo")
            );
            assert_eq!(undo.revision(), 2);
            assert!(!undo.can_undo());
            assert!(undo.can_redo());
            fixture::assert_inputs_eq(&model, &before);
            undo.redo(&mut model);
            assert_eq!(model.nodes[0].restraint, sepika_core::dof::Dof6Mask::FIXED);
        }
    }
}

#[test]
fn 同じ支持区間の構面内節点移動は確認なしで版idと入力荷重を保持する() {
    for wall in [false, true] {
        let mut model = fixture::with_plate(wall);
        model.assign_stb_node_ids().unwrap();
        let before = model.clone();
        let plates = (model.slabs.clone(), model.wall_plates.clone());
        let command = || {
            Box::new(CompositeCommand {
                label: "支持境界の幅を変更".into(),
                children: vec![
                    Box::new(SetNodeCoord {
                        node: NodeId(1),
                        coord: [5000., 0., 0.],
                    }),
                    Box::new(SetNodeCoord {
                        node: NodeId(2),
                        coord: if wall {
                            [5000., 0., 3000.]
                        } else {
                            [5000., 3000., 0.]
                        },
                    }),
                ],
            })
        };
        assert!(preview_plate_assignment_loss(&model, command().as_ref())
            .unwrap()
            .is_empty());
        let mut undo = UndoStack::new();
        assert!(undo.run_preserving_plate_assignments(&mut model, command()));
        assert_eq!(model.nodes[1].coord, [5000., 0., 0.]);
        assert_eq!((model.slabs.clone(), model.wall_plates.clone()), plates);
        assert_eq!(
            if wall {
                model.wall_assignment_regions.regions[0].id.0
            } else {
                model.floor_assignment_regions.regions[0].id.0
            },
            0
        );
        let after = model.clone();
        undo.undo(&mut model);
        fixture::assert_inputs_eq(&model, &before);
        undo.redo(&mut model);
        fixture::assert_inputs_eq(&model, &after);
    }
}

#[test]
fn 拒否redoは床壁の衝突と重複版でモデル両履歴ラベルrevisionを保持し修正後に再試行できる() {
    for wall in [false, true] {
        for duplicate_plate in [false, true] {
            let mut model = fixture::with_plate_metadata(wall);
            model.assign_stb_node_ids().unwrap();
            let mut undo = UndoStack::new();
            assert!(undo.run(
                &mut model,
                Box::new(SetNodeRestraint {
                    node: NodeId(0),
                    restraint: sepika_core::dof::Dof6Mask::FIXED,
                })
            ));
            let valid = model.clone();
            assert!(undo.run(&mut model, Box::new(place(wall))));
            let divided = model.clone();
            undo.undo(&mut model);
            fixture::assert_inputs_eq(&model, &valid);
            if wall {
                let mut r = model.wall_assignment_regions.regions[0].clone();
                r.id = WallPlateAssignmentRegionId(90);
                if duplicate_plate {
                    r.boundary[0].span[1] = 0.5;
                }
                model.wall_assignment_regions.regions.push(r);
            } else {
                let mut r = model.floor_assignment_regions.regions[0].clone();
                r.id = FloorPlateAssignmentRegionId(90);
                if duplicate_plate {
                    r.boundary[0].span[1] = 0.5;
                }
                model.floor_assignment_regions.regions.push(r);
            }
            let before = model.clone();
            let labels = (
                undo.undo_label().unwrap().to_owned(),
                undo.redo_label().unwrap().to_owned(),
            );
            assert_eq!(undo.revision(), 3);
            for _ in 0..2 {
                undo.redo(&mut model);
                fixture::assert_inputs_eq(&model, &before);
                assert_eq!(undo.revision(), 3);
                assert!(undo.can_undo() && undo.can_redo());
                assert_eq!(undo.undo_label(), Some(labels.0.as_str()));
                assert_eq!(undo.redo_label(), Some(labels.1.as_str()));
                assert!(undo.id_changes().is_empty());
                let reason = undo.last_error().unwrap();
                assert!(reason.contains("未更新"));
                assert!(reason.contains(if duplicate_plate {
                    "重複割当"
                } else {
                    "境界キー"
                }));
            }
            model = valid.clone();
            undo.redo(&mut model);
            fixture::assert_inputs_eq(&model, &divided);
            assert_eq!(undo.revision(), 4);
            assert!(undo.last_error().is_none());
            assert!(!undo.can_redo());
            undo.undo(&mut model);
            fixture::assert_inputs_eq(&model, &valid);
            assert_eq!(undo.revision(), 5);
        }
    }
}

#[test]
fn noop_redoは候補変更を確定せず履歴とrevisionを保持する() {
    struct CandidateNoop;
    impl EditCommand for CandidateNoop {
        fn apply(&self, model: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
            model.next_secondary_member_id = 999;
            Box::new(Noop)
        }
        fn label(&self) -> &str {
            "候補Noop"
        }
    }
    struct MakeRedo;
    impl EditCommand for MakeRedo {
        fn apply(&self, _: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
            Box::new(CandidateNoop)
        }
        fn label(&self) -> &str {
            "戻す"
        }
    }
    struct Initial;
    impl EditCommand for Initial {
        fn apply(&self, _: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
            Box::new(MakeRedo)
        }
        fn label(&self) -> &str {
            "開始"
        }
    }
    let mut model = fixture::with_plate_metadata(false);
    model.assign_stb_node_ids().unwrap();
    let mut undo = UndoStack::new();
    assert!(undo.run(&mut model, Box::new(Initial)));
    undo.undo(&mut model);
    let before = model.clone();
    for _ in 0..2 {
        undo.redo(&mut model);
        fixture::assert_inputs_eq(&model, &before);
        assert_eq!(undo.revision(), 2);
        assert!(!undo.can_undo());
        assert!(undo.can_redo());
        assert_eq!(undo.redo_label(), Some("候補Noop"));
        assert!(undo.last_error().is_none());
        assert!(undo.id_changes().is_empty());
    }
}

#[test]
fn コマンド拒否redoはstb復元ラッパー越しに理由を伝え候補と履歴を確定しない() {
    struct Reject;
    impl EditCommand for Reject {
        fn apply(&self, _: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
            Box::new(Noop)
        }
        fn label(&self) -> &str {
            "拒否"
        }
        fn rejection(&self) -> Option<&str> {
            Some("候補編集を拒否")
        }
        fn is_noop(&self) -> bool {
            true
        }
    }
    struct Candidate;
    impl EditCommand for Candidate {
        fn apply(&self, model: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
            model.next_secondary_member_id = 999;
            Box::new(Reject)
        }
        fn label(&self) -> &str {
            "再実行"
        }
    }
    struct Inverse;
    impl EditCommand for Inverse {
        fn apply(&self, _: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
            Box::new(Candidate)
        }
        fn label(&self) -> &str {
            "戻す"
        }
    }
    struct Initial;
    impl EditCommand for Initial {
        fn apply(&self, _: &mut sepika_core::model::Model) -> Box<dyn EditCommand> {
            Box::new(Inverse)
        }
        fn label(&self) -> &str {
            "開始"
        }
    }
    let mut model = fixture::with_plate_metadata(false);
    model.assign_stb_node_ids().unwrap();
    let mut undo = UndoStack::new();
    assert!(undo.run(&mut model, Box::new(Initial)));
    undo.undo(&mut model);
    let before = model.clone();
    undo.redo(&mut model);
    fixture::assert_inputs_eq(&model, &before);
    assert_eq!(undo.revision(), 2);
    assert!(!undo.can_undo() && undo.can_redo());
    assert_eq!(undo.redo_label(), Some("再実行"));
    assert!(undo.last_error().unwrap().contains("候補編集を拒否"));
    assert!(undo.id_changes().is_empty());
}
