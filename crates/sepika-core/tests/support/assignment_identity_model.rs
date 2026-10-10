use super::core_fixture_api as sepika_core;
use sepika_core::ids::{ElemId, NodeId, SecondaryMemberId, SlabId, WallPlateId};
use sepika_core::model::*;

pub fn rectangle(wall: bool) -> Model {
    let mut model = Model::default();
    for (i, [x, y]) in [[0., 0.], [4000., 0.], [4000., 3000.], [0., 3000.]]
        .into_iter()
        .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord: if wall { [x, 0., y] } else { [x, y, 0.] },
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, [a, b]) in [[0, 1], [1, 2], [2, 3], [3, 0]].into_iter().enumerate() {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: ElementKind::Beam,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0., 0., 1.],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    if wall {
        model.rebuild_wall_assignment_regions();
        sepika_core::wall_region_rebuild::rebuild_wall_regions(&mut model);
    } else {
        model.rebuild_floor_assignment_regions();
        sepika_core::region_rebuild::rebuild_floor_regions(&mut model);
    }
    model
}

pub fn with_plate(wall: bool) -> Model {
    let mut model = rectangle(wall);
    let loads = vec![AreaLoad {
        kind: "入力仕上げ".into(),
        value: 0.0025,
    }];
    if wall {
        model.wall_plates.push(WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: None,
            opening_area: 0.,
            opening_weight: 1234.,
            openings: vec![],
            loads,
            slit: Default::default(),
            self_weight_shares: vec![],
        });
        model.wall_assignment_regions.regions[0].assignment =
            PlateAssignment::Plate(WallPlateId(0));
        model.wall_regions[0].wall_plate_ids = vec![WallPlateId(0)];
    } else {
        model.slabs.push(Slab {
            id: SlabId(0),
            shape: SlabShape::Enclosed,
            plate: SlabPlate {
                loads,
                ..Default::default()
            },
            tip_loads: vec![],
        });
        model.floor_assignment_regions.regions[0].assignment = PlateAssignment::Plate(SlabId(0));
        model.floor_regions[0].slab_ids = vec![SlabId(0)];
    }
    model
}

pub fn divider(wall: bool) -> SecondaryMember {
    SecondaryMember {
        id: SecondaryMemberId(0),
        kind: if wall {
            SecondaryMemberKind::Post
        } else {
            SecondaryMemberKind::Beam
        },
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(0)),
                position: 0.5,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(2)),
                position: 0.5,
            },
        ]),
        ..Default::default()
    }
}

pub fn assert_inputs_eq(actual: &Model, expected: &Model) {
    assert_eq!(
        actual.floor_assignment_regions,
        expected.floor_assignment_regions
    );
    assert_eq!(
        actual.wall_assignment_regions,
        expected.wall_assignment_regions
    );
    assert_eq!(actual.floor_regions, expected.floor_regions);
    assert_eq!(actual.wall_regions, expected.wall_regions);
    assert_eq!(actual.slabs, expected.slabs);
    assert_eq!(actual.wall_plates, expected.wall_plates);
    assert_eq!(actual.unassigned_beams, expected.unassigned_beams);
    assert_eq!(actual.unassigned_posts, expected.unassigned_posts);
    assert_eq!(
        actual.next_secondary_member_id,
        expected.next_secondary_member_id
    );
    assert_eq!(actual.nodes, expected.nodes);
    assert_eq!(actual.elements, expected.elements);
    assert_eq!(actual.load_cases, expected.load_cases);
    assert_eq!(actual.stb_node_ids, expected.stb_node_ids);
    assert!(actual.eq_ignoring_dofmap(expected));
}
