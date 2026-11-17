use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node,
};
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
};
pub fn model() -> Model {
    let column = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 25.0,
            x: vec![4],
            y: vec![4],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let beam = SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: RcBeamRebar {
            main_dia: 25.0,
            top: vec![4],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    let sections = [column.clone(), column, beam.clone(), beam]
        .into_iter()
        .enumerate()
        .map(|(i, shape)| {
            let mut section = shape.to_section(SectionId(i as u32), format!("断面{i}"));
            section.frame_use = Some(if i < 2 {
                FrameSectionUse::Column
            } else {
                FrameSectionUse::Girder
            });
            section.material = Some(MaterialId(0));
            section.rebar_material = Some(MaterialId(1));
            section.shear_rebar_material = Some(MaterialId(1));
            section
        })
        .collect();
    Model {
        nodes: [
            [0.0, 0.0, 3000.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 6000.0],
            [-6000.0, 0.0, 3000.0],
            [6000.0, 0.0, 3000.0],
        ]
        .into_iter()
        .enumerate()
        .map(|(i, coord)| Node {
            id: NodeId(i as u32),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        })
        .collect(),
        elements: [(1, 0), (0, 2), (3, 0), (0, 4)]
            .into_iter()
            .enumerate()
            .map(|(i, (a, b))| ElementData {
                id: ElemId(i as u32),
                kind: ElementKind::Beam,
                nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
                section: Some(SectionId(i as u32)),
                local_axis: LocalAxis {
                    ref_vector: if i < 2 {
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
            })
            .collect(),
        sections,
        materials: [
            (MaterialCategory::Concrete, "Fc24", Some(24.0), None),
            (MaterialCategory::Rebar, "SD345", None, Some(345.0)),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (category, name, fc, fy))| Material {
            id: MaterialId(i as u32),
            name: name.into(),
            category,
            young: 23000.0,
            poisson: 0.2,
            density: 0.0,
            shear: None,
            fc,
            fy,
            concrete_class: Default::default(),
            strength_factor: None,
        })
        .collect(),
        ..Default::default()
    }
}
