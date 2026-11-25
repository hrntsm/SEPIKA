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

pub fn simply_supported_with_middle_brace(point_mm: f64, load_n: f64) -> Model {
    use sepika_core::dof::{Dof, Dof6Mask};
    use sepika_core::model::SteelDesignAttr;

    let mut model = cantilever(
        0.0,
        0.0,
        Some(MemberLoadKind::Point {
            a: point_mm,
            p: -load_n,
        }),
    );
    for node in &mut model.nodes {
        node.restraint = Dof6Mask::PINNED;
    }
    model.nodes[0].restraint.set_fixed(Dof::Rx);
    model.load_cases[0].nodal.clear();
    model.steel_design_attrs.push(SteelDesignAttr {
        elem: BEAM,
        lateral_brace_count: Some(1),
        lb_direct: None,
        c_direct: None,
        lk_y_direct: None,
        lk_z_direct: None,
        column_lateral_buckling_end_supports: None,
        joint_flange_loss: 0.0,
        joint_web_loss: 0.0,
        scallop_web_loss: 0.0,
    });
    model
}
