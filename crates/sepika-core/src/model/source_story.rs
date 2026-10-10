//! STB の明示階・標準節点識別子。解析用階とは独立した保存入力。

use super::{Model, NodeId};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SourceStoryKind {
    General,
    Basement,
    Roof,
    Penthouse,
    Isolation,
    Dependence,
}

impl SourceStoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "GENERAL",
            Self::Basement => "BASEMENT",
            Self::Roof => "ROOF",
            Self::Penthouse => "PENTHOUSE",
            Self::Isolation => "ISOLATION",
            Self::Dependence => "DEPENDENCE",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StbNodeIdentity {
    pub node: NodeId,
    pub id: u32,
    pub guid: Option<String>,
}

/// 未知節点も元 ID を保持して診断する。推定所属で補わない。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceStoryNode {
    pub id: u32,
    pub node: Option<NodeId>,
}

/// height は GL から意匠 FL までの高さ [mm]。節点 Z と同一とは限らない。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourceStory {
    pub id: u32,
    pub guid: Option<String>,
    pub name: String,
    pub height: f64,
    pub kind: SourceStoryKind,
    /// native の明示階種別編集を反映する原階。STB 取り込み時は false。
    #[serde(default)]
    pub kind_from_native: bool,
    pub id_dependence: Option<u32>,
    pub strength_concrete: Option<String>,
    pub node_ids: Vec<SourceStoryNode>,
}

impl Model {
    /// 利用者の階定義を明示階として保存する。STB 取り込み済みの原階には触らない。
    pub fn initialize_source_stories(&mut self) -> Result<(), String> {
        if self.source_stories_initialized || !self.source_stories.is_empty() {
            return Ok(());
        }
        self.assign_stb_node_ids()?;
        self.source_stories = self
            .stories
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut members: Vec<_> = s
                    .node_ids
                    .iter()
                    .copied()
                    .chain(
                        self.nodes
                            .iter()
                            .filter(|n| n.story == Some(s.id))
                            .map(|n| n.id),
                    )
                    .filter(|n| !self.generated_masters.contains(n))
                    .collect();
                members.sort_unstable_by_key(|n| n.0);
                members.dedup();
                SourceStory {
                    id: i as u32 + 1,
                    guid: None,
                    name: s.name.clone(),
                    height: s.elevation,
                    kind: match s.level_kind {
                        super::StoryLevelKind::Normal => SourceStoryKind::General,
                        super::StoryLevelKind::Basement { .. } => SourceStoryKind::Basement,
                        super::StoryLevelKind::Penthouse { .. } => SourceStoryKind::Penthouse,
                    },
                    kind_from_native: true,
                    id_dependence: None,
                    strength_concrete: None,
                    node_ids: members
                        .iter()
                        .filter_map(|n| {
                            self.stb_node_ids.iter().find(|id| id.node == *n).map(|id| {
                                SourceStoryNode {
                                    id: id.id,
                                    node: Some(*n),
                                }
                            })
                        })
                        .collect(),
                }
            })
            .collect();
        self.source_stories_initialized = true;
        Ok(())
    }

    /// 元所属と解析用推定が異なる節点を表示用に返す。未所属を補完しない。
    pub fn source_story_assignment_diagnostics(&self) -> Vec<String> {
        let mut result = Vec::new();
        if self.source_stories.is_empty() {
            return result;
        }
        for node in self
            .nodes
            .iter()
            .filter(|n| !self.generated_masters.contains(&n.id))
        {
            let sources: Vec<_> = self
                .source_stories
                .iter()
                .filter(|s| s.node_ids.iter().any(|n| n.node == Some(node.id)))
                .collect();
            let inferred = node
                .story
                .and_then(|id| self.stories.iter().find(|s| s.id == id));
            if sources.len() != 1 || !inferred.is_some_and(|s| s.elevation == sources[0].height) {
                result.push(format!(
                    "節点 {}: 明示原階 {:?} / 解析用推定 {}",
                    node.id.0,
                    sources.iter().map(|s| s.id).collect::<Vec<_>>(),
                    inferred.map(|s| s.name.as_str()).unwrap_or("未所属")
                ));
            }
        }
        result
    }

    /// 未採番の構造節点へ衝突しない正整数を保存する。解析代表節点は対象外。
    pub fn assign_stb_node_ids(&mut self) -> Result<(), String> {
        let mut next = self
            .stb_node_ids
            .iter()
            .map(|n| n.id)
            .chain(
                self.source_stories
                    .iter()
                    .flat_map(|s| s.node_ids.iter().map(|n| n.id)),
            )
            .max()
            .unwrap_or(0);
        for node in &self.nodes {
            if self.generated_masters.contains(&node.id)
                || self.stb_node_ids.iter().any(|n| n.node == node.id)
            {
                continue;
            }
            next = next.checked_add(1).ok_or("STB節点IDの上限です")?;
            self.stb_node_ids.push(StbNodeIdentity {
                node: node.id,
                id: next,
                guid: None,
            });
        }
        Ok(())
    }

    /// 原階入力の不整合。材料解決とは独立に階依存処理の可否を判定する。
    pub fn source_story_diagnostics(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let mut ids = HashSet::new();
        let mut membership = HashMap::<NodeId, Vec<u32>>::new();
        for (index, story) in self.source_stories.iter().enumerate() {
            if story.id == 0 || !ids.insert(story.id) {
                errors.push(format!("原階 {}: ID は一意な正整数が必要です", story.id));
            }
            if !story.height.is_finite() {
                errors.push(format!("原階 {}: height が有限値ではありません", story.id));
            }
            for other in &self.source_stories[..index] {
                if story.height == other.height {
                    errors.push(format!(
                        "原階 {} と {}: 同一height {} mm",
                        other.id, story.id, story.height
                    ));
                }
            }
            let mut seen = HashSet::new();
            for reference in &story.node_ids {
                match reference.node.filter(|id| self.node(*id).is_some()) {
                    Some(node) => {
                        if !seen.insert(node) {
                            errors.push(format!(
                                "原階 {}: 節点 {} の所属が重複",
                                story.id, reference.id
                            ));
                        }
                        membership.entry(node).or_default().push(story.id);
                    }
                    None => {
                        errors.push(format!("原階 {}: 未知の節点ID {}", story.id, reference.id))
                    }
                }
            }
            if story.kind == SourceStoryKind::Dependence && story.id_dependence.is_none() {
                errors.push(format!(
                    "原階 {}: DEPENDENCE の id_dependence がありません",
                    story.id
                ));
            }
            if let Some(target) = story.id_dependence {
                if target == story.id {
                    errors.push(format!("原階 {}: id_dependence が自己参照", story.id));
                } else if !self.source_stories.iter().any(|s| s.id == target) {
                    errors.push(format!(
                        "原階 {}: id_dependence {} が存在しません",
                        story.id, target
                    ));
                }
            }
            let mut path = HashSet::new();
            let mut current = Some(story.id);
            while let Some(id) = current {
                if !path.insert(id) {
                    errors.push(format!("原階 {}: id_dependence が循環", story.id));
                    break;
                }
                current = self
                    .source_stories
                    .iter()
                    .find(|s| s.id == id)
                    .and_then(|s| s.id_dependence);
            }
        }
        let mut membership: Vec<_> = membership.into_iter().collect();
        membership.sort_by_key(|(node, _)| node.0);
        for (node, stories) in membership {
            if stories.len() > 1 {
                errors.push(format!(
                    "階依存処理の所属が未確定: 節点 {} は原階 {:?} に多重所属",
                    node.0, stories
                ));
            }
        }
        errors
    }

    /// 部材 > 断面 > 原階 > 共通で Fc [N/mm²] を解決する。原階競合時は共通へ迂回しない。
    pub fn resolve_source_concrete_fc(
        &self,
        node: NodeId,
        member: Option<f64>,
        section: Option<f64>,
        common: Option<f64>,
    ) -> Result<Option<f64>, String> {
        if let Some(value) = member.or(section) {
            return if value.is_finite() && value > 0.0 {
                Ok(Some(value))
            } else {
                Err("明示Fcは有限な正値が必要です".into())
            };
        }
        let mut values = Vec::new();
        for story in self
            .source_stories
            .iter()
            .filter(|s| s.node_ids.iter().any(|n| n.node == Some(node)))
        {
            if let Some(name) = &story.strength_concrete {
                let value = crate::material_grade::parse_concrete_fc(name)
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .ok_or_else(|| format!("原階 {} のFc {} を解決できません", story.id, name))?;
                if !values.contains(&Some(value)) {
                    values.push(Some(value));
                }
            } else if !values.contains(&common) {
                values.push(common);
            }
        }
        match values.as_slice() {
            [] => validate_common_fc(common),
            [value] => validate_common_fc(*value),
            _ => Err(format!("原階Fcが競合: 節点 {} のFc {:?}", node.0, values)),
        }
    }
}

fn validate_common_fc(value: Option<f64>) -> Result<Option<f64>, String> {
    if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
        Err("採用Fcは有限な正値が必要です".into())
    } else {
        Ok(value)
    }
}
