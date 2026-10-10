use super::super::StbError;
use super::xml::{get_u32, Attrs};
use sepika_core::ids::*;
use sepika_core::model::*;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct StrengthInputs {
    common: Option<StbCommonStrength>,
    sections: Vec<RawSection>,
    members: Vec<RawMember>,
    current_section: Option<usize>,
    current_member: Option<usize>,
}
struct RawSection {
    tag: String,
    id: u32,
    name: Option<String>,
    floor: Option<String>,
    concrete: Option<String>,
    bars: Vec<StbRebarStrength>,
}
struct RawMember {
    tag: String,
    section: Option<u32>,
    concrete: Option<String>,
    nodes: Vec<u32>,
    is_concrete: bool,
}
impl StrengthInputs {
    pub(super) fn start(&mut self, tag: &str, a: &Attrs) -> Result<(), StbError> {
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
            "StbSecColumn_RC"
                | "StbSecColumn_SRC"
                | "StbSecColumn_CFT"
                | "StbSecBeam_RC"
                | "StbSecBeam_SRC"
                | "StbSecSlab_RC"
                | "StbSecWall_RC"
        ) {
            self.sections.push(RawSection {
                tag: tag.into(),
                id: get_u32(a, "id")?,
                name: a.get("name").cloned(),
                floor: a.get("floor").cloned(),
                concrete: a.get("strength_concrete").cloned(),
                bars: Vec::new(),
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
                is_concrete: matches!(
                    a.get("kind_structure").map(String::as_str),
                    Some("RC" | "SRC" | "CFT")
                ),
            });
            self.current_member = Some(self.members.len() - 1);
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
    pub(super) fn apply(
        self,
        model: &mut Model,
        section_index: &HashMap<u32, u32>,
        warnings: &mut Vec<String>,
    ) -> Result<(), StbError> {
        model.stb_strengths.common = self.common;
        let sections = self.sections;
        for source in &sections {
            let id =
                if source.tag.starts_with("StbSecColumn") || source.tag.starts_with("StbSecBeam") {
                    section_index.get(&source.id).copied().map(SectionId)
                } else if source.tag == "StbSecSlab_RC" {
                    model
                        .sections
                        .iter()
                        .find(|s| Some(&s.name) == source.name.as_ref() && s.floor == source.floor)
                        .map(|s| s.id)
                } else {
                    None
                };
            if let Some(id) = id {
                model.stb_strengths.sections.push(StbSectionStrength {
                    section: id,
                    concrete: source.concrete.clone(),
                    reinforcement: source.bars.clone(),
                });
            }
        }
        for input in self.members {
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
            if nodes.is_empty() {
                continue;
            }
            let source_section = sections.iter().find(|s| {
                Some(s.id) == input.section
                    && match input.tag.as_str() {
                        "StbColumn" | "StbPost" => s.tag.starts_with("StbSecColumn"),
                        "StbGirder" | "StbBeam" => s.tag.starts_with("StbSecBeam"),
                        "StbSlab" => s.tag == "StbSecSlab_RC",
                        "StbWall" => s.tag == "StbSecWall_RC",
                        _ => false,
                    }
            });
            if !input.is_concrete && source_section.is_none() && input.concrete.is_none() {
                continue;
            }
            let (target, section) = match input.tag.as_str() {
                "StbColumn" | "StbGirder" => {
                    let Some(e) = model.elements.iter().find(|e| e.nodes.as_slice() == nodes)
                    else {
                        continue;
                    };
                    (StrengthTarget::Element(e.id), e.section)
                }
                "StbPost" | "StbBeam" => {
                    let member = model
                        .floor_regions
                        .iter()
                        .flat_map(|r| &r.secondary_beams)
                        .chain(model.wall_regions.iter().flat_map(|r| &r.posts))
                        .chain(&model.unassigned_beams)
                        .chain(&model.unassigned_posts)
                        .find(|e| {
                            model.secondary_member_end_points(e).is_some_and(|(a, b)| {
                                a == model.node(nodes[0]).unwrap().coord
                                    && b == model.node(nodes[1]).unwrap().coord
                            })
                        });
                    let Some(e) = member else {
                        continue;
                    };
                    (StrengthTarget::Secondary(e.id), e.section)
                }
                "StbSlab" => {
                    let Some(e) = model
                        .slabs
                        .iter()
                        .find(|e| same_nodes(e.boundary_nodes(model), &nodes))
                    else {
                        continue;
                    };
                    (StrengthTarget::Slab(e.id), e.plate.section)
                }
                "StbWall" => {
                    let Some(e) = model
                        .wall_plates
                        .iter()
                        .find(|e| same_nodes(e.boundary_nodes(model), &nodes))
                    else {
                        continue;
                    };
                    (StrengthTarget::Wall(e.id), e.section)
                }
                _ => continue,
            };
            if let (Some(section), Some(source)) = (section, source_section) {
                if !model
                    .stb_strengths
                    .sections
                    .iter()
                    .any(|s| s.section == section)
                {
                    model.stb_strengths.sections.push(StbSectionStrength {
                        section,
                        concrete: source.concrete.clone(),
                        reinforcement: source.bars.clone(),
                    });
                }
            }
            let node = if matches!(input.tag.as_str(), "StbColumn" | "StbPost" | "StbWall") {
                *nodes.last().unwrap()
            } else {
                nodes[0]
            };
            model.stb_strengths.members.push(StbMemberStrength {
                target,
                node,
                concrete: input.concrete,
            });
        }
        model.prepare_stb_strength_materials();
        warnings.extend(model.stb_strength_diagnostics());
        Ok(())
    }
}
fn same_nodes(left: Option<Vec<NodeId>>, right: &[NodeId]) -> bool {
    let Some(mut left) = left else {
        return false;
    };
    let mut right = right.to_vec();
    left.sort_unstable_by_key(|n| n.0);
    right.sort_unstable_by_key(|n| n.0);
    left == right
}
