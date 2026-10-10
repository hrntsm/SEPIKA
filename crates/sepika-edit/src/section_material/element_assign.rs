//! 部材への断面・材料・履歴則・制振ダンパーの割当編集コマンド。

use super::*;
use sepika_core::ids::*;

/// 部材の断面割当変更。
///
/// 実在しない断面を指定した場合は何もしない（`Noop`。crate::refs の規約）。
pub struct SetElementSection {
    pub elem: ElemId,
    pub section: Option<SectionId>,
}

impl EditCommand for SetElementSection {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.elem.index();
        if idx >= model.elements.len() || model.elements[idx].id != self.elem {
            return Box::new(Noop);
        }
        if !crate::refs::frame_element_section_ref_ok(model, &model.elements[idx], self.section) {
            return Box::new(Noop);
        }
        let old = model.elements[idx].section;
        model.elements[idx].section = self.section;
        Box::new(SetElementSection {
            elem: self.elem,
            section: old,
        })
    }

    fn label(&self) -> &str {
        "部材断面割当変更"
    }
}

/// 断面の材料割当変更。
///
/// **材料は断面が持つ**（`Section::material` ほか）。役割ごとに欄が分かれており、
/// どれを変更するかは [`SectionMaterialRole`] で指定する。
///
/// 実在しない材料を指定した場合は何もしない（`Noop`。crate::refs の規約）。
pub struct SetSectionMaterial {
    pub section: sepika_core::ids::SectionId,
    pub role: SectionMaterialRole,
    pub material: Option<sepika_core::ids::MaterialId>,
}

/// 断面が持つ材料の役割。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionMaterialRole {
    /// 主材料（弾性剛性 E・ν と自重の密度を決める）。
    Main,
    /// 主筋。
    Rebar,
    /// せん断補強筋。
    ShearRebar,
    /// SRC 断面の内蔵鉄骨。
    Steel,
}

impl SectionMaterialRole {
    fn slot(
        self,
        sec: &mut sepika_core::model::Section,
    ) -> &mut Option<sepika_core::ids::MaterialId> {
        match self {
            SectionMaterialRole::Main => &mut sec.material,
            SectionMaterialRole::Rebar => &mut sec.rebar_material,
            SectionMaterialRole::ShearRebar => &mut sec.shear_rebar_material,
            SectionMaterialRole::Steel => &mut sec.steel_material,
        }
    }
}

impl EditCommand for SetSectionMaterial {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.section.index();
        if idx >= model.sections.len() || model.sections[idx].id != self.section {
            return Box::new(Noop);
        }
        if !crate::refs::material_ref_ok(model, self.material) {
            return Box::new(Noop);
        }
        let source_snapshot = model.stb_strengths.clone();
        let source_stories = model.source_stories.clone();
        let grade = self
            .material
            .and_then(|id| model.materials.get(id.index()))
            .map(|m| {
                if self.role == SectionMaterialRole::Main {
                    m.fc.map(|v| format!("Fc{v}"))
                        .unwrap_or_else(|| m.name.clone())
                } else {
                    model
                        .stb_strengths
                        .materials
                        .iter()
                        .find(|record| record.material == m.id)
                        .map(|record| record.grade.clone())
                        .unwrap_or_else(|| m.name.clone())
                }
            });
        if let Some(input) = model
            .stb_strengths
            .sections
            .iter_mut()
            .find(|s| s.section == self.section)
        {
            match self.role {
                SectionMaterialRole::Main => {
                    input.concrete = grade;
                    input.native_material = self.material;
                }
                SectionMaterialRole::Rebar => {
                    for bar in &mut input.reinforcement {
                        if bar.part == "main" {
                            bar.strength = grade.clone();
                            bar.native_material = self.material;
                        }
                    }
                }
                SectionMaterialRole::ShearRebar => {
                    for bar in &mut input.reinforcement {
                        if matches!(bar.part.as_str(), "band" | "stirrup") {
                            bar.strength = grade.clone();
                            bar.native_material = self.material;
                        }
                    }
                }
                SectionMaterialRole::Steel => {
                    for steel in &mut input.steel {
                        steel.strength = grade.clone().unwrap_or_default();
                        steel.native_material = self.material;
                    }
                }
            }
        }
        model.prepare_stb_strength_materials();
        let slot = self.role.slot(&mut model.sections[idx]);
        let old = std::mem::replace(slot, self.material);
        Box::new(crate::strength::RestoreStrengthInput {
            input: source_snapshot,
            stories: source_stories,
            inverse: Box::new(SetSectionMaterial {
                section: self.section,
                role: self.role,
                material: old,
            }),
        })
    }

    fn label(&self) -> &str {
        "断面材料割当変更"
    }
}

/// 部材の履歴則（復元力特性）変更（各履歴則の原典による）。
/// `HysteresisModel::Auto` を指定すると個別指定を解除し既定へ戻す。
pub struct SetMemberHysteresis {
    pub elem: ElemId,
    pub rule: sepika_core::model::HysteresisModel,
}

impl EditCommand for SetMemberHysteresis {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.elem.index();
        if idx >= model.elements.len() || model.elements[idx].id != self.elem {
            return Box::new(Noop);
        }
        let old = model.set_member_hysteresis(self.elem, self.rule);
        Box::new(SetMemberHysteresis {
            elem: self.elem,
            rule: old.unwrap_or(sepika_core::model::HysteresisModel::Auto),
        })
    }

    fn label(&self) -> &str {
        "部材履歴則変更"
    }
}

/// 部材の履歴則(時刻歴応答解析用スロット)変更。`None` は「増分と同じ」へ戻す。
pub struct SetMemberHysteresisTh {
    pub elem: ElemId,
    pub rule_th: Option<sepika_core::model::HysteresisModel>,
}

impl EditCommand for SetMemberHysteresisTh {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.elem.index();
        if idx >= model.elements.len() || model.elements[idx].id != self.elem {
            return Box::new(Noop);
        }
        let old = model.set_member_hysteresis_th(self.elem, self.rule_th);
        Box::new(SetMemberHysteresisTh {
            elem: self.elem,
            rule_th: old,
        })
    }

    fn label(&self) -> &str {
        "部材履歴則変更(時刻歴)"
    }
}

/// 制振ダンパーの特性（Kd・C0・α）変更（制振部材の力学モデル: Maxwell モデル等）。
/// `props=None` で指定を解除する。
pub struct SetDamperProps {
    pub elem: ElemId,
    pub props: Option<sepika_core::model::DamperProps>,
}

impl EditCommand for SetDamperProps {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.elem.index();
        if idx >= model.elements.len() || model.elements[idx].id != self.elem {
            return Box::new(Noop);
        }
        let old = model.set_damper_props(self.elem, self.props);
        Box::new(SetDamperProps {
            elem: self.elem,
            props: old,
        })
    }

    fn label(&self) -> &str {
        "制振ダンパー特性変更"
    }
}
