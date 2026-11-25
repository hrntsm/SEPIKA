use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::{
    FrameSectionUse, LoadCase, LoadCaseKind, LoadCombination, MemberLoad, MemberLoadKind, Model,
    NodalLoad, LL_FRAME_CASE_NAME,
};

use super::combined_column;

pub const BEAM: ElemId = ElemId(0);
pub const DEAD: LoadCaseId = LoadCaseId(0);
pub const LENGTH_MM: f64 = 8_000.0;

pub fn cantilever(tip_n: f64, tip_my_nmm: f64, load: Option<MemberLoadKind>) -> Model {
    let mut model = combined_column::cantilever(0.0, 0.0, 0.0);
    model.sections[0].frame_use = Some(FrameSectionUse::Girder);
    model.nodes[1].coord = [LENGTH_MM, 0.0, 0.0];
    model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    model.materials[0].name = "SN400B".into();
    model.materials[0].fy = None;
    model.materials[0].shear = Some(79_000.0);
    model.load_cases[0].name = "長期試験荷重".into();
    model.load_cases[0].nodal = vec![NodalLoad::manual(
        NodeId(1),
        [0.0, 0.0, tip_n, 0.0, tip_my_nmm, 0.0],
    )];
    model.load_cases[0].member = load
        .into_iter()
        .map(|kind| MemberLoad::manual(BEAM, [0.0, 0.0, 1.0], kind))
        .collect();
    model.combinations = vec![];
    model
}

pub fn short_point_combination() -> Model {
    let mut model = cantilever(22_500.0, 0.0, None);
    let snow = LoadCaseId(11);
    let live = LoadCaseId(12);
    model.load_cases.extend([
        LoadCase {
            id: snow,
            name: "積雪試験荷重".into(),
            kind: LoadCaseKind::Snow,
            origin: Default::default(),
            nodal: vec![],
            member: vec![MemberLoad::manual(
                BEAM,
                [0.0, 0.0, 1.0],
                MemberLoadKind::Point {
                    a: LENGTH_MM / 4.0,
                    p: -40_000.0,
                },
            )],
        },
        LoadCase {
            id: live,
            name: LL_FRAME_CASE_NAME.into(),
            kind: LoadCaseKind::Live,
            origin: Default::default(),
            nodal: vec![],
            member: vec![],
        },
    ]);
    model.combinations = vec![LoadCombination {
        name: "G+P+S".into(),
        terms: vec![(DEAD, 1.0), (live, 1.0), (snow, 1.0)],
    }];
    model
}
