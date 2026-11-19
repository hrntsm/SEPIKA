use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;

pub fn cantilever(axial_n: f64, strong_moment_nmm: f64, weak_shear_n: f64) -> Model {
    let mut section = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
        root_r: Some(0.0),
    }
    .to_section(SectionId(0), "H-400x200x8x13".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Column);

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
                coord: [0.0, 0.0, 3000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        materials: vec![Material {
            id: MaterialId(0),
            name: "直接鋼材".into(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        sections: vec![section],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        load_cases: vec![LoadCase {
            origin: Default::default(),
            id: LoadCaseId(0),
            name: "柱頭荷重".into(),
            kind: LoadCaseKind::Dead,
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, weak_shear_n, axial_n, 0.0, strong_moment_nmm, 0.0],
            )],
            member: vec![],
        }],
        panel_zone: PanelZoneMode::None,
        ..Model::default()
    }
}
