//! 節点・部材の編集コマンド。

use super::*;
use sepika_core::ids::*;
use sepika_core::model::{
    FloorPlateAssignmentRegions, FloorRegion, SecondaryMember, SecondaryMemberEnds, Slab,
    SupportMemberId, WallPlate, WallPlateAssignmentRegions, WallRegion,
};

/// 位相（節点座標・部材）を変えるコマンドが割当領域を再構築したときの undo 用
/// スナップショット。孤児化した版の除去までを 1 つの Undo 単位に含めるため、
/// 再構築が変更しうる派生データ（床領域・壁領域・割当領域・版）を退避する。
#[derive(Clone)]
struct AssignmentTopology {
    src_joint_hoops: Vec<sepika_core::model::SrcJointHoopAssignment>,
    floor_regions: Vec<FloorRegion>,
    wall_regions: Vec<WallRegion>,
    floor_assignment_regions: FloorPlateAssignmentRegions,
    wall_assignment_regions: WallPlateAssignmentRegions,
    slabs: Vec<Slab>,
    wall_plates: Vec<WallPlate>,
    unassigned_beams: Vec<SecondaryMember>,
    unassigned_posts: Vec<SecondaryMember>,
}

impl AssignmentTopology {
    fn capture(model: &Model) -> Self {
        Self {
            src_joint_hoops: model.src_joint_hoops.clone(),
            floor_regions: model.floor_regions.clone(),
            wall_regions: model.wall_regions.clone(),
            floor_assignment_regions: model.floor_assignment_regions.clone(),
            wall_assignment_regions: model.wall_assignment_regions.clone(),
            slabs: model.slabs.clone(),
            wall_plates: model.wall_plates.clone(),
            unassigned_beams: model.unassigned_beams.clone(),
            unassigned_posts: model.unassigned_posts.clone(),
        }
    }

    fn restore(self, model: &mut Model) {
        model.src_joint_hoops = self.src_joint_hoops;
        model.floor_regions = self.floor_regions;
        model.wall_regions = self.wall_regions;
        model.floor_assignment_regions = self.floor_assignment_regions;
        model.wall_assignment_regions = self.wall_assignment_regions;
        model.slabs = self.slabs;
        model.wall_plates = self.wall_plates;
        model.unassigned_beams = self.unassigned_beams;
        model.unassigned_posts = self.unassigned_posts;
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

    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let redo = self.inverse.apply_candidate(model);
        self.snapshot.clone().restore(model);
        redo
    }

    fn apply_in_transaction(&self, before: &Model, model: &mut Model) -> Box<dyn EditCommand> {
        let redo = self.inverse.apply_in_transaction(before, model);
        self.snapshot.clone().restore(model);
        redo
    }

    fn label(&self) -> &str {
        self.inverse.label()
    }
}

/// 節点座標 `[mm]` と全長追従の区間を変更する。Manual荷重を保持できない最終座標は編集全体を拒否する。
pub struct SetNodeCoord {
    pub node: NodeId,
    pub coord: [f64; 3],
}

pub(crate) fn validate_coordinate_loads(before: &Model, after: &Model) -> Result<(), String> {
    use sepika_core::model::LoadSource;
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
            if let Err(reason) = load.validate_extent(length_mm) {
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
        let inverse = self.apply_in_transaction(model, &mut candidate);
        if inverse.rejection().is_none() && !inverse.is_noop() {
            *model = candidate;
        }
        inverse
    }

    fn apply_in_transaction(&self, before: &Model, model: &mut Model) -> Box<dyn EditCommand> {
        let inverse = self.apply_candidate(model);
        if inverse.is_noop() || inverse.rejection().is_some() {
            return inverse;
        }
        if let Err(reason) = validate_coordinate_loads(before, model) {
            return Box::new(RejectedEdit(reason));
        }
        let report = model.rebuild_assignment_regions_dropping_orphan_plates();
        if let Some(reason) = report.floor.rejection.or(report.wall.rejection) {
            return Box::new(RejectedEdit(reason));
        }
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
        model.nodes.push(sepika_core::model::Node {
            id: new_id,
            coord: self.coord,
            restraint: self.restraint,
            mass: None,
            story: None,
            support_spring: None,
        });
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
        let story_membership = model.stories.iter().map(|s| s.node_ids.clone()).collect();
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
        let src_joint_hoops = model.src_joint_hoops.clone();
        model.src_joint_hoops.retain(|entry| entry.node != self.id);
        let removed = model.nodes.remove(idx);
        shift_node_ids(model, |id| {
            if id.0 > self.id.0 {
                id.0 -= 1;
            }
        });
        Box::new(InsertNode {
            seismic_weight_generation,
            src_joint_hoops,
            index: idx,
            coord: removed.coord,
            restraint: removed.restraint,
            mass: removed.mass,
            story: removed.story,
            support_spring: removed.support_spring,
            generated_master,
            axis_membership,
            story_membership,
        })
    }

    fn label(&self) -> &str {
        "節点削除"
    }
}

/// 指定インデックスへ節点を再挿入する（[`DeleteNode`] の逆操作専用）。
pub struct InsertNode {
    pub src_joint_hoops: Vec<sepika_core::model::SrcJointHoopAssignment>,
    pub seismic_weight_generation: Option<sepika_core::model::SeismicWeightGeneration>,
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
        model.src_joint_hoops = self.src_joint_hoops.clone();
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
/// 座屈拘束ブレースの検定属性に参照先欠落があれば、追加を拒否する。
pub struct AddMember {
    pub elem: sepika_core::model::ElementData,
}

impl EditCommand for AddMember {
    fn changes_assignment_boundaries(&self) -> bool {
        true
    }

    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut candidate = model.clone();
        let inverse = self.apply_in_transaction(model, &mut candidate);
        if inverse.rejection().is_none() && !inverse.is_noop() {
            *model = candidate;
        }
        inverse
    }

    fn apply_in_transaction(&self, _before: &Model, model: &mut Model) -> Box<dyn EditCommand> {
        let inverse = self.apply_candidate(model);
        if inverse.rejection().is_none() && !inverse.is_noop() {
            let report = model.rebuild_assignment_regions_dropping_orphan_plates();
            if let Some(reason) = report.floor.rejection.or(report.wall.rejection) {
                return Box::new(RejectedEdit(reason));
            }
        }
        inverse
    }

    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Err(reason) = model.validate_assignment_region_identity() {
            return Box::new(RejectedEdit(reason));
        }
        if let Err(error) = self.elem.validate_frame_node_count() {
            return Box::new(RejectedEdit(error.to_string()));
        }
        if let Some(reason) =
            crate::refs::rc_element_section_error(model, &self.elem, self.elem.section)
        {
            return Box::new(RejectedEdit(reason));
        }
        if !crate::refs::new_elem_ok(model, &self.elem)
            || !crate::refs::frame_element_section_ref_ok(model, &self.elem, self.elem.section)
        {
            return Box::new(Noop);
        }
        if let Err(error) = model.validate_brb_references() {
            return Box::new(RejectedEdit(error.to_string()));
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
/// `elems` の件数分だけ末尾から取り除く。対象に BRB 検定属性があれば除去を拒否する。
/// 逆操作は [`PushTailMembers`]（同じ部材の末尾再追加）。
pub struct PopTailMembers {
    pub elems: Vec<sepika_core::model::ElementData>,
}

impl EditCommand for PopTailMembers {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let k = self.elems.len();
        let start = model.elements.len().saturating_sub(k);
        if let Some((index, attr)) = model.brb_attrs.iter().enumerate().find(|(_, attr)| {
            model.elements[start..]
                .iter()
                .any(|elem| elem.id == attr.elem)
        }) {
            return Box::new(RejectedEdit(format!(
                "部材 ID {} に座屈拘束ブレースの検定属性 brb_attrs[{index}] があるため、末尾部材を除去できません",
                attr.elem.0
            )));
        }
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
        if let Err(error) = model.validate_brb_references() {
            return Box::new(RejectedEdit(error.to_string()));
        }
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
        if let Some(reason) =
            crate::refs::rc_element_section_error(model, &self.elem, self.elem.section)
        {
            return Box::new(RejectedEdit(reason));
        }
        if !crate::refs::new_elem_ok(model, &self.elem)
            || !crate::refs::frame_element_section_ref_ok(model, &self.elem, self.elem.section)
        {
            return Box::new(Noop);
        }
        if let Err(error) = model.validate_brb_references() {
            return Box::new(RejectedEdit(error.to_string()));
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

/// 部材削除（中間の部材も可）。逆操作は [`InsertMember`]。
pub struct DeleteMember {
    pub id: ElemId,
}

fn detached_secondary_ends(
    model: &Model,
    deleted: ElemId,
) -> Result<Vec<(SecondaryMemberId, SecondaryMemberEnds)>, String> {
    let mut lost_supports = std::collections::HashSet::from([SupportMemberId::Primary(deleted)]);
    loop {
        let before = lost_supports.len();
        for member in model.beams().chain(model.posts()) {
            if member
                .ends
                .anchors()
                .iter()
                .any(|a| lost_supports.contains(&a.support))
            {
                lost_supports.insert(SupportMemberId::Secondary(member.id));
            }
        }
        if lost_supports.len() == before {
            break;
        }
    }
    model
        .beams()
        .chain(model.posts())
        .filter(|member| lost_supports.contains(&SupportMemberId::Secondary(member.id)))
        .map(|member| {
            let (a, b) = model
                .secondary_member_end_points(member)
                .filter(|(a, b)| a.iter().chain(b).all(|v| v.is_finite()))
                .ok_or_else(|| {
                    format!(
                        "部材 {} の削除で支持を失う二次部材 {} の端点座標を解決できません",
                        deleted.0, member.id.0
                    )
                })?;
            Ok((member.id, SecondaryMemberEnds::Detached([a, b])))
        })
        .collect()
}

impl EditCommand for DeleteMember {
    fn changes_assignment_boundaries(&self) -> bool {
        true
    }

    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut candidate = model.clone();
        let inverse = self.apply_in_transaction(model, &mut candidate);
        if inverse.rejection().is_none() && !inverse.is_noop() {
            *model = candidate;
        }
        inverse
    }

    fn apply_in_transaction(&self, _before: &Model, model: &mut Model) -> Box<dyn EditCommand> {
        let inverse = self.apply_candidate(model);
        if inverse.rejection().is_none() && !inverse.is_noop() {
            let report = model.rebuild_assignment_regions_dropping_orphan_plates();
            if let Some(reason) = report.floor.rejection.or(report.wall.rejection) {
                return Box::new(RejectedEdit(reason));
            }
        }
        inverse
    }

    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if model
            .stress_cfg
            .misc_wall_representatives
            .iter()
            .any(|entry| entry.column == self.id)
        {
            return Box::new(crate::RejectedEdit(
                "自立壁の代表内柱に指定された柱です。指定を解除してから削除してください".into(),
            ));
        }
        if let Err(reason) = model.validate_assignment_region_identity() {
            return Box::new(RejectedEdit(reason));
        }
        let idx = self.id.index();
        if idx >= model.elements.len() || model.elements[idx].id != self.id {
            return Box::new(Noop);
        }
        let detached = match detached_secondary_ends(model, self.id) {
            Ok(detached) => detached,
            Err(reason) => return Box::new(RejectedEdit(reason)),
        };
        let snapshot = AssignmentTopology::capture(model);
        let mut lost_supports =
            std::collections::HashSet::from([SupportMemberId::Primary(self.id)]);
        for (id, ends) in detached {
            lost_supports.insert(SupportMemberId::Secondary(id));
            if let Some(member) = crate::secondary::find_secondary_mut(model, id) {
                member.ends = ends;
            }
        }
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
        model.floor_assignment_regions.regions.retain(|region| {
            !region
                .boundary
                .iter()
                .any(|edge| lost_supports.contains(&edge.support))
        });
        model.wall_assignment_regions.regions.retain(|region| {
            !region
                .boundary
                .iter()
                .any(|edge| lost_supports.contains(&edge.support))
        });
        for assignment in &mut model.src_joint_hoops {
            if assignment.columns.contains(&self.id) || assignment.beams.contains(&self.id) {
                assignment.columns.clear();
                assignment.beams.clear();
            }
        }
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
