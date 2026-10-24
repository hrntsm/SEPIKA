//! 階（[`Story`]）の編集コマンド。
//!
//! 階は利用者が定義するデータであり（`sepika_core::model::story`）、
//! 階名・階レベルの変更、階の追加・削除をここで行う。準備計算が埋める欄
//! （所属節点・地震用重量・主要構造種別）と、剛床（`Constraint::RigidDiaphragm`）は
//! 触らない。階の追加・削除の直後はそれらが古い状態のまま残るため、呼び出し側は
//! 続けて階生成（準備計算）を実行して整合させる。
//!
//! `model.stories` は **`elevation` の昇順**かつ **`StoryId` ＝配列位置**という
//! 2 つの不変条件を持つ。追加・削除はこの両方を保つため、挿入位置を標高から決め、
//! 以降の階の ID を [`Model::visit_story_ids`] で繰り上げる。

use crate::EditCommand;
use sepika_core::ids::StoryId;
use sepika_core::model::{Model, Story, StoryLevelKind};

/// 元の STB 階 ID を指定し、明示所属を置き換える。節点 ID は内部 ID。
pub struct SetSourceStoryNodes {
    pub source_story: u32,
    pub nodes: Vec<sepika_core::ids::NodeId>,
}

struct RestoreSourceStories(Vec<sepika_core::model::SourceStory>);

impl EditCommand for RestoreSourceStories {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        Box::new(Self(std::mem::replace(
            &mut model.source_stories,
            self.0.clone(),
        )))
    }
    fn label(&self) -> &str {
        "原階所属の復元"
    }
}

impl EditCommand for SetSourceStoryNodes {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if self
            .nodes
            .iter()
            .any(|n| model.node(*n).is_none() || model.generated_masters.contains(n))
        {
            return Box::new(crate::RejectedEdit(
                "原階所属には実在する構造節点が必要です".into(),
            ));
        }
        let Some(index) = model
            .source_stories
            .iter()
            .position(|s| s.id == self.source_story)
        else {
            return Box::new(crate::RejectedEdit("原階IDが存在しません".into()));
        };
        if model.source_stories[index]
            .node_ids
            .iter()
            .map(|n| n.node)
            .eq(self.nodes.iter().copied().map(Some))
        {
            return Box::new(crate::Noop);
        }
        let mut seen = std::collections::HashSet::new();
        if !self.nodes.iter().all(|n| seen.insert(*n)) {
            return Box::new(crate::RejectedEdit(
                "原階所属の節点IDが重複しています".into(),
            ));
        }
        if let Err(reason) = model.assign_stb_node_ids() {
            return Box::new(crate::RejectedEdit(reason));
        }
        let before = model.source_stories.clone();
        model.source_stories[index].node_ids = self
            .nodes
            .iter()
            .map(|node| sepika_core::model::SourceStoryNode {
                id: model
                    .stb_node_ids
                    .iter()
                    .find(|n| n.node == *node)
                    .unwrap()
                    .id,
                node: Some(*node),
            })
            .collect();
        Box::new(RestoreSourceStories(before))
    }
    fn label(&self) -> &str {
        "原階の明示所属変更"
    }
}

pub struct SetStoryFireproof {
    pub story: StoryId,
    pub conditions: sepika_core::model::StoryFireproof,
}

impl EditCommand for SetStoryFireproof {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if self.conditions.validate().is_err() {
            return Box::new(crate::Noop);
        }
        let Some(story) = model.stories.iter_mut().find(|s| s.id == self.story) else {
            return Box::new(crate::Noop);
        };
        let old = std::mem::replace(&mut story.fireproof, self.conditions);
        Box::new(Self {
            story: self.story,
            conditions: old,
        })
    }
    fn label(&self) -> &str {
        "階共通耐火被覆条件変更"
    }
}

pub struct SetColumnFinishAreaWeight {
    pub story: StoryId,
    pub weight_n_per_mm2: f64,
}

impl EditCommand for SetColumnFinishAreaWeight {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if !self.weight_n_per_mm2.is_finite() || self.weight_n_per_mm2 < 0.0 {
            return Box::new(crate::Noop);
        }
        let Some(story) = model.stories.iter_mut().find(|s| s.id == self.story) else {
            return Box::new(crate::Noop);
        };
        let old = std::mem::replace(&mut story.column_finish_area_weight, self.weight_n_per_mm2);
        Box::new(Self {
            story: self.story,
            weight_n_per_mm2: old,
        })
    }

    fn label(&self) -> &str {
        "RC/SRC柱の階共通仕上げ面重量変更"
    }
}

/// 階名・FL・ΔSL を変更する。未入力は保持できるが、非有限値・上下逆転は拒否する。
pub struct SetStoryLevel {
    pub story: StoryId,
    pub name: String,
    pub elevation: Option<f64>,
    pub sl_offset_mm: Option<f64>,
}

/// 階の利用者定義欄の一括復元（[`SetStoryLevel`]・[`AddStory`]・[`DeleteStory`] の逆操作）。
///
/// `model.stories` を丸ごと差し替え、`StoryId` の参照も復元前の対応へ戻す。
pub struct RestoreStoryDefs {
    pub seismic_weight_generation: Option<sepika_core::model::SeismicWeightGeneration>,
    pub source_stories_initialized: bool,
    pub source_stories: Vec<sepika_core::model::SourceStory>,
    pub stb_node_ids: Vec<sepika_core::model::StbNodeIdentity>,
    pub stories: Vec<Story>,
    /// 復元後の各節点の所属階（`model.nodes` と同順）。
    pub node_story: Vec<Option<StoryId>>,
    /// 復元後の拘束（剛床の `story` 参照を含む）。
    pub constraints: Vec<sepika_core::model::Constraint>,
}

/// 現在の階定義・階参照のスナップショットを撮る。
pub(crate) fn snapshot(model: &Model) -> RestoreStoryDefs {
    RestoreStoryDefs {
        seismic_weight_generation: model.seismic_weight_generation.clone(),
        source_stories_initialized: model.source_stories_initialized,
        source_stories: model.source_stories.clone(),
        stb_node_ids: model.stb_node_ids.clone(),
        stories: model.stories.clone(),
        node_story: model.nodes.iter().map(|n| n.story).collect(),
        constraints: model.constraints.clone(),
    }
}

pub(crate) fn source_story_index(model: &Model, story: StoryId) -> Result<Option<usize>, String> {
    let level = &model.stories[story.index()];
    match level.source_story {
        Some(id) => model
            .source_stories
            .iter()
            .position(|s| s.id == id)
            .map(Some)
            .ok_or_else(|| format!("階 {}: 原階 {id} が存在しません", story.0)),
        None => Ok(None),
    }
}

/// 標高昇順へ並べ替え、`StoryId` ＝配列位置になるよう全参照を振り直す。
fn resort_and_renumber(model: &mut Model) {
    let mut order: Vec<usize> = (0..model.stories.len()).collect();
    order.sort_by(|&a, &b| {
        model.stories[a]
            .fl_mm()
            .expect("FL は階追加前に検査済み")
            .total_cmp(&model.stories[b].fl_mm().expect("FL は階追加前に検査済み"))
    });
    let mut remap = vec![StoryId(0); model.stories.len()];
    for (new_idx, &old_idx) in order.iter().enumerate() {
        if let Some(slot) = remap.get_mut(model.stories[old_idx].id.index()) {
            *slot = StoryId(new_idx as u32);
        }
    }
    model.visit_story_ids(|sid| {
        if let Some(&new) = remap.get(sid.index()) {
            *sid = new;
        }
    });
    model.stories.sort_by_key(|s| s.id.0);
}

fn invalid_level_edit(model: &Model, index: usize) -> Option<String> {
    let story = &model.stories[index];
    let owner = format!("階 {} ({})", story.id.0, story.name);
    if story.elevation.is_some_and(|v| !v.is_finite())
        || story.sl_offset_mm.is_some_and(|v| !v.is_finite())
    {
        return Some(format!("{owner}: FL・ΔSL は有限値が必要です"));
    }
    if story.elevation.is_some() && story.sl_offset_mm.is_some() && story.sl_mm().is_none() {
        return Some(format!("{owner}: SL の導出結果が有限値ではありません"));
    }
    for pair in model
        .stories
        .windows(2)
        .filter(|pair| pair[0].id == story.id || pair[1].id == story.id)
    {
        for (field, bottom, top) in [
            ("FL", pair[0].fl_mm(), pair[1].fl_mm()),
            ("SL", pair[0].sl_mm(), pair[1].sl_mm()),
        ] {
            if let (Some(bottom), Some(top)) = (bottom, top) {
                let height = top - bottom;
                if !height.is_finite() || height <= 0.0 {
                    return Some(format!(
                        "{owner}: 階 {}・{} の {field} 間高さは有限な正値が必要です",
                        pair[0].id.0, pair[1].id.0
                    ));
                }
            }
        }
    }
    None
}

impl EditCommand for SetStoryLevel {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.story.index();
        if idx >= model.stories.len() || model.stories[idx].id != self.story {
            return Box::new(crate::Noop);
        }
        if let Err(reason) = source_story_index(model, self.story) {
            return Box::new(crate::RejectedEdit(reason));
        }
        let before = snapshot(model);
        let mut candidate = model.clone();
        if let Err(reason) = candidate.initialize_source_stories() {
            return Box::new(crate::RejectedEdit(reason));
        }
        let story = &mut candidate.stories[idx];
        story.name = self.name.clone();
        story.elevation = self.elevation;
        story.sl_offset_mm = self.sl_offset_mm;
        if let Some(reason) = invalid_level_edit(&candidate, idx) {
            return Box::new(crate::RejectedEdit(reason));
        }
        if let Some(source) = candidate.stories[idx].source_story.and_then(|id| {
            candidate
                .source_stories
                .iter_mut()
                .find(|s| s.id == id && s.kind_from_native)
        }) {
            source.name = self.name.clone();
            if let Some(fl) = self.elevation {
                source.height = fl;
            }
        }
        *model = candidate;
        Box::new(before)
    }

    fn label(&self) -> &str {
        "階の設定変更"
    }
}

/// 階を追加する。標高の昇順を保つ位置へ挿入し、以降の階の ID を繰り上げる。
///
/// 既存階の FL が未設定・不正で位置を確定できない場合は、モデルを変更せず拒否する。
/// 所属節点・地震用重量は空のまま追加する（準備計算が埋める）。
pub struct AddStory {
    pub name: String,
    pub elevation: Option<f64>,
    pub sl_offset_mm: Option<f64>,
}

impl EditCommand for AddStory {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let before = snapshot(model);
        if self.elevation.is_none() || self.sl_offset_mm.is_none() {
            return Box::new(crate::RejectedEdit(
                "階追加には FL・ΔSL の入力が必要です".into(),
            ));
        }
        if self.elevation.is_some_and(|v| !v.is_finite())
            || self.sl_offset_mm.is_some_and(|v| !v.is_finite())
        {
            return Box::new(crate::RejectedEdit("FL・ΔSL は有限値が必要です".into()));
        }
        if let Some(story) = model.stories.iter().find(|story| story.fl_mm().is_none()) {
            return Box::new(crate::RejectedEdit(format!(
                "階 {} ({}): FL が未設定または有限値ではないため、階追加位置を確定できません",
                story.id.0, story.name
            )));
        }
        let diagnostics = model.story_level_input_diagnostics();
        if !diagnostics.is_empty() {
            return Box::new(crate::RejectedEdit(diagnostics.join("; ")));
        }
        if let Err(reason) = model.initialize_source_stories() {
            return Box::new(crate::RejectedEdit(reason));
        }
        let Some(id) = model
            .source_stories
            .iter()
            .map(|s| s.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        else {
            return Box::new(crate::RejectedEdit("STB原階IDの上限です".into()));
        };
        model.source_stories.push(sepika_core::model::SourceStory {
            kind_from_native: true,
            id,
            guid: None,
            name: self.name.clone(),
            height: self.elevation.unwrap(),
            kind: sepika_core::model::SourceStoryKind::General,
            id_dependence: None,
            strength_concrete: None,
            node_ids: Vec::new(),
        });
        model.stories.push(Story {
            wall_weights: Vec::new(),
            id: StoryId(model.stories.len() as u32),
            name: self.name.clone(),
            elevation: self.elevation,
            sl_offset_mm: self.sl_offset_mm,
            source_story: Some(id),
            node_ids: Vec::new(),
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: StoryLevelKind::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        });
        resort_and_renumber(model);
        if let Some(reason) = invalid_level_edit(
            model,
            model
                .stories
                .iter()
                .position(|s| s.source_story == Some(id))
                .unwrap(),
        ) {
            before.apply(model);
            return Box::new(crate::RejectedEdit(reason));
        }
        Box::new(before)
    }

    fn label(&self) -> &str {
        "階の追加"
    }
}

/// 階（床）を削除する。節点・部材は残す。基部の階は削除できない。
pub struct DeleteStory {
    pub story: StoryId,
}

impl EditCommand for DeleteStory {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.story.index();
        if idx >= model.stories.len() || model.stories[idx].id != self.story {
            return Box::new(crate::Noop);
        }
        if idx == 0 {
            return Box::new(crate::Noop);
        }
        let before = snapshot(model);
        if let Err(reason) = model.initialize_source_stories() {
            return Box::new(crate::RejectedEdit(reason));
        }
        let index = match source_story_index(model, self.story) {
            Ok(index) => index,
            Err(reason) => return Box::new(crate::RejectedEdit(reason)),
        };
        if let Some(index) = index {
            let source_id = model.source_stories[index].id;
            if model
                .source_stories
                .iter()
                .any(|s| s.id_dependence == Some(source_id))
            {
                return Box::new(crate::RejectedEdit(
                    "従属階から参照されている原階は削除できません".into(),
                ));
            }
            model.source_stories.remove(index);
        }
        model.stories.remove(idx);
        model.constraints.retain(|c| {
            !matches!(
                c,
                sepika_core::model::Constraint::RigidDiaphragm { story, .. } if *story == self.story
            )
        });
        let removed = self.story;
        if let Some(record) = &mut model.seismic_weight_generation {
            record
                .automatic_node_stories
                .retain(|(_, story)| *story != removed);
            record
                .calculated_weights
                .retain(|(story, _)| *story != removed);
            let removed_masters: Vec<_> =
                record
                    .automatic_diaphragms
                    .iter()
                    .filter_map(|constraint| match constraint {
                        sepika_core::model::Constraint::RigidDiaphragm {
                            story, master, ..
                        } if *story == removed => Some(*master),
                        _ => None,
                    })
                    .collect();
            record
                .automatic_master_restraints
                .retain(|(master, _)| !removed_masters.contains(master));
            record.automatic_diaphragms.retain(|constraint| {
                !matches!(constraint, sepika_core::model::Constraint::RigidDiaphragm { story, .. } if *story == removed)
            });
        }
        for node in &mut model.nodes {
            if node.story == Some(removed) {
                node.story = None;
            }
        }
        model.visit_story_ids(|story| {
            if story.0 > removed.0 {
                story.0 -= 1;
            }
        });
        Box::new(before)
    }

    fn label(&self) -> &str {
        "階の削除"
    }
}

impl EditCommand for RestoreStoryDefs {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let redo = snapshot(model);
        model.seismic_weight_generation = self.seismic_weight_generation.clone();
        model.stories = self.stories.clone();
        model.source_stories = self.source_stories.clone();
        model.source_stories_initialized = self.source_stories_initialized;
        model.stb_node_ids = self.stb_node_ids.clone();
        for (node, story) in model.nodes.iter_mut().zip(self.node_story.iter()) {
            node.story = *story;
        }
        model.constraints = self.constraints.clone();
        Box::new(redo)
    }

    fn label(&self) -> &str {
        "階定義の復元"
    }
}
