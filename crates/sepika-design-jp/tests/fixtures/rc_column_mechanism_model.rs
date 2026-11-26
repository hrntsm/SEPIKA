use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, SlabId};
use sepika_core::model::{
    ElementKind, FrameSectionUse, LoadCase, LoadCaseKind, LoadCombination, LoadTransfer,
    MaterialCategory, Model, NodalLoad, RegionAnchor, Slab, SlabPlate, SlabShape,
};
use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RebarSize, SectionShape};

pub const COLUMN: ElemId = ElemId(0);
pub const GIRDER: ElemId = ElemId(1);
const OPPOSITE_GIRDER: ElemId = ElemId(3);
pub const GIRDER_SECTION: SectionId = SectionId(1);
pub const REBAR: MaterialId = MaterialId(1);
pub const WRONG_REBAR: MaterialId = MaterialId(2);
pub const GRAVITY: &str = "常時";

pub fn column_with_girder(mut model: Model) -> Model {
    model.nodes[1].coord[2] = 1500.0;
    model.elements[COLUMN.index()].rigid_zone.face_i = Some(0.0);
    model.elements[COLUMN.index()].rigid_zone.face_j = Some(0.0);
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

pub fn column_with_opposing_head_girders(model: Model, opposite_kind: ElementKind) -> Model {
    let mut model = column_with_yielding_head_girders(model);
    for node in &mut model.nodes[1..] {
        node.coord[2] = 1700.0;
        node.restraint.set_fixed(Dof::Rz);
    }
    model.stories[1].elevation = Some(1700.0);

    let mut opposite_end = model.nodes[2].clone();
    opposite_end.id = NodeId(4);
    opposite_end.coord[0] = -4000.0;
    let mut opposite = model.elements[GIRDER.index()].clone();
    opposite.id = OPPOSITE_GIRDER;
    opposite.kind = opposite_kind;
    opposite.nodes = [opposite_end.id, NodeId(1)].into_iter().collect();
    model.nodes.push(opposite_end);
    model.elements.push(opposite);

    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [0.0, 0.0, -200_000.0, 0.0, 0.0, 0.0],
    )];
    let seismic = LoadCaseId(2);
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: seismic,
        name: "X方向地震".into(),
        kind: LoadCaseKind::Seismic,
        nodal: vec![NodalLoad::manual(
            NodeId(1),
            [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )],
        member: Vec::new(),
    });
    model.combinations.push(LoadCombination {
        name: "X方向短期".into(),
        terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0), (seismic, 1.0)],
    });
    model
}

pub fn remove_opposite_girder_yield_strength(model: &mut Model) {
    let material = &mut model.materials[WRONG_REBAR.index()];
    material.category = MaterialCategory::Rebar;
    material.fy = None;
    let mut section = model.sections[GIRDER_SECTION.index()].clone();
    section.id = SectionId(2);
    section.rebar_material = Some(material.id);
    model.elements[OPPOSITE_GIRDER.index()].section = Some(section.id);
    model.sections.push(section);
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

pub fn column_with_reversible_girders(model: Model, head_bars: u32, foot_bars: u32) -> Model {
    let mut model = column_with_yielding_head_girders(model);
    add_column_above(&mut model, NodeId(1), ElementKind::Beam);

    let mut bottom = model.nodes[0].clone();
    bottom.id = NodeId(model.nodes.len() as u32);
    bottom.coord[2] = -3000.0;
    let mut lower = model.elements[COLUMN.index()].clone();
    lower.id = ElemId(model.elements.len() as u32);
    lower.nodes = [bottom.id, NodeId(0)].into_iter().collect();
    model.nodes[0].restraint = Dof6Mask::FREE;
    model.nodes.push(bottom);
    model.elements.push(lower);

    let foot_section = SectionId(2);
    let mut foot = model.sections[GIRDER_SECTION.index()].clone();
    foot.id = foot_section;
    foot.name = "柱脚の梁".into();
    model.sections.push(foot);
    for (section, count) in [(GIRDER_SECTION, head_bars), (foot_section, foot_bars)] {
        let Some(SectionShape::RcBeamRect { rebar, .. }) =
            &mut model.sections[section.index()].shape
        else {
            unreachable!();
        };
        rebar.top = vec![count];
        rebar.bottom = vec![count];
    }

    for (joint, x, section) in [
        (NodeId(1), -4000.0, GIRDER_SECTION),
        (NodeId(0), 4000.0, foot_section),
        (NodeId(0), -4000.0, foot_section),
    ] {
        let mut far = model.nodes[joint.index()].clone();
        far.id = NodeId(model.nodes.len() as u32);
        far.coord[0] = x;
        let mut girder = model.elements[GIRDER.index()].clone();
        girder.id = ElemId(model.elements.len() as u32);
        girder.section = Some(section);
        girder.nodes = if x > 0.0 {
            [joint, far.id]
        } else {
            [far.id, joint]
        }
        .into_iter()
        .collect();
        model.nodes.push(far);
        model.elements.push(girder);
    }
    for joint in [NodeId(7), NodeId(8)] {
        let mut base = model.nodes[joint.index()].clone();
        base.id = NodeId(model.nodes.len() as u32);
        base.coord[2] = -3000.0;
        base.restraint = Dof6Mask::FIXED;
        let mut side_column = model.elements[COLUMN.index()].clone();
        side_column.id = ElemId(model.elements.len() as u32);
        side_column.nodes = [base.id, joint].into_iter().collect();
        model.nodes.push(base);
        model.elements.push(side_column);
    }
    model
}

pub fn column_for_head_axis_comparison(model: Model) -> Model {
    let mut model = column_with_reversible_girders(model, 2, 5);
    let section = &mut model.sections[2];
    let mut shape = section.shape.take().unwrap();
    let SectionShape::RcBeamRect { b, .. } = &mut shape else {
        unreachable!()
    };
    *b = 400.0;
    let mut widened = shape.to_section(section.id, section.name.clone());
    widened.frame_use = section.frame_use;
    widened.material = section.material;
    widened.rebar_material = section.rebar_material;
    widened.shear_rebar_material = section.shear_rebar_material;
    *section = widened;
    model
}

pub fn column_with_legacy_foot_girders(model: Model) -> Model {
    let mut model = column_with_reversible_girders(model, 4, 1);
    model.elements[3].nodes[0] = NodeId(2);
    model.nodes[4].coord[0] = model.nodes[2].coord[0];
    model.elements[2].section = Some(SectionId(2));
    model
}

pub fn add_wall_below_reversible_column(model: &mut Model) {
    use sepika_core::ids::{WallPlateId, WallRegionId};
    use sepika_core::model::{WallPlate, WallPlateShape, WallRegion};

    let boundary = [NodeId(5), NodeId(9), NodeId(7), NodeId(0)];
    let mut base_girder = model.elements[GIRDER.index()].clone();
    base_girder.id = ElemId(model.elements.len() as u32);
    base_girder.nodes = [boundary[0], boundary[1]].into_iter().collect();
    model.elements.push(base_girder);

    let section = SectionId(model.sections.len() as u32);
    let mut wall_section = SectionShape::RcWall {
        thickness: 150.0,
        ps: 0.0025,
        pwh_ratio: Some(0.0025),
        pwv_ratio: Some(0.0025),
    }
    .to_section(section, "柱脚に接する壁".into());
    wall_section.material = model.sections[0].material;
    wall_section.rebar_material = Some(REBAR);
    model.sections.push(wall_section);
    let plate = model.add_enclosed_wall_plate_from_nodes(
        &boundary,
        WallPlate {
            src_wall_detailing_confirmed: false,
            src_steel_plate: None,
            self_weight_shares: Vec::new(),
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(section),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: Vec::new(),
            slit: Default::default(),
            dl_support: None,
        },
    );
    let mut region = WallRegion::new(WallRegionId(0), boundary.to_vec());
    region.wall_plate_ids.push(plate);
    model.wall_regions.push(region);
}

pub fn attach_slab(model: &mut Model, nodes: [NodeId; 2]) {
    let section = SectionId(model.sections.len() as u32);
    let mut slab = SectionShape::RcSlab { thickness: 150.0 }.to_section(section, "床板".into());
    slab.material = model.sections[0].material;
    model.sections.push(slab);
    model.slabs.push(Slab {
        id: SlabId(model.slabs.len() as u32),
        shape: SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes,
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: [1500.0; 2],
        },
        plate: SlabPlate {
            section: Some(section),
            ..Default::default()
        },
        reference_story: None,
        vertical_eccentricity_mm: None,
        tip_loads: Vec::new(),
    });
}

pub fn column_with_slab_sensitive_head(model: Model, continuous: bool) -> (Model, [NodeId; 2]) {
    let (mut model, bars, edge) = if continuous {
        (
            column_with_reversible_girders(model, 2, 4),
            2,
            [NodeId(1), NodeId(6)],
        )
    } else {
        (
            column_with_opposing_head_girders(model, ElementKind::Beam),
            1,
            [NodeId(1), NodeId(4)],
        )
    };
    let Some(SectionShape::RcBeamRect { rebar, .. }) =
        &mut model.sections[GIRDER_SECTION.index()].shape
    else {
        unreachable!();
    };
    rebar.top = vec![bars];
    rebar.bottom = vec![bars];
    if continuous {
        let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut model.sections[0].shape else {
            unreachable!();
        };
        rebar.x = vec![4];
        rebar.y = vec![4];
    }
    (model, edge)
}
