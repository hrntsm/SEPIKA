use super::super::StbError;
use super::xml::{get_u32, Attrs};
use sepika_core::ids::*;
use sepika_core::model::*;

#[derive(Default)]
pub(super) struct StrengthInputs {
    common: Option<StbCommonStrength>,
    frame_section_ids: Vec<(u32, String)>,
    sections: Vec<RawSection>,
    members: Vec<RawMember>,
    current_section: Option<usize>,
    current_member: Option<usize>,
}
struct RawSection {
    tag: String,
    id: u32,
    bound: Option<SectionId>,
    concrete: Option<String>,
    bars: Vec<StbRebarStrength>,
    steel: Vec<StbSteelStrength>,
}
struct RawMember {
    tag: String,
    section: Option<u32>,
    concrete: Option<String>,
    nodes: Vec<u32>,
    is_concrete: bool,
    targets: Vec<StrengthTarget>,
}
impl StrengthInputs {
    pub(super) fn same_section_strength(&self, a: u32, b: u32) -> bool {
        let source = |id| {
            self.sections.iter().find(|s| s.id == id).map(|s| {
                (
                    s.concrete.clone(),
                    s.bars.clone(),
                    s.steel
                        .iter()
                        .map(|r| {
                            (
                                super::super::strength_export::steel_element_key(&r.element)
                                    .to_owned(),
                                r.part.clone(),
                                r.position.clone(),
                                r.strength.clone(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
        };
        source(a) == source(b)
    }

    pub(super) fn start(&mut self, tag: &str, a: &Attrs) -> Result<(), StbError> {
        if matches!(
            tag,
            "StbSecColumn_RC"
                | "StbSecColumn_SRC"
                | "StbSecColumn_CFT"
                | "StbSecColumn_S"
                | "StbSecBeam_RC"
                | "StbSecBeam_SRC"
                | "StbSecBeam_S"
                | "StbSecBrace_S"
        ) {
            let id = get_u32(a, "id")?;
            if self
                .frame_section_ids
                .iter()
                .any(|(other, kind)| *other == id && kind != tag)
            {
                return Err(StbError::Unmappable(format!(
                    "断面系列間で重複するID {id} は現行の断面参照で区別できません ({tag})"
                )));
            }
            self.frame_section_ids.push((id, tag.to_owned()));
        }
        if tag == "StbCommon" {
            let required = |key| {
                a.get(key)
                    .cloned()
                    .ok_or_else(|| StbError::Parse(format!("StbCommon {key} がありません")))
            };
            self.common = Some(StbCommonStrength {
                project_name: required("project_name")?,
                app_name: required("app_name")?,
                app_version: required("app_version")?,
                strength_concrete: a.get("strength_concrete").cloned(),
                reinforcement: Vec::new(),
            });
        } else if tag == "StbReinforcementStrength" {
            let diameter = a
                .get("D")
                .cloned()
                .ok_or_else(|| StbError::Parse("径別強度の D がありません".into()))?;
            let strength = a
                .get("strength")
                .cloned()
                .ok_or_else(|| StbError::Parse("径別強度の strength がありません".into()))?;
            let common = self
                .common
                .as_mut()
                .ok_or_else(|| StbError::Parse("径別強度の StbCommon がありません".into()))?;
            common
                .reinforcement
                .push(DiameterStrength { diameter, strength });
        } else if matches!(
            tag,
            "StbSecColumn_S"
                | "StbSecBeam_S"
                | "StbSecBrace_S"
                | "StbSecColumn_RC"
                | "StbSecColumn_SRC"
                | "StbSecColumn_CFT"
                | "StbSecBeam_RC"
                | "StbSecBeam_SRC"
                | "StbSecSlab_RC"
                | "StbSecSlabDeck"
                | "StbSecWall_RC"
        ) {
            self.sections.push(RawSection {
                tag: tag.into(),
                id: get_u32(a, "id")?,
                bound: None,
                concrete: a.get("strength_concrete").cloned(),
                bars: Vec::new(),
                steel: Vec::new(),
            });
            self.current_section = Some(self.sections.len() - 1);
        } else if matches!(
            tag,
            "StbColumn" | "StbPost" | "StbGirder" | "StbBeam" | "StbSlab" | "StbWall"
        ) {
            let nodes = match tag {
                "StbColumn" | "StbPost" => {
                    vec![get_u32(a, "id_node_bottom")?, get_u32(a, "id_node_top")?]
                }
                "StbGirder" | "StbBeam" => {
                    vec![get_u32(a, "id_node_start")?, get_u32(a, "id_node_end")?]
                }
                _ => Vec::new(),
            };
            self.members.push(RawMember {
                tag: tag.into(),
                section: a.get("id_section").and_then(|s| s.parse().ok()),
                concrete: a.get("strength_concrete").cloned(),
                nodes,
                targets: Vec::new(),
                is_concrete: matches!(
                    a.get("kind_structure").map(String::as_str),
                    Some("RC" | "SRC" | "CFT")
                ),
            });
            self.current_member = Some(self.members.len() - 1);
        }
        if let Some(index) = self.current_section {
            let is_steel = tag.starts_with("StbSecSteel")
                || tag.starts_with("StbSecColumn_SRC_SameShape")
                || tag.starts_with("StbSecColumn_SRC_NotSameShape")
                || tag.starts_with("StbSecColumn_SRC_ThreeTypesShape");
            if is_steel {
                for key in a
                    .names()
                    .into_iter()
                    .filter(|name| name.starts_with("strength_") || *name == "strength")
                {
                    self.sections[index].steel.push(StbSteelStrength {
                        native_material: None,
                        element: tag.into(),
                        part: key.strip_prefix("strength_").unwrap_or("").into(),
                        position: a.get("pos").cloned(),
                        strength: a.get(key).unwrap().clone(),
                    });
                }
            }
            // 非標準SRC属性の入力を拒否せず保持するが、出力先は標準の鋼材子要素とする。
            if matches!(tag, "StbSecColumn_SRC" | "StbSecBeam_SRC") {
                if let Some(grade) = a.get("strength_steel").or_else(|| a.get("strength_main_S")) {
                    let element = if tag == "StbSecColumn_SRC" {
                        "StbSecColumn_SRC_SameShapeH"
                    } else {
                        "StbSecSteelBeam_SRC_Straight"
                    };
                    self.sections[index].steel.push(StbSteelStrength {
                        native_material: None,
                        element: element.into(),
                        part: "main".into(),
                        position: None,
                        strength: grade.clone(),
                    });
                }
            }
        }
        if tag.starts_with("StbSecBar") {
            if let Some(index) = self.current_section {
                let mut parts: Vec<_> = a
                    .names()
                    .into_iter()
                    .filter_map(|n| n.strip_prefix("D_").or_else(|| n.strip_prefix("strength_")))
                    .collect();
                parts.sort_unstable();
                parts.dedup();
                for part in parts {
                    let diameter = a.get(&format!("D_{part}")).cloned();
                    let strength = a.get(&format!("strength_{part}")).cloned();
                    if diameter.is_some() || strength.is_some() {
                        self.sections[index].bars.push(StbRebarStrength {
                            element: tag.into(),
                            part: part.into(),
                            position: a.get("pos").cloned(),
                            native_material: None,
                            diameter,
                            strength,
                        });
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn end(&mut self, tag: &str) {
        if self
            .current_section
            .is_some_and(|i| self.sections[i].tag == tag)
        {
            self.current_section = None;
        }
        if self
            .current_member
            .is_some_and(|i| self.members[i].tag == tag)
        {
            self.current_member = None;
        }
    }
    pub(super) fn nodes(&mut self, text: &str) {
        if let Some(index) = self.current_member {
            super::xml::push_node_id_tokens(text, &mut self.members[index].nodes);
        }
    }
    pub(super) fn current_member_index(&self) -> Option<usize> {
        self.current_member
    }
    pub(super) fn bind_member(&mut self, index: Option<usize>, target: StrengthTarget) {
        if let Some(input) = index.and_then(|i| self.members.get_mut(i)) {
            if !input.targets.contains(&target) {
                input.targets.push(target);
            }
        }
    }
    pub(super) fn bind_section(&mut self, id: u32, kind: &str, target: SectionId) {
        for source in &mut self.sections {
            if source.id == id && source.tag.starts_with(kind) {
                source.bound = Some(target);
            }
        }
    }
    pub(super) fn same_member_strength(&self, model: &Model, a: usize, b: usize) -> bool {
        let resolve = |index: usize| {
            let input = &self.members[index];
            let node = input
                .nodes
                .first()
                .and_then(|id| {
                    model
                        .stb_node_ids
                        .iter()
                        .find(|n| n.id == *id)
                        .map(|n| n.node)
                })
                .ok_or("元床の第1節点がありません".to_string())?;
            let section = self
                .sections
                .iter()
                .find(|s| Some(s.id) == input.section && s.tag.starts_with("StbSecSlab"))
                .and_then(|s| s.concrete.as_deref());
            model.resolve_stb_concrete_at(
                node,
                input.concrete.as_deref(),
                section,
                self.common
                    .as_ref()
                    .and_then(|c| c.strength_concrete.as_deref()),
            )
        };
        match (resolve(a), resolve(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => a == b,
        }
    }
    pub(super) fn apply(
        self,
        model: &mut Model,
        warnings: &mut Vec<String>,
    ) -> Result<(), StbError> {
        model.stb_strengths.common = self.common;
        for source in &self.sections {
            if let Some(section) = source.bound {
                if !model
                    .stb_strengths
                    .sections
                    .iter()
                    .any(|s| s.section == section)
                {
                    model.stb_strengths.sections.push(StbSectionStrength {
                        section,
                        native_material: None,
                        concrete: source.concrete.clone(),
                        reinforcement: source.bars.clone(),
                        steel: source.steel.clone(),
                    });
                }
            }
        }
        for input in self.members {
            let source_section = self.sections.iter().any(|s| {
                Some(s.id) == input.section
                    && s.bound.is_some()
                    && !matches!(
                        s.tag.as_str(),
                        "StbSecColumn_S" | "StbSecBeam_S" | "StbSecBrace_S"
                    )
                    && match input.tag.as_str() {
                        "StbColumn" | "StbPost" => s.tag.starts_with("StbSecColumn"),
                        "StbGirder" | "StbBeam" => s.tag.starts_with("StbSecBeam"),
                        "StbSlab" => s.tag.starts_with("StbSecSlab"),
                        "StbWall" => s.tag.starts_with("StbSecWall"),
                        _ => false,
                    }
            });
            if !input.is_concrete && !source_section && input.concrete.is_none() {
                continue;
            }
            let Some(nodes): Option<Vec<_>> = input
                .nodes
                .iter()
                .map(|id| {
                    model
                        .stb_node_ids
                        .iter()
                        .find(|n| n.id == *id)
                        .map(|n| n.node)
                })
                .collect()
            else {
                continue;
            };
            let Some(&node) = (if matches!(input.tag.as_str(), "StbColumn" | "StbPost" | "StbWall")
            {
                nodes.last()
            } else {
                nodes.first()
            }) else {
                continue;
            };
            for target in input.targets {
                if model
                    .stb_strengths
                    .members
                    .iter()
                    .any(|m| m.target == target)
                {
                    continue;
                }
                model.stb_strengths.members.push(StbMemberStrength {
                    target,
                    node,
                    node_order: nodes.clone(),
                    concrete: input.concrete.clone(),
                });
            }
        }
        model.prepare_stb_strength_materials();
        warnings.extend(model.stb_strength_diagnostics());
        Ok(())
    }
}
