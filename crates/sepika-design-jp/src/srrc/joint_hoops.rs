use crate::material_strength::rebar_allowable_shear;
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{
    same_src_joint_plane_axis, src_joint_columns, src_joint_planes, Model, SrcJointHoopAssignment,
};
use sepika_core::panel_zone::PanelPlane;
use serde::Serialize;
use std::collections::BTreeMap;

/// 短期接合部せん断検定用の帯筋比（無次元）と許容引張応力度 [N/mm²]。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SrcShortTermJointHoops {
    pub ratio: f64,
    pub allowable_tension_n_per_mm2: f64,
}

/// 1構面の帯筋入力と短期検定への準備結果。構面が消失した場合、部材IDの配列は空。
#[derive(Debug, Serialize)]
pub struct SrcJointHoopInputStatus<'a> {
    pub node: NodeId,
    pub axis: [f64; 3],
    pub column_ids: Vec<ElemId>,
    pub beam_ids: Vec<ElemId>,
    pub assignments: Vec<&'a SrcJointHoopAssignment>,
    pub prepared: Option<SrcShortTermJointHoops>,
    pub reason: Option<String>,
}

fn target_reason(node: NodeId, axis: [f64; 3], reason: &str) -> String {
    format!("SRC柱梁接合部 節点 {} 構面 {:?}: {reason}", node.0, axis)
}

fn same_members(registered: &[ElemId], current: &[ElemId]) -> bool {
    registered.len() == current.len() && current.iter().all(|id| registered.contains(id))
}

fn prepare(
    column_ids: &[ElemId],
    beam_ids: &[ElemId],
    assignments: &[&SrcJointHoopAssignment],
) -> Result<SrcShortTermJointHoops, String> {
    let entry = match assignments {
        [] => return Err("この構面の帯筋が未入力です".into()),
        [entry] => entry,
        _ => return Err("この構面の帯筋指定が重複しています".into()),
    };
    if entry.columns.is_empty() || entry.beams.is_empty() {
        return Err(
            "登録時の柱・梁との対応が失われています。現在の構面で帯筋を確認して再登録してください"
                .into(),
        );
    }
    if !same_members(&entry.columns, column_ids) {
        return Err(
            "接続柱が登録時と異なります。現在の構面で帯筋を確認して再登録してください".into(),
        );
    }
    if !same_members(&entry.beams, beam_ids) {
        return Err(
            "構面内の梁が登録時と異なります。現在の構面で帯筋を確認して再登録してください".into(),
        );
    }
    let ratio = entry.hoops.ratio()?;
    Ok(SrcShortTermJointHoops {
        ratio,
        allowable_tension_n_per_mm2: rebar_allowable_shear(&entry.hoops.grade, false),
    })
}

fn matching_assignments(
    model: &Model,
    node: NodeId,
    axis: [f64; 3],
) -> Vec<&SrcJointHoopAssignment> {
    model
        .src_joint_hoops
        .iter()
        .filter(|entry| entry.node == node && same_src_joint_plane_axis(entry.axis, axis))
        .collect()
}

fn beam_ids(plane: &PanelPlane) -> Vec<ElemId> {
    plane.beams.iter().map(|beam| beam.element).collect()
}

/// 指定構面の短期検定用帯筋を解決する。構面は SRC 柱が接続する実モデルの水平軸で指定する。
/// 組立・配筋細則や耐力の適合は判定しない。未設定・無効な場合は節点と構面を含む理由を返す。
pub fn resolve_src_short_term_joint_hoops(
    model: &Model,
    adjacency: &NodeAdjacency,
    node: NodeId,
    axis: [f64; 3],
) -> Result<SrcShortTermJointHoops, String> {
    let with_target = |reason: String| target_reason(node, axis, &reason);
    let planes = src_joint_planes(model, adjacency, node);
    let mut matches = planes
        .iter()
        .filter(|plane| same_src_joint_plane_axis(plane.axis, axis));
    let (Some(plane), None) = (matches.next(), matches.next()) else {
        return Err(with_target(
            "指定軸でSRC柱梁接合部の構面を一意に確認できません".into(),
        ));
    };
    let columns: Vec<_> = src_joint_columns(model, adjacency, node)
        .map(|column| column.id)
        .collect();
    prepare(
        &columns,
        &beam_ids(plane),
        &matching_assignments(model, node, plane.axis),
    )
    .map_err(with_target)
}

/// SRC柱梁接合部の構面ごとの指定と、現在の構面に対応しない保存済みの指定を返す。
pub fn src_joint_hoop_input_statuses(model: &Model) -> Vec<SrcJointHoopInputStatus<'_>> {
    let adjacency = NodeAdjacency::primary_frame(model);
    let mut by_node: BTreeMap<NodeId, Vec<&SrcJointHoopAssignment>> = BTreeMap::new();
    for entry in &model.src_joint_hoops {
        by_node.entry(entry.node).or_default().push(entry);
    }
    let mut statuses = Vec::new();
    for node in &model.nodes {
        let planes = src_joint_planes(model, &adjacency, node.id);
        let saved = by_node.get(&node.id).map_or(&[][..], Vec::as_slice);
        let columns: Vec<_> = src_joint_columns(model, &adjacency, node.id)
            .map(|column| column.id)
            .collect();
        for plane in &planes {
            let assignments = saved
                .iter()
                .copied()
                .filter(|entry| same_src_joint_plane_axis(entry.axis, plane.axis))
                .collect::<Vec<_>>();
            let beam_ids = beam_ids(plane);
            let (prepared, reason) = match prepare(&columns, &beam_ids, &assignments) {
                Ok(input) => (Some(input), None),
                Err(reason) => (None, Some(target_reason(node.id, plane.axis, &reason))),
            };
            statuses.push(SrcJointHoopInputStatus {
                node: node.id,
                axis: plane.axis,
                column_ids: columns.clone(),
                beam_ids,
                assignments,
                prepared,
                reason,
            });
        }
        for &entry in saved.iter().filter(|entry| {
            !planes
                .iter()
                .any(|plane| same_src_joint_plane_axis(plane.axis, entry.axis))
        }) {
            statuses.push(SrcJointHoopInputStatus {
                node: node.id,
                axis: entry.axis,
                column_ids: Vec::new(),
                beam_ids: Vec::new(),
                assignments: vec![entry],
                prepared: None,
                reason: Some(target_reason(
                    node.id,
                    entry.axis,
                    "帯筋の登録先に対応する現在のSRC柱梁接合部の構面がありません",
                )),
            });
        }
    }
    statuses
}
