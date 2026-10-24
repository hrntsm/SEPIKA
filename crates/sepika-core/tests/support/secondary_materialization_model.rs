use sepika_core::{ids::*, model::*};

pub fn model() -> Model {
    let mut model = Model::default();
    model.materials.push(Material {
        id: MaterialId(0),
        name: "鋼材".into(),
        category: MaterialCategory::Steel,
        young: 205000.,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.),
        concrete_class: Default::default(),
        strength_factor: None,
    });
    let mut section = sepika_core::section_shape::SectionShape::SteelH {
        height: 400.,
        width: 200.,
        web_thick: 8.,
        flange_thick: 13.,
        root_r: Some(0.),
    }
    .to_section(SectionId(0), "H400".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    for (i, coord) in [[0., 0., 3000.], [6000., 0., 3000.]]
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
    model.elements.push(ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: vec![NodeId(0), NodeId(1)].into(),
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0., 0., 1.],
        },
        end_cond: [EndCondition::Pinned; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(-300.),
        plastic_zone: None,
        spring: None,
    });
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(1)),
                position: 0.,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(2)),
                position: 0.,
            },
        ]),
        section: Some(SectionId(0)),
        name: "独立小梁794".into(),
        vertical_eccentricity: Some(0.),
        ..Default::default()
    });
    for (i, x) in [0., 6000.].into_iter().enumerate() {
        let mut node = model.nodes[i].clone();
        node.id = NodeId((i + 2) as u32);
        node.coord = [x, 1000., 3000.];
        model.nodes.push(node);
        let mut support = model.elements[0].clone();
        support.id = ElemId((i + 1) as u32);
        support.nodes = vec![NodeId(i as u32), NodeId((i + 2) as u32)].into();
        model.elements.push(support);
    }
    model
}
