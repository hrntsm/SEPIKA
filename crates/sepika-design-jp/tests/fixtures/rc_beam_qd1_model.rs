use sepika_core::dof::Dof6Mask;
use sepika_core::ids::ElemId;
use sepika_core::model::Model;
use sepika_core::section_shape::{RebarSize, SectionShape, StandardRebar};

use super::asymmetric;

pub fn model() -> Model {
    let mut model = asymmetric::model(false);
    model.nodes[1].coord[0] = 4500.0;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    let beam = &mut model.elements[0];
    beam.local_axis.ref_vector = [0.0, 0.0, 1.0];
    beam.rigid_zone.face_i = Some(250.0);
    beam.rigid_zone.face_j = Some(250.0);
    let mut section = asymmetric::section(350.0, 600.0, 19.0, &[2], &[4]);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut section.shape else {
        unreachable!();
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D19);
    rebar.cover = 30.5;
    model.sections[0] = section;
    model
}

pub fn add_columns_at_outer_nodes(model: &mut Model, reference: [f64; 3]) {
    use sepika_core::ids::{NodeId, SectionId};
    use sepika_core::model::FrameSectionUse;
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};

    let column_shape = SectionShape::RcColumnRect {
        b: 500.0,
        d: 500.0,
        rebar: RcRectColumnRebar {
            main_dia: RebarSize::Standard(StandardRebar::D19),
            x: vec![3],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: RebarSize::Standard(StandardRebar::D10),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let section_id = SectionId(model.sections.len() as u32);
    let mut section = column_shape.to_section(section_id, "C1".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = model.sections[0].material;
    model.sections.push(section);
    let beam = model.elements[0].clone();
    for node_id in [NodeId(0), NodeId(1)] {
        model.nodes[node_id.index()].restraint = Dof6Mask::FREE;
        let mut base = model.nodes[node_id.index()].clone();
        let base_id = NodeId(model.nodes.len() as u32);
        base.id = base_id;
        base.coord[2] = -3000.0;
        base.restraint = Dof6Mask::FIXED;
        model.nodes.push(base);
        let mut column = beam.clone();
        column.id = ElemId(model.elements.len() as u32);
        column.section = Some(section_id);
        column.nodes = [base_id, node_id].into_iter().collect();
        column.local_axis.ref_vector = reference;
        model.elements.push(column);
    }
}

pub fn extend_j_column(model: &mut Model) {
    use sepika_core::ids::NodeId;

    let mut tip = model.nodes[3].clone();
    tip.id = NodeId(4);
    tip.coord[2] = -6000.0;
    model.nodes.push(tip);
    model.nodes[3].restraint = Dof6Mask::FREE;
    let mut extension = model.elements[2].clone();
    extension.id = ElemId(3);
    extension.nodes = [NodeId(4), NodeId(3)].into_iter().collect();
    model.elements.push(extension);
}

pub fn beam_with_recorded_continuous_slab_rebar() -> Model {
    use sepika_core::ids::{NodeId, SectionId, SlabId, StoryId};
    use sepika_core::model::{
        PlateAssignment, Slab, SlabPlate, SlabShape, SlabStraightBar, SlabTopReinforcement, Story,
    };
    use sepika_core::section_shape::StandardRebar;

    let mut model = model();
    model.nodes[1].coord[0] = 6_500.0;
    add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    model
        .stories
        .push(Story::new(StoryId(0), "2F".into(), Some(300.0), Some(0.0)));
    let mut grid = [[NodeId(0); 2]; 5];
    for (i, x) in [-8_000.0, -4_000.0, 0.0, 6_500.0, 10_500.0]
        .into_iter()
        .enumerate()
    {
        for (j, y) in [0.0, 2_000.0].into_iter().enumerate() {
            if matches!((i, j), (2, 0) | (3, 0)) {
                grid[i][j] = NodeId((i - 2) as u32);
            } else {
                let mut n = model.nodes[0].clone();
                n.id = NodeId(model.nodes.len() as u32);
                n.coord = [x, y, 0.0];
                grid[i][j] = n.id;
                model.nodes.push(n);
            }
        }
    }
    let mut add_girder = |a, b| {
        let mut elem = model.elements[0].clone();
        elem.id = ElemId(model.elements.len() as u32);
        elem.nodes = [a, b].into_iter().collect();
        model.elements.push(elem);
    };
    for i in 0..5 {
        add_girder(grid[i][0], grid[i][1]);
        if i < 4 {
            for j in 0..2 {
                if (i, j) != (2, 0) {
                    add_girder(grid[i][j], grid[i + 1][j]);
                }
            }
        }
    }
    let report = model.rebuild_floor_assignment_regions();
    assert_eq!(report.rejection, None);
    assert_eq!(report.regions, 4);
    let section_id = SectionId(model.sections.len() as u32);
    let mut section = SectionShape::RcSlab { thickness: 150.0 }.to_section(section_id, "S1".into());
    section.material = model.sections[0].material;
    model.sections.push(section);
    for (idx, region) in model
        .floor_assignment_regions
        .regions
        .iter_mut()
        .enumerate()
    {
        let id = SlabId(idx as u32);
        region.assignment = PlateAssignment::Plate(id);
        model.slabs.push(Slab {
            id,
            shape: SlabShape::Enclosed,
            plate: SlabPlate {
                section: Some(section_id),
                top_reinforcement: Some(SlabTopReinforcement {
                    monolithic_pour: "2F-A".into(),
                    straight_bars: Vec::new(),
                }),
                ..Default::default()
            },
            reference_story: Some(StoryId(0)),
            vertical_eccentricity_mm: Some(0.0),
            tip_loads: Vec::new(),
        });
    }
    let owner = model
        .slabs
        .iter()
        .position(|slab| {
            model
                .resolved_slab_placement(slab.id)
                .unwrap()
                .top_boundary
                .iter()
                .map(|p| p[0])
                .min_by(f64::total_cmp)
                == Some(-4_000.0)
        })
        .unwrap();
    model.slabs[owner]
        .plate
        .top_reinforcement
        .as_mut()
        .unwrap()
        .straight_bars = [300.0, 1_000.0]
        .map(|y| SlabStraightBar {
            ends_mm: [[-1_400.0, y, 270.0], [7_900.0, y, 270.0]],
            size: StandardRebar::D13,
            grade: "SD345".into(),
            yield_strength_n_mm2: 345.0,
            bond_yield_strength_n_mm2: Some(380.0),
        })
        .to_vec();
    model
}
