use super::rc_wall_side_column_model;

use sepika_core::ids::{LoadCaseId, MaterialId};
use sepika_core::model::{
    LoadCase, LoadCaseKind, LoadCombination, Material, MaterialCategory, Model, SrcWallSteelPlate,
    StoryStructure,
};
use sepika_core::section_shape::SectionShape;

pub fn input_model(embedded: bool) -> Model {
    let mut model = rc_wall_side_column_model::input_model();
    let column = &mut model.sections[0];
    let Some(SectionShape::RcColumnRect { b, d, rebar }) = column.shape.take() else {
        unreachable!()
    };
    let mut rebar = rebar;
    rebar.hoop.legs_x = 4;
    rebar.hoop.legs_y = 4;
    column.shape = Some(SectionShape::SrcColumnRect {
        b,
        d,
        rebar,
        steel_height: 300.0,
        steel_width: 150.0,
        steel_web_thick: 8.0,
        steel_flange_thick: 12.0,
        steel_manufacture: Default::default(),
    });
    column.steel_material = Some(MaterialId(3));
    let beam = &mut model.sections[1];
    let Some(SectionShape::RcBeamRect { b, d, rebar }) = beam.shape.take() else {
        unreachable!()
    };
    beam.shape = Some(SectionShape::SrcBeamRect {
        b,
        d,
        rebar,
        steel_height: 300.0,
        steel_width: 150.0,
        steel_web_thick: 8.0,
        steel_flange_thick: 12.0,
        steel_manufacture: Default::default(),
    });
    beam.steel_material = Some(MaterialId(3));
    model.sections[2].shape = Some(SectionShape::RcWall {
        thickness: 180.0,
        ps: 0.004,
        pwv_ratio: Some(0.004),
        pwh_ratio: Some(0.004),
    });
    model.materials.push(Material {
        id: MaterialId(3),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        fy: Some(235.0),
        shear: None,
        fc: None,
        concrete_class: Default::default(),
        strength_factor: Some(1.0),
    });
    model.wall_plates[0].src_wall_detailing_confirmed = true;
    if embedded {
        model.wall_plates[0].src_steel_plate = Some(SrcWallSteelPlate {
            thickness_mm: 12.0,
            material: MaterialId(3),
            force_transfer_confirmed: true,
        });
    }
    for story in &mut model.stories {
        story.structure = StoryStructure::Src;
    }
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(1),
        name: "架構用積載".into(),
        kind: LoadCaseKind::Live,
        nodal: vec![],
        member: vec![],
    });
    model.combinations = vec![LoadCombination {
        name: "SRC壁の長期組合せ".into(),
        terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
    }];
    model.ultimate_gravity_combination = Some(0);
    model
}

#[allow(dead_code)]
pub fn analysis_input_model(embedded: bool) -> Model {
    let mut model = input_model(embedded);
    for node in &mut model.nodes {
        node.coord[0] *= 0.55;
        if node.coord[2] > 0.0 {
            node.coord[2] = 7000.0;
        }
    }
    model.stories[1].elevation = Some(7000.0);
    let previous = &model.sections[2];
    let mut shape = previous.shape.clone().unwrap();
    if let SectionShape::RcWall { thickness, .. } = &mut shape {
        *thickness = 240.0;
    }
    let mut section = shape.to_section(previous.id, previous.name.clone());
    section.material = previous.material;
    section.rebar_material = previous.rebar_material;
    section.shear_rebar_material = previous.shear_rebar_material;
    model.sections[2] = section;
    model
}
