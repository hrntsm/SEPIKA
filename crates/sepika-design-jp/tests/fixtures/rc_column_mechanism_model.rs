use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementKind, FrameSectionUse, LoadCase, LoadCaseKind, LoadCombination, MaterialCategory, Model,
};
use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RebarSize, SectionShape};

pub const COLUMN: ElemId = ElemId(0);
pub const GIRDER: ElemId = ElemId(1);
pub const GIRDER_SECTION: SectionId = SectionId(1);
pub const REBAR: MaterialId = MaterialId(1);
pub const WRONG_REBAR: MaterialId = MaterialId(2);
pub const GRAVITY: &str = "常時";

pub fn column_with_girder(mut model: Model) -> Model {
    model.nodes[1].coord[2] = 1500.0;
    model.stories[1].elevation = Some(model.nodes[1].coord[2]);
    let mut far_node = model.nodes[1].clone();
    far_node.id = NodeId(2);
    far_node.coord[0] = 4000.0;
    model.nodes.push(far_node);

    let mut wrong = model.materials[REBAR.index()].clone();
    wrong.id = WRONG_REBAR;
    wrong.category = MaterialCategory::Steel;
    model.materials.push(wrong);

    let mut section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: RebarSize::DiameterMm(22.0),
            top: vec![3],
            bottom: vec![3],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs: 2,
            },
        },
    }
    .to_section(GIRDER_SECTION, "接続大梁".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = model.sections[0].material;
    section.rebar_material = Some(WRONG_REBAR);
    section.shear_rebar_material = Some(REBAR);
    model.sections.push(section);

    let mut beam = model.elements[COLUMN.index()].clone();
    beam.id = GIRDER;
    beam.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    beam.section = Some(GIRDER_SECTION);
    beam.local_axis.ref_vector = [0.0, 0.0, 1.0];
    model.elements.push(beam);
    let live = LoadCaseId(1);
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: live,
        name: "架構用積載".into(),
        kind: LoadCaseKind::Live,
        nodal: Vec::new(),
        member: Vec::new(),
    });
    model.combinations.push(LoadCombination {
        name: GRAVITY.into(),
        terms: vec![(model.load_cases[0].id, 1.0), (live, 1.0)],
    });
    model.validate().unwrap();
    model
}

pub fn column_with_yielding_head_girders(model: Model) -> Model {
    let mut model = column_with_girder(model);
    let section = &mut model.sections[GIRDER_SECTION.index()];
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut section.shape else {
        unreachable!();
    };
    rebar.main_dia = RebarSize::DiameterMm(19.0);
    rebar.top = vec![2];
    rebar.bottom = vec![2];
    section.rebar_material = Some(REBAR);

    let mut far_node = model.nodes[2].clone();
    far_node.id = NodeId(3);
    far_node.coord = [0.0, 4000.0, 1500.0];
    model.nodes.push(far_node);
    let mut girder = model.elements[GIRDER.index()].clone();
    girder.id = ElemId(2);
    girder.nodes[1] = NodeId(3);
    model.elements.push(girder);
    model
}

pub fn add_column_above(model: &mut Model, base: NodeId, kind: ElementKind) {
    let mut top = model.nodes[base.index()].clone();
    top.id = NodeId(model.nodes.len() as u32);
    top.coord[2] = 4500.0;
    let mut upper = model.elements[COLUMN.index()].clone();
    upper.id = ElemId(model.elements.len() as u32);
    upper.kind = kind;
    upper.nodes = [base, top.id].into_iter().collect();
    upper.end_offsets_mm = [[0.0; 3]; 2];
    model.nodes.push(top);
    model.elements.push(upper);
}
