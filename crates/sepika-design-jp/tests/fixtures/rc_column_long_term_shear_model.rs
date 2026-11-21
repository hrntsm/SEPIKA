use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, NodeId, SectionId};
use sepika_core::model::*;

pub const COLUMN: ElemId = ElemId(0);
pub const LENGTH_MM: f64 = 3000.0;

pub fn model(mut model: Model, mut beam_section: Section, inner_peak: bool) -> Model {
    beam_section.id = SectionId(1);
    model.sections.push(beam_section);
    let mut beam_end = model.nodes[0].clone();
    beam_end.id = NodeId(2);
    beam_end.coord = [3000.0, 0.0, 0.0];
    beam_end.restraint = Dof6Mask::FIXED;
    model.nodes.push(beam_end);
    let mut beam = model.elements[0].clone();
    beam.id = ElemId(1);
    beam.nodes = [NodeId(0), NodeId(2)].into_iter().collect();
    beam.local_axis.ref_vector = [0.0, 0.0, 1.0];
    beam.section = Some(SectionId(1));
    model.elements.push(beam);
    model.panel_zone = PanelZoneMode::None;
    model.girder_torsion = GirderTorsionMode::Keep;

    model.load_cases = default_load_cases();
    model.combinations = vec![default_combinations().remove(0)];
    let dead = &mut model.load_cases[0];
    dead.nodal.push(NodalLoad::manual(
        NodeId(1),
        [12_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ));
    dead.member
        .push(distributed([1.0, 0.0, 0.0], 0.0, LENGTH_MM, 1.0, 1.0));
    if inner_peak {
        dead.member.push(distributed(
            [1.0, 0.0, 0.0],
            1050.0,
            1100.0,
            -6000.0,
            -6000.0,
        ));
        dead.member
            .push(distributed([1.0, 0.0, 0.0], 1100.0, 1150.0, 6000.0, 6000.0));
    }
    model
}

pub fn distributed(dir: [f64; 3], a: f64, b: f64, w1: f64, w2: f64) -> MemberLoad {
    MemberLoad::manual(COLUMN, dir, MemberLoadKind::Distributed { a, b, w1, w2 })
}
