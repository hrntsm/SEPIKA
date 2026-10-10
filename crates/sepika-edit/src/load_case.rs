//! 荷重ケース・荷重組み合わせ・階・スラブの編集コマンド。

use super::*;
use sepika_core::ids::*;

/// 荷重ケース追加。末尾に `LoadCaseId(len)` で追加する。逆操作は荷重ケース削除。
pub struct AddLoadCase {
    pub name: String,
}

impl EditCommand for AddLoadCase {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let new_id = LoadCaseId(model.load_cases.len() as u32);
        model.load_cases.push(sepika_core::model::LoadCase {
            kind: Default::default(),
            id: new_id,
            name: self.name.clone(),
            nodal: Vec::new(),
            member: Vec::new(),
        });
        Box::new(DeleteLoadCase { id: new_id })
    }

    fn label(&self) -> &str {
        "荷重ケース追加"
    }
}

/// 荷重ケース削除。先端荷重の参照を取り除き、残るケース ID を詰める。
pub struct DeleteLoadCase {
    pub id: LoadCaseId,
}

impl EditCommand for DeleteLoadCase {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.id.index();
        if model.load_cases.get(idx).map(|case| case.id) != Some(self.id)
            || load_case_in_use(model, self.id)
        {
            return Box::new(Noop);
        }
        let tip_loads: Vec<_> = model
            .slabs
            .iter()
            .map(|slab| slab.tip_loads.clone())
            .collect();
        for slab in &mut model.slabs {
            slab.tip_loads.retain(|load| load.case != self.id);
        }
        let old = model.load_cases.remove(idx);
        shift_load_case_ids(model, |id| {
            if id.0 > self.id.0 {
                id.0 -= 1;
            }
        });
        Box::new(InsertLoadCase {
            index: idx,
            old,
            tip_loads,
        })
    }

    fn label(&self) -> &str {
        "荷重ケース削除"
    }
}

/// 荷重ケース削除の Undo。先端荷重の設定も元に戻す。
pub struct InsertLoadCase {
    pub index: usize,
    pub old: sepika_core::model::LoadCase,
    pub tip_loads: Vec<Vec<sepika_core::model::SlabTipLoad>>,
}

impl EditCommand for InsertLoadCase {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if self.index > model.load_cases.len() || self.tip_loads.len() != model.slabs.len() {
            return Box::new(Noop);
        }
        let id = LoadCaseId(self.index as u32);
        shift_load_case_ids(model, |case| {
            if case.0 >= id.0 {
                case.0 += 1;
            }
        });
        for (slab, loads) in model.slabs.iter_mut().zip(&self.tip_loads) {
            slab.tip_loads = loads.clone();
        }
        let mut old = self.old.clone();
        old.id = id;
        model.load_cases.insert(self.index, old);
        Box::new(DeleteLoadCase { id })
    }

    fn label(&self) -> &str {
        "荷重ケース削除の取り消し"
    }
}

/// 指定荷重ケースを参照している荷重組合せが存在するか（削除ガード用）。
fn load_case_in_use(model: &Model, id: LoadCaseId) -> bool {
    model
        .combinations
        .iter()
        .any(|c| c.terms.iter().any(|(lc, _)| *lc == id))
}

/// モデル内の全ての `LoadCaseId` 参照（ケース自身の ID を含む）に `f` を適用する。
fn shift_load_case_ids(model: &mut Model, mut f: impl FnMut(&mut LoadCaseId)) {
    for lc in &mut model.load_cases {
        f(&mut lc.id);
    }
    for combo in &mut model.combinations {
        for (lcid, _) in &mut combo.terms {
            f(lcid);
        }
    }
    for slab in &mut model.slabs {
        for load in &mut slab.tip_loads {
            f(&mut load.case);
        }
    }
}

/// 荷重組合せ追加。末尾に追加する。逆操作は末尾の組合せ削除。
///
/// `LoadCombination` は ID を持たず配列インデックスのみで管理されるため、
/// 他の追加系コマンド（[`AddLoadCase`] 等）と異なり ID 採番は発生しない。
/// 参照する `LoadCaseId` の存在チェックは行わない（[`Model::validate`] も
/// 組合せの `LoadCaseId` 参照はダングリングチェックの対象外であり、既存の
/// [`DeleteLoadCase`] が参照側で削除を防ぐことで整合性を保っている）。
pub struct AddCombination {
    pub combo: sepika_core::model::LoadCombination,
}

impl EditCommand for AddCombination {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        model.combinations.push(self.combo.clone());
        let index = model.combinations.len() - 1;
        Box::new(DeleteCombination { index })
    }

    fn label(&self) -> &str {
        "荷重組合せ追加"
    }
}

indexed_delete_insert!(
    /// 荷重組合せを index 指定で削除。逆操作は [`InsertCombination`]（同じ位置への復元）。
    /// 組合せは他のデータから参照されないため ID 再採番は不要。index が範囲外なら Noop。
    DeleteCombination,
    /// 指定インデックスへ荷重組合せを再挿入する（[`DeleteCombination`] の逆操作専用）。
    InsertCombination,
    entity = sepika_core::model::LoadCombination,
    vec = combinations,
    field = combo,
    del_label = "荷重組合せ削除",
    ins_label = "荷重組合せ削除の取り消し",
);

/// 階定義の一括適用（階自動生成の結果を反映する）。
///
/// `model.stories`・各節点の所属階・剛床拘束(`Constraint::RigidDiaphragm`)を
/// まとめて差し替える。既存の RigidDiaphragm 拘束は除去し、Mpc / RigidLink は
/// 保持する。逆操作は差し替え前の状態の復元。
pub struct ApplyStories {
    pub wall_weight_generation: sepika_core::model::WallWeightGenerationMode,
    pub damper_mass_generation: sepika_core::model::DamperMassGeneration,
    pub stories: Vec<sepika_core::model::Story>,
    /// `model.nodes` と同順の所属階。長さが合わない分は無視する。
    pub node_story: Vec<Option<sepika_core::ids::StoryId>>,
    /// 追加する剛床拘束（既存の RigidDiaphragm と置換）。
    pub constraints: Vec<sepika_core::model::Constraint>,
    /// 剛床代表節点。ID が既存範囲内なら置換（再利用）、範囲外（＝末尾連番）なら追加。
    pub rep_nodes: Vec<sepika_core::model::Node>,
    /// 適用後の `model.generated_masters` の全量。
    pub generated_masters: Vec<NodeId>,
    /// 適用する動的解析の質量方式（[`sepika_core::model::MassMethod`]）。
    /// `rep_nodes` の質点質量はこの方式で算定済みの前提（呼び出し側の責務）。
    pub mass_method: sepika_core::model::MassMethod,
}

impl EditCommand for ApplyStories {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        use sepika_core::model::Constraint;
        let old_nodes = model.nodes.clone();
        let old_generated_masters = model.generated_masters.clone();
        let old_mass_method = model.mass_method;
        let old_wall_weight_generation = model.wall_weight_generation;
        let old_damper_mass_generation = model.damper_mass_generation.clone();

        let old_stories = std::mem::replace(&mut model.stories, self.stories.clone());
        for (node, st) in model.nodes.iter_mut().zip(self.node_story.iter()) {
            node.story = *st;
        }
        let old_constraints = model.constraints.clone();
        model
            .constraints
            .retain(|c| !matches!(c, Constraint::RigidDiaphragm { .. }));
        model.constraints.extend(self.constraints.iter().cloned());

        for rn in &self.rep_nodes {
            let idx = rn.id.index();
            if idx < model.nodes.len() {
                model.nodes[idx] = rn.clone();
            } else {
                debug_assert_eq!(idx, model.nodes.len(), "rep_nodes は昇順の連番である前提");
                model.nodes.push(rn.clone());
            }
        }
        model.generated_masters = self.generated_masters.clone();
        model.mass_method = self.mass_method;
        model.damper_mass_generation = Some(self.damper_mass_generation.clone());
        model.wall_weight_generation = Some(self.wall_weight_generation);

        Box::new(RestoreStories {
            wall_weight_generation: old_wall_weight_generation,
            stories: old_stories,
            nodes: old_nodes,
            constraints: old_constraints,
            generated_masters: old_generated_masters,
            mass_method: old_mass_method,
            damper_mass_generation: old_damper_mass_generation,
        })
    }

    fn label(&self) -> &str {
        "階定義の適用"
    }
}

/// [`ApplyStories`] の逆操作。`model.nodes` を丸ごと復元することで、
/// 追加された剛床代表節点の除去（truncate）や既存節点の置換をまとめて元に戻す。
pub struct RestoreStories {
    pub wall_weight_generation: Option<sepika_core::model::WallWeightGenerationMode>,
    pub damper_mass_generation: Option<sepika_core::model::DamperMassGeneration>,
    pub stories: Vec<sepika_core::model::Story>,
    pub nodes: Vec<sepika_core::model::Node>,
    pub constraints: Vec<sepika_core::model::Constraint>,
    pub generated_masters: Vec<NodeId>,
    pub mass_method: sepika_core::model::MassMethod,
}

impl EditCommand for RestoreStories {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let new_wall_weight_generation = std::mem::replace(
            &mut model.wall_weight_generation,
            self.wall_weight_generation,
        );
        let new_stories = std::mem::replace(&mut model.stories, self.stories.clone());
        let new_nodes = std::mem::replace(&mut model.nodes, self.nodes.clone());
        let new_constraints = std::mem::replace(&mut model.constraints, self.constraints.clone());
        let new_generated_masters =
            std::mem::replace(&mut model.generated_masters, self.generated_masters.clone());
        let new_mass_method = std::mem::replace(&mut model.mass_method, self.mass_method);
        let new_damper_mass_generation = std::mem::replace(
            &mut model.damper_mass_generation,
            self.damper_mass_generation.clone(),
        );
        Box::new(RestoreStories {
            wall_weight_generation: new_wall_weight_generation,
            stories: new_stories,
            nodes: new_nodes,
            constraints: new_constraints,
            generated_masters: new_generated_masters,
            mass_method: new_mass_method,
            damper_mass_generation: new_damper_mass_generation,
        })
    }

    fn label(&self) -> &str {
        "階定義の復元"
    }
}

/// 床板割当領域へ床板を割り当てる。床板を末尾に生成し、領域の状態を
/// `Plate(生成した床板)` へ変える。領域が無い、または既に版ありのときは Noop。
pub struct AssignSlabToFloorPlateRegion {
    pub region: FloorPlateAssignmentRegionId,
    pub plate: sepika_core::model::SlabPlate,
}

impl EditCommand for AssignSlabToFloorPlateRegion {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let Some(region) = model.floor_assignment_regions.get(self.region) else {
            return Box::new(Noop);
        };
        if region.assignment.plate().is_some() {
            return Box::new(Noop);
        }
        if !crate::refs::plate_section_ref_ok(model, self.plate.section) {
            return Box::new(Noop);
        }
        if model.floor_assignment_region_nodes(self.region).is_none() {
            return Box::new(Noop);
        }
        let new_id = SlabId(model.slabs.len() as u32);
        SetFloorPlateRegionAssignment {
            region: self.region,
            assignment: sepika_core::model::PlateAssignment::Plate(new_id),
            slab: Some(sepika_core::model::Slab {
                id: new_id,
                shape: sepika_core::model::SlabShape::Enclosed,
                plate: self.plate.clone(),
                tip_loads: Vec::new(),
            }),
            region_refs: Vec::new(),
        }
        .apply(model)
    }

    fn label(&self) -> &str {
        "床板の割当"
    }
}

/// 床板割当領域を「版なし」にする。版ありなら床板の削除も同一 Undo 単位に含める。
pub struct SetFloorPlateRegionNoPlate {
    pub region: FloorPlateAssignmentRegionId,
}

impl EditCommand for SetFloorPlateRegionNoPlate {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let Some(region) = model.floor_assignment_regions.get(self.region) else {
            return Box::new(Noop);
        };
        if region.assignment.is_no_plate() {
            return Box::new(Noop);
        }
        SetFloorPlateRegionAssignment {
            region: self.region,
            assignment: sepika_core::model::PlateAssignment::NoPlate,
            slab: None,
            region_refs: Vec::new(),
        }
        .apply(model)
    }

    fn label(&self) -> &str {
        "版なしにする"
    }
}

/// 床板割当領域の割当を未設定へ戻す。版ありなら床板の削除も同一 Undo 単位に含める。
pub struct UnsetFloorPlateRegion {
    pub region: FloorPlateAssignmentRegionId,
}

impl EditCommand for UnsetFloorPlateRegion {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let Some(region) = model.floor_assignment_regions.get(self.region) else {
            return Box::new(Noop);
        };
        if region.assignment.is_unset() {
            return Box::new(Noop);
        }
        SetFloorPlateRegionAssignment {
            region: self.region,
            assignment: sepika_core::model::PlateAssignment::Unset,
            slab: None,
            region_refs: Vec::new(),
        }
        .apply(model)
    }

    fn label(&self) -> &str {
        "割当を未設定に戻す"
    }
}

/// 割当領域の状態と、それに伴う床板の生成・削除をまとめて適用する内部コマンド。
/// 逆操作は適用前の状態を復元する自分自身。
///
/// `retain_slabs` は取り除いた床板の `FloorRegion.slab_ids` の所属も落とすため、
/// その所属位置を `region_refs` に控えて逆操作で戻す（直後の状態でも `FloorRegion` と
/// 割当領域が食い違わないようにする）。
struct SetFloorPlateRegionAssignment {
    region: FloorPlateAssignmentRegionId,
    assignment: sepika_core::model::PlateAssignment<SlabId>,
    slab: Option<sepika_core::model::Slab>,
    /// 適用時に取り除く床板が持っていた `FloorRegion.slab_ids` の所属位置
    /// （床領域添字, リスト内位置）。
    region_refs: Vec<(usize, usize)>,
}

impl EditCommand for SetFloorPlateRegionAssignment {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        use sepika_core::model::PlateAssignment;
        let Some(region) = model.floor_assignment_regions.get(self.region) else {
            return Box::new(Noop);
        };
        let previous_assignment = region.assignment;
        let previous_slab = previous_assignment
            .plate()
            .and_then(|id| model.slab(id).cloned());
        match (&self.assignment, &self.slab) {
            (PlateAssignment::Plate(id), Some(slab)) if slab.id == *id => {}
            (PlateAssignment::Unset | PlateAssignment::NoPlate, None) => {}
            _ => return Box::new(Noop),
        }
        let previous_refs = if let Some(previous) = &previous_slab {
            let refs: Vec<(usize, usize)> = model
                .floor_regions
                .iter()
                .enumerate()
                .flat_map(|(ri, region)| {
                    region
                        .slab_ids
                        .iter()
                        .enumerate()
                        .filter(|(_, sid)| **sid == previous.id)
                        .map(move |(pos, _)| (ri, pos))
                })
                .collect();
            model.retain_slabs(|slab| slab.id != previous.id);
            refs
        } else {
            Vec::new()
        };
        if let Some(slab) = &self.slab {
            let insert_at = slab.id.0 as usize;
            if insert_at > model.slabs.len() {
                return Box::new(Noop);
            }
            model.visit_slab_ids(|id| {
                if id.0 as usize >= insert_at {
                    id.0 += 1;
                }
            });
            model.slabs.insert(insert_at, slab.clone());
        }
        if let Some(region) = model.floor_assignment_regions.get_mut(self.region) {
            region.assignment = self.assignment;
        }
        if let Some(id) = self.assignment.plate() {
            for &(ri, pos) in self.region_refs.iter().rev() {
                if let Some(region) = model.floor_regions.get_mut(ri) {
                    let insert_pos = pos.min(region.slab_ids.len());
                    region.slab_ids.insert(insert_pos, id);
                }
            }
        }
        Box::new(SetFloorPlateRegionAssignment {
            region: self.region,
            assignment: previous_assignment,
            slab: previous_slab,
            region_refs: previous_refs,
        })
    }

    fn label(&self) -> &str {
        "割当領域の状態変更"
    }
}

/// 床板の削除（中間の床板も可）。逆操作は [`InsertSlab`]。
///
/// ID＝配列インデックスの不変条件を保つため、削除後は当該床板より後ろの
/// ID を 1 つずつ繰り上げる。削除前に `FloorRegion.slab_ids` から該当 ID を
/// 除去する（カスケード削除。[`crate::DeleteSecondaryMember`] と同じ理由。
/// 素通しで繰り上げるだけだと、繰り上がった別の床板の ID と衝突し
/// 「複数の床領域から参照されている」不整合を生む）。
pub struct DeleteSlab {
    pub id: SlabId,
}

impl EditCommand for DeleteSlab {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.id.index();
        if idx >= model.slabs.len() || model.slabs[idx].id != self.id {
            return Box::new(Noop);
        }

        let mut region_refs: Vec<(usize, usize)> = Vec::new();
        for (ri, region) in model.floor_regions.iter_mut().enumerate() {
            let mut pos = 0;
            while pos < region.slab_ids.len() {
                if region.slab_ids[pos] == self.id {
                    region.slab_ids.remove(pos);
                    region_refs.push((ri, pos));
                } else {
                    pos += 1;
                }
            }
        }

        let mut assignment_refs: Vec<(
            FloorPlateAssignmentRegionId,
            sepika_core::model::PlateAssignment<SlabId>,
        )> = Vec::new();
        for region in &mut model.floor_assignment_regions.regions {
            if region.assignment == sepika_core::model::PlateAssignment::Plate(self.id) {
                assignment_refs.push((region.id, region.assignment));
                region.assignment = sepika_core::model::PlateAssignment::Unset;
            }
        }

        let removed = model.slabs.remove(idx);
        let target = self.id.0;
        shift_slab_ids(model, |id| {
            if id.0 > target {
                id.0 -= 1;
            }
        });

        Box::new(InsertSlab {
            index: idx,
            slab: removed,
            region_refs,
            assignment_refs,
        })
    }

    fn label(&self) -> &str {
        "床削除"
    }
}

/// 指定インデックスへ床板を再挿入し、後続 ID と参照を繰り下げ、床領域の
/// `slab_ids` も元の位置へ復元する（[`DeleteSlab`] の逆操作専用）。
pub struct InsertSlab {
    pub index: usize,
    pub slab: sepika_core::model::Slab,
    /// 削除時に床領域から除去した参照の (床領域添字, リスト内位置)。
    pub region_refs: Vec<(usize, usize)>,
    /// 削除時に未設定へ戻した割当領域の (領域 ID, 元の状態)。
    pub assignment_refs: Vec<(
        FloorPlateAssignmentRegionId,
        sepika_core::model::PlateAssignment<SlabId>,
    )>,
}

impl EditCommand for InsertSlab {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if self.index > model.slabs.len() {
            return Box::new(Noop);
        }
        let id = SlabId(self.index as u32);
        shift_slab_ids(model, |x| {
            if x.0 >= id.0 {
                x.0 += 1;
            }
        });
        let mut slab = self.slab.clone();
        slab.id = id;
        model.slabs.insert(self.index, slab);

        for &(ri, pos) in self.region_refs.iter().rev() {
            if let Some(region) = model.floor_regions.get_mut(ri) {
                let insert_pos = pos.min(region.slab_ids.len());
                region.slab_ids.insert(insert_pos, id);
            }
        }
        for (region_id, assignment) in &self.assignment_refs {
            if let Some(region) = model.floor_assignment_regions.get_mut(*region_id) {
                region.assignment = *assignment;
            }
        }

        Box::new(DeleteSlab { id })
    }

    fn label(&self) -> &str {
        "床削除の取り消し"
    }
}

/// モデル内の全ての `SlabId` 参照（床板自身の ID・床領域の `slab_ids`）に `f` を適用する。
fn shift_slab_ids(model: &mut Model, f: impl FnMut(&mut SlabId)) {
    model.visit_slab_ids(f);
}

/// 準備計算の確定済み作業コピーを、重量・質量・荷重とともに一括採用する。
pub struct ApplyPreparedModel {
    pub prepared: Model,
}

impl EditCommand for ApplyPreparedModel {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let previous = std::mem::replace(model, self.prepared.clone());
        Box::new(Self { prepared: previous })
    }

    fn label(&self) -> &str {
        "準備計算の採用"
    }
}
