use super::eccentricity_model::{column_model, TOP};
use sepika_core::dof::Dof;
use sepika_core::ids::{LoadCaseId, NodeId};
use sepika_core::model::{Constraint, GirderTorsionMode, LoadCase, LoadCaseKind, Model, NodalLoad};

pub const HEADS: [NodeId; 4] = [NodeId(1), NodeId(3), NodeId(5), NodeId(7)];
pub const XY_MM: [[f64; 2]; 4] = [[0.0, 0.0], [2000.0, 0.0], [0.0, 4000.0], [2000.0, 4000.0]];
pub const UNIFORM_MM: f64 = 0.25;
pub const SMALL_SCALE: f64 = 0.001;
pub type HeadForces = [[f64; 2]; 4];

pub fn model_with_two_diaphragms() -> Model {
    let mut model = column_model(&XY_MM, 0.0, 0.0);
    model.girder_torsion = GirderTorsionMode::Keep;
    model.sections[0].iy = 12_000_000.0;
    for (id, direction) in HEADS
        .into_iter()
        .zip([[1.0, 0.0], [1.0, 0.0], [1.0, -1.0], [1.0, 1.0]])
    {
        let head = &mut model.nodes[id.index()];
        head.mass = Some([1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        for dof in [Dof::Uz, Dof::Rx, Dof::Ry] {
            head.restraint.set_fixed(dof);
        }
        let [x, y] = direction;
        model.elements[id.index() / 2].local_axis.ref_vector = [x, y, 0.0];
    }
    for [master, slave] in [[HEADS[0], HEADS[2]], [HEADS[1], HEADS[3]]] {
        model
            .constraints
            .push(Constraint::rigid_diaphragm(TOP, master, vec![slave]));
    }
    model
}

pub fn scale(forces: HeadForces, factor: f64) -> HeadForces {
    forces.map(|pair| pair.map(|value| value * factor))
}

pub fn add_head_load_case(model: &mut Model, name: &str, forces: HeadForces) -> LoadCaseId {
    let id = LoadCaseId(model.load_cases.len() as u32);
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id,
        name: name.into(),
        kind: LoadCaseKind::Seismic,
        nodal: HEADS
            .into_iter()
            .zip(forces)
            .map(|(node, [fx, fy])| NodalLoad::manual(node, [fx, fy, 0.0, 0.0, 0.0, 0.0]))
            .collect(),
        member: Vec::new(),
    });
    id
}
