use sepika_core::{dof::Dof6Mask, ids::*, model::*, section_shape::SectionShape};

pub fn steel_post_model() -> Model {
    let mut m = Model::with_default_load_cases();
    m.materials.push(Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    });
    let mut sec = SectionShape::SteelBox {
        height: 200.0,
        width: 100.0,
        thick: 8.0,
        horizontal_thick: 8.0,
        corner_r: Some(0.0),
    }
    .to_section(SectionId(0), "P1".into());
    sec.material = Some(MaterialId(0));
    sec.frame_use = Some(FrameSectionUse::Column);
    m.sections.push(sec.clone());
    sec.id = SectionId(1);
    sec.frame_use = Some(FrameSectionUse::Girder);
    m.sections.push(sec.clone());
    sec.id = SectionId(2);
    sec.frame_use = Some(FrameSectionUse::Column);
    m.sections.push(sec);
    for (i, coord) in [
        [-1000.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [-1000.0, 0.0, 3000.0],
        [1000.0, 0.0, 3000.0],
    ]
    .into_iter()
    .enumerate()
    {
        m.nodes.push(Node {
            id: NodeId(i as u32),
            coord,
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, pair) in [[0, 1], [2, 3], [0, 2], [1, 3]].into_iter().enumerate() {
        m.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: ElementKind::Beam,
            nodes: pair.into_iter().map(NodeId).collect(),
            section: Some(SectionId(if i < 2 { 1 } else { 2 })),
            local_axis: LocalAxis {
                ref_vector: if i < 2 {
                    [0.0, 0.0, 1.0]
                } else {
                    [1.0, 0.0, 0.0]
                },
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
            vertical_eccentricity: Some(0.0),
        });
    }
    let post = SecondaryMember {
        id: SecondaryMemberId(0),
        kind: SecondaryMemberKind::Post,
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(0)),
                position: 0.5,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(1)),
                position: 0.5,
            },
        ]),
        section: Some(SectionId(0)),
        name: "間柱P1".into(),
        gravity_end_shares: Some([1.0, 0.0]),
        ..Default::default()
    };
    let mut wall = WallRegion::new(
        WallRegionId(0),
        vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)],
    );
    wall.posts.push(post);
    m.wall_regions.push(wall);
    m.next_secondary_member_id = 1;
    m.post_design_inputs.push(PostDesignInput {
        member: SecondaryMemberId(0),
        strong_direction_deg: Some(0.0),
        buckling_lengths_mm: Some([3000.0, 3000.0]),
        lateral_buckling_length_mm: Some(3000.0),
        deflection_limit_denom: Some(250.0),
        lateral_loads: vec![PostLateralLoad {
            case: LoadCaseId(0),
            direction: [1.0, 0.0],
            load: MemberLoadKind::Distributed {
                a: 0.0,
                b: 3000.0,
                w1: 2.0,
                w2: 2.0,
            },
        }],
        combinations: vec![(0, false)],
        ..Default::default()
    });
    m
}
