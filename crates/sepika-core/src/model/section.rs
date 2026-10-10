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
    pub fn ensure_properties_resolved(&self) -> Result<(), String> {
        let basis = self.property_basis;
        for (name, value) in [
            ("A", self.area),
            ("Iy", self.iy),
            ("Iz", self.iz),
            ("J", self.j),
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

    pub fn resolved_area(&self) -> Result<f64, String> {
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

    /// フィレット半径・角Rを変更し、形状算定の A・Iy・Iz だけを更新する。
    /// 算定不能なら元の断面を変更せずエラー。材料・用途・階と入力性能は維持する。
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
            let p = shape
                .rounded_steel_properties()?
                .ok_or("鋼材性能の算定対象ではありません")?;
            if derived(basis.area) {
                result.area = p.area;
                result.property_basis.area = PropertyBasis::Shape;
            }
            if derived(basis.iy) {
                result.iy = p.iy;
                result.property_basis.iy = PropertyBasis::Shape;
            }
            if derived(basis.iz) {
                result.iz = p.iz;
                result.property_basis.iz = PropertyBasis::Shape;
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
    /// 要素の断面。
    pub fn element_section(&self, elem: &ElementData) -> Option<&Section> {
        self.sections.get(elem.section?.index())
    }

    /// 要素の主材料（弾性剛性 E・ν と自重の密度を決める材料）。
    /// 断面が未割当、または断面が材料を持たない場合は `None`。
    pub fn element_material(&self, elem: &ElementData) -> Option<&Material> {
        if self
            .stb_strengths
            .members
            .iter()
            .any(|m| m.target == StrengthTarget::Element(elem.id))
        {
            return self.stb_concrete_material(StrengthTarget::Element(elem.id));
        }
        self.materials
            .get(self.element_section(elem)?.material?.index())
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
        self.materials.get(sec.material?.index())
    }

    /// 要素の主筋材料（RC・SRC 断面のみ）。
    pub fn element_rebar_material(&self, elem: &ElementData) -> Option<&Material> {
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
        self.materials
            .get(self.element_section(elem)?.steel_material?.index())
    }
}
