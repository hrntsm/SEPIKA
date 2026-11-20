use sepika_core::ids::{ElemId, NodeId, SectionId};
use sepika_core::model::{FrameSectionUse, Model, SrcJointHoops};
use sepika_core::section_shape::{RebarSize, StandardRebar};

mod src_h_steel_product {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sepika-design-jp/tests/fixtures/src_h_steel_product_model.rs"
    ));
}

pub const NODE: NodeId = NodeId(1);
pub const X: [f64; 3] = [1.0, 0.0, 0.0];
pub const Y: [f64; 3] = [0.0, 1.0, 0.0];

pub fn frame() -> Model {
    let mut model = src_h_steel_product::model(true, "SN400B", 0.0, 0.0);
    let beam_model = src_h_steel_product::model(false, "SN400B", 0.0, 0.0);
    let mut girder = beam_model.sections[0].clone();
    girder.id = SectionId(1);
    assert_eq!(girder.frame_use, Some(FrameSectionUse::Girder));
    model.sections.push(girder);

    let column = &mut model.sections[0];
    let Some(sepika_core::section_shape::SectionShape::SrcColumnRect { b, d, rebar, .. }) =
        &mut column.shape
    else {
        unreachable!();
    };
    *b = 800.0;
    *d = 800.0;
    rebar.hoop.dia = RebarSize::Standard(StandardRebar::D13);
    let mut rebuilt = column
        .shape
        .as_ref()
        .unwrap()
        .to_section(column.id, column.name.clone());
    rebuilt.frame_use = column.frame_use;
    rebuilt.material = column.material;
    rebuilt.rebar_material = column.rebar_material;
    rebuilt.shear_rebar_material = column.shear_rebar_material;
    rebuilt.steel_material = column.steel_material;
    *column = rebuilt;

    let template = model.nodes[1].clone();
    model.nodes = [
        [20_000.0; 3],
        [0.0, 0.0, 3_500.0],
        [0.0; 3],
        [6_000.0, 0.0, 3_500.0],
        [0.0, 6_000.0, 3_500.0],
        [-6_000.0, 0.0, 3_500.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(id, coord)| sepika_core::model::Node {
        id: NodeId(id as u32),
        coord,
        ..template.clone()
    })
    .collect();
    model.elements[0].nodes = [NodeId(2), NODE].into_iter().collect();
    for (id, end) in [(1, 3), (2, 4), (3, 5)] {
        let mut element = beam_model.elements[0].clone();
        element.id = ElemId(id);
        element.section = Some(SectionId(1));
        element.nodes = [NODE, NodeId(end)].into_iter().collect();
        model.elements.push(element);
    }
    model.load_cases.clear();
    model.combinations.clear();
    model.stories.clear();
    model
}

pub fn example_hoops() -> SrcJointHoops {
    SrcJointHoops {
        effective_legs: 2,
        diameter: StandardRebar::D13,
        spacing_mm: 425.0,
        width_mm: 800.0,
        grade: "SD295".into(),
    }
}
