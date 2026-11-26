use super::{combined_column, support_column};
use sepika_core::ids::{LoadCaseId, NodeId};
use sepika_core::model::{MemberLoad, MemberLoadKind, Model, NodalLoad};

pub const CASE: LoadCaseId = LoadCaseId(0);

pub fn column(head_n: f64, member_n: f64) -> Model {
    let mut model = support_column::column(0.0, true);
    model.load_cases = combined_column::cantilever(0.0, 0.0, 0.0).load_cases;
    let load = &mut model.load_cases[0];
    load.nodal = vec![NodalLoad::manual(
        NodeId(1),
        [head_n, 0.0, 0.0, 0.0, 0.0, 0.0],
    )];
    if member_n != 0.0 {
        load.member = vec![MemberLoad::manual(
            model.elements[0].id,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Point {
                a: 1_000.0,
                p: member_n,
            },
        )];
    }
    model
}
