use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
    Model, Node, Section,
};
use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
const LENGTH_MM: f64 = 3000.0;

fn rebar(count: u32) -> RcRectColumnRebar {
    RcRectColumnRebar {
        main_dia: if count == 0 { 0.0 } else { 19.0 },
        x: if count == 0 { vec![] } else { vec![count] },
        y: if count == 0 { vec![] } else { vec![count] },
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: 10.0,
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    }
}

fn fixture(shape: Option<SectionShape>, category: MaterialCategory) -> Model {
    let mut section = shape.map_or_else(
        || Section {
            area: 12345.0,
            width: 300.0,
            depth: 600.0,
            iy: 8.0e8,
            iz: 2.0e8,
            j: 1.0e6,
            as_y: 10000.0,
            as_z: 10000.0,
            ..Section::zero(SectionId(0), "保存AとbDが異なる".into())
        },
        |shape| shape.to_section(SectionId(0), "独立質量fixture".into()),
    );
    section.frame_use = match section.shape {
        Some(SectionShape::RcBeamRect { .. }) => Some(sepika_core::model::FrameSectionUse::Girder),
        Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. }) => {
            Some(sepika_core::model::FrameSectionUse::Column)
        }
        _ => section.frame_use,
    };
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section.steel_material = Some(MaterialId(2));
    let material = |id, category, density, young, fc, fy| Material {
        id: MaterialId(id),
        name: format!("material-{id}"),
        category,
        density,
        young,
        poisson: 0.3,
        fc,
        fy,
        shear: None,
        strength_factor: None,
        concrete_class: Default::default(),
    };
    Model {
        nodes: [[0.0, 0.0, 0.0], [LENGTH_MM, 0.0, 0.0]]
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
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![
            material(
                0,
                category,
                if category == MaterialCategory::Steel {
                    7.85e-9
                } else {
                    2.4e-9
                },
                205000.0,
                if category == MaterialCategory::Steel {
                    None
                } else {
                    Some(24.0)
                },
                Some(235.0),
            ),
            material(
                1,
                MaterialCategory::Rebar,
                7.85e-9,
                200000.0,
                None,
                Some(400.0),
            ),
            material(
                2,
                MaterialCategory::Steel,
                7.85e-9,
                205000.0,
                None,
                Some(235.0),
            ),
        ],
        ..Default::default()
    }
}

pub fn wall_connection_fixture(kind: ElementKind) -> Model {
    use sepika_core::model::FrameSectionUse;
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar};
    let mut model = fixture(
        Some(SectionShape::RcColumnRect {
            b: 600.0,
            d: 600.0,
            rebar: rebar(4),
        }),
        MaterialCategory::Concrete,
    );
    model.materials[0].young = 24000.0;
    let mut girder = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: vec![4],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 200.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(1), "G".into());
    girder.frame_use = Some(FrameSectionUse::Girder);
    girder.material = Some(MaterialId(0));
    girder.rebar_material = Some(MaterialId(1));
    girder.shear_rebar_material = Some(MaterialId(1));
    let mut wall = SectionShape::RcWall {
        thickness: 150.0,
        pwh_ratio: None,
        pwv_ratio: None,
        ps: 0.0025,
    }
    .to_section(SectionId(2), "W".into());
    wall.material = Some(MaterialId(0));
    model.sections.extend([girder, wall]);
    let node = model.nodes[0].clone();
    model.nodes = [
        [0.0, 0.0, 0.0],
        [5000.0, 0.0, 0.0],
        [0.0, 0.0, 3000.0],
        [5000.0, 0.0, 3000.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(i, coord)| Node {
        id: NodeId(i as u32),
        coord,
        ..node.clone()
    })
    .collect();
    let member = model.elements[0].clone();
    model.elements = [(0, 0, 2, 0), (1, 1, 3, 0), (2, 0, 1, 1), (3, 2, 3, 1)]
        .into_iter()
        .map(|(id, a, b, section)| ElementData {
            id: ElemId(id),
            kind,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: Some(SectionId(section)),
            local_axis: LocalAxis {
                ref_vector: if section == 0 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                },
            },
            ..member.clone()
        })
        .collect();
    model.elements.push(ElementData {
        id: ElemId(4),
        kind: ElementKind::Wall,
        nodes: [NodeId(0), NodeId(1), NodeId(3), NodeId(2)]
            .into_iter()
            .collect(),
        section: Some(SectionId(2)),
        ..member
    });
    use sepika_core::ids::{WallPlateId, WallRegionId};
    use sepika_core::model::{WallPlate, WallPlateShape, WallRegion, WallWeightGenerationMode};
    model.rebuild_assignment_regions().unwrap();
    assert_eq!(model.wall_assignment_regions.regions.len(), 1);
    model.wall_assignment_regions.regions[0].assignment =
        sepika_core::model::PlateAssignment::Plate(WallPlateId(0));
    model.wall_plates.push(WallPlate {
        id: WallPlateId(0),
        shape: WallPlateShape::Enclosed,
        section: Some(SectionId(2)),
        opening_area: 0.0,
        opening_weight: 0.0,
        openings: vec![],
        loads: vec![],
        slit: Default::default(),
        self_weight_shares: vec![],
        dl_support: None,
    });
    let mut region = WallRegion::new(
        WallRegionId(0),
        vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)],
    );
    region.wall_plate_ids.push(WallPlateId(0));
    model.wall_regions.push(region);
    model
        .generated_wall_origins
        .insert(ElemId(4), WallPlateId(0));
    model.wall_weight_generation = Some(WallWeightGenerationMode::GravityCasesOnly);
    model
}
