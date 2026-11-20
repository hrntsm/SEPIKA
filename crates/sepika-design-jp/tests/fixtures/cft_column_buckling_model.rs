use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;

pub fn direct_lengths(elem: ElemId, strong: Option<f64>, weak: Option<f64>) -> SteelDesignAttr {
    SteelDesignAttr {
        elem,
        joint_flange_loss: 0.0,
        joint_web_loss: 0.0,
        scallop_web_loss: 0.0,
        lb_direct: None,
        lateral_brace_count: None,
        column_lateral_buckling_end_supports_confirmed: false,
        lk_y_direct: strong,
        lk_z_direct: weak,
        c_direct: None,
    }
}

pub fn cantilever(circular: bool, axial_compression_n: f64) -> Model {
    let shape = if circular {
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 12.0,
        }
    } else {
        SectionShape::CftBox {
            height: 400.0,
            width: 400.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: Some(0.0),
        }
    };
    Model {
        nodes: [(0, 0.0, Dof6Mask::FIXED), (1, 3000.0, Dof6Mask::FREE)]
            .into_iter()
            .map(|(id, z, restraint)| Node {
                id: NodeId(id),
                coord: [0.0, 0.0, z],
                restraint,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        sections: vec![Section {
            frame_use: Some(FrameSectionUse::Column),
            material: Some(MaterialId(0)),
            steel_material: Some(MaterialId(1)),
            ..shape.to_section(SectionId(0), "CFT400".into())
        }],
        materials: vec![
            Material {
                id: MaterialId(0),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 25000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
                strength_factor: None,
                concrete_class: Default::default(),
            },
            Material {
                id: MaterialId(1),
                name: "SN400B".into(),
                category: MaterialCategory::Steel,
                young: 205000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(235.0),
                strength_factor: None,
                concrete_class: Default::default(),
            },
        ],
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
        load_cases: vec![
            LoadCase {
                origin: Default::default(),
                id: LoadCaseId(0),
                name: "柱頭鉛直荷重".into(),
                kind: LoadCaseKind::Dead,
                nodal: vec![NodalLoad::manual(
                    NodeId(1),
                    [0.0, 0.0, -axial_compression_n, 0.0, 0.0, 0.0],
                )],
                member: vec![],
            },
            LoadCase {
                origin: Default::default(),
                id: LoadCaseId(1),
                name: "P=0".into(),
                kind: LoadCaseKind::Live,
                nodal: vec![],
                member: vec![],
            },
        ],
        combinations: vec![LoadCombination {
            name: "長期".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
        }],
        ultimate_gravity_combination: Some(0),
        ..Default::default()
    }
}
