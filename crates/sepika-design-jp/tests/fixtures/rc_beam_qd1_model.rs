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
