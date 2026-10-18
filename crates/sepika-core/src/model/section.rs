//! 断面の型。

use super::*;

pub fn rect_shear_area(area: f64) -> f64 {
    area * 5.0 / 6.0
}

/// 主架構線材の設計上の断面用途。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FrameSectionUse {
    Girder,
    Column,
    Brace,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Section {
    pub id: SectionId,
    /// 断面符号。単独では断面を一意に定めない。
    pub name: String,
    /// 主架構線材として使用するときの断面用途。床・壁用断面では `None`。
    pub frame_use: Option<FrameSectionUse>,
    /// 階。[`Story`](crate::model::Story) への参照ではない。
    ///
    /// 断面の同一性は符号＋階で決まる。階を持たない断面は `None` とし、
    /// このときは符号だけが同一性キーになる。
    #[serde(default)]
    pub floor: Option<String>,
    pub area: f64,
    pub iy: f64,
    pub iz: f64,
    pub j: f64,
    #[serde(default)]
    pub depth: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub as_y: f64,
    #[serde(default)]
    pub as_z: f64,
    #[serde(default)]
    pub panel_thickness: Option<f64>,
    #[serde(default)]
    pub thickness: Option<f64>,
    /// パラメトリック形状定義。形状から生成されなかった断面は None。
    #[serde(default)]
    pub shape: Option<crate::section_shape::SectionShape>,
    /// 主材料（この断面の弾性剛性 E・ν と自重の密度を決める材料）。
    ///
    /// `None` は未割当。
    #[serde(default)]
    pub material: Option<MaterialId>,
    /// 主筋の材料（RC・SRC 断面のみ意味を持つ）。
    #[serde(default)]
    pub rebar_material: Option<MaterialId>,
    /// せん断補強筋の材料（RC・SRC 断面のみ意味を持つ）。`None` は未設定。
    #[serde(default)]
    pub shear_rebar_material: Option<MaterialId>,
    /// SRC 断面の内蔵鉄骨、CFT 断面の鋼管の材料。
    #[serde(default)]
    pub steel_material: Option<MaterialId>,
    pub property_basis: SectionPropertyBasis,
}

/// 各断面性能が形状算定値か、カタログ・直接入力値かを表す。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PropertyBasis {
    Shape,
    /// 形状算定に必要な入力が未知で未算定。数値欄の格納値は計算に使わない。
    PendingShape,
    #[default]
    Supplied,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SectionPropertyBasis {
    pub area: PropertyBasis,
    pub iy: PropertyBasis,
    pub iz: PropertyBasis,
    pub j: PropertyBasis,
    pub depth: PropertyBasis,
    pub width: PropertyBasis,
    pub as_y: PropertyBasis,
    pub as_z: PropertyBasis,
}

impl SectionPropertyBasis {
    pub const SHAPE: Self = Self {
        area: PropertyBasis::Shape,
        iy: PropertyBasis::Shape,
        iz: PropertyBasis::Shape,
        j: PropertyBasis::Shape,
        depth: PropertyBasis::Shape,
        width: PropertyBasis::Shape,
        as_y: PropertyBasis::Shape,
        as_z: PropertyBasis::Shape,
    };
}

/// 断面の同一性キー（符号＋階）。モデル内で重複してはならない。
pub type SectionKey<'a> = (&'a str, Option<&'a str>);

impl Section {
    /// リップ溝形の寸法と性能を検証する。他形状・形状なし断面は対象外。
    pub fn validate_lip_channel(&self) -> Result<(), String> {
        let Some(
            shape @ crate::section_shape::SectionShape::SteelLipChannel {
                height,
                width,
                lip,
                thick,
            },
        ) = &self.shape
        else {
            return Ok(());
        };
        let check = || {
            shape.validate_lip_channel()?;
            for (name, value) in [
                ("A", self.area),
                ("Iy", self.iy),
                ("Iz", self.iz),
                ("J", self.resolved_j()?),
                ("せい", self.depth),
                ("幅", self.width),
                ("Asy", self.as_y),
                ("Asz", self.as_z),
            ] {
                if !value.is_finite() || value < 0.0 {
                    return Err(format!(
                        "リップ溝形 H={height}, B={width}, C={lip}, t={thick} mm: {name} は有限な非負値が必要です"
                    ));
                }
            }
            Ok(())
        };
        check().map_err(|reason| format!("断面 {}「{}」: {reason}", self.id.0, self.name))
    }

    pub fn ensure_properties_resolved(&self) -> Result<(), String> {
        self.validate_lip_channel()?;
        let basis = self.property_basis;
        for (name, value) in [
            ("A", self.area),
            ("Iy", self.iy),
            ("Iz", self.iz),
            ("J", self.resolved_j()?),
            ("せい", self.depth),
            ("幅", self.width),
            ("Asy", self.as_y),
            ("Asz", self.as_z),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!(
                    "断面「{}」の {name} は有限な非負値が必要です",
                    self.name
                ));
            }
        }
        if let Some(shape) = &self.shape {
            shape.validate_surface_radius()?;
            if [basis.area, basis.iy, basis.iz].contains(&PropertyBasis::Shape) {
                shape.rounded_steel_properties()?;
            }
        }
        for (name, source) in [
            ("A", basis.area),
            ("Iy", basis.iy),
            ("Iz", basis.iz),
            ("J", basis.j),
            ("せい", basis.depth),
            ("幅", basis.width),
            ("Asy", basis.as_y),
            ("Asz", basis.as_z),
        ] {
            if source == PropertyBasis::PendingShape {
                return Err(format!(
                    "断面「{}」の {name} は未算定です。フィレット半径・角Rを設定してください",
                    self.name
                ));
            }
        }
        Ok(())
    }

    /// ねじり定数 J `[mm⁴]`。供給値を優先し、形状算定では現在の形状を用いる。
    /// 未算定・形状欠落・適用域外・不正な供給値はエラー。CFT は鋼管単体の値。
    pub fn resolved_j(&self) -> Result<f64, String> {
        let error = |reason| format!("断面 {}「{}」の J: {reason}", self.id.0, self.name);
        match self.property_basis.j {
            PropertyBasis::Supplied => {
                if self.j.is_finite() && self.j >= 0.0 {
                    Ok(self.j)
                } else {
                    Err(error("供給値は有限な非負値が必要です".into()))
                }
            }
            PropertyBasis::Shape => self
                .shape
                .as_ref()
                .ok_or_else(|| error("形状が未設定です".into()))?
                .try_calc_j()
                .map_err(error),
            PropertyBasis::PendingShape => {
                let reason = self
                    .shape
                    .as_ref()
                    .and_then(|shape| shape.try_calc_j().err())
                    .unwrap_or_else(|| "形状性能が未算定です".into());
                Err(error(format!("未算定です。{reason}")))
            }
        }
    }

    pub fn resolved_area(&self) -> Result<f64, String> {
        self.validate_lip_channel()?;
        if !self.area.is_finite() || self.area < 0.0 {
            return Err(format!(
                "断面「{}」の A は有限な非負値が必要です",
                self.name
            ));
        }
        if let Some(shape) = &self.shape {
            shape.validate_surface_radius()?;
            if self.property_basis.area == PropertyBasis::Shape {
                shape.rounded_steel_properties()?;
            }
        }
        if self.property_basis.area == PropertyBasis::PendingShape {
            return Err(format!(
                "断面「{}」の A は未算定です。フィレット半径・角Rを設定してください",
                self.name
            ));
        }
        Ok(self.area)
    }

    /// フィレット半径・角Rを変更し、形状算定の A・Iy・Iz・J を更新する。
    /// 未知半径・J の適用域外は未算定として保持し、不正な半径・寸法はエラー。
    /// 材料・用途・階と供給性能は維持する。
    pub fn with_surface_radius(&self, radius_mm: Option<f64>) -> Result<Self, String> {
        use crate::section_shape::SectionShape;
        let mut result = self.clone();
        let shape = result.shape.as_mut().ok_or("断面形状が未設定です")?;
        match shape {
            SectionShape::SteelH { root_r, .. } => *root_r = radius_mm,
            SectionShape::SteelBox { corner_r, .. } | SectionShape::CftBox { corner_r, .. } => {
                *corner_r = radius_mm
            }
            _ => return Err("フィレット半径・角Rの編集対象ではありません".into()),
        }
        shape.validate_surface_radius()?;
        let basis = self.property_basis;
        let derived = |source| source != PropertyBasis::Supplied;
        if [basis.area, basis.iy, basis.iz].into_iter().any(derived) {
            let p = match shape.rounded_steel_properties() {
                Err(error) if radius_mm.is_some() => return Err(error),
                properties => properties,
            };
            for (source, value, result_basis, resolved) in [
                (
                    basis.area,
                    &mut result.area,
                    &mut result.property_basis.area,
                    p.as_ref().ok().and_then(|p| p.as_ref()).map(|p| p.area),
                ),
                (
                    basis.iy,
                    &mut result.iy,
                    &mut result.property_basis.iy,
                    p.as_ref().ok().and_then(|p| p.as_ref()).map(|p| p.iy),
                ),
                (
                    basis.iz,
                    &mut result.iz,
                    &mut result.property_basis.iz,
                    p.as_ref().ok().and_then(|p| p.as_ref()).map(|p| p.iz),
                ),
            ] {
                if derived(source) {
                    *value = resolved.unwrap_or(0.0);
                    *result_basis = if resolved.is_some() {
                        PropertyBasis::Shape
                    } else {
                        PropertyBasis::PendingShape
                    };
                }
            }
        }
        if derived(basis.j) {
            match shape.try_calc_j() {
                Ok(j) => {
                    result.j = j;
                    result.property_basis.j = PropertyBasis::Shape;
                }
                Err(_) => {
                    result.j = 0.0;
                    result.property_basis.j = PropertyBasis::PendingShape;
                }
            }
        }
        Ok(result)
    }

    pub fn is_cft(&self) -> bool {
        matches!(
            self.shape,
            Some(
                crate::section_shape::SectionShape::CftBox { .. }
                    | crate::section_shape::SectionShape::CftPipe { .. }
            )
        )
    }

    /// 小梁・間柱には CFT 断面を割り当てられない（柱用途でも禁止）。
    pub fn secondary_member_use_allowed(&self) -> bool {
        !self.is_cft()
    }

    /// RC 専用型の主架構用途。その他の型は用途を限定しない。
    pub fn rc_frame_use(&self) -> Option<FrameSectionUse> {
        use crate::section_shape::SectionShape;
        match self.shape {
            Some(SectionShape::RcBeamRect { .. }) => Some(FrameSectionUse::Girder),
            Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. }) => {
                Some(FrameSectionUse::Column)
            }
            _ => None,
        }
    }

    /// RC 専用型と明示した主架構用途の不一致を拒否する。未使用・二次専用の未設定は許容する。
    pub fn validate_rc_frame_use(&self) -> Result<(), String> {
        if let Some(expected) = self.rc_frame_use() {
            if self.frame_use.is_some_and(|usage| usage != expected) {
                return Err(self.rc_purpose_error());
            }
        }
        Ok(())
    }

    fn rc_purpose_error(&self) -> String {
        let reason = match self.rc_frame_use() {
            Some(FrameSectionUse::Girder) => "RC 梁用断面は大梁・小梁専用です",
            _ => "RC 柱用断面は柱・間柱専用です",
        };
        format!("SectionId({}): {reason}", self.id.0)
    }

    /// RC 梁・柱の専用型は床板・壁版へ割り当てられない。他型は制限しない。
    pub fn validate_rc_plate_use(&self) -> Result<(), String> {
        if self.rc_frame_use().is_some() {
            return Err(self.rc_purpose_error());
        }
        Ok(())
    }

    /// RC 専用型を主架構に参照する場合、対応用途を持つ曲げフレームに限る。
    pub fn validate_rc_element_use(&self, kind: ElementKind) -> Result<(), String> {
        if let Some(expected) = self.rc_frame_use() {
            if self.frame_use != Some(expected)
                || !matches!(
                    kind,
                    ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
                )
            {
                return Err(self.rc_purpose_error());
            }
        }
        Ok(())
    }

    /// RC 梁型は小梁、RC 柱型は間柱に限る。二次専用断面の主架構用途は未設定でもよい。
    pub fn validate_rc_secondary_use(&self, kind: SecondaryMemberKind) -> Result<(), String> {
        self.validate_rc_frame_use()?;
        if let Some(expected) = self.rc_frame_use() {
            let actual = match kind {
                SecondaryMemberKind::Beam => FrameSectionUse::Girder,
                SecondaryMemberKind::Post => FrameSectionUse::Column,
            };
            if expected != actual {
                return Err(self.rc_purpose_error());
            }
        }
        Ok(())
    }

    pub fn cft_frame_use_allowed(&self) -> bool {
        !self.is_cft() || self.frame_use == Some(FrameSectionUse::Column)
    }

    /// 物性がすべてゼロ・形状も材料も持たない断面。
    pub fn zero(id: SectionId, name: String) -> Self {
        Self {
            id,
            name,
            frame_use: None,
            floor: None,
            area: 0.0,
            iy: 0.0,
            iz: 0.0,
            j: 0.0,
            depth: 0.0,
            width: 0.0,
            as_y: 0.0,
            as_z: 0.0,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: None,
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: SectionPropertyBasis::default(),
        }
    }

    /// 同一性キー（符号＋階）を借用で返す。
    pub fn key(&self) -> SectionKey<'_> {
        (self.name.as_str(), self.floor.as_deref())
    }

    /// 表示用のラベル。階を持つ断面は `C1 (2)`、持たない断面は符号のみ。
    pub fn display_name(&self) -> String {
        match &self.floor {
            Some(f) => format!("{} ({})", self.name, f),
            None => self.name.clone(),
        }
    }

    /// 断面性能・形状・材料が一致するか（同一性キーは見ない）。
    pub fn properties_eq(&self, other: &Section) -> bool {
        self.area == other.area
            && self.iy == other.iy
            && self.iz == other.iz
            && self.j == other.j
            && self.depth == other.depth
            && self.width == other.width
            && self.as_y == other.as_y
            && self.as_z == other.as_z
            && self.panel_thickness == other.panel_thickness
            && self.thickness == other.thickness
            && self.shape == other.shape
            && self.material == other.material
            && self.rebar_material == other.rebar_material
            && self.shear_rebar_material == other.shear_rebar_material
            && self.steel_material == other.steel_material
            && self.property_basis == other.property_basis
    }
}

/// `sections` に符号＋階が `key` と一致する断面があるか（`skip` の添字は除く）。
pub fn section_key_taken(sections: &[Section], key: SectionKey<'_>, skip: Option<usize>) -> bool {
    sections
        .iter()
        .enumerate()
        .any(|(i, s)| Some(i) != skip && s.key() == key)
}

impl Model {
    /// 要素の断面を [`Self::section`] と同じ優先順で返す。未割当・参照先欠落は `None`。
    pub fn element_section(&self, elem: &ElementData) -> Option<&Section> {
        self.section(elem.section?)
    }

    /// 要素の主材料（弾性剛性 E・ν と自重の密度を決める材料）。
    /// 断面が未割当、または断面が材料を持たない場合は `None`。
    pub fn element_material(&self, elem: &ElementData) -> Option<&Material> {
        if elem.kind == ElementKind::Wall {
            if let Some(origin) = self.generated_wall_origins.get(&elem.id) {
                return self.wall_plate_material(self.wall_plate(*origin)?);
            }
        }
        if self
            .stb_strengths
            .members
            .iter()
            .any(|m| m.target == StrengthTarget::Element(elem.id))
        {
            return self.stb_concrete_material(StrengthTarget::Element(elem.id));
        }
        let section = self.element_section(elem)?;
        if section
            .shape
            .as_ref()
            .is_some_and(|shape| !shape.is_concrete_like())
            && self
                .stb_strengths
                .sections
                .iter()
                .any(|input| input.section == section.id && !input.steel.is_empty())
        {
            return self.stb_steel_material(section.id);
        }
        self.materials.get(section.material?.index())
    }

    /// 二次部材（小梁・間柱）の主材料（自重算定に用いる）。
    /// 規約は [`Model::element_material`] と同じ。
    pub fn secondary_material(&self, sm: &SecondaryMember) -> Option<&Material> {
        if self
            .stb_strengths
            .members
            .iter()
            .any(|m| m.target == StrengthTarget::Secondary(sm.id))
        {
            return self.stb_concrete_material(StrengthTarget::Secondary(sm.id));
        }
        let sec = self.sections.get(sm.section?.index())?;
        if sec
            .shape
            .as_ref()
            .is_some_and(|shape| !shape.is_concrete_like())
            && self
                .stb_strengths
                .sections
                .iter()
                .any(|input| input.section == sec.id && !input.steel.is_empty())
        {
            return self.stb_steel_material(sec.id);
        }
        self.materials.get(sec.material?.index())
    }

    /// 要素の主筋材料（RC・SRC 断面のみ）。
    pub fn element_rebar_material(&self, elem: &ElementData) -> Option<&Material> {
        if matches!(
            self.element_section(elem)?.shape,
            Some(crate::section_shape::SectionShape::RcWall { .. })
        ) {
            if self.stb_strengths.sections.iter().any(|s| {
                Some(s.section) == elem.section
                    && s.reinforcement.iter().any(|r| r.part == "vertical")
            }) {
                return self.stb_rebar_material(elem.section?, "vertical");
            }
            return self
                .materials
                .get(self.element_section(elem)?.rebar_material?.index());
        }
        if self.stb_strengths.sections.iter().any(|s| {
            Some(s.section) == elem.section && s.reinforcement.iter().any(|r| r.part == "main")
        }) {
            return self.stb_rebar_material(elem.section?, "main");
        }
        self.materials
            .get(self.element_section(elem)?.rebar_material?.index())
    }

    /// 要素のせん断補強筋材料（RC・SRC 断面のみ）。
    pub fn element_shear_rebar_material(&self, elem: &ElementData) -> Option<&Material> {
        if matches!(
            self.element_section(elem)?.shape,
            Some(crate::section_shape::SectionShape::RcWall { .. })
        ) {
            if self.stb_strengths.sections.iter().any(|s| {
                Some(s.section) == elem.section
                    && s.reinforcement.iter().any(|r| r.part == "horizontal")
            }) {
                return self.stb_rebar_material(elem.section?, "horizontal");
            }
            return self
                .materials
                .get(self.element_section(elem)?.shear_rebar_material?.index());
        }
        if let Some(input) = self
            .stb_strengths
            .sections
            .iter()
            .find(|s| Some(s.section) == elem.section)
        {
            let part = if self.element_section(elem)?.frame_use == Some(FrameSectionUse::Column) {
                "band"
            } else {
                "stirrup"
            };
            if input.reinforcement.iter().any(|r| r.part == part) {
                return self.stb_rebar_material(elem.section?, part);
            }
        }
        self.materials
            .get(self.element_section(elem)?.shear_rebar_material?.index())
    }

    /// 二次部材の主筋材料。
    pub fn secondary_rebar_material(&self, sm: &SecondaryMember) -> Option<&Material> {
        let section = sm.section?;
        if self
            .stb_strengths
            .sections
            .iter()
            .any(|s| s.section == section && s.reinforcement.iter().any(|r| r.part == "main"))
        {
            return self.stb_rebar_material(section, "main");
        }
        self.materials
            .get(self.section(section)?.rebar_material?.index())
    }

    /// 二次部材の帯筋・あばら筋材料。
    pub fn secondary_shear_rebar_material(&self, sm: &SecondaryMember) -> Option<&Material> {
        let section = sm.section?;
        let part = if sm.kind == super::SecondaryMemberKind::Post {
            "band"
        } else {
            "stirrup"
        };
        if self
            .stb_strengths
            .sections
            .iter()
            .any(|s| s.section == section && s.reinforcement.iter().any(|r| r.part == part))
        {
            return self.stb_rebar_material(section, part);
        }
        self.materials
            .get(self.section(section)?.shear_rebar_material?.index())
    }

    /// 要素の内蔵鉄骨・鋼管材料。
    pub fn element_steel_material(&self, elem: &ElementData) -> Option<&Material> {
        let section = self.element_section(elem)?;
        if self
            .stb_strengths
            .sections
            .iter()
            .any(|input| input.section == section.id && !input.steel.is_empty())
        {
            return self.stb_steel_material(section.id);
        }
        self.materials.get(section.steel_material?.index())
    }
}
