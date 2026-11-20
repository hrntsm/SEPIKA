//! 層に帰属する柱の列挙（中間節点分割を連なりとして 1 本とみなす）。
//!
//! 層間変位・偏心率など、当該層の柱を数える処理の判定ロジックの単一情報源。

use sepika_core::ids::{ElemId, NodeId, StoryId};
use sepika_core::model::{ElementKind, FrameSectionUse, Layer, Model};
use std::collections::{HashMap, HashSet};

/// 層に帰属する 1 本の柱（中間節点で分割された柱の連なりを束ねたもの）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryColumn {
    /// 最上側セグメントの部材 ID（`ColumnDrift.elem` 等の代表 ID）。
    pub top_elem: ElemId,
    pub top: NodeId,
    pub bottom: NodeId,
    /// 上端から下端へ連なる柱セグメント。各部材を一度ずつ含む。
    pub segments: Vec<ElemId>,
}

/// `story`（層の上端階）に属する柱の一覧を返す。
///
/// 上端階 SL の節点から下端階 SL に達した柱の連なりを返す。
/// 上下階情報が未確定、または下端に達しない連なりは対象外。
pub fn story_columns(model: &Model, story: StoryId) -> Vec<StoryColumn> {
    analysis_story_columns(model, story).unwrap_or_default()
}

pub(super) fn analysis_story_columns(
    model: &Model,
    story: StoryId,
) -> Result<Vec<StoryColumn>, String> {
    collect_story_columns(model, story, ColumnSelection::Analysis)
}

/// 上下階SLを結ぶ非分岐柱を列挙し、柱節点の参照・座標不整合はErrを返す。
pub fn checked_story_columns(model: &Model, story: StoryId) -> Result<Vec<StoryColumn>, String> {
    collect_story_columns(model, story, ColumnSelection::Nonbranching)
}

pub(super) fn drift_story_columns(
    model: &Model,
    story: StoryId,
) -> Result<Vec<StoryColumn>, String> {
    mixed_story_connections(model, story)?;
    collect_story_columns(model, story, ColumnSelection::Drift)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnSelection {
    Analysis,
    Nonbranching,
    Drift,
}

fn collect_story_columns(
    model: &Model,
    story: StoryId,
    selection: ColumnSelection,
) -> Result<Vec<StoryColumn>, String> {
    let Some(layer) = story_layer(model, story) else {
        return Err(format!("上端階{}の上下階SL又はFL間高さが不正です", story.0));
    };
    validate_column_elements(model)?;
    let adj = column_adjacency(model);
    let has_mixed_columns = has_mixed_columns(model);
    let mut out = Vec::new();
    let mut used_segments = HashMap::new();
    for node in &model.nodes {
        if model.on_diaphragm_level(story, node.coord[2]) {
            if let Some((col, mixed_connection)) =
                trace_column_down(model, &adj, node.id, &layer, selection, has_mixed_columns)
            {
                if selection == ColumnSelection::Drift {
                    if let Some(reason) =
                        boundary_link_error(story, drift_boundary_links(model, &adj, &layer, &col))
                    {
                        return Err(reason);
                    }
                }
                if has_mixed_columns {
                    for &segment in &col.segments {
                        if let Some((other, other_mixed)) =
                            used_segments.insert(segment, (col.top_elem, mixed_connection))
                        {
                            if mixed_connection || other_mixed {
                                return Err(format!(
                                    "階{}の柱{}と柱{}は中間節点で分岐し、部材{}を共有するため層内の柱を一意に特定できません",
                                    story.0, other.0, col.top_elem.0, segment.0
                                ));
                            }
                        }
                    }
                }
                out.push(col);
            }
        }
    }
    Ok(out)
}

fn boundary_link_at(
    model: &Model,
    adj: &HashMap<NodeId, Vec<(NodeId, ElemId)>>,
    level: StoryId,
    node: NodeId,
) -> Option<ElemId> {
    adj.get(&node)?.iter().find_map(|&(next, elem)| {
        (next != node && model.on_diaphragm_level(level, model.nodes[next.index()].coord[2]))
            .then_some(elem)
    })
}

fn drift_boundary_links(
    model: &Model,
    adj: &HashMap<NodeId, Vec<(NodeId, ElemId)>>,
    layer: &Layer,
    column: &StoryColumn,
) -> [Option<ElemId>; 2] {
    let top_link = boundary_link_at(model, adj, layer.top, column.top).or_else(|| {
        column.segments.iter().find_map(|&id| {
            let elem = model.element(id)?;
            (elem.nodes[0] != elem.nodes[1]
                && elem.nodes.iter().all(|node| {
                    model.on_diaphragm_level(layer.top, model.nodes[node.index()].coord[2])
                }))
            .then_some(id)
        })
    });
    [
        top_link,
        boundary_link_at(model, adj, layer.bottom, column.bottom),
    ]
}

fn boundary_link_error(story: StoryId, links: [Option<ElemId>; 2]) -> Option<String> {
    let members = match links {
        [Some(top), Some(bottom)] => format!("上階SLの柱{}・下階SLの柱{}", top.0, bottom.0),
        [Some(top), None] => format!("上階SLの柱{}", top.0),
        [None, Some(bottom)] => format!("下階SLの柱{}", bottom.0),
        [None, None] => return None,
    };
    Some(format!(
        "階{}の{}に層境界のSL節点どうしを結ぶ柱接続があり、上下階SL間の区間を一意に特定できません",
        story.0, members
    ))
}

fn validate_column_elements(model: &Model) -> Result<(), String> {
    for elem in &model.elements {
        if model
            .element_section(elem)
            .is_some_and(|s| s.frame_use == Some(FrameSectionUse::Column))
        {
            if !elem.kind.is_primary_frame_support() || elem.nodes.len() != 2 {
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
    Ok(())
}

fn story_layer(model: &Model, top_story: StoryId) -> Option<Layer> {
    model
        .layers()
        .into_iter()
        .find(|layer| layer.top == top_story)
}

fn is_mixed_member(elem: &sepika_core::model::ElementData) -> bool {
    matches!(elem.kind, ElementKind::Fiber | ElementKind::MultiSpring)
}

fn has_mixed_columns(model: &Model) -> bool {
    model.elements.iter().any(|elem| {
        is_mixed_member(elem)
            && model
                .element_section(elem)
                .is_some_and(|section| section.frame_use == Some(FrameSectionUse::Column))
    })
}

fn is_layer_interior(model: &Model, layer: &Layer, z: f64) -> bool {
    layer.bottom_sl_mm < z
        && z < layer.top_sl_mm
        && !model.on_diaphragm_level(layer.top, z)
        && !model.on_diaphragm_level(layer.bottom, z)
}

pub(super) fn mixed_story_connections(
    model: &Model,
    story: StoryId,
) -> Result<Vec<(NodeId, ElemId)>, String> {
    if !has_mixed_columns(model) {
        return Ok(Vec::new());
    }
    let Some(layer) = story_layer(model, story) else {
        return Ok(Vec::new());
    };
    validate_column_elements(model)?;
    let adj = column_adjacency(model);
    let at_top = |z| model.on_diaphragm_level(story, z);
    let at_bottom = |z| model.on_diaphragm_level(layer.bottom, z);
    let inside = |z| is_layer_interior(model, &layer, z);
    let link_boundaries = |first: NodeId, second: NodeId| {
        let z = [first, second].map(|id| model.nodes[id.index()].coord[2]);
        [story, layer.bottom].map(|level| {
            first != second
                && z.iter()
                    .all(|&height| model.on_diaphragm_level(level, height))
        })
    };
    let is_mixed_kind = |id| model.element(id).is_some_and(is_mixed_member);
    let linked_sl: HashSet<_> = adj
        .iter()
        .filter(|&(&node, neighbors)| {
            neighbors
                .iter()
                .any(|&(next, _)| link_boundaries(node, next).into_iter().any(|linked| linked))
        })
        .map(|(&node, _)| node)
        .collect();
    let mut visited = HashSet::new();
    let mut connections = Vec::new();
    for top in model.nodes.iter().filter(|node| at_top(node.coord[2])) {
        let starts_at_boundary_link = linked_sl.contains(&top.id);
        for &(start, top_elem) in adj.get(&top.id).into_iter().flatten() {
            let z = model.nodes[start.index()].coord[2];
            let mut reaches_bottom = !starts_at_boundary_link && at_bottom(z);
            let mut stack = Vec::new();
            if starts_at_boundary_link {
                if !visited.insert(top.id) {
                    break;
                }
                stack.push(top.id);
            } else if inside(z) || linked_sl.contains(&start) {
                if !visited.insert(start) {
                    continue;
                }
                stack.push(start);
            } else if !reaches_bottom {
                continue;
            }
            let mut includes_mixed_kind = !starts_at_boundary_link && is_mixed_kind(top_elem);
            let mut boundary_links = [None; 2];
            while let Some(current) = stack.pop() {
                let current_is_interior = inside(model.nodes[current.index()].coord[2]);
                for &(next, elem) in &adj[&current] {
                    let z = model.nodes[next.index()].coord[2];
                    let mixed = is_mixed_kind(elem);
                    for (link, linked) in boundary_links
                        .iter_mut()
                        .zip(link_boundaries(current, next))
                    {
                        if linked && (link.is_none() || mixed) {
                            *link = Some(elem);
                        }
                    }
                    reaches_bottom |= at_bottom(z);
                    // 明示的な境界内の柱接続がないSL節点を通ると、独立した柱まで連結してしまう。
                    if inside(z) || linked_sl.contains(&next) {
                        if visited.insert(next) {
                            stack.push(next);
                        }
                    // 層内の中間節点に取り付く層外枝も、そこでの分岐を判定するために必要。
                    } else if !current_is_interior && !at_top(z) && !at_bottom(z) {
                        continue;
                    }
                    includes_mixed_kind |= mixed;
                }
            }
            if reaches_bottom && includes_mixed_kind {
                if let Some(reason) = boundary_link_error(story, boundary_links) {
                    return Err(reason);
                }
                connections.push((top.id, top_elem));
            }
        }
    }
    Ok(connections)
}

fn on_bottom_level(model: &Model, bottom: StoryId, z: f64) -> bool {
    model.on_diaphragm_level(bottom, z)
}

fn column_adjacency(model: &Model) -> HashMap<NodeId, Vec<(NodeId, ElemId)>> {
    let mut adj: HashMap<NodeId, Vec<(NodeId, ElemId)>> = HashMap::new();
    for elem in &model.elements {
        if !elem.kind.is_primary_frame_support() || elem.nodes.len() != 2 {
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

fn exclude_branches_without_lower_sl(
    model: &Model,
    adj: &HashMap<NodeId, Vec<(NodeId, ElemId)>>,
    layer: &Layer,
    candidates: &mut Vec<(NodeId, ElemId)>,
) -> bool {
    let mut includes_mixed_member = false;
    let mut reaches_lower_sl = HashSet::new();
    for &(start, representative) in candidates.iter() {
        let mut stack = vec![(start, representative)];
        let mut visited = HashSet::new();
        while let Some((node, elem)) = stack.pop() {
            includes_mixed_member |= model.element(elem).is_some_and(is_mixed_member);
            let z = model.nodes[node.index()].coord[2];
            if model.on_diaphragm_level(layer.bottom, z) {
                reaches_lower_sl.insert(representative);
            } else if is_layer_interior(model, layer, z) && visited.insert(node) {
                stack.extend(adj[&node].iter().copied());
            }
        }
    }
    if includes_mixed_member {
        candidates.retain(|(_, id)| reaches_lower_sl.contains(id));
    }
    includes_mixed_member
}

fn trace_column_down(
    model: &Model,
    adj: &HashMap<NodeId, Vec<(NodeId, ElemId)>>,
    top: NodeId,
    layer: &Layer,
    selection: ColumnSelection,
    has_mixed_columns: bool,
) -> Option<(StoryColumn, bool)> {
    let reject_branch = selection != ColumnSelection::Analysis;
    let mut current = top;
    let mut top_elem = None;
    let mut segments = Vec::new();
    let mut visited = HashSet::new();
    visited.insert(top);
    let mut crossed_boundary = false;
    let mut mixed_connection = false;

    loop {
        let cur_node = &model.nodes[current.index()];
        if current != top && on_bottom_level(model, layer.bottom, cur_node.coord[2]) {
            if crossed_boundary && mixed_connection {
                return None;
            }
            return Some((
                StoryColumn {
                    top_elem: top_elem?,
                    top,
                    bottom: current,
                    segments,
                },
                mixed_connection,
            ));
        }

        // 通常柱の列挙を変えないため、層境界を通過したかどうかと、新型を含むかを別々に追う。
        if current != top
            && has_mixed_columns
            && !is_layer_interior(model, layer, cur_node.coord[2])
        {
            crossed_boundary = true;
        }
        if crossed_boundary && mixed_connection {
            return None;
        }
        if reject_branch && current != top && adj.get(&current)?.len() != 2 {
            return None;
        }
        if cur_node.coord[2] < layer.bottom_sl_mm {
            return None;
        }
        if reject_branch && cur_node.coord[2] > model.nodes[top.index()].coord[2] {
            return None;
        }
        let cur_z = cur_node.coord[2];
        let mut neighbors: Vec<_> = adj
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

        if current == top && neighbors.len() > 1 {
            if has_mixed_columns {
                mixed_connection =
                    exclude_branches_without_lower_sl(model, adj, layer, &mut neighbors);
            }
            if selection == ColumnSelection::Drift && neighbors.len() > 1 {
                // 上階SL内の分割材を枝に数えると、曖昧な境界があっても経路を検査できない。
                neighbors.retain(|(node, _)| {
                    !model.on_diaphragm_level(layer.top, model.nodes[node.index()].coord[2])
                });
            }
        }
        if neighbors.len() != 1 {
            return None;
        }
        let (next, elem) = neighbors[0];
        mixed_connection |= has_mixed_columns && model.element(elem).is_some_and(is_mixed_member);
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
        ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Node, RigidZone, Story,
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
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
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
    fn mixed_column_reachability_stays_in_the_layer_and_stops_at_sl_nodes() {
        let (mut model, top) = build_split_column_model();
        model.elements[0].kind = ElementKind::Fiber;
        assert_eq!(
            mixed_story_connections(&model, top).unwrap(),
            [(NodeId(2), ElemId(0))]
        );

        let mut outside = model.nodes[0].clone();
        outside.id = NodeId(3);
        outside.coord[2] = -1000.0;
        model.nodes.push(outside);
        model.elements[1].nodes[0] = NodeId(3);
        for (id, nodes) in [(2, [3, 0]), (3, [2, 0])] {
            let mut elem = model.elements[1].clone();
            elem.id = ElemId(id);
            elem.nodes = nodes.map(NodeId).into_iter().collect();
            model.elements.push(elem);
        }
        assert!(mixed_story_connections(&model, top).unwrap().is_empty());
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
