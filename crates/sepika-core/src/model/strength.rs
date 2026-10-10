//! ST-Bridge の省略を含む強度入力と、採用元を伴う解決結果。

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StbCommonStrength {
    pub project_name: String,
    pub app_name: String,
    pub app_version: String,
    pub strength_concrete: Option<String>,
    pub reinforcement: Vec<DiameterStrength>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiameterStrength {
    pub diameter: String,
    pub strength: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StrengthSource {
    Member,
    Section,
    Story,
    Common,
    Diameter,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedStrength {
    pub grade: String,
    pub source: StrengthSource,
    /// 明示native割当の数値。標準gradeの数値照合とは区別する。
    pub native_override: bool,
    /// コンクリートは Fc、鉄筋は降伏点 [N/mm²]。
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StbRebarStrength {
    pub element: String,
    pub part: String,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub native_material: Option<MaterialId>,
    pub diameter: Option<String>,
    pub strength: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StbSectionStrength {
    pub section: SectionId,
    pub concrete: Option<String>,
    #[serde(default)]
    pub native_material: Option<MaterialId>,
    pub reinforcement: Vec<StbRebarStrength>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StrengthTarget {
    Element(ElemId),
    Secondary(SecondaryMemberId),
    Slab(SlabId),
    Wall(WallPlateId),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StbMemberStrength {
    pub target: StrengthTarget,
    /// 規格が指定する参照節点。解析用所属から推定しない。
    pub node: NodeId,
    #[serde(default)]
    pub node_order: Vec<NodeId>,
    pub concrete: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StbStrengthInput {
    pub common: Option<StbCommonStrength>,
    pub sections: Vec<StbSectionStrength>,
    pub members: Vec<StbMemberStrength>,
    pub materials: Vec<StbGradeMaterial>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StbGradeMaterial {
    pub grade: String,
    pub material: MaterialId,
}

impl Model {
    /// 材料の意図的なグレード編集を、同じ材料を採る元指定へ反映する。
    pub fn replace_stb_material_grade(&mut self, id: MaterialId, grade: &str) {
        let old: Vec<_> = self
            .stb_strengths
            .materials
            .iter()
            .filter(|m| m.material == id)
            .map(|m| m.grade.clone())
            .collect();
        let replace = |value: &mut Option<String>| {
            if value.as_ref().is_some_and(|v| old.contains(v)) {
                *value = Some(grade.into());
            }
        };
        for member in &mut self.stb_strengths.members {
            replace(&mut member.concrete);
        }
        for section in &mut self.stb_strengths.sections {
            if section.native_material == Some(id) {
                section.concrete = Some(grade.into());
            }
            replace(&mut section.concrete);
            for bar in &mut section.reinforcement {
                replace(&mut bar.strength);
            }
        }
        for story in &mut self.source_stories {
            replace(&mut story.strength_concrete);
        }
        if let Some(common) = &mut self.stb_strengths.common {
            replace(&mut common.strength_concrete);
            for bar in &mut common.reinforcement {
                if old.contains(&bar.strength) {
                    bar.strength = grade.into();
                }
            }
        }
        for material in &mut self.stb_strengths.materials {
            if material.material == id {
                material.grade = grade.into();
            }
        }
    }

    /// 強度解決とは別に、認識済みグレードの物性既定値を準備する。
    pub fn prepare_stb_strength_materials(&mut self) {
        let mut grades = Vec::new();
        for m in &self.stb_strengths.members {
            grades.extend(m.concrete.iter().cloned());
        }
        for s in &self.stb_strengths.sections {
            grades.extend(s.concrete.iter().cloned());
            for r in &s.reinforcement {
                grades.extend(r.strength.iter().cloned());
            }
        }
        for s in &self.source_stories {
            grades.extend(s.strength_concrete.iter().cloned());
        }
        if let Some(c) = &self.stb_strengths.common {
            grades.extend(c.strength_concrete.iter().cloned());
            grades.extend(c.reinforcement.iter().map(|r| r.strength.clone()));
        }

        for m in &self.stb_strengths.members {
            if let Ok(r) = self.resolve_stb_concrete(m) {
                grades.push(r.grade);
            }
        }
        for s in &self.stb_strengths.sections {
            for bar in &s.reinforcement {
                if let Ok(r) = self.resolve_stb_rebar(bar) {
                    grades.push(r.grade);
                }
            }
        }
        grades.sort();
        grades.dedup();
        for grade in grades {
            if self
                .stb_strengths
                .materials
                .iter()
                .any(|r| r.grade == grade)
            {
                continue;
            }
            let id = if let Some(material) = self.materials.iter().find(|m| {
                m.name == grade
                    && crate::standard_material::standard_material_properties(&grade)
                        .is_some_and(|p| m.fc == p.fc && m.fy == p.fy)
            }) {
                material.id
            } else {
                let Some(properties) =
                    crate::standard_material::standard_material_properties(&grade)
                else {
                    continue;
                };
                let id = MaterialId(self.materials.len() as u32);
                self.materials.push(Material {
                    id,
                    name: grade.clone(),
                    category: if properties.fc.is_some() {
                        MaterialCategory::Concrete
                    } else if crate::standard_material::rebar_grade_strength(&grade).is_some() {
                        MaterialCategory::Rebar
                    } else {
                        MaterialCategory::Steel
                    },
                    young: properties.young,
                    poisson: properties.poisson,
                    density: properties.density,
                    shear: None,
                    fc: properties.fc,
                    fy: properties.fy,
                    strength_factor: None,
                    concrete_class: Default::default(),
                });
                id
            };
            self.stb_strengths.materials.push(StbGradeMaterial {
                grade,
                material: id,
            });
        }
    }

    pub fn stb_concrete_material(&self, target: StrengthTarget) -> Option<&Material> {
        let input = self
            .stb_strengths
            .members
            .iter()
            .find(|m| m.target == target)?;
        let resolved = self.resolve_stb_concrete(input).ok()?;
        if resolved.source == StrengthSource::Section {
            let section = self.strength_target_section(target)?;
            if let Some(id) = self
                .stb_strengths
                .sections
                .iter()
                .find(|s| s.section == section)
                .and_then(|s| s.native_material)
            {
                return self.materials.get(id.index());
            }
        }
        let id = self
            .stb_strengths
            .materials
            .iter()
            .find(|m| m.grade == resolved.grade)?
            .material;
        self.materials.get(id.index())
    }

    pub fn stb_rebar_material(&self, section: SectionId, part: &str) -> Option<&Material> {
        let inputs: Vec<_> = self
            .stb_strengths
            .sections
            .iter()
            .find(|s| s.section == section)?
            .reinforcement
            .iter()
            .filter(|r| r.part == part)
            .collect();
        let resolved = self.resolve_stb_rebar(inputs.first()?).ok()?;
        if inputs.iter().any(|input| {
            self.resolve_stb_rebar(input)
                .ok()
                .is_none_or(|other| other.grade != resolved.grade || other.value != resolved.value)
        }) {
            return None;
        }
        if let Some(id) = inputs.first()?.native_material {
            return self.materials.get(id.index());
        }
        let id = self
            .stb_strengths
            .materials
            .iter()
            .find(|m| m.grade == resolved.grade)?
            .material;
        self.materials.get(id.index())
    }

    fn strength_target_section(&self, target: StrengthTarget) -> Option<SectionId> {
        match target {
            StrengthTarget::Element(id) => self.element(id).and_then(|e| e.section),
            StrengthTarget::Secondary(id) => self.secondary_member(id).and_then(|e| e.section),
            StrengthTarget::Slab(id) => self
                .slabs
                .iter()
                .find(|e| e.id == id)
                .and_then(|e| e.plate.section),
            StrengthTarget::Wall(id) => self
                .wall_plates
                .iter()
                .find(|e| e.id == id)
                .and_then(|e| e.section),
        }
    }

    pub fn resolve_stb_concrete(
        &self,
        input: &StbMemberStrength,
    ) -> Result<ResolvedStrength, String> {
        let section = self.strength_target_section(input.target);
        if input.concrete.is_none() {
            if let Some(material) = section
                .and_then(|id| self.stb_strengths.sections.iter().find(|s| s.section == id))
                .and_then(|s| s.native_material)
                .and_then(|id| self.materials.get(id.index()))
            {
                let value = material
                    .fc
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .ok_or("明示したnative材料のFcがありません")?;
                return Ok(ResolvedStrength {
                    grade: format!("Fc{value}"),
                    source: StrengthSource::Section,
                    native_override: true,
                    value,
                });
            }
        }
        let section_grade = section
            .and_then(|id| self.stb_strengths.sections.iter().find(|s| s.section == id))
            .and_then(|s| s.concrete.as_deref())
            .or_else(|| {
                section
                    .filter(|id| !self.stb_strengths.sections.iter().any(|s| s.section == *id))
                    .and_then(|id| self.section(id))
                    .and_then(|s| s.material)
                    .and_then(|id| self.materials.get(id.index()))
                    .map(|m| m.name.as_str())
            });
        self.resolve_stb_concrete_at(
            input.node,
            input.concrete.as_deref(),
            section_grade,
            self.stb_strengths
                .common
                .as_ref()
                .and_then(|c| c.strength_concrete.as_deref()),
        )
    }

    /// 元の指定節点と明示属性からFcを解決する。組立前の床供給も同じ優先規則を使う。
    pub fn resolve_stb_concrete_at(
        &self,
        node: NodeId,
        member: Option<&str>,
        section: Option<&str>,
        common: Option<&str>,
    ) -> Result<ResolvedStrength, String> {
        for (grade, source) in [
            (member, StrengthSource::Member),
            (section, StrengthSource::Section),
        ] {
            if let Some(grade) = grade {
                return concrete_strength(grade, source);
            }
        }
        let stories: Vec<_> = self
            .source_stories
            .iter()
            .filter(|s| s.node_ids.iter().any(|n| n.node == Some(node)))
            .collect();
        let mut candidates = Vec::new();
        for story in &stories {
            let result = match story.strength_concrete.as_deref() {
                Some(grade) => concrete_strength(grade, StrengthSource::Story)?,
                None => concrete_strength(
                    common.ok_or("原階・共通のFc指定がありません")?,
                    StrengthSource::Common,
                )?,
            };
            if !candidates
                .iter()
                .any(|v: &ResolvedStrength| v.value == result.value)
            {
                candidates.push(result);
            }
        }
        if candidates.len() > 1 {
            return Err(format!("原階Fcが競合: 節点 {}", node.0));
        }
        if let Some(result) = candidates.pop() {
            self.resolve_source_concrete_fc(
                node,
                None,
                None,
                common.and_then(crate::standard_material::concrete_grade_strength),
            )?;
            return Ok(result);
        }
        concrete_strength(
            common.ok_or("部材・断面・原階・共通のFc指定がありません")?,
            StrengthSource::Common,
        )
    }

    pub fn resolve_stb_rebar(&self, input: &StbRebarStrength) -> Result<ResolvedStrength, String> {
        if let Some(id) = input.native_material {
            let material = self
                .materials
                .get(id.index())
                .ok_or("明示したnative鉄筋材料がありません")?;
            let value = material
                .fy
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or("明示したnative鉄筋材料のfyがありません")?;
            return Ok(ResolvedStrength {
                grade: input
                    .strength
                    .clone()
                    .unwrap_or_else(|| material.name.clone()),
                source: StrengthSource::Section,
                native_override: true,
                value,
            });
        }
        let (grade, source) = if let Some(grade) = input.strength.as_deref() {
            (grade, StrengthSource::Section)
        } else {
            let diameter = input
                .diameter
                .as_deref()
                .ok_or("鉄筋径と個別強度がありません")?;
            let mut grades = self
                .stb_strengths
                .common
                .as_ref()
                .into_iter()
                .flat_map(|c| &c.reinforcement)
                .filter(|r| r.diameter == diameter)
                .map(|r| r.strength.as_str());
            let grade = grades
                .next()
                .ok_or_else(|| format!("径 {diameter} の鉄筋強度がありません"))?;
            if grades.any(|g| g != grade) {
                return Err(format!("径 {diameter} の鉄筋強度が競合"));
            }
            (grade, StrengthSource::Diameter)
        };
        let value = crate::standard_material::rebar_grade_strength(grade)
            .ok_or_else(|| format!("鉄筋強度 {grade} を解決できません"))?;
        Ok(ResolvedStrength {
            grade: grade.into(),
            source,
            native_override: false,
            value,
        })
    }

    pub fn stb_strength_diagnostics(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if let Some(common) = &self.stb_strengths.common {
            for (i, bar) in common.reinforcement.iter().enumerate() {
                if common.reinforcement[..i]
                    .iter()
                    .any(|other| other.diameter == bar.diameter && other.strength != bar.strength)
                {
                    errors.push(format!("径 {} の鉄筋強度が競合", bar.diameter));
                }
                if crate::standard_material::rebar_grade_strength(&bar.strength).is_none() {
                    errors.push(format!(
                        "径 {} の鉄筋強度 {} を解決できません",
                        bar.diameter, bar.strength
                    ));
                }
            }
        }
        for input in &self.stb_strengths.members {
            if let Err(reason) = self.resolve_stb_concrete(input) {
                errors.push(format!("{:?}: {reason}", input.target));
            }
        }
        for section in &self.stb_strengths.sections {
            for part in ["main", "band", "stirrup"] {
                let resolved: Vec<_> = section
                    .reinforcement
                    .iter()
                    .filter(|r| r.part == part)
                    .filter_map(|r| self.resolve_stb_rebar(r).ok())
                    .collect();
                if resolved.first().is_some_and(|first| {
                    resolved
                        .iter()
                        .any(|r| r.grade != first.grade || r.value != first.value)
                }) {
                    errors.push(format!(
                        "断面 {} / {}: 位置別鉄筋強度を現行材料消費口で縮約できません",
                        section.section.0, part
                    ));
                }
            }
            for input in &section.reinforcement {
                if let Err(reason) = self.resolve_stb_rebar(input) {
                    errors.push(format!(
                        "断面 {} / {} / {}: {reason}",
                        section.section.0, input.element, input.part
                    ));
                }
            }
        }
        errors
    }
}

fn concrete_strength(grade: &str, source: StrengthSource) -> Result<ResolvedStrength, String> {
    let value = crate::standard_material::concrete_grade_strength(grade)
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| format!("コンクリート強度 {grade} を解決できません"))?;
    Ok(ResolvedStrength {
        grade: grade.into(),
        source,
        native_override: false,
        value,
    })
}
