use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::*;
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
};

pub fn expanded_model() -> Model {
    let mut model = Model {
        panel_zone: PanelZoneMode::None,
        ..Default::default()
    };
    for (i, coord) in [
        [0.0, 0.0, 0.0],
        [4000.0, 0.0, 0.0],
        [0.0, 0.0, 3000.0],
        [4000.0, 0.0, 3000.0],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord,
            restraint: if i < 2 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: Some([1.0; 6]),
            story: Some(StoryId(u32::from(i >= 2))),
            support_spring: None,
        });
    }
    let concrete = Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 23000.0,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.0),
        fy: None,
        concrete_class: Default::default(),
        strength_factor: Some(1.0),
    };
    let vertical = Material {
        id: MaterialId(1),
        name: "SD345".into(),
        category: MaterialCategory::Rebar,
        young: 205000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: None,
        fy: Some(345.0),
        concrete_class: Default::default(),
        strength_factor: Some(1.0),
    };
    let horizontal = Material {
        id: MaterialId(2),
        name: "SD295A".into(),
        fy: Some(295.0),
        ..vertical.clone()
    };
    model.materials = vec![concrete, vertical, horizontal];
    let column = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![4],
            y: vec![3],
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
        b: 600.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
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
    let wall = SectionShape::RcWall {
        thickness: 180.0,
        ps: 0.002,
        pwh_ratio: Some(0.004),
        pwv_ratio: Some(0.002),
    };
    for (i, shape) in [column, beam, wall].into_iter().enumerate() {
        let mut section = shape.to_section(SectionId(i as u32), format!("S{i}"));
        section.frame_use = match i {
            0 => Some(FrameSectionUse::Column),
            1 => Some(FrameSectionUse::Girder),
            _ => None,
        };
        section.material = Some(MaterialId(0));
        section.rebar_material = Some(MaterialId(1));
        section.shear_rebar_material = Some(MaterialId(2));
        model.sections.push(section);
    }
    for (id, nodes, section) in [
        (0, [0, 2], 0),
        (1, [1, 3], 0),
        (2, [2, 3], 1),
        (3, [0, 1], 1),
    ] {
        model.elements.push(ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: nodes.map(NodeId).into_iter().collect(),
            section: Some(SectionId(section)),
            local_axis: LocalAxis {
                ref_vector: if section == 0 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                },
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::AxialBendingInteract,
            rigid_zone: RigidZone {
                face_i: Some(300.0),
                face_j: Some(300.0),
                ..Default::default()
            },
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        });
    }
    model.elements.push(ElementData {
        id: ElemId(4),
        kind: ElementKind::Wall,
        nodes: [NodeId(0), NodeId(1), NodeId(3), NodeId(2)]
            .into_iter()
            .collect(),
        section: Some(SectionId(2)),
        local_axis: LocalAxis {
            ref_vector: [1.0, 0.0, 0.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    });
    for i in 0..2 {
        model.stories.push(Story {
            id: StoryId(i),
            name: format!("{}F", i + 1),
            elevation: Some(f64::from(i) * 3000.0),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(i * 2), NodeId(i * 2 + 1)],
            seismic_weight: Some(1_000_000.0),
            weight_override: Some(1_000_000.0),
            structure: StoryStructure::Rc,
            level_kind: StoryLevelKind::Normal,
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
            wall_weights: Vec::new(),
        });
    }
    model.load_cases = vec![LoadCase {
        id: LoadCaseId(0),
        name: "G".into(),
        kind: LoadCaseKind::Dead,
        nodal: vec![
            NodalLoad::manual(NodeId(2), [0.0, 0.0, -300_000.0, 0.0, 0.0, 0.0]),
            NodalLoad::manual(NodeId(3), [0.0, 0.0, -500_000.0, 0.0, 0.0, 0.0]),
        ],
        member: Vec::new(),
    }];
    model.combinations.clear();
    model
}

#[allow(dead_code)]
pub fn input_model() -> Model {
    let mut model = expanded_model();
    model.elements.pop();
    let boundary = vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)];
    model.add_enclosed_wall_plate_from_nodes(
        &boundary,
        WallPlate {
            id: sepika_core::ids::WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(2)),
            dl_support: Some(WallDlSupport::LowerBeam),
            self_weight_shares: Vec::new(),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: Vec::new(),
            slit: Default::default(),
        },
    );
    model.wall_regions.push(WallRegion {
        id: sepika_core::ids::WallRegionId(0),
        name: "RC壁".into(),
        boundary,
        wall_plate_ids: vec![sepika_core::ids::WallPlateId(0)],
        posts: Vec::new(),
    });
    model.wall_weight_generation = Some(WallWeightGenerationMode::GravityCasesOnly);
    model
}
