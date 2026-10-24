use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;
fn material(name: &str) -> Material {
    Material {
        id: MaterialId(0),
        name: name.into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    }
}
fn section() -> Section {
    let mut sec = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 10.0,
        flange_thick: 15.0,
        root_r: Some(0.0),
    }
    .to_section(SectionId(0), "H-reference".into());
    sec.frame_use = Some(FrameSectionUse::Girder);
    sec.material = Some(MaterialId(0));
    sec.area = 10000.0;
    sec.iz = 100_000_000.0;
    sec.iy = 300_000_000.0;
    sec
}
fn node(id: u32, x: f64, z: f64) -> Node {
    Node {
        id: NodeId(id),
        coord: [x, 0.0, z],
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    }
}
fn element(id: u32, i: u32, j: u32, section: u32) -> ElementData {
    ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: vec![NodeId(i), NodeId(j)].into_iter().collect(),
        section: Some(SectionId(section)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    }
}
pub fn frame(split: bool) -> Model {
    let mut column = section();
    column.id = SectionId(1);
    column.frame_use = Some(FrameSectionUse::Column);
    let mut model = Model {
        nodes: vec![
            node(0, 0.0, 0.0),
            node(1, 20000.0, 0.0),
            node(2, 0.0, -3000.0),
            node(3, 20000.0, -3000.0),
        ],
        sections: vec![section(), column],
        materials: vec![material("SS400")],
        elements: vec![
            element(0, 0, 1, 0),
            element(1, 2, 0, 1),
            element(2, 3, 1, 1),
        ],
        ..Default::default()
    };
    if split {
        model.nodes.push(node(4, 5000.0, 0.0));
        model.nodes.push(node(5, 15000.0, 0.0));
        model.elements[0].nodes[1] = NodeId(4);
        model.elements.push(element(3, 4, 5, 0));
        model.elements.push(element(4, 5, 1, 0));
        model.girder_groups = vec![vec![ElemId(0), ElemId(3), ElemId(4)]];
    }
    for id in [1, 2] {
        model.elements[id].local_axis.ref_vector = [1.0, 0.0, 0.0];
    }
    for id in [2, 3] {
        model.nodes[id].restraint = Dof6Mask::FIXED;
    }
    for sec in &mut model.sections {
        sec.property_basis = Default::default();
    }
    model.panel_zone = PanelZoneMode::None;
    model.load_cases.push(LoadCase {
        id: LoadCaseId(0),
        name: "reference-load".into(),
        kind: LoadCaseKind::Dead,
        nodal: vec![NodalLoad::manual(
            NodeId(1),
            [0.0, 0.0, -1000.0, 0.0, 0.0, 0.0],
        )],
        member: vec![],
    });
    model
}
