use sepika_app::sample::portal_frame;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{Haunch, MemberDetailAttr, MemberJoint, Model};
use sepika_design_jp::design_position::design_positions;
use sepika_edit::{AddMember, DeleteMember, UndoStack};
use sepika_io::ovika::{load_ovika, save_ovika};

fn detail(id: u32, joint_mm: f64, haunch_mm: f64) -> MemberDetailAttr {
    MemberDetailAttr {
        elem: ElemId(id),
        haunch_i: Some(Haunch {
            length: haunch_mm,
            depth_increase: 150.0,
            width_increase: 25.0,
            qd1_face: None,
        }),
        haunch_j: None,
        joints: vec![MemberJoint {
            distance: joint_mm,
            kind: Default::default(),
        }],
    }
}

fn ovika_roundtrip(model: &Model) -> Model {
    let path = std::env::temp_dir().join(format!(
        "sepika-member-detail-references-{}.ovika",
        uuid::Uuid::new_v4()
    ));
    save_ovika(&path, model, Default::default()).unwrap();
    let loaded = load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    loaded
}

#[test]
fn deleting_a_member_keeps_details_on_their_original_members_through_undo_and_ovika() {
    let mut original = portal_frame();
    original.member_detail_attrs = vec![
        detail(1, 500.0, 400.0),
        detail(2, 600.0, 450.0),
        detail(1, 800.0, 700.0),
        detail(2, 900.0, 750.0),
        detail(0, 100.0, 200.0),
    ];

    for (deleted_id, survivor, expected, positions) in [
        (
            ElemId(1),
            &original.elements[2],
            vec![
                detail(1, 600.0, 450.0),
                detail(1, 900.0, 750.0),
                detail(0, 100.0, 200.0),
            ],
            [0.0, 0.075, 0.1, 0.5, 1.0],
        ),
        (
            ElemId(2),
            &original.elements[1],
            vec![
                detail(1, 500.0, 400.0),
                detail(1, 800.0, 700.0),
                detail(0, 100.0, 200.0),
            ],
            [0.0, 400.0 / 3500.0, 500.0 / 3500.0, 0.5, 1.0],
        ),
    ] {
        let mut model = ovika_roundtrip(&original);
        assert_eq!(model.elements, original.elements);
        assert_eq!(model.member_detail_attrs, original.member_detail_attrs);
        let mut edits = UndoStack::new();
        assert!(edits.run(&mut model, Box::new(DeleteMember { id: deleted_id })));
        assert_eq!(model.elements[1].nodes, survivor.nodes);
        assert_eq!(model.member_detail_attrs, expected);
        assert_eq!(model.member_detail(ElemId(1)), Some(&expected[0]));

        edits.undo(&mut model);
        assert_eq!(model.elements, original.elements);
        assert_eq!(model.member_detail_attrs, original.member_detail_attrs);
        edits.redo(&mut model);
        assert_eq!(model.elements[1].nodes, survivor.nodes);
        assert_eq!(model.member_detail_attrs, expected);

        let mut loaded = ovika_roundtrip(&model);
        assert_eq!(loaded.elements, model.elements);
        assert_eq!(loaded.member_detail_attrs, expected);
        let survivor = &loaded.elements[1];
        assert_eq!(
            design_positions(survivor, &loaded, loaded.member_length(survivor)),
            positions
        );
        let mut replacement = original.elements[2].clone();
        replacement.nodes = [NodeId(0), NodeId(3)].into_iter().collect();
        assert!(UndoStack::new().run(
            &mut loaded,
            Box::new(AddMember {
                elem: replacement.clone(),
            }),
        ));
        assert_eq!(loaded.elements[2], replacement);
        assert_eq!(loaded.member_detail_attrs, expected);
        assert!(loaded.member_detail(ElemId(2)).is_none());
        assert_eq!(
            design_positions(&replacement, &loaded, loaded.member_length(&replacement)),
            [0.0, 0.5, 1.0]
        );
    }
}
