//! 節点・部材の編集コマンド。

use super::*;
use sepika_core::ids::*;
use sepika_core::model::{
    FloorPlateAssignmentRegions, FloorRegion, Slab, WallPlate, WallPlateAssignmentRegions,
    WallRegion,
};

/// 位相（節点座標・部材）を変えるコマンドが割当領域を再構築したときの undo 用
/// スナップショット。孤児化した版の除去までを 1 つの Undo 単位に含めるため、
/// 再構築が変更しうる派生データ（床領域・壁領域・割当領域・版）を退避する。
#[derive(Clone)]
struct AssignmentTopology {
    strengths: sepika_core::model::StbStrengthInput,
    floor_regions: Vec<FloorRegion>,
    wall_regions: Vec<WallRegion>,
    floor_assignment_regions: FloorPlateAssignmentRegions,
    wall_assignment_regions: WallPlateAssignmentRegions,
    slabs: Vec<Slab>,
    wall_plates: Vec<WallPlate>,
}

impl AssignmentTopology {
    fn capture(model: &Model) -> Self {
        Self {
            strengths: model.stb_strengths.clone(),
            floor_regions: model.floor_regions.clone(),
            wall_regions: model.wall_regions.clone(),
            floor_assignment_regions: model.floor_assignment_regions.clone(),
            wall_assignment_regions: model.wall_assignment_regions.clone(),
            slabs: model.slabs.clone(),
            wall_plates: model.wall_plates.clone(),
        }
    }

    fn restore(self, model: &mut Model) {
        model.stb_strengths = self.strengths;
        model.floor_regions = self.floor_regions;
        model.wall_regions = self.wall_regions;
        model.floor_assignment_regions = self.floor_assignment_regions;
        model.wall_assignment_regions = self.wall_assignment_regions;
        model.slabs = self.slabs;
        model.wall_plates = self.wall_plates;
    }
}

/// 位相変更コマンドの逆操作。子の逆操作を適用してから、適用前の割当領域と版を
/// 復元する（復元は ID 繰上げ・繰下げの後に来る必要があるため、この順序で行う）。
struct RestoreAssignmentTopology {
    snapshot: AssignmentTopology,
    inverse: Box<dyn EditCommand>,
}

impl EditCommand for RestoreAssignmentTopology {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let redo = self.inverse.apply(model);
        self.snapshot.clone().restore(model);
        redo
    }

    fn label(&self) -> &str {
        self.inverse.label()
    }
}

/// 節点座標 [mm] と全長追従の区間を変更する。Manual荷重を保持できない最終座標は編集全体を拒否する。
pub struct SetNodeCoord {
    pub node: NodeId,
    pub coord: [f64; 3],
}

pub(crate) fn validate_coordinate_loads(before: &Model, after: &Model) -> Result<(), String> {
    use sepika_core::model::{LoadSource, MemberLoadKind};
    for case in &after.load_cases {
        for (index, load) in case.member.iter().enumerate() {
            if load.source != LoadSource::Manual {
                continue;
            }
            let Some(elem) = after.element(load.elem) else {
                continue;
            };
            if !elem.nodes.iter().any(|id| {
                before
                    .node(*id)
                    .zip(after.node(*id))
                    .is_some_and(|(a, b)| a.coord != b.coord)
            }) {
                continue;
            }
            let length_mm = after.member_length(elem);
            let reason = if !length_mm.is_finite() || length_mm <= 1e-9 {
                Some("材長が非正または非有限になるため荷重区間を保持できません".to_owned())
            } else if let Err(reason) = load.validate_extent(length_mm) {
                Some(reason)
            } else {
                match load.kind {
                    MemberLoadKind::Point { a, .. }
                        if !a.is_finite() || a < 0.0 || a > length_mm =>
                    {
                        Some(format!(
                            "集中荷重位置 a={a} mm が材長 {length_mm} mm の範囲外です"
                        ))
                    }
                    MemberLoadKind::Distributed { a, b, .. }
                        if !a.is_finite()
                            || !b.is_finite()
                            || a < 0.0
                            || b <= a
                            || b > length_mm =>
                    {
                        Some(format!(
                            "分布区間 [{a}, {b}] mm を材長 {length_mm} mm で保持できません"
                        ))
                    }
                    _ => None,
                }
            };
            if let Some(reason) = reason {
                return Err(format!(
                    "荷重ケース {} ({}) member[{}] 部材 {} 荷重「{}」: {}",
                    case.id.0, case.name, index, load.elem.0, load.name, reason
                ));
            }
        }
    }
    Ok(())
}

struct RestoreNodeCoord {
    node: NodeId,
    coord: [f64; 3],
    loads: Vec<(usize, usize, sepika_core::model::MemberLoad)>,
    topology: AssignmentTopology,
}

impl EditCommand for RestoreNodeCoord {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let redo = RestoreNodeCoord {
            node: self.node,
            coord: model.nodes[self.node.index()].coord,
            loads: self
                .loads
                .iter()
                .map(|(case, index, _)| {
                    (
                        *case,
                        *index,
                        model.load_cases[*case].member[*index].clone(),
                    )
                })
                .collect(),
            topology: AssignmentTopology::capture(model),
        };
        model.nodes[self.node.index()].coord = self.coord;
        for (case, index, load) in &self.loads {
            model.load_cases[*case].member[*index] = load.clone();
        }
        self.topology.clone().restore(model);
        Box::new(redo)
    }
    fn label(&self) -> &str {
        "節点座標変更"
    }
}

impl EditCommand for SetNodeCoord {
    fn changes_assignment_boundaries(&self) -> bool {
        true
    }

    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut candidate = model.clone();
        let inverse = self.apply_candidate(&mut candidate);
        if inverse.is_noop() || inverse.rejection().is_some() {
            return inverse;
        }
        if let Err(reason) = validate_coordinate_loads(model, &candidate) {
            return Box::new(RejectedEdit(reason));
        }
        let report = candidate.rebuild_assignment_regions_dropping_orphan_plates();
        if let Some(reason) = report.floor.rejection.or(report.wall.rejection) {
            return Box::new(RejectedEdit(reason));
        }
        *model = candidate;
        inverse
    }

    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Err(reason) = model.validate_assignment_region_identity() {
            return Box::new(RejectedEdit(reason));
        }
        use sepika_core::model::{LoadSource, MemberLoadExtent, MemberLoadKind};
        let idx = self.node.index();
        if idx >= model.nodes.len() || model.nodes[idx].id != self.node {
            return Box::new(Noop);
        }
        let old_coord = model.nodes[idx].coord;
        let topology = AssignmentTopology::capture(model);
        model.nodes[idx].coord = self.coord;
        let lengths: std::collections::HashMap<_, _> = model
            .elements
            .iter()
            .filter(|e| e.nodes.contains(&self.node))
            .map(|e| (e.id, model.member_length(e)))
            .collect();
        let mut loads = Vec::new();
        for (case, lc) in model.load_cases.iter_mut().enumerate() {
            for (index, load) in lc.member.iter_mut().enumerate() {
                if load.source == LoadSource::Manual
                    && load.extent == MemberLoadExtent::FullLengthUniform
                {
                    if let Some(&length_mm) = lengths.get(&load.elem) {
                        loads.push((case, index, load.clone()));
                        if let MemberLoadKind::Distributed { a, b, .. } = &mut load.kind {
                            *a = 0.0;
                            *b = length_mm;
                        }
                    }
                }
            }
        }
        Box::new(RestoreNodeCoord {
            node: self.node,
            coord: old_coord,
            loads,
            topology,
        })
    }

    fn label(&self) -> &str {
        "節点座標変更"
    }
}

/// 節点拘束（支点条件）変更。逆操作は変更前マスクへの復元。
pub struct SetNodeRestraint {
    pub node: NodeId,
    pub restraint: sepika_core::dof::Dof6Mask,
}

impl EditCommand for SetNodeRestraint {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.node.index();
        if idx >= model.nodes.len() || model.nodes[idx].id != self.node {
            return Box::new(Noop);
        }
        let old = model.nodes[idx].restraint;
        model.nodes[idx].restraint = self.restraint;
        Box::new(SetNodeRestraint {
            node: self.node,
            restraint: old,
        })
    }

    fn label(&self) -> &str {
        "節点拘束変更"
    }
}

/// 節点の支点ばね変更。逆操作は変更前の指定への復元。
///
/// `restraint` で固定されている自由度のばね値は解析側（ソルバー）で無視される
/// （`Node::support_spring` の仕様）。本コマンドは restraint との整合チェックは
/// 行わない（先に固定を解除してからばねを設定する、または逆でもよい）。
/// 負のばね剛性は物理的に無意味なため 0 にクランプする。
pub struct SetNodeSupportSpring {
    pub node: NodeId,
    pub spring: Option<[f64; 6]>,
}

impl EditCommand for SetNodeSupportSpring {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.node.index();
        if idx >= model.nodes.len() || model.nodes[idx].id != self.node {
            return Box::new(Noop);
        }
        let old = model.nodes[idx].support_spring;
        let clamped = self.spring.map(|s| s.map(|v| v.max(0.0)));
        model.nodes[idx].support_spring = clamped;
        Box::new(SetNodeSupportSpring {
            node: self.node,
            spring: old,
        })
    }

    fn label(&self) -> &str {
        "支点ばね変更"
    }
}

/// 節点追加。末尾に `NodeId(len)` で追加する（ID＝配列インデックスの不変条件を維持）。
/// 逆操作は節点削除。
pub struct AddNode {
    pub coord: [f64; 3],
    pub restraint: sepika_core::dof::Dof6Mask,
}

impl EditCommand for AddNode {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let new_id = NodeId(model.nodes.len() as u32);
        if let Err(reason) = model.assign_stb_node_ids() {
            return Box::new(RejectedEdit(reason));
        }
        model.nodes.push(sepika_core::model::Node {
            id: new_id,
            coord: self.coord,
            restraint: self.restraint,
            mass: None,
            story: None,
            support_spring: None,
        });
        if let Err(reason) = model.assign_stb_node_ids() {
            return Box::new(RejectedEdit(reason));
        }
        Box::new(DeleteNode { id: new_id })
    }

    fn label(&self) -> &str {
        "節点追加"
    }
}

/// 節点削除（末尾以外の中間節点も可）。逆操作は [`InsertNode`]。
/// 削除後は ID を繰り上げる。参照されている節点は Noop とする。
pub struct DeleteNode {
    pub id: NodeId,
}

impl EditCommand for DeleteNode {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.id.index();
        if idx >= model.nodes.len() || model.nodes[idx].id != self.id {
            return Box::new(Noop);
        }
        if model.elements.iter().any(|e| e.nodes.contains(&self.id))
            || model.node_referenced_by_regions_or_plates(self.id)
        {
            return Box::new(Noop);
        }
        let seismic_weight_generation = model.seismic_weight_generation.clone();
        if let Some(record) = &mut model.seismic_weight_generation {
            record.retain_node_references(|node| node != self.id);
        }
        let source_stories = model.source_stories.clone();
        let stb_node_ids = model.stb_node_ids.clone();
        let story_membership = model.stories.iter().map(|s| s.node_ids.clone()).collect();
        for story in &mut model.source_stories {
            story.node_ids.retain(|n| n.node != Some(self.id));
        }
        model.stb_node_ids.retain(|n| n.node != self.id);
        for story in &mut model.stories {
            story.node_ids.retain(|n| *n != self.id);
        }
        let generated_master =
            if let Some(pos) = model.generated_masters.iter().position(|n| *n == self.id) {
                model.generated_masters.remove(pos);
                true
            } else {
                false
            };
        let mut axis_membership = Vec::new();
        for (gi, group) in model.axes.iter_mut().enumerate() {
            for (ai, axis) in group.axes.iter_mut().enumerate() {
                if let Some(pos) = axis.nodes.iter().position(|n| *n == self.id) {
                    axis.nodes.remove(pos);
                    axis_membership.push((gi, ai));
                }
            }
        }
        let removed = model.nodes.remove(idx);
        shift_node_ids(model, |id| {
            if id.0 > self.id.0 {
                id.0 -= 1;
            }
        });
        Box::new(InsertNode {
            seismic_weight_generation,
            index: idx,
            coord: removed.coord,
            restraint: removed.restraint,
            mass: removed.mass,
            story: removed.story,
            support_spring: removed.support_spring,
            generated_master,
            axis_membership,
            source_stories,
            stb_node_ids,
            story_membership,
        })
    }

    fn label(&self) -> &str {
        "節点削除"
    }
}

/// 指定インデックスへ節点を再挿入する（[`DeleteNode`] の逆操作専用）。
pub struct InsertNode {
    pub seismic_weight_generation: Option<sepika_core::model::SeismicWeightGeneration>,
    pub source_stories: Vec<sepika_core::model::SourceStory>,
    pub stb_node_ids: Vec<sepika_core::model::StbNodeIdentity>,
    pub story_membership: Vec<Vec<NodeId>>,
    pub index: usize,
    pub coord: [f64; 3],
    pub restraint: sepika_core::dof::Dof6Mask,
    pub mass: Option<[f64; 6]>,
    pub story: Option<sepika_core::ids::StoryId>,
    pub support_spring: Option<[f64; 6]>,
    pub generated_master: bool,
    pub axis_membership: Vec<(usize, usize)>,
}

impl EditCommand for InsertNode {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let id = NodeId(self.index as u32);
        shift_node_ids(model, |nid| {
            if nid.0 >= id.0 {
                nid.0 += 1;
            }
        });
        model.nodes.insert(
            self.index,
            sepika_core::model::Node {
                id,
                coord: self.coord,
                restraint: self.restraint,
                mass: self.mass,
                story: self.story,
                support_spring: self.support_spring,
            },
        );
        if self.generated_master {
            model.generated_masters.push(id);
            model.generated_masters.sort();
        }
        for &(gi, ai) in &self.axis_membership {
            if let Some(axis) = model.axes.get_mut(gi).and_then(|g| g.axes.get_mut(ai)) {
                let pos = axis.nodes.partition_point(|n| *n < id);
                axis.nodes.insert(pos, id);
            }
        }
        model.seismic_weight_generation = self.seismic_weight_generation.clone();
        model.source_stories = self.source_stories.clone();
        model.stb_node_ids = self.stb_node_ids.clone();
        for (story, nodes) in model.stories.iter_mut().zip(&self.story_membership) {
            story.node_ids = nodes.clone();
        }
        Box::new(DeleteNode { id })
    }

    fn label(&self) -> &str {
        "節点削除の取り消し"
    }
}

/// モデル内の全ての `NodeId` 参照（節点自身の ID を含む）に `f` を適用する。
/// [`DeleteNode`]／[`InsertNode`] の ID 繰り上げ・繰り下げで共用する。
/// 走査そのものはフィールド定義と同じ core 側（[`Model::visit_node_ids`]）が
/// 単一情報源として持つ（新フィールド追加時の追随漏れを防ぐ）。
fn shift_node_ids(model: &mut Model, f: impl FnMut(&mut NodeId)) {
    model.visit_node_ids(f);
}

/// 部材追加。逆操作は部材削除。
///
/// `elem.id` は `ElemId(model.elements.len())`（末尾の次の添字）と一致し、参照する
/// 節点・断面が実在していること（crate::refs の規約）。満たさない場合は `Noop`。
pub struct AddMember {
    pub elem: sepika_core::model::ElementData,
}

impl EditCommand for AddMember {
    fn changes_assignment_boundaries(&self) -> bool {
        true
    }

    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut candidate = model.clone();
        let inverse = self.apply_candidate(&mut candidate);
        if inverse.rejection().is_none() && !inverse.is_noop() {
            let report = candidate.rebuild_assignment_regions_dropping_orphan_plates();
            if let Some(reason) = report.floor.rejection.or(report.wall.rejection) {
                return Box::new(RejectedEdit(reason));
            }
            *model = candidate;
        }
        inverse
    }

    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Err(reason) = model.validate_assignment_region_identity() {
            return Box::new(RejectedEdit(reason));
        }
        if !crate::refs::new_elem_ok(model, &self.elem)
            || !crate::refs::frame_element_section_ref_ok(model, &self.elem, self.elem.section)
        {
            return Box::new(Noop);
        }
        let snapshot = AssignmentTopology::capture(model);
        model.elements.push(self.elem.clone());
        Box::new(RestoreAssignmentTopology {
            snapshot,
            inverse: Box::new(DeleteMember { id: self.elem.id }),
        })
    }

    fn label(&self) -> &str {
        "部材追加"
    }
}

/// モデル末尾の部材を除去する（部材を末尾へ追加するコマンドの逆操作）。
/// `elems` の件数分だけ末尾から取り除く（生成直後の undo を想定し、末尾＝生成分）。
/// 逆操作は [`PushTailMembers`]（同じ部材の末尾再追加）。
pub struct PopTailMembers {
    pub elems: Vec<sepika_core::model::ElementData>,
}

impl EditCommand for PopTailMembers {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let k = self.elems.len();
        let start = model.elements.len().saturating_sub(k);
        let removed: Vec<_> = model.elements.split_off(start);
        Box::new(PushTailMembers { elems: removed })
    }

    fn label(&self) -> &str {
        "実部材化の取り消し"
    }
}

/// モデル末尾へ部材を再追加する（[`PopTailMembers`] の逆操作）。
pub struct PushTailMembers {
    pub elems: Vec<sepika_core::model::ElementData>,
}

impl EditCommand for PushTailMembers {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        for e in &self.elems {
            model.elements.push(e.clone());
        }
        Box::new(PopTailMembers {
            elems: self.elems.clone(),
        })
    }

    fn label(&self) -> &str {
        "実部材化の再適用"
    }
}

/// 制振ダンパー要素の追加（制振部材の力学モデル: Maxwell モデル等）。
/// 要素（`ElementKind::Damper`）と特性（`Model::damper_attrs`）を原子的に追加する。
/// 逆操作は部材削除（`DeleteMember` が側テーブル属性も退避・復元する）。
///
/// `elem` の ID・節点・断面の要件は [`AddMember`] と同じ（crate::refs の規約）。
pub struct AddDamper {
    pub elem: sepika_core::model::ElementData,
    pub props: sepika_core::model::DamperProps,
}

impl EditCommand for AddDamper {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if !crate::refs::new_elem_ok(model, &self.elem)
            || !crate::refs::frame_element_section_ref_ok(model, &self.elem, self.elem.section)
        {
            return Box::new(Noop);
        }
        let id = self.elem.id;
        model.elements.push(self.elem.clone());
        model.set_damper_props(id, Some(self.props));
        Box::new(DeleteMember { id })
    }

    fn label(&self) -> &str {
        "制振ダンパー追加"
    }
}

/// 免震支承材要素の追加（各免震部材指針）。
/// 要素（`ElementKind::Isolator`）と特性（`Model::isolator_attrs`）を原子的に追加する。
/// 逆操作は部材削除（`DeleteMember` が側テーブル属性も退避・復元する）。
///
/// `elem` の ID・節点・断面の要件は [`AddMember`] と同じ（crate::refs の規約）。
pub struct AddIsolator {
    pub elem: sepika_core::model::ElementData,
    pub props: sepika_core::model::IsolatorProps,
}

impl EditCommand for AddIsolator {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if !crate::refs::new_elem_ok(model, &self.elem)
            || !crate::refs::frame_element_section_ref_ok(model, &self.elem, self.elem.section)
        {
            return Box::new(Noop);
        }
        let id = self.elem.id;
        model.elements.push(self.elem.clone());
        model.isolator_attrs.push(sepika_core::model::IsolatorAttr {
            elem: id,
            props: self.props,
        });
        Box::new(DeleteMember { id })
    }

    fn label(&self) -> &str {
        "免震支承材追加"
    }
}

/// 支点への免震装置の設置（既存の運用: 基礎節点↔上部節点間に零長 Isolator 要素）。
///
/// 対象節点 `node` と同一座標に接地節点（`restraint=FIXED`）を新規作成し、
/// その2節点間に零長 [`ElementKind::Isolator`](sepika_core::model::ElementKind::Isolator)
/// 要素＋ [`IsolatorAttr`](sepika_core::model::IsolatorAttr) を追加した上で、対象節点
/// 自身の `restraint` を `FREE` に変更する（免震装置を介して支持されるため、
/// 対象節点はもはや直接の固定支点ではない）。
///
/// 要素の節点順は `[接地節点, 対象節点]`（i端=接地/下端、j端=対象/上端）とする。
/// `element/src/springs/isolator.rs` の零長特例（2節点が同一座標の場合、局所 x 軸＝
/// 全体座標系の鉛直方向、節点0→節点1 の向き）に整合する。
///
/// 逆操作（[`UndoPlaceSupportIsolator`]）は生成した接地節点・Isolator 要素（＋属性）を
/// 削除し、対象節点の `restraint` を元へ戻す。要素削除を節点削除より先に行う
/// （`node_in_use` は要素が参照している間、節点の削除を拒否するため）。
pub struct PlaceSupportIsolator {
    pub node: NodeId,
    pub props: sepika_core::model::IsolatorProps,
}

impl EditCommand for PlaceSupportIsolator {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.node.index();
        if idx >= model.nodes.len() || model.nodes[idx].id != self.node {
            return Box::new(Noop);
        }
        let coord = model.nodes[idx].coord;
        let old_restraint = model.nodes[idx].restraint;

        let ground_id = NodeId(model.nodes.len() as u32);
        model.nodes.push(sepika_core::model::Node {
            id: ground_id,
            coord,
            restraint: sepika_core::dof::Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });

        let elem_id = ElemId(model.elements.len() as u32);
        model.elements.push(sepika_core::model::ElementData {
            id: elem_id,
            kind: sepika_core::model::ElementKind::Isolator,
            nodes: [ground_id, self.node].into_iter().collect(),
            section: None,
            local_axis: sepika_core::model::LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [
                sepika_core::model::EndCondition::Fixed,
                sepika_core::model::EndCondition::Fixed,
            ],
            force_regime: sepika_core::model::ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
        model.isolator_attrs.push(sepika_core::model::IsolatorAttr {
            elem: elem_id,
            props: self.props,
        });

        model.nodes[idx].restraint = sepika_core::dof::Dof6Mask::FREE;

        Box::new(UndoPlaceSupportIsolator {
            node: self.node,
            props: self.props,
            old_restraint,
            ground_node: ground_id,
            elem: elem_id,
        })
    }

    fn label(&self) -> &str {
        "支点免震装置の設置"
    }
}

/// [`PlaceSupportIsolator`] の逆操作。
pub struct UndoPlaceSupportIsolator {
    node: NodeId,
    props: sepika_core::model::IsolatorProps,
    old_restraint: sepika_core::dof::Dof6Mask,
    ground_node: NodeId,
    elem: ElemId,
}

impl EditCommand for UndoPlaceSupportIsolator {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.node.index();
        if idx >= model.nodes.len() || model.nodes[idx].id != self.node {
            return Box::new(Noop);
        }
        model.nodes[idx].restraint = self.old_restraint;
        DeleteMember { id: self.elem }.apply(model);
        DeleteNode {
            id: self.ground_node,
        }
        .apply(model);
        Box::new(PlaceSupportIsolator {
            node: self.node,
            props: self.props,
        })
    }

    fn label(&self) -> &str {
        "支点免震装置の設置の取り消し"
    }
}

/// [`PlaceSupportIsolator`] で配置した支点免震要素の撤去（単体削除）。
/// 撤去後の拘束は常に `FIXED` に統一する（設置前の拘束は復元しない）。
pub struct RemoveSupportIsolator {
    pub node: NodeId,
}

impl EditCommand for RemoveSupportIsolator {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let found = model.elements.iter().find_map(|e| {
            model
                .support_isolator_ends(e.id)
                .filter(|(upper, _)| *upper == self.node)
                .map(|(_, ground)| (e.id, ground))
        });
        let Some((elem_id, ground)) = found else {
            return Box::new(Noop);
        };
        let idx = self.node.index();
        if idx >= model.nodes.len() || model.nodes[idx].id != self.node {
            return Box::new(Noop);
        }
        let old_restraint = model.nodes[idx].restraint;

        let undo_member = DeleteMember { id: elem_id }.apply(model);
        let undo_node = DeleteNode { id: ground }.apply(model);

        let idx = self.node.index();
        if idx < model.nodes.len() && model.nodes[idx].id == self.node {
            model.nodes[idx].restraint = sepika_core::dof::Dof6Mask::FIXED;
        }

        Box::new(UndoRemoveSupportIsolator {
            node: self.node,
            old_restraint,
            undo_node,
            undo_member,
        })
    }

    fn label(&self) -> &str {
        "支点免震装置の撤去"
    }
}

/// [`RemoveSupportIsolator`] の逆操作。
struct UndoRemoveSupportIsolator {
    node: NodeId,
    old_restraint: sepika_core::dof::Dof6Mask,
    undo_node: Box<dyn EditCommand>,
    undo_member: Box<dyn EditCommand>,
}

impl EditCommand for UndoRemoveSupportIsolator {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        self.undo_node.apply(model);
        self.undo_member.apply(model);
        let idx = self.node.index();
        if idx < model.nodes.len() && model.nodes[idx].id == self.node {
            model.nodes[idx].restraint = self.old_restraint;
        }
        Box::new(RemoveSupportIsolator { node: self.node })
    }

    fn label(&self) -> &str {
        "支点免震装置の撤去の取り消し"
    }
}

/// 部材削除（中間の部材も可）。逆操作は [`InsertMember`]。
pub struct DeleteMember {
    pub id: ElemId,
}

impl EditCommand for DeleteMember {
    fn changes_assignment_boundaries(&self) -> bool {
        true
    }

    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut candidate = model.clone();
        let inverse = self.apply_candidate(&mut candidate);
        if inverse.rejection().is_none() && !inverse.is_noop() {
            let report = candidate.rebuild_assignment_regions_dropping_orphan_plates();
            if let Some(reason) = report.floor.rejection.or(report.wall.rejection) {
                return Box::new(RejectedEdit(reason));
            }
            *model = candidate;
        }
        inverse
    }

    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Err(reason) = model.validate_assignment_region_identity() {
            return Box::new(RejectedEdit(reason));
        }
        let idx = self.id.index();
        if idx >= model.elements.len() || model.elements[idx].id != self.id {
            return Box::new(Noop);
        }
        let snapshot = AssignmentTopology::capture(model);
        let mut removed_loads = Vec::new();
        for (lci, lc) in model.load_cases.iter_mut().enumerate() {
            let mut li = 0;
            while li < lc.member.len() {
                if lc.member[li].elem == self.id {
                    removed_loads.push((lci, li, lc.member.remove(li)));
                } else {
                    li += 1;
                }
            }
        }
        let removed_attrs = model.take_elem_attrs(self.id);
        let mut removed_group_refs = Vec::new();
        for (gi, group) in model.girder_groups.iter_mut().enumerate() {
            let mut pos = 0;
            while pos < group.len() {
                if group[pos] == self.id {
                    group.remove(pos);
                    removed_group_refs.push((gi, pos));
                } else {
                    pos += 1;
                }
            }
        }
        model
            .stb_strengths
            .members
            .retain(|m| m.target != sepika_core::model::StrengthTarget::Element(self.id));
        let removed = model.elements.remove(idx);
        shift_elem_ids(model, |id| {
            if id.0 > self.id.0 {
                id.0 -= 1;
            }
        });
        Box::new(RestoreAssignmentTopology {
            snapshot,
            inverse: Box::new(InsertMember {
                index: idx,
                elem: removed,
                member_loads: removed_loads,
                elem_attrs: removed_attrs,
                girder_group_refs: removed_group_refs,
            }),
        })
    }

    fn label(&self) -> &str {
        "部材削除"
    }
}

/// 指定インデックスへ部材を再挿入する（[`DeleteMember`] の逆操作専用）。
pub struct InsertMember {
    pub index: usize,
    pub elem: sepika_core::model::ElementData,
    /// (荷重ケース index, 荷重 index, 内容)
    pub member_loads: Vec<(usize, usize, sepika_core::model::MemberLoad)>,
    /// 削除時に退避した側テーブル属性。
    pub elem_attrs: sepika_core::model::ElemAttrs,
    /// 削除時に一本部材指定から外した参照の (グループ index, グループ内位置)。
    pub girder_group_refs: Vec<(usize, usize)>,
}

impl EditCommand for InsertMember {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if self.index > model.elements.len() {
            return Box::new(Noop);
        }
        let id = ElemId(self.index as u32);
        shift_elem_ids(model, |eid| {
            if eid.0 >= id.0 {
                eid.0 += 1;
            }
        });
        let mut elem = self.elem.clone();
        elem.id = id;
        model.elements.insert(self.index, elem);
        for (lci, li, load) in self.member_loads.iter().rev() {
            if let Some(lc) = model.load_cases.get_mut(*lci) {
                let pos = (*li).min(lc.member.len());
                lc.member.insert(pos, load.clone());
            }
        }
        model.restore_elem_attrs(id, self.elem_attrs.clone());
        for &(gi, pos) in self.girder_group_refs.iter().rev() {
            if let Some(group) = model.girder_groups.get_mut(gi) {
                group.insert(pos.min(group.len()), id);
            }
        }
        Box::new(DeleteMember { id })
    }

    fn label(&self) -> &str {
        "部材削除の取り消し"
    }
}

/// モデル内の全ての `ElemId` 参照に `f` を適用する。
fn shift_elem_ids(model: &mut Model, f: impl FnMut(&mut ElemId)) {
    model.visit_elem_ids(f);
}

/// 何もしないコマンド。
pub struct Noop;

impl EditCommand for Noop {
    fn apply(&self, _model: &mut Model) -> Box<dyn EditCommand> {
        Box::new(Noop)
    }

    fn label(&self) -> &str {
        "Noop"
    }

    fn is_noop(&self) -> bool {
        true
    }
}
