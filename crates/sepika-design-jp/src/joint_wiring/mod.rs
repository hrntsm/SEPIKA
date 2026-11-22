//! 節点単位の断面検定（柱梁接合部・パネルゾーン・耐震壁）の
//! 入力組み立て（令82条・各構造規準の断面検定）。
//!
//! `Model` と部材内力から入力を組み立てて一括実行する。

mod common;
mod rc_joint;
mod src_panel;
mod steel_panel;
mod wall;

pub use self::common::ForcesAt;
use self::common::MemberInfo;
pub(crate) use self::src_panel::GirderFaceForces;

#[cfg(test)]
pub(crate) use crate::rc::wall::{rc_wall_shear_check, RcWallInput};
#[cfg(test)]
pub(crate) use crate::wall_opening::equivalent_opening;

use crate::{CheckOutcome, LoadTerm, MemberKind};
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{ElementKind, FrameSectionUse, Model};
use sepika_solver::statics::linear::SteelPanelColumnFaceForce;
use std::collections::HashMap;

/// モデルと部材内力から節点単位の検定を一括実行する。
///
/// 戻り値: `(節点, 種別ラベル, 検定結果/検定不能)` のリスト。
/// SRC接合部では、解析から求めた実柱面の評価点を部材内力に含める。
pub fn collect_joint_checks(
    model: &Model,
    member_forces: &[(ElemId, ForcesAt<'_>)],
    term: LoadTerm,
) -> Vec<(NodeId, String, CheckOutcome)> {
    collect_joint_checks_with_panel_moments(model, member_forces, &[], term)
}

/// `panel_moments` には、仕口パネルをモデル化した接合部で解析が出力した
/// せん断モーメント `{MSX, MSY}` [N·mm] を節点ごとに渡す。該当する節点の
/// S/CFT 造仕口パネル検定は、柱2主軸へ仕事共役な逆変換で復元する。
/// 応答がない節点では鉛直柱の材端力を符号付きで集約する。非鉛直柱の材端力組立は未算定。
/// 標準形式と方向別のフランジ高さ・dbが不整合な軸は理由付き未算定。
pub fn collect_joint_checks_with_panel_moments(
    model: &Model,
    member_forces: &[(ElemId, ForcesAt<'_>)],
    panel_moments: &[(NodeId, [f64; 2])],
    term: LoadTerm,
) -> Vec<(NodeId, String, CheckOutcome)> {
    collect_joint_checks_impl(
        model,
        member_forces,
        panel_moments,
        GirderFaceForces::EvaluationPoints(member_forces),
        None,
        term,
        true,
    )
}

pub(crate) fn collect_joint_checks_impl(
    model: &Model,
    member_forces: &[(ElemId, ForcesAt<'_>)],
    panel_moments: &[(NodeId, [f64; 2])],
    src_face_forces: GirderFaceForces<'_>,
    steel_panel_face_forces: Option<&[SteelPanelColumnFaceForce]>,
    term: LoadTerm,
    include_walls: bool,
) -> Vec<(NodeId, String, CheckOutcome)> {
    let mut out = Vec::new();

    let members = member_infos(model, member_forces);
    let adjacency = NodeAdjacency::primary_frame(model);
    let mut connected_braces = HashMap::new();
    for element in &model.elements {
        if matches!(element.kind, ElementKind::Brace { .. })
            || (element.kind.is_primary_frame_support()
                && model
                    .element_section(element)
                    .and_then(|section| section.frame_use)
                    == Some(FrameSectionUse::Brace))
        {
            for &node in &element.nodes {
                connected_braces.entry(node).or_insert(element.id);
            }
        }
    }

    if include_walls {
        wall::check_walls(
            model,
            member_forces,
            member_forces,
            None,
            &members,
            term,
            &mut out,
        );
    }

    for node in &model.nodes {
        let nid = node.id;
        src_panel::check_src_panel(
            model,
            &adjacency,
            src_face_forces,
            nid,
            connected_braces.get(&nid).copied(),
            term,
            &mut out,
        );
        let cols: Vec<&MemberInfo> = members
            .iter()
            .filter(|m| m.is_column() && m.elem.nodes.contains(&nid))
            .collect();
        let beams: Vec<&MemberInfo> = members
            .iter()
            .filter(|m| m.is_beam_horiz() && m.elem.nodes.contains(&nid))
            .collect();
        if !beams.is_empty() {
            if let Some(column) = model.elements.iter().find(|element| {
                element.nodes.contains(&nid)
                    && element.end_offsets_mm != [[0.0; 3]; 2]
                    && model.element_section(element).is_some_and(|section| {
                        section.frame_use == Some(sepika_core::model::FrameSectionUse::Column)
                    })
            }) {
                if sepika_core::structure_kind::member_structure_kind(model, column)
                    != sepika_core::structure_kind::StructureKind::Src
                {
                    out.push((
                        nid,
                        "柱梁接合部".into(),
                        CheckOutcome::Skipped {
                            reason: format!(
                                "接合部 ID {}・柱 ID {}: 非0XYZ偏心柱の柱梁接合部式は未対応です",
                                nid.0, column.id.0
                            ),
                        },
                    ));
                }
                continue;
            }
        }
        let panel_moment = panel_moments
            .iter()
            .find(|(n, _)| *n == nid)
            .map(|(_, m)| *m);

        steel_panel::check_s_panel(
            model,
            &cols,
            &beams,
            nid,
            panel_moment,
            steel_panel_face_forces,
            &mut out,
        );
        if !cols.is_empty() && !beams.is_empty() {
            rc_joint::check_rc_joint(&cols, &beams, nid, &mut out);
        }
        rc_joint::check_rc_joint_ultimate(model, nid, &mut out);
    }

    out
}

#[cfg(test)]
mod tests;

fn member_infos<'a>(
    model: &'a Model,
    member_forces: &'a [(ElemId, ForcesAt<'a>)],
) -> Vec<MemberInfo<'a>> {
    let mut members: Vec<MemberInfo<'_>> = Vec::new();
    for (eid, forces) in member_forces {
        let Some(elem) = model.element(*eid) else {
            continue;
        };
        if elem.nodes.len() < 2 {
            continue;
        }
        let sec = model.element_section(elem);
        let mat = model.element_material(elem);
        let rebar_mat = model.element_rebar_material(elem);
        let shear_mat = model.element_shear_rebar_material(elem);
        let steel_mat = model.element_steel_material(elem);
        let (Some(sec), Some(mat)) = (sec, mat) else {
            continue;
        };
        let length = model.member_length(elem);
        if length < 1e-9 {
            continue;
        }
        let Ok(member_kind) = MemberKind::try_of_element(elem, model) else {
            continue;
        };
        members.push(MemberInfo {
            elem,
            sec,
            mat,
            rebar_mat,
            shear_mat,
            steel_mat,
            forces,
            member_kind,
            length,
        });
    }

    members
}

/// 壁版・生成壁要素から候補を列挙し、種別ごとの未検定理由を保持する。
pub fn collect_wall_design_checks(
    model: &Model,
    member_forces: &[(ElemId, ForcesAt<'_>)],
    term: LoadTerm,
    index: Option<&sepika_core::wall_expand::WallExpansionIndex>,
    case: &str,
    load_terms: Option<&[(sepika_core::ids::LoadCaseId, f64)]>,
) -> Vec<crate::wall_check::WallCheck> {
    let members = member_infos(model, member_forces);
    wall::collect_wall_design_checks(
        model,
        member_forces,
        &members,
        term,
        index,
        case,
        load_terms,
    )
}

pub(crate) use steel_panel::validate_steel_panel_inputs;
