pub fn scalar_model(kind: sepika_core::model::ElementKind) -> sepika_core::model::Model {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
    use sepika_core::model::*;
    let mut section = Section::zero(SectionId(0), "scalar".into());
    section.frame_use = Some(if matches!(kind, ElementKind::Brace { .. }) {
        FrameSectionUse::Brace
    } else {
        FrameSectionUse::Column
    });
    section.material = Some(MaterialId(0));
    section.area = 1000.0;
    section.iy = 1e7;
    section.iz = 1e7;
    section.j = 1e6;
    section.as_y = 800.0;
    section.as_z = 800.0;
    section.depth = 100.0;
    section.width = 100.0;
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0; 3],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 1000.0],
                restraint: Dof6Mask(0b111011),
                mass: Some([1.0; 6]),
                story: None,
                support_spring: Some([0.0, 0.0, 200000.0, 0.0, 0.0, 0.0]),
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind,
            nodes: vec![NodeId(0), NodeId(1)].into(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "steel".into(),
            category: MaterialCategory::Steel,
            young: 200000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        load_cases: vec![LoadCase {
            id: LoadCaseId(0),
            name: "scalar".into(),
            kind: LoadCaseKind::Dead,
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -1000.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        }],
        ..Default::default()
    }
}

pub fn cycling_brace_model() -> sepika_core::model::Model {
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, NodeId, SectionId};
    use sepika_core::model::{ElementKind, NodalLoad};
    let directions = [
        [
            -0.8507818867882608,
            -0.0586757415469004,
            -0.5222330308080179,
        ],
        [0.8306742914629665, -0.1162111460550285, 0.5444953544660187],
        [-0.270347796907251, -0.7808101202048884, 0.563247392264734],
        [0.1354743014179929, -0.32162822882661984, 0.9371243226366277],
        [-0.6278941660512217, -0.7506651104207652, 0.2055500139523746],
        [
            -0.8619826460712914,
            -0.3928828605377694,
            0.32035757485596383,
        ],
        [
            -0.3533187726366192,
            -0.7294912462668324,
            -0.5856691613211479,
        ],
    ];
    let stiffness = [
        2.948079688477276,
        4143.244716013973,
        870.2769486998493,
        499.43411327618225,
        72.50379158252144,
        6517.338217768038,
        6.85626186663234,
    ];
    let mut model = scalar_model(ElementKind::Brace { tension_only: true });
    model.nodes.truncate(1);
    model.nodes[0].restraint = Dof6Mask(0b111000);
    model.nodes[0].support_spring = Some([
        0.0014663058393709928,
        80.37349162142415,
        1.259091156043921,
        0.0,
        0.0,
        0.0,
    ]);
    let template_elem = model.elements[0].clone();
    let template_section = model.sections[0].clone();
    model.elements.clear();
    model.sections.clear();
    for (i, (direction, k)) in directions.into_iter().zip(stiffness).enumerate() {
        let mut node = model.nodes[0].clone();
        node.id = NodeId(i as u32 + 1);
        node.coord = direction.map(|value| -1000.0 * value);
        node.restraint = Dof6Mask::FIXED;
        node.support_spring = None;
        model.nodes.push(node);
        let mut element = template_elem.clone();
        element.id = ElemId(i as u32);
        element.nodes = vec![NodeId(i as u32 + 1), NodeId(0)].into();
        element.section = Some(SectionId(i as u32));
        model.elements.push(element);
        let mut section = template_section.clone();
        section.id = SectionId(i as u32);
        section.area = k * 1000.0 / 200000.0;
        model.sections.push(section);
    }
    model.stress_cfg.tension_only_iteration = true;
    model.load_cases[0].name = "cycling-braces".into();
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(0),
        [
            -0.07013774776591161,
            0.8372764298097826,
            0.3370558850569952,
            0.0,
            0.0,
            0.0,
        ],
    )];
    model
}
