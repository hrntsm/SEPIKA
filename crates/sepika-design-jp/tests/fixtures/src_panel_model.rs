use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::*;
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RebarSize, RectColumnHoop, SectionShape,
    StandardRebar,
};

pub const PANEL_X: &str = "柱梁接合部(SRC) 検討面 [1.000, 0.000, 0.000]";
pub const PANEL_Y: &str = "柱梁接合部(SRC) 検討面 [0.000, 1.000, 0.000]";
pub const JOINT: NodeId = NodeId(1);

pub fn input_model() -> Model {
    let column = SectionShape::SrcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: RebarSize::DiameterMm(22.0),
            x: vec![2],
            y: vec![2],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: RebarSize::DiameterMm(10.0),
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
    };
    let beam = SectionShape::RcBeamRect {
        b: 300.0,
        d: 500.0,
        rebar: RcBeamRebar {
            main_dia: RebarSize::DiameterMm(22.0),
            top: vec![4],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    let material = |id, name: &str, category, young, fc, fy| Material {
        id: MaterialId(id),
        name: name.into(),
        category,
        young,
        poisson: if category == MaterialCategory::Concrete {
            0.2
        } else {
            0.3
        },
        density: if category == MaterialCategory::Concrete {
            2.4e-9
        } else {
            7.85e-9
        },
        shear: None,
        fc,
        fy,
        strength_factor: None,
        concrete_class: Default::default(),
    };
    Model {
        nodes: [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 4000.0],
            [6000.0, 0.0, 4000.0],
            [6000.0, 0.0, 0.0],
            [0.0, 6000.0, 4000.0],
        ]
        .into_iter()
        .enumerate()
        .map(|(id, coord)| Node {
            id: NodeId(id as u32),
            coord,
            restraint: if id == 2 {
                Dof6Mask::FREE
            } else {
                Dof6Mask::FIXED
            },
            mass: None,
            story: Some(StoryId(u32::from(coord[2] > 0.0))),
            support_spring: None,
        })
        .collect(),
        sections: vec![
            Section {
                frame_use: Some(FrameSectionUse::Column),
                material: Some(MaterialId(0)),
                steel_material: Some(MaterialId(1)),
                rebar_material: Some(MaterialId(2)),
                shear_rebar_material: Some(MaterialId(2)),
                ..column.to_section(SectionId(0), "C".into())
            },
            Section {
                frame_use: Some(FrameSectionUse::Girder),
                material: Some(MaterialId(0)),
                rebar_material: Some(MaterialId(2)),
                shear_rebar_material: Some(MaterialId(2)),
                ..beam.to_section(SectionId(1), "G".into())
            },
        ],
        materials: vec![
            material(
                0,
                "Fc24",
                MaterialCategory::Concrete,
                25_000.0,
                Some(24.0),
                None,
            ),
            material(
                1,
                "SN400B",
                MaterialCategory::Steel,
                205_000.0,
                None,
                Some(235.0),
            ),
            material(
                2,
                "SD295A",
                MaterialCategory::Rebar,
                205_000.0,
                None,
                Some(295.0),
            ),
        ],
        elements: [(0, 0, 0, 1), (1, 1, 1, 2), (2, 1, 0, 3), (3, 1, 1, 4)]
            .into_iter()
            .map(|(id, section, i, j)| ElementData {
                id: ElemId(id),
                kind: ElementKind::Beam,
                nodes: [NodeId(i), NodeId(j)].into_iter().collect(),
                section: Some(SectionId(section)),
                local_axis: LocalAxis {
                    ref_vector: if section == 0 {
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
        stories: [0.0, 4000.0]
            .into_iter()
            .enumerate()
            .map(|(i, elevation)| {
                let mut story = Story::new(
                    StoryId(i as u32),
                    format!("{}F", i + 1),
                    Some(elevation),
                    Some(0.0),
                );
                story.weight_override = Some(1_000_000.0);
                story
            })
            .collect(),
        load_cases: vec![LoadCase {
            origin: Default::default(),
            id: LoadCaseId(0),
            name: "先端荷重".into(),
            kind: LoadCaseKind::Live,
            nodal: vec![NodalLoad::manual(
                NodeId(2),
                [0.0, 0.0, -50_000.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        }],
        ..Default::default()
    }
}

pub fn with_unavailable_column_face(mut model: Model) -> Model {
    model.load_cases[0].member.push(MemberLoad::manual(
        ElemId(1),
        [0.0, 0.0, -1.0],
        MemberLoadKind::Point {
            a: 300.0,
            p: 1000.0,
        },
    ));
    model
}

#[derive(Clone, Copy)]
pub enum ShortBeam {
    Src,
    Steel,
    Rc,
}

pub fn short_model(beam: ShortBeam) -> Model {
    let mut model = input_model();
    let beam_section = &mut model.sections[1];
    let SectionShape::RcBeamRect { b, d, rebar } = beam_section.shape.clone().unwrap() else {
        panic!("矩形RC梁が必要");
    };
    match beam {
        ShortBeam::Src => {
            let shape = SectionShape::SrcBeamRect {
                b,
                d,
                rebar,
                steel_height: 300.0,
                steel_width: 150.0,
                steel_web_thick: 8.0,
                steel_flange_thick: 13.0,
                steel_manufacture: Default::default(),
            };
            *beam_section = Section {
                frame_use: beam_section.frame_use,
                material: beam_section.material,
                rebar_material: beam_section.rebar_material,
                shear_rebar_material: beam_section.shear_rebar_material,
                steel_material: Some(MaterialId(1)),
                ..shape.to_section(SectionId(1), "SRC梁".into())
            };
        }
        ShortBeam::Steel => {
            let shape = SectionShape::SteelH {
                height: d,
                width: 200.0,
                web_thick: 10.0,
                flange_thick: 13.0,
                root_r: Some(0.0),
            };
            *beam_section = Section {
                frame_use: beam_section.frame_use,
                material: Some(MaterialId(1)),
                ..shape.to_section(SectionId(1), "S梁".into())
            };
        }
        ShortBeam::Rc => {}
    }
    model.src_joint_hoops = vec![SrcJointHoopAssignment {
        node: JOINT,
        axis: [1.0, 0.0, 0.0],
        columns: vec![ElemId(0)],
        beams: vec![ElemId(1)],
        hoops: SrcJointHoops {
            effective_legs: 2,
            diameter: StandardRebar::D13,
            spacing_mm: 425.0,
            width_mm: 600.0,
            grade: "SD295".into(),
        },
    }];
    model.load_cases = [
        (0, "用途別積載", LoadCaseKind::Live, 15_000.0),
        (1, "固定荷重", LoadCaseKind::Dead, 10_000.0),
        (2, "下向き風圧", LoadCaseKind::Wind, 25_000.0),
    ]
    .into_iter()
    .map(|(id, name, kind, load)| LoadCase {
        origin: Default::default(),
        id: LoadCaseId(id),
        name: name.into(),
        kind,
        nodal: vec![NodalLoad::manual(
            NodeId(2),
            [0.0, 0.0, -load, 0.0, 0.0, 0.0],
        )],
        member: Vec::new(),
    })
    .collect();
    model.combinations = vec![LoadCombination {
        name: "短期 G+P+W".into(),
        terms: vec![
            (LoadCaseId(1), 1.0),
            (LoadCaseId(0), 1.0),
            (LoadCaseId(2), 1.0),
        ],
    }];
    model
}
