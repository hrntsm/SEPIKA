use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LoadCase, LoadCaseKind,
    LoadCombination, LocalAxis, Material, MaterialCategory, Model, NodalLoad, Node,
    SteelDesignAttr, Story,
};
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RebarSize, RectColumnHoop, SectionShape,
};

pub fn model(column: bool, steel_grade: &str, tension_n: f64, moment_nmm: f64) -> Model {
    let main_dia = RebarSize::DiameterMm(25.0);
    let shear_dia = RebarSize::DiameterMm(10.0);
    let (shape, name, frame_use, tip) = if column {
        (
            SectionShape::SrcColumnRect {
                b: 600.0,
                d: 600.0,
                rebar: RcRectColumnRebar {
                    main_dia,
                    x: vec![4],
                    y: vec![2],
                    cover: 40.0,
                    hoop: RectColumnHoop {
                        dia: shear_dia,
                        pitch: 100.0,
                        legs_x: 2,
                        legs_y: 2,
                    },
                },
                steel_height: 400.0,
                steel_width: 200.0,
                steel_web_thick: 8.0,
                steel_flange_thick: 13.0,
                steel_manufacture: Default::default(),
            },
            "C1",
            FrameSectionUse::Column,
            [0.0, 0.0, 6_000.0],
        )
    } else {
        (
            SectionShape::SrcBeamRect {
                b: 600.0,
                d: 600.0,
                rebar: RcBeamRebar {
                    main_dia,
                    top: vec![4],
                    bottom: vec![4],
                    cover: 40.0,
                    stirrup: BeamStirrup {
                        dia: shear_dia,
                        pitch: 100.0,
                        legs: 2,
                    },
                },
                steel_height: 400.0,
                steel_width: 200.0,
                steel_web_thick: 8.0,
                steel_flange_thick: 13.0,
                steel_manufacture: Default::default(),
            },
            "G1",
            FrameSectionUse::Girder,
            [6_000.0, 0.0, 0.0],
        )
    };
    let mut section = shape.to_section(SectionId(0), name.into());
    section.frame_use = Some(frame_use);
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section.steel_material = Some(MaterialId(2));
    let materials = [
        (
            "Fc24",
            MaterialCategory::Concrete,
            23_000.0,
            Some(24.0),
            None,
        ),
        (
            "SD345",
            MaterialCategory::Rebar,
            205_000.0,
            None,
            Some(345.0),
        ),
        (
            steel_grade,
            MaterialCategory::Steel,
            205_000.0,
            None,
            Some(100.0),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, category, young, fc, fy))| Material {
        id: MaterialId(index as u32),
        name: name.into(),
        category,
        young,
        poisson: 0.2,
        density: match category {
            MaterialCategory::Concrete => 2.55e-9,
            _ => 7.85e-9,
        },
        shear: None,
        concrete_class: Default::default(),
        fc,
        fy,
        strength_factor: None,
    })
    .collect();
    Model {
        sections: vec![section],
        materials,
        nodes: [
            (NodeId(0), [0.0; 3], Dof6Mask::FIXED),
            (NodeId(1), tip, Dof6Mask::FREE),
        ]
        .into_iter()
        .map(|(id, coord, restraint)| Node {
            id,
            coord,
            restraint,
            mass: None,
            story: None,
            support_spring: None,
        })
        .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: if column {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                },
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
                id: LoadCaseId(0),
                name: "固定荷重".into(),
                kind: LoadCaseKind::Dead,
                origin: Default::default(),
                member: Vec::new(),
                nodal: vec![NodalLoad::manual(
                    NodeId(1),
                    [0.0, 0.0, tension_n, 0.0, moment_nmm, 0.0],
                )],
            },
            LoadCase {
                id: LoadCaseId(1),
                name: "架構用積載".into(),
                kind: LoadCaseKind::Live,
                origin: Default::default(),
                member: Vec::new(),
                nodal: Vec::new(),
            },
        ],
        combinations: vec![LoadCombination {
            name: "長期".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
        }],
        stories: vec![
            Story::new(StoryId(0), "1F".into(), Some(0.0), Some(0.0)),
            Story::new(StoryId(1), "2F".into(), Some(6_000.0), Some(0.0)),
        ],
        steel_design_attrs: column
            .then_some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: None,
                lk_y_direct: Some(6000.0),
                lk_z_direct: Some(6000.0),
                c_direct: None,
                column_lateral_buckling_end_supports: None,
            })
            .into_iter()
            .collect(),
        ..Default::default()
    }
}
