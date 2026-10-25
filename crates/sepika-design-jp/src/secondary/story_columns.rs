//! 層に帰属する柱の列挙（中間節点分割を連なりとして 1 本とみなす）。
//!
//! 層間変位・偏心率など、当該層の柱を数える処理の判定ロジックの単一情報源。

use sepika_core::ids::{ElemId, NodeId, StoryId};
use sepika_core::model::{ElementKind, FrameSectionUse, Model};
use std::collections::{HashMap, HashSet};

/// 層に帰属する 1 本の柱（中間節点で分割された柱の連なりを束ねたもの）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryColumn {
    /// 最上側セグメントの部材 ID（`ColumnDrift.elem` 等の代表 ID）。
    pub top_elem: ElemId,
    pub top: NodeId,
    pub bottom: NodeId,
    pub(crate) segments: Vec<ElemId>,
}

/// `story`（層の上端階）に属する柱の一覧を返す。
///
/// 上端階 SL の節点から下端階 SL に達した柱の連なりを返す。
/// 上下階情報が未確定、または下端に達しない連なりは対象外。
pub fn story_columns(model: &Model, story: StoryId) -> Vec<StoryColumn> {
    collect_story_columns(model, story, false).unwrap_or_default()
}

/// 上下階SLを結ぶ非分岐柱を列挙し、柱節点の参照・座標不整合はErrを返す。
pub fn checked_story_columns(model: &Model, story: StoryId) -> Result<Vec<StoryColumn>, String> {
    collect_story_columns(model, story, true)
}

fn collect_story_columns(
    model: &Model,
    story: StoryId,
    reject_branch: bool,
) -> Result<Vec<StoryColumn>, String> {
    let Some((_, bottom)) = layer_bounds(model, story) else {
        return Err(format!("上端階{}の上下階SL又はFL間高さが不正です", story.0));
    };
    for elem in &model.elements {
        if model
            .element_section(elem)
            .is_some_and(|s| s.frame_use == Some(FrameSectionUse::Column))
        {
            if elem.kind != ElementKind::Beam || elem.nodes.len() != 2 {
                return Err(format!("柱{}の接続節点数又は要素種別が不正です", elem.id.0));
            }
            for id in &elem.nodes {
                if !model
                    .nodes
                    .get(id.index())
                    .is_some_and(|n| n.id == *id && n.coord.iter().all(|v| v.is_finite()))
                {
                    return Err(format!(
                        "柱{}の節点{}の参照又は座標が不正です",
                        elem.id.0, id.0
                    ));
                }
            }
        }
    }
    let adj = vertical_beam_adjacency(model);
    let mut out = Vec::new();
    for node in &model.nodes {
        if model.on_diaphragm_level(story, node.coord[2]) {
            if let Some(col) = trace_column_down(model, &adj, node.id, bottom, reject_branch) {
                out.push(col);
            }
        }
    }
    Ok(out)
}

fn layer_bounds(model: &Model, top_story: StoryId) -> Option<(StoryId, StoryId)> {
    model
        .layers()
        .into_iter()
        .find(|layer| layer.top == top_story)
        .map(|layer| (top_story, layer.bottom))
}

fn on_bottom_level(model: &Model, bottom: StoryId, z: f64) -> bool {
    model.on_diaphragm_level(bottom, z)
}

fn vertical_beam_adjacency(model: &Model) -> HashMap<NodeId, Vec<(NodeId, ElemId)>> {
    let mut adj: HashMap<NodeId, Vec<(NodeId, ElemId)>> = HashMap::new();
    for elem in &model.elements {
        if elem.kind != ElementKind::Beam || elem.nodes.len() != 2 {
            continue;
        }
        let Some(section) = model.element_section(elem) else {
            continue;
        };
        if section.frame_use != Some(FrameSectionUse::Column) {
            continue;
        }
        let n0_id = elem.nodes[0];
        let n1_id = elem.nodes[1];
        adj.entry(n0_id).or_default().push((n1_id, elem.id));
        adj.entry(n1_id).or_default().push((n0_id, elem.id));
    }
    adj
}

fn trace_column_down(
    model: &Model,
    adj: &HashMap<NodeId, Vec<(NodeId, ElemId)>>,
    top: NodeId,
    bottom: StoryId,
    reject_branch: bool,
) -> Option<StoryColumn> {
    let mut current = top;
    let mut top_elem = None;
    let mut segments = Vec::new();
    let mut visited = HashSet::new();
    visited.insert(top);

    loop {
        let cur_node = &model.nodes[current.index()];
        if current != top && on_bottom_level(model, bottom, cur_node.coord[2]) {
            return Some(StoryColumn {
                top_elem: top_elem?,
                top,
                bottom: current,
                segments,
            });
        }

        if reject_branch && current != top && adj.get(&current)?.len() != 2 {
            return None;
        }
        if cur_node.coord[2] < model.stories.iter().find(|s| s.id == bottom)?.sl_mm()? {
            return None;
        }
        if reject_branch && cur_node.coord[2] > model.nodes[top.index()].coord[2] {
            return None;
        }
        let cur_z = cur_node.coord[2];
        let neighbors: Vec<_> = adj
            .get(&current)?
            .iter()
            .filter(|(nid, _)| {
                if reject_branch && current != top {
                    !visited.contains(nid)
                } else {
                    model.nodes[nid.index()].coord[2] < cur_z - 1e-9
                }
            })
            .copied()
            .collect();

        if neighbors.len() != 1 {
            return None;
        }
        let (next, elem) = neighbors[0];
        if !visited.insert(next) {
            return None;
        }
        if top_elem.is_none() {
            top_elem = Some(elem);
        }
        segments.push(elem);
        current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, NodeId, SectionId};
    use sepika_core::model::Section;
    use sepika_core::model::{
        ElementData, EndCondition, ForceRegime, LocalAxis, Node, RigidZone, Story,
    };
    use smallvec::SmallVec;

    /// 中間節点（z=2000）で 2 分割された 1 本柱（z=0, 2000, 4000）。
    fn build_split_column_model() -> (Model, StoryId) {
        let base = StoryId(0);
        let top_story = StoryId(1);
        let nodes = vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: Some(base),
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 2000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: Some(top_story),
                support_spring: None,
            },
            Node {
                id: NodeId(2),
                coord: [0.0, 0.0, 4000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: Some(top_story),
                support_spring: None,
            },
        ];
        let mk_elem = |id: u32, n0: u32, n1: u32| ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: {
                let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
                v.push(NodeId(n0));
                v.push(NodeId(n1));
                v
            },
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        };
        let elements = vec![mk_elem(0, 1, 2), mk_elem(1, 0, 1)];
        let model = Model {
            nodes,
            elements,
            sections: vec![{
                let mut section = Section::zero(sepika_core::ids::SectionId(0), "C".into());
                section.frame_use = Some(FrameSectionUse::Column);
                section
            }],
            stories: vec![
                Story {
                    wall_weights: Vec::new(),
                    id: base,
                    name: "1F".to_string(),
                    elevation: Some(0.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(0)],
                    seismic_weight: None,
                    weight_override: None,
                    structure: Default::default(),
                    level_kind: Default::default(),
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
                Story {
                    wall_weights: Vec::new(),
                    id: top_story,
                    name: "2F".to_string(),
                    elevation: Some(4000.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(2)],
                    seismic_weight: None,
                    weight_override: None,
                    structure: Default::default(),
                    level_kind: Default::default(),
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
            ],
            ..Default::default()
        };
        (model, top_story)
    }

    #[test]
    fn drift_490_cross_story_without_middle_sl_node_is_excluded() {
        let (mut model, _) = build_split_column_model();
        model.elements.truncate(1);
        model.elements[0].nodes = [NodeId(0), NodeId(2)].into_iter().collect();
        model.stories = [0.0, 2000.0, 4000.0]
            .into_iter()
            .enumerate()
            .map(|(i, z)| Story::new(StoryId(i as u32), format!("{}F", i + 1), Some(z), Some(0.0)))
            .collect();
        assert!(checked_story_columns(&model, StoryId(1))
            .unwrap()
            .is_empty());
        assert!(checked_story_columns(&model, StoryId(2))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn drift_490_interior_branch_in_both_orientations_is_excluded() {
        let (mut model, top) = build_split_column_model();
        let mut extra = model.nodes[2].clone();
        extra.id = NodeId(3);
        extra.coord[0] = 1000.0;
        model.nodes.push(extra);
        let mut branch = model.elements[0].clone();
        branch.id = ElemId(2);
        branch.nodes = [NodeId(1), NodeId(3)].into_iter().collect();
        model.elements.push(branch);
        assert!(checked_story_columns(&model, top).unwrap().is_empty());
        model.nodes[3].coord[2] = 0.0;
        assert!(checked_story_columns(&model, top).unwrap().is_empty());
    }

    #[test]
    fn drift_490_invalid_column_node_is_diagnosed() {
        let (mut model, top) = build_split_column_model();
        model.elements[0].nodes[0] = NodeId(99);
        assert!(checked_story_columns(&model, top)
            .unwrap_err()
            .contains("節点99"));
    }

    #[test]
    fn test_story_columns_merges_split_segments() {
        let (model, top) = build_split_column_model();
        let cols = story_columns(&model, top);
        assert_eq!(cols.len(), 1, "分割柱は 1 本と数える");
        assert_eq!(cols[0].top, NodeId(2));
        assert_eq!(cols[0].bottom, NodeId(0));
        assert_eq!(cols[0].top_elem, ElemId(0), "代表 ID は最上側セグメント");
    }

    #[test]
    fn test_story_columns_includes_sloped_column_use() {
        let (mut model, top) = build_split_column_model();
        model.nodes[1].coord = [500.0, 0.0, 2000.0];
        model.nodes[2].coord = [1000.0, 0.0, 4000.0];

        let cols = story_columns(&model, top);
        assert_eq!(cols.len(), 1, "傾斜していても柱用途の連なりを数える");
        assert_eq!(cols[0].top, NodeId(2));
        assert_eq!(cols[0].bottom, NodeId(0));
    }
    #[test]
    fn missing_middle_fl_never_extends_the_upper_story_to_structure_bottom() {
        let (mut model, _) = build_split_column_model();
        model.stories = [0.0, 2000.0, 4000.0]
            .into_iter()
            .enumerate()
            .map(|(i, fl)| {
                Story::new(
                    StoryId(i as u32),
                    format!("{}F", i + 1),
                    Some(fl),
                    Some(0.0),
                )
            })
            .collect();
        assert_eq!(story_columns(&model, StoryId(2))[0].bottom, NodeId(1));
        model.stories[1].elevation = None;
        assert!(story_columns(&model, StoryId(2)).is_empty());
        model.stories[1].elevation = Some(2000.0);
        model.stories[1].sl_offset_mm = None;
        assert!(story_columns(&model, StoryId(2)).is_empty());
        model.stories.truncate(1);
        assert!(story_columns(&model, StoryId(0)).is_empty());
    }
}
