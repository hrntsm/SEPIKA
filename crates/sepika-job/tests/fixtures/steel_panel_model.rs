use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node, PanelZoneMode, Story,
};
use sepika_core::section_shape::SectionShape;

pub(crate) fn steel_panel_model() -> Model {
    let mut model = Model {
        panel_zone: PanelZoneMode::Model,
        stories: [0.0, 3000.0, 6000.0]
            .into_iter()
            .enumerate()
            .map(|(i, fl)| {
                Story::new(
                    StoryId(i as u32),
                    format!("{}F", i + 1),
                    Some(fl),
                    Some(0.0),
                )
            })
            .collect(),
        ..Default::default()
    };
    for (i, coord) in [
        [0.0, 0.0, 3000.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 6000.0],
        [6000.0, 0.0, 3000.0],
    ]
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
    model.materials.push(Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    });
    let shape = SectionShape::SteelH {
        height: 400.0,
        width: 400.0,
        web_thick: 13.0,
        flange_thick: 21.0,
        root_r: Some(0.0),
    };
    for (i, usage) in [
        FrameSectionUse::Column,
        FrameSectionUse::Column,
        FrameSectionUse::Girder,
    ]
    .into_iter()
    .enumerate()
    {
        let mut section = shape.to_section(SectionId(i as u32), format!("S{i}"));
        section.frame_use = Some(usage);
        section.material = Some(MaterialId(0));
        model.sections.push(section);
    }
    for (i, nodes) in [[1, 0], [0, 2], [0, 3]].into_iter().enumerate() {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: ElementKind::Beam,
            nodes: nodes.map(NodeId).into_iter().collect(),
            section: Some(SectionId(i as u32)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        });
    }
    model
}
