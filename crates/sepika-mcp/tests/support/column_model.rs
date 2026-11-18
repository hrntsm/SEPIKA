use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, FrameSectionUse, LoadCase, LoadCaseKind, LocalAxis, Material,
    MaterialCategory, Model, NodalLoad, Node, Section, Story,
};

pub(crate) fn column_stories() -> Vec<Story> {
    serde_json::from_value(serde_json::json!([
        {"id": 0, "name": "1F", "elevation": 0.0, "sl_offset_mm": 0.0, "node_ids": [0], "seismic_weight": null},
        {"id": 1, "name": "2F", "elevation": 3000.0, "sl_offset_mm": 0.0, "node_ids": [1], "seismic_weight": null}
    ]))
    .unwrap()
}

/// RC 矩形の片持ち柱モデル。長期荷重ケース 1 つ。
pub(crate) fn rc_column_model() -> Model {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};

    let rebar = RcRectColumnRebar {
        main_dia: 25.0,
        x: vec![8],
        y: vec![8],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: 10.0,
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    let shape = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar,
    };
    Model {
        stories: column_stories(),
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: sepika_core::dof::Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 3000.0],
                restraint: sepika_core::dof::Dof6Mask::FREE,
                mass: None,
                story: Some(sepika_core::ids::StoryId(1)),
                support_spring: None,
            },
        ],
        sections: vec![Section {
            material: Some(MaterialId(0)),
            rebar_material: Some(MaterialId(1)),
            shear_rebar_material: Some(MaterialId(1)),
            frame_use: Some(FrameSectionUse::Column),
            ..shape.to_section(SectionId(0), "C600".into())
        }],
        materials: vec![
            Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 23000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
            },
            Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(1),
                name: "SD345".into(),
                category: MaterialCategory::Rebar,
                young: 205000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(345.0),
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [
                sepika_core::model::EndCondition::Fixed,
                sepika_core::model::EndCondition::Fixed,
            ],
            force_regime: sepika_core::model::ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        }],
        load_cases: vec![LoadCase {
            origin: Default::default(),
            kind: LoadCaseKind::Dead,
            id: LoadCaseId(0),
            name: "長期".into(),
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -500_000.0, 0.0, 0.0, 0.0],
            )],
            member: Vec::new(),
        }],
        ..Default::default()
    }
}
