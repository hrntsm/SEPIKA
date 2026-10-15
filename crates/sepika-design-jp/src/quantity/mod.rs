//! 数量積算（部位別の概算数量集計）。
//!
//! 建物モデルから部位別（柱・大梁・小梁・基礎梁・床・壁・ブレース）に
//! コンクリート体積・型枠面積・鉄筋重量・鉄骨重量を概算集計する。
//! 構造計算モデルを唯一の入力とする概算数量であり、詳細積算
//! （フック・余長・継手長さ・開口補強筋・接合部プレート等）は
//! 考慮しない（各部位共通事項）。
//!
//! - [`crate::quantity::QuantityCfg`] — 積算の設定（定着長さ係数・鉄筋比等）
//! - [`crate::quantity::try_compute_quantity_takeoff`] — モデル全体の数量集計
//! - [`crate::quantity::QuantityTakeoff`] / [`crate::quantity::MemberQuantity`] — 集計結果
//! - [`crate::quantity::member`] — 部位別の算定式（純関数）
//! - [`crate::quantity::rebar`] — 鉄筋の単位質量
//!
//! ## 単位
//!
//! モデルの内部単位系は N-mm-s だが、集計結果は実務慣用単位で持つ:
//! コンクリート体積 `[m³]`・型枠面積 `[m²]`・鉄筋/鉄骨重量 `[t]`・長さ `[m]`。
//! 鉄骨重量は `W = L×A×7.85`（鉄骨単位重量 7.85 t/m³ 固定。
//! 各部位共通事項）による。

mod contact;
pub mod member;
pub mod rebar;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};

use sepika_core::ids::{ElemId, SecondaryMemberId, SectionId, SlabId, StoryId, WallPlateId};
use sepika_core::model::{
    ElementData, ElementKind, Model, RegionAnchor, SecondaryMember, SecondaryMemberEnds,
    SecondaryMemberKind, Section, Slab, SupportMemberId, WallPlate, WallPlateShape,
};
use sepika_core::section_shape::{
    RcBeamRebar, RcCircleColumnRebar, RcRectColumnRebar, SectionShape,
};

use member::{BeamBarEnd, Haunch};

/// 鉄骨・鉄筋の単位重量 [t/m³]（数量積算の慣用値 7.85 t/m³。各部位共通事項）。
/// 固定荷重の設計用単位体積重量 γs=78.5 kN/m³（`sepika_core::units::STEEL_UNIT_WEIGHT_KN_M3`）
/// とは用途が別の独立値である（`sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3` 参照）。
const STEEL_UNIT_WEIGHT_T_PER_M3: f64 = sepika_core::units::STEEL_UNIT_WEIGHT_TAKEOFF_T_M3;

/// 鉄骨単位重量 [t/mm³]（[`STEEL_UNIT_WEIGHT_T_PER_M3`] の内部単位系換算）。
const STEEL_UNIT_WEIGHT_T_PER_MM3: f64 = STEEL_UNIT_WEIGHT_T_PER_M3 * 1e-9;

/// 「同一レベル」とみなす標高差 [mm]（基礎梁レベルの判定用）。
const LEVEL_TOL_MM: f64 = 10.0;

pub use sepika_core::model::QuantityCfg;

/// 部位分類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberCategory {
    /// 基礎梁（最下層レベルで柱に取り付く大梁用途の線材）。
    FoundationGirder,
    /// 柱。
    Column,
    /// 大梁（柱に取り付く大梁用途の線材）。
    Girder,
    /// 小梁（二次小梁、または柱に取り付かない大梁用途の線材）。
    Beam,
    /// 床（一般スラブ）。
    Slab,
    /// 片持ち床・出隅。
    CantileverSlab,
    /// 壁（耐震壁・フレーム内雑壁）。
    Wall,
    /// フレーム外雑壁。
    MiscWall,
    /// ブレース。
    Brace,
}

impl MemberCategory {
    /// 表示名（日本語）。
    pub fn label(self) -> &'static str {
        match self {
            MemberCategory::FoundationGirder => "基礎梁",
            MemberCategory::Column => "柱",
            MemberCategory::Girder => "大梁",
            MemberCategory::Beam => "小梁",
            MemberCategory::Slab => "床",
            MemberCategory::CantileverSlab => "片持ち床",
            MemberCategory::Wall => "壁",
            MemberCategory::MiscWall => "雑壁",
            MemberCategory::Brace => "ブレース",
        }
    }
}

pub use sepika_core::structure_kind::StructureKind;

/// 鉄筋の用途分類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum RebarUsage {
    /// 柱・梁の主筋。
    MainBar,
    /// 柱フープ。
    Hoop,
    /// 梁スターラップ。
    Stirrup,
    /// 壁横筋。
    WallHorizontal,
    /// 壁縦筋。
    WallVertical,
    /// 床筋（鉄筋比による概算）。
    SlabBar,
    /// 小梁主筋（鉄筋比による概算）。
    BeamMain,
    /// 小梁スターラップ（鉄筋比による概算）。
    BeamStirrup,
}

impl RebarUsage {
    /// 表示名（日本語）。
    pub fn label(self) -> &'static str {
        match self {
            RebarUsage::MainBar => "主筋",
            RebarUsage::Hoop => "フープ",
            RebarUsage::Stirrup => "スターラップ",
            RebarUsage::WallHorizontal => "壁横筋",
            RebarUsage::WallVertical => "壁縦筋",
            RebarUsage::SlabBar => "床筋",
            RebarUsage::BeamMain => "小梁主筋",
            RebarUsage::BeamStirrup => "小梁スターラップ",
        }
    }
}

/// 鉄筋数量 1 件（用途×呼び径ごと）。
#[derive(Clone, Debug)]
pub struct RebarItem {
    pub usage: RebarUsage,
    /// 呼び径 `[mm]`。鉄筋比による概算（床・小梁）は None。
    pub dia: Option<f64>,
    /// 総長さ `[m]`（鉄筋比による概算は 0）。
    pub total_length_m: f64,
    /// 重量 `[t]`。
    pub weight_t: f64,
}

/// 鉄骨数量 1 件。
#[derive(Clone, Debug)]
pub struct SteelItem {
    /// 断面名（種類別集計のキー）。
    pub section_name: String,
    /// 長さ `[m]`。
    pub length_m: f64,
    /// 重量 `[t]`（W = L×A×7.85）。
    pub weight_t: f64,
}

/// 未算定鉄筋の理由の種別。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RebarIssueKind {
    Missing,
    Invalid,
    Unsupported,
}

/// 未算定対象の ID・方向・理由。算定済み数量には加算しない。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct RebarIssue {
    pub elem: Option<ElemId>,
    pub wall_plate: Option<WallPlateId>,
    pub section: Option<SectionId>,
    pub usage: RebarUsage,
    pub kind: RebarIssueKind,
    pub reason: String,
}
impl RebarIssue {
    /// 対象 ID と方向を含む利用者向け診断。
    pub fn description(&self) -> String {
        format!(
            "要素 {:?} / 壁版 {:?} / 断面 {:?} / {}: {}",
            self.elem.map(|id| id.0),
            self.wall_plate.map(|id| id.0),
            self.section.map(|id| id.0),
            self.usage.label(),
            self.reason
        )
    }
}

/// 鉄筋数量の完全性。完全でも実配筋との照合を意味しない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityStatus {
    Empty,
    Complete,
    Incomplete,
}
impl QuantityStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "対象なし",
            Self::Complete => "算定済み概算",
            Self::Incomplete => "不完全小計",
        }
    }
}

/// 部材（またはスラブ・雑壁）1 件分の数量。
#[derive(Clone, Debug)]
pub struct MemberQuantity {
    /// 対象要素（スラブ・雑壁は None）。
    pub elem: Option<ElemId>,
    /// 対象二次部材。実部材へ集約した明細は None。
    pub secondary: Option<SecondaryMemberId>,
    /// 元の壁版。直接入力された要素は None。
    pub wall_plate: Option<WallPlateId>,
    /// 算定対象の断面 ID。
    pub section: Option<SectionId>,
    /// 対象床板（部材・雑壁は None）。
    pub slab: Option<SlabId>,
    /// 符号（断面名等）。
    pub label: String,
    /// 所属階名（未設定は "-"）。
    pub story: String,
    pub category: MemberCategory,
    pub structure: StructureKind,
    /// コンクリート体積 `[m³]`。
    pub concrete_m3: f64,
    /// 型枠面積 `[m²]`。
    pub formwork_m2: f64,
    /// 鉄筋数量（用途別）。
    pub rebar: Vec<RebarItem>,
    /// 未算定方向と理由。空なら対象鉄筋は算定済み概算。
    pub rebar_issues: Vec<RebarIssue>,
    /// 鉄骨数量（S・SRC・CFT・ブレース）。
    pub steel: Option<SteelItem>,
    /// 鉄筋継手個所数（圧接個所数の集計。柱・大梁・基礎梁）。
    pub rebar_joints: f64,
}

impl MemberQuantity {
    pub fn rebar_status(&self) -> QuantityStatus {
        if self.rebar_issues.is_empty() {
            QuantityStatus::Complete
        } else {
            QuantityStatus::Incomplete
        }
    }
    /// 算定済み鉄筋重量の小計 `[t]`。完全性は `rebar_status` を併用する。
    pub fn rebar_weight_t(&self) -> f64 {
        self.rebar.iter().map(|r| r.weight_t).sum::<f64>() + 0.0
    }

    /// 鉄骨重量 `[t]`（なければ 0）。
    pub fn steel_weight_t(&self) -> f64 {
        self.steel.as_ref().map(|s| s.weight_t).unwrap_or(0.0)
    }
}

/// 数量の小計（コンクリート・型枠・鉄筋・鉄骨・継手）。
#[derive(Clone, Debug, Default)]
pub struct QuantityTotals {
    pub member_count: usize,
    pub rebar_issues: Vec<RebarIssue>,
    pub concrete_m3: f64,
    pub formwork_m2: f64,
    /// 算定済み鉄筋重量の小計 `[t]`。完全性は `rebar_status` を併用する。
    pub rebar_t: f64,
    pub steel_t: f64,
    pub rebar_joints: f64,
}

impl QuantityTotals {
    pub fn rebar_status(&self) -> QuantityStatus {
        if self.member_count == 0 {
            QuantityStatus::Empty
        } else if self.rebar_issues.is_empty() {
            QuantityStatus::Complete
        } else {
            QuantityStatus::Incomplete
        }
    }
    fn add(&mut self, it: &MemberQuantity) {
        self.member_count += 1;
        self.rebar_issues.extend(it.rebar_issues.clone());
        self.concrete_m3 += it.concrete_m3;
        self.formwork_m2 += it.formwork_m2;
        self.rebar_t += it.rebar_weight_t();
        self.steel_t += it.steel_weight_t();
        self.rebar_joints += it.rebar_joints;
    }
}

/// 数量積算の結果一式。
#[derive(Clone, Debug, Default)]
pub struct QuantityTakeoff {
    /// 部材・スラブ・雑壁ごとの明細。
    pub items: Vec<MemberQuantity>,
    /// 前提・未対応事項の注記。
    pub notes: Vec<String>,
}

impl QuantityTakeoff {
    /// 全体の算定済み小計。未算定理由と対象なしの状態を保持する。
    pub fn totals(&self) -> QuantityTotals {
        let mut t = QuantityTotals::default();
        for it in &self.items {
            t.add(it);
        }
        t
    }

    /// 部位別小計（`MemberCategory` の定義順）。
    pub fn totals_by_category(&self) -> Vec<(MemberCategory, QuantityTotals)> {
        let mut map: BTreeMap<MemberCategory, QuantityTotals> = BTreeMap::new();
        for it in &self.items {
            map.entry(it.category).or_default().add(it);
        }
        map.into_iter().collect()
    }

    /// 階別小計（明細の出現順を保持）。
    pub fn totals_by_story(&self) -> Vec<(String, QuantityTotals)> {
        let mut order: Vec<String> = Vec::new();
        let mut map: HashMap<String, QuantityTotals> = HashMap::new();
        for it in &self.items {
            if !map.contains_key(&it.story) {
                order.push(it.story.clone());
            }
            map.entry(it.story.clone()).or_default().add(it);
        }
        order
            .into_iter()
            .map(|k| {
                let v = map.remove(&k).unwrap();
                (k, v)
            })
            .collect()
    }

    /// 構造種別の小計。
    pub fn totals_by_structure(&self) -> Vec<(StructureKind, QuantityTotals)> {
        let mut rows: Vec<(StructureKind, QuantityTotals)> = Vec::new();
        for it in &self.items {
            if let Some((_, t)) = rows.iter_mut().find(|(k, _)| *k == it.structure) {
                t.add(it);
            } else {
                let mut t = QuantityTotals::default();
                t.add(it);
                rows.push((it.structure, t));
            }
        }
        rows
    }

    /// 鉄骨の種類別（断面名別）長さ・重量集計。
    pub fn steel_by_section(&self) -> Vec<SteelItem> {
        let mut order: Vec<String> = Vec::new();
        let mut map: HashMap<String, (f64, f64)> = HashMap::new();
        for it in self.items.iter().filter_map(|i| i.steel.as_ref()) {
            if !map.contains_key(&it.section_name) {
                order.push(it.section_name.clone());
            }
            let e = map.entry(it.section_name.clone()).or_default();
            e.0 += it.length_m;
            e.1 += it.weight_t;
        }
        order
            .into_iter()
            .map(|name| {
                let (l, w) = map[&name];
                SteelItem {
                    section_name: name,
                    length_m: l,
                    weight_t: w,
                }
            })
            .collect()
    }

    /// 鉄筋の呼び径別長さ・重量集計（径 None＝鉄筋比概算は径 0 に集約）。
    pub fn rebar_by_dia(&self) -> Vec<(f64, f64, f64)> {
        let mut map: BTreeMap<u32, (f64, f64)> = BTreeMap::new();
        for it in &self.items {
            for r in &it.rebar {
                let key = r.dia.unwrap_or(0.0).round() as u32;
                let e = map.entry(key).or_default();
                e.0 += r.total_length_m;
                e.1 += r.weight_t;
            }
        }
        map.into_iter()
            .map(|(d, (l, w))| (d as f64, l, w))
            .collect()
    }
}

/// 2 点間距離 [mm]。算定の情報源は `sepika-core` に置く。
use sepika_core::geom::vec3::dist as dist3;

/// 主架構線材の明示用途（柱・大梁・ブレース）。
use sepika_core::model::FrameSectionUse;

/// 平面多角形（3D 座標）の面積 [mm²]。算定の情報源は `sepika-core` に置く。
use sepika_core::geom::polygon::area_3d as polygon_area_3d;

/// SRC 内蔵 H 形鉄骨の断面積 [mm²]。
fn src_steel_area(shape: &SectionShape) -> Option<f64> {
    match *shape {
        SectionShape::SrcBeamRect {
            steel_height,
            steel_width,
            steel_web_thick,
            steel_flange_thick,
            ..
        }
        | SectionShape::SrcColumnRect {
            steel_height,
            steel_width,
            steel_web_thick,
            steel_flange_thick,
            ..
        } => Some(
            2.0 * steel_width * steel_flange_thick
                + (steel_height - 2.0 * steel_flange_thick) * steel_web_thick,
        ),
        _ => None,
    }
}

/// CFT の充填コンクリート断面積 [mm²]。
fn cft_infill_area(shape: &SectionShape) -> Option<f64> {
    shape.cft_core_props().map(|core| core.area)
}

fn quantity_area(section: &Section) -> Result<f64, String> {
    let Some(shape) = &section.shape else {
        return section.resolved_area();
    };
    shape.validate_surface_radius()?;
    match shape.try_calc_area() {
        Ok(area) => Ok(area),
        Err(_)
            if matches!(
                shape,
                SectionShape::SteelH { root_r: None, .. }
                    | SectionShape::SteelBox { corner_r: None, .. }
            ) && section.property_basis.area == sepika_core::model::PropertyBasis::Supplied =>
        {
            section.resolved_area()
        }
        Err(error) => Err(error),
    }
}

fn validate_circular_rc_posts(model: &Model) -> Result<(), String> {
    for sm in model
        .beams()
        .chain(model.posts())
        .filter(|sm| sm.kind == SecondaryMemberKind::Post)
    {
        let Some(sec) = sm.section.and_then(|id| model.sections.get(id.index())) else {
            continue;
        };
        let Some(mat) = model.secondary_material(sm) else {
            continue;
        };
        if sepika_core::structure_kind::structure_kind_of(Some(sec), Some(mat.category))
            != StructureKind::Rc
        {
            continue;
        }
        let Some(SectionShape::RcColumnCircle { d, .. }) = sec.shape.as_ref() else {
            continue;
        };
        if !d.is_finite() || *d <= 0.0 {
            return Err(format!(
                "{:?}: 円形RC間柱の直径 D [mm] は有限かつ正である必要があります（D={d}）",
                sm.id
            ));
        }
        let Some((ci, cj)) = model.secondary_member_end_points(sm) else {
            continue;
        };
        let length_mm = dist3(ci, cj);
        if !length_mm.is_finite() || length_mm <= 0.0 {
            return Err(format!("{:?}: 円形RC間柱の解決端点間実長 L [mm] は有限かつ正である必要があります（L={length_mm}）", sm.id));
        }
        if !member::circle_column_concrete_volume(*d, length_mm).is_finite()
            || !member::circle_column_formwork_area(*d, length_mm).is_finite()
        {
            return Err(format!(
                "{:?}: 円形RC間柱の数量が有限範囲を超えています",
                sm.id
            ));
        }
    }
    Ok(())
}

struct SecondaryQuantity {
    materialized: Option<(ElemId, bool)>,
    story: Option<StoryId>,
}

fn secondary_story(
    model: &Model,
    sm: &SecondaryMember,
    visiting: &mut HashSet<SecondaryMemberId>,
) -> Result<Option<StoryId>, String> {
    if !visiting.insert(sm.id) {
        return Err(format!("{:?}: 階帰属の支持参照が循環しています", sm.id));
    }
    let result = (|| {
        let (a, b) = model.secondary_member_end_points(sm).ok_or_else(|| {
            format!(
                "{:?}: 端点または支持参照を解決できません（循環を含む）",
                sm.id
            )
        })?;
        let lower = usize::from(a[2] > b[2]);
        let p = [a, b][lower];
        let stories: HashSet<_> = model
            .nodes
            .iter()
            .filter(|n| dist3(n.coord, p) <= sepika_core::geom::MEMBER_AXIS_TOL_MM)
            .filter_map(|n| n.story)
            .filter(|id| model.stories.get(id.index()).is_some())
            .collect();
        if stories.len() == 1 {
            return Ok(stories.into_iter().next());
        }
        let anchor = match sm.ends {
            SecondaryMemberEnds::Supported(anchors) => Some(anchors[lower]),
            SecondaryMemberEnds::Cantilever { support, .. } if lower == 0 => Some(support),
            _ => None,
        };
        let Some(anchor) = anchor else {
            return Ok(None);
        };
        match anchor.support {
            SupportMemberId::Primary(id) => {
                let Some(elem) = model.element(id).filter(|e| e.nodes.len() == 2) else {
                    return Ok(None);
                };
                let story = |i| {
                    model
                        .node(elem.nodes[i])
                        .and_then(|n| n.story)
                        .filter(|id| model.stories.get(id.index()).is_some())
                };
                Ok(story(0).filter(|id| Some(*id) == story(1)))
            }
            SupportMemberId::Secondary(id) => {
                let support = model
                    .secondary_member(id)
                    .ok_or_else(|| format!("{:?}: 支持二次部材 {id:?} が不明です", sm.id))?;
                secondary_story(model, support, visiting)
            }
        }
    })();
    visiting.remove(&sm.id);
    result
}

fn resolve_secondary_quantities(
    model: &Model,
) -> Result<HashMap<SecondaryMemberId, SecondaryQuantity>, String> {
    let mut resolved = HashMap::new();
    let mut materialized_ids = HashSet::new();
    for sm in model.beams().chain(model.posts()) {
        let error = |reason| format!("{:?}: {reason}", sm.id);
        let sec = sm
            .section
            .and_then(|id| model.sections.get(id.index()))
            .ok_or_else(|| {
                error(format!(
                    "断面 {:?} の参照が不明または未設定です",
                    sm.section
                ))
            })?;
        let mat = model
            .secondary_material(sm)
            .ok_or_else(|| error("主材料の参照が未設定または不明です".to_string()))?;
        let steel = if model
            .stb_strengths
            .sections
            .iter()
            .any(|input| input.section == sec.id && !input.steel.is_empty())
        {
            model.stb_steel_material(sec.id)
        } else {
            sec.steel_material
                .and_then(|id| model.materials.get(id.index()))
        };
        sepika_core::model::SectionMassProperties::try_from_section(
            sec,
            Some(mat),
            model.secondary_rebar_material(sm),
            model.secondary_shear_rebar_material(sm),
            steel,
        )
        .map_err(error)?;
        if sec.shape.is_none()
            && mat.category == sepika_core::model::MaterialCategory::Concrete
            && [sec.width, sec.depth]
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err(error(
                "断面幅・せい [mm] は有限かつ正である必要があります".into(),
            ));
        }
        let area = quantity_area(sec).map_err(error)?;
        if !area.is_finite() || area <= 0.0 {
            return Err(error("断面積は有限かつ正である必要があります".into()));
        }
        let (a, b) = model
            .secondary_member_end_points(sm)
            .ok_or_else(|| error("端点または支持参照を解決できません（循環を含む）".into()))?;
        let length_mm = dist3(a, b);
        if a.iter().chain(b.iter()).any(|v| !v.is_finite())
            || !length_mm.is_finite()
            || length_mm <= 0.0
        {
            return Err(error(
                "解決端点間実長 L [mm] は有限かつ正である必要があります".into(),
            ));
        }
        let equal = |p, q| dist3(p, q) <= sepika_core::geom::MEMBER_AXIS_TOL_MM;
        let candidates: Vec<_> = model
            .elements
            .iter()
            .filter_map(|e| {
                if e.kind != ElementKind::Beam || e.nodes.len() != 2 {
                    return None;
                }
                let (p, q) = (model.node(e.nodes[0])?.coord, model.node(e.nodes[1])?.coord);
                if equal(a, p) && equal(b, q) {
                    Some((e, false))
                } else if equal(a, q) && equal(b, p) {
                    Some((e, true))
                } else {
                    None
                }
            })
            .collect();
        let materialized = match candidates.as_slice() {
            [] => None,
            [(elem, reverse)] => {
                let expected_use = match sm.kind {
                    SecondaryMemberKind::Beam => FrameSectionUse::Girder,
                    SecondaryMemberKind::Post => FrameSectionUse::Column,
                };
                if elem.section != Some(sec.id)
                    || sec.frame_use != Some(expected_use)
                    || model.element_material(elem).map(|m| m.id) != Some(mat.id)
                {
                    return Err(error(format!(
                        "対応実材 {:?} の種別・用途・断面・解決材料が一致しません",
                        elem.id
                    )));
                }
                if !materialized_ids.insert(elem.id) {
                    return Err(error(format!(
                        "対応実材 {:?} を複数の二次部材が参照しています",
                        elem.id
                    )));
                }
                Some((elem.id, *reverse))
            }
            _ => {
                return Err(error(format!(
                    "対応実材が複数あります: {:?}",
                    candidates.iter().map(|(e, _)| e.id).collect::<Vec<_>>()
                )))
            }
        };
        if resolved
            .insert(
                sm.id,
                SecondaryQuantity {
                    materialized,
                    story: secondary_story(model, sm, &mut HashSet::new())?,
                },
            )
            .is_some()
        {
            return Err(error("二次部材IDが重複しています".into()));
        }
    }
    Ok(resolved)
}

/// 二次 CFT、必要な材料領域・床接触が未知、不正または適用対象外の場合は理由付きエラー。
/// 壁版は内部で展開して集計する。未参照断面の未入力は許容する。
/// 円形RC間柱の直径・解決実長 `[mm]` が有限かつ正でない場合は部材 ID と原因を返す。
pub fn try_compute_quantity_takeoff(
    model: &Model,
    cfg: &QuantityCfg,
) -> Result<QuantityTakeoff, String> {
    model
        .validate_frame_node_counts()
        .map_err(|e| e.to_string())?;
    model
        .validate_rc_section_usage()
        .map_err(|e| e.to_string())?;
    model
        .validate_secondary_section_usage()
        .map_err(|e| e.to_string())?;
    validate_circular_rc_posts(model)?;
    let secondary = resolve_secondary_quantities(model)?;
    model.validate_lip_channels().map_err(|e| e.to_string())?;
    let contacts = contact::Contacts::from_model(model, &secondary)?;
    let ids: HashSet<_> = model
        .elements
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                ElementKind::Beam
                    | ElementKind::Brace { .. }
                    | ElementKind::Shell
                    | ElementKind::Wall
            )
        })
        .map(|e| e.section)
        .chain(model.beams().chain(model.posts()).map(|m| m.section))
        .chain(model.slabs.iter().map(|s| s.section()))
        .chain(model.wall_plates.iter().map(|w| w.section))
        .flatten()
        .collect();
    for section in model.sections.iter().filter(|s| ids.contains(&s.id)) {
        quantity_area(section)?;
        if section.is_cft() {
            section
                .shape
                .as_ref()
                .ok_or("CFT の形状が未設定です")?
                .try_cft_core_props()?;
        }
    }
    let mut out = compute_quantity_takeoff_validated(model, cfg, &contacts, &secondary);
    for sm in model.beams().chain(model.posts()) {
        let item = out
            .items
            .iter_mut()
            .find(|item| match secondary[&sm.id].materialized {
                Some((elem, _)) => item.elem == Some(elem),
                None => item.secondary == Some(sm.id),
            })
            .ok_or_else(|| format!("{:?}: 数量明細を解決できません", sm.id))?;
        item.story = secondary[&sm.id]
            .story
            .map(|id| model.stories[id.index()].name.clone())
            .unwrap_or_else(|| "-".into());
        if !item.concrete_m3.is_finite()
            || !item.formwork_m2.is_finite()
            || !item.steel_weight_t().is_finite()
            || !item.rebar_weight_t().is_finite()
            || !item.rebar_joints.is_finite()
        {
            return Err(format!("{:?}: 数量が有限範囲を超えています", sm.id));
        }
    }
    Ok(out)
}

/// モデル走査用の前処理データ。
struct Ctx<'a> {
    model: &'a Model,
    cfg: &'a QuantityCfg,
    /// 柱用途の線材が取り付く節点集合（大梁/小梁の分類用）。
    column_nodes: HashSet<usize>,
    contacts: &'a contact::Contacts,
    secondary: &'a HashMap<SecondaryMemberId, SecondaryQuantity>,
    /// 節点 index → その節点に取り付く水平梁（elem index, 節点から見た
    /// 梁の伸びる向きの単位ベクトル xy）。主筋の外端・内端判定
    /// （梁の連続性）に用いる。
    beams_at_node: HashMap<usize, Vec<(usize, [f64; 2])>>,
    /// 最下層レベル（基礎梁判定用の最小節点標高 [mm]）。
    min_z: f64,
}

impl Ctx<'_> {
    /// 節点の所属階名（未設定は "-"）。
    fn story_name(&self, node_idx: usize) -> String {
        self.model.nodes[node_idx]
            .story
            .and_then(|sid| self.model.stories.get(sid.index()))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "-".to_string())
    }

    /// 梁端の主筋定着条件。節点 `node_idx` から `outward`（端部の外向き
    /// 水平単位ベクトル）方向へ概ね同一直線上（±45°）に続く別の水平梁が
    /// あれば内端（通し、Dc/2＝フェイス距離）、なければ外端（定着 L2）。
    fn beam_bar_end(
        &self,
        elem_idx: usize,
        node_idx: usize,
        outward: [f64; 2],
        face: f64,
        l2: f64,
    ) -> BeamBarEnd {
        let continues = self
            .beams_at_node
            .get(&node_idx)
            .map(|list| {
                list.iter().any(|&(ei, d)| {
                    ei != elem_idx
                        && d[0] * outward[0] + d[1] * outward[1] > std::f64::consts::FRAC_1_SQRT_2
                })
            })
            .unwrap_or(false);
        if continues {
            BeamBarEnd::Interior { half_dc: face }
        } else {
            BeamBarEnd::Exterior { l2 }
        }
    }
}

fn compute_quantity_takeoff_validated(
    model: &Model,
    cfg: &QuantityCfg,
    contacts: &contact::Contacts,
    secondary: &HashMap<SecondaryMemberId, SecondaryQuantity>,
) -> QuantityTakeoff {
    let expanded_storage;
    let model: &Model = if sepika_load::wall_expand::model_has_wall_plates_to_expand(model) {
        let (expanded, _wall_index, _wall_report) =
            sepika_load::wall_expand::expand_wall_elements(model);
        expanded_storage = expanded;
        &expanded_storage
    } else {
        model
    };
    let mut column_nodes: HashSet<usize> = HashSet::new();
    let mut beams_at_node: HashMap<usize, Vec<(usize, [f64; 2])>> = HashMap::new();
    let mut min_z = f64::INFINITY;

    for (idx, e) in model.elements.iter().enumerate() {
        if e.kind != ElementKind::Beam || e.nodes.len() < 2 {
            continue;
        }
        let ni = e.nodes[0].index();
        let nj = e.nodes[1].index();
        let (Some(a), Some(b)) = (model.nodes.get(ni), model.nodes.get(nj)) else {
            continue;
        };
        let (ca, cb) = (a.coord, b.coord);
        min_z = min_z.min(ca[2]).min(cb[2]);
        let Some(section) = model.element_section(e) else {
            continue;
        };
        match section.frame_use {
            Some(FrameSectionUse::Column) => {
                column_nodes.insert(ni);
                column_nodes.insert(nj);
            }
            Some(FrameSectionUse::Girder) => {
                let dx = cb[0] - ca[0];
                let dy = cb[1] - ca[1];
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.0 {
                    let dir = [dx / len, dy / len];
                    beams_at_node.entry(ni).or_default().push((idx, dir));
                    beams_at_node
                        .entry(nj)
                        .or_default()
                        .push((idx, [-dir[0], -dir[1]]));
                }
            }
            None | Some(FrameSectionUse::Brace) => {}
        }
    }

    let ctx = Ctx {
        model,
        cfg,
        column_nodes,
        contacts,
        secondary,
        beams_at_node,
        min_z,
    };

    let mut out = QuantityTakeoff::default();

    for (idx, elem) in model.elements.iter().enumerate() {
        match elem.kind {
            ElementKind::Beam if elem.nodes.len() >= 2 => {
                if let Some(item) = line_member_quantity(&ctx, idx, elem) {
                    out.items.push(item);
                }
            }
            ElementKind::Brace { .. } if elem.nodes.len() >= 2 => {
                if let Some(item) = brace_quantity(&ctx, elem) {
                    out.items.push(item);
                }
            }
            ElementKind::Wall | ElementKind::Shell if elem.nodes.len() >= 3 => {
                if let Some(item) = wall_quantity(&ctx, elem) {
                    out.items.push(item);
                }
            }
            _ => {}
        }
    }

    for slab in &model.slabs {
        if let Some(item) = slab_quantity(&ctx, slab) {
            out.items.push(item);
        }
    }

    for plate in &model.wall_plates {
        if !plate.is_attached()
            && model
                .generated_wall_origins
                .values()
                .any(|id| *id == plate.id)
        {
            continue;
        }
        if let Some(item) = wall_plate_quantity(&ctx, plate) {
            out.items.push(item);
        }
    }

    for sm in model.beams().chain(model.posts()) {
        if let Some(item) = secondary_member_quantity(&ctx, sm) {
            out.items.push(item);
        }
    }

    if !cfg.anchorage_dia_factor.is_finite() || cfg.anchorage_dia_factor < 0.0 {
        for item in &mut out.items {
            if item.rebar.iter().any(|r| r.usage == RebarUsage::MainBar)
                && matches!(
                    item.category,
                    MemberCategory::Girder | MemberCategory::FoundationGirder
                )
            {
                item.rebar.retain(|r| r.usage != RebarUsage::MainBar);
                item.rebar_issues.push(RebarIssue {
                    elem: item.elem,
                    wall_plate: item.wall_plate,
                    section: item.section,
                    usage: RebarUsage::MainBar,
                    kind: RebarIssueKind::Invalid,
                    reason: "定着係数は有限・非負が必要".into(),
                });
            }
        }
    }
    out.notes = build_notes(model, cfg);
    for sm in model.beams().chain(model.posts()) {
        if secondary[&sm.id].story.is_none() {
            out.notes
                .push(format!("{:?}: 階帰属未確定（数量は階 - に計上）", sm.id));
        }
    }
    out
}

/// 前提・未対応事項の注記を組み立てる。
fn build_notes(model: &Model, cfg: &QuantityCfg) -> Vec<String> {
    let mut notes = vec![
        "鉄筋のフック・余長・重ね継手長さ・溶接継手長さは考慮しない（各部位共通事項）。".to_string(),
        "SRC のコンクリート体積は鉄骨体積を差し引かずに計算する（各部位共通事項）。".to_string(),
        "鉄骨重量は W=L×A×7.85（単位重量 7.85t/m³ 固定）による。".to_string(),
        "主筋は全長 1 断面（全断面）配筋として算定する（カットオフ筋の +15d は対象外）。".to_string(),
        "梁の腹筋・幅止筋は段数情報がモデルにないため計上しない。".to_string(),
        format!("壁筋は明示方向比（全厚内の両面合計）による概算。仮定径 {} mm・定着係数 {}。面積 πd²/4 と呼び径表の単位質量で換算する。開口欠損・開口補強・端部配置は詳細に反映せず、実配筋数量の照合済みではない。", cfg.assumed_wall_bar_dia, cfg.anchorage_dia_factor),
        "基礎フーチングはモデルに定義がないため計上しない。".to_string(),
        "梁端ハンチは部材付帯情報（ハンチ長・せい増分・幅増分）から平均断面×ハンチ長で加算する（未入力の部材はハンチなし）。".to_string(),
        "鉄筋継手は個所数（圧接個所数）として集計する（梁 0.5 個所/本＋5m 毎 0.5、柱 1 個所/本＋7m 毎 1）。".to_string(),
        "二次部材の小梁・間柱は解決端点間の実長で数量に含める。完全端点対・用途・断面・解決材料が一致する一意の実材がある場合は線材側へ集約する。".to_string(),
        "鉄骨継手（部材付帯情報の継手位置）は位置・種別の保持のみで、プレート・ボルト重量は計上しない。".to_string(),
    ];
    if !model.slabs.is_empty() {
        notes.push(
            "床厚は床ごとに割り当てた断面の板厚による（デッキスラブのデッキ高さ控除は未対応）。"
                .to_string(),
        );
    }
    notes
}

/// 解析要素ではない二次部材（小梁・間柱）の数量。実部材化済みの二次部材は線材側で数える。
fn secondary_member_quantity(ctx: &Ctx, sm: &SecondaryMember) -> Option<MemberQuantity> {
    let model = ctx.model;
    let resolved = &ctx.secondary[&sm.id];
    if resolved.materialized.is_some() {
        return None;
    }
    let sec = model.sections.get(sm.section?.index())?;
    let mat = model.secondary_material(sm)?;
    let (ci, cj) = model.secondary_member_end_points(sm)?;
    let len = dist3(ci, cj);
    let structure = sepika_core::structure_kind::structure_kind_of(Some(sec), Some(mat.category));
    let category = if sm.kind == SecondaryMemberKind::Post {
        MemberCategory::Column
    } else {
        MemberCategory::Beam
    };
    let story = resolved
        .story
        .map(|id| model.stories[id.index()].name.clone())
        .unwrap_or_else(|| "-".into());
    let mut item = MemberQuantity {
        secondary: Some(sm.id),
        elem: None,
        slab: None,
        wall_plate: None,
        section: Some(sec.id),
        label: if sm.name.is_empty() {
            sec.name.clone()
        } else {
            sm.name.clone()
        },
        story,
        category,
        structure,
        concrete_m3: 0.0,
        formwork_m2: 0.0,
        rebar: Vec::new(),
        rebar_issues: Vec::new(),
        steel: None,
        rebar_joints: 0.0,
    };
    if structure == StructureKind::S || structure == StructureKind::Cft {
        let area = quantity_area(sec).expect("数量積算の断面積を解決できません");
        item.steel = Some(SteelItem {
            section_name: sec.name.clone(),
            length_m: len / 1_000.0,
            weight_t: area * len * STEEL_UNIT_WEIGHT_T_PER_MM3,
        });
        return Some(item);
    }
    if sm.kind == SecondaryMemberKind::Post && structure == StructureKind::Rc {
        if let Some(SectionShape::RcColumnCircle { d, .. }) = sec.shape.as_ref() {
            item.concrete_m3 = member::circle_column_concrete_volume(*d, len) * 1e-9;
            item.formwork_m2 = member::circle_column_formwork_area(*d, len) * 1e-6;
            return Some(item);
        }
    }
    let (width, depth) = match sec.shape.as_ref() {
        Some(SectionShape::RcBeamRect { b, d, .. } | SectionShape::SrcBeamRect { b, d, .. }) => {
            (*b, *d)
        }
        Some(
            SectionShape::RcColumnRect { b, d, .. } | SectionShape::SrcColumnRect { b, d, .. },
        ) => (*b, *d),
        _ => (sec.width, sec.depth),
    };
    if sm.kind == SecondaryMemberKind::Post {
        item.concrete_m3 = member::column_concrete_volume(width, depth, len) * 1e-9;
        item.formwork_m2 = member::column_formwork_area(width, depth, len) * 1e-6;
    } else {
        item.concrete_m3 = member::beam_concrete_volume(width, depth, len) * 1e-9;
        item.formwork_m2 = (member::beam_formwork_area(width, depth, len)
            - ctx
                .contacts
                .area(sepika_core::model::SupportMemberId::Secondary(sm.id)))
            * 1e-6;
    }
    Some(item)
}

/// 線材（柱・大梁・小梁・基礎梁）の数量。
fn line_member_quantity(ctx: &Ctx, elem_idx: usize, elem: &ElementData) -> Option<MemberQuantity> {
    let model = ctx.model;
    let sec = model.sections.get(elem.section?.index())?;
    let mat = model.element_material(elem)?;
    let ni = elem.nodes[0].index();
    let nj = elem.nodes[1].index();
    let (ci, cj) = (model.nodes.get(ni)?.coord, model.nodes.get(nj)?.coord);
    let len = dist3(ci, cj);
    if len <= 0.0 {
        return None;
    }
    let vertical = match sec.frame_use {
        Some(FrameSectionUse::Column) => true,
        Some(FrameSectionUse::Girder) => false,
        None | Some(FrameSectionUse::Brace) => return None,
    };
    let structure = sepika_core::structure_kind::structure_kind_of(Some(sec), Some(mat.category));

    if vertical {
        Some(column_quantity(ctx, elem, sec, mat, ni, nj, len, structure))
    } else {
        Some(beam_quantity(
            ctx, elem_idx, elem, sec, ni, nj, ci, cj, len, structure,
        ))
    }
}

/// 柱の実配筋情報。
enum ColumnRebar<'a> {
    Rect(&'a RcRectColumnRebar),
    Circle(&'a RcCircleColumnRebar),
}

/// 梁の実配筋情報。
enum BeamRebar<'a> {
    Actual(&'a RcBeamRebar),
}

/// 新型矩形柱（`RcColumnRect`/`SrcColumnRect`）の主筋・帯筋数量を追加する。
///
/// 主筋本数は段別本数の固有本数 `2·Σx + 2·Σy − 4·nx·ny`（`nx`/`ny`: X/Y 段数）。
/// 帯筋は X/Y 方向の脚数で一組長さを算定する。未入力なら数量 0。
fn push_rect_column_rebar(
    item: &mut MemberQuantity,
    rebar: &RcRectColumnRebar,
    b: f64,
    d: f64,
    h: f64,
) {
    if rebar.is_unset() {
        return;
    }
    let nx = rebar.x.len() as u32;
    let ny = rebar.y.len() as u32;
    let sx: u32 = rebar.x.iter().sum();
    let sy: u32 = rebar.y.iter().sum();
    let main_bars = (2 * sx + 2 * sy).saturating_sub(4 * nx * ny);
    if main_bars > 0 && rebar.main_dia > 0.0 {
        let total_len = main_bars as f64 * h;
        item.rebar.push(RebarItem {
            usage: RebarUsage::MainBar,
            dia: Some(rebar.main_dia),
            total_length_m: total_len / 1_000.0,
            weight_t: rebar::rebar_weight_t(total_len, rebar.main_dia),
        });
    }
    let sh = &rebar.hoop;
    if sh.dia > 0.0 && sh.pitch > 0.0 {
        let set_len = member::hoop_set_length(b, d, sh.legs_x.max(1), sh.legs_y.max(1));
        let count = member::shear_bar_count(h, sh.pitch);
        let total_len = set_len * count;
        item.rebar.push(RebarItem {
            usage: RebarUsage::Hoop,
            dia: Some(sh.dia),
            total_length_m: total_len / 1_000.0,
            weight_t: rebar::rebar_weight_t(total_len, sh.dia),
        });
    }
    item.rebar_joints = main_bars as f64 * member::column_joint_count(h);
}

/// 新型円形柱（`RcColumnCircle`）の主筋・帯筋数量を追加する。
///
/// 主筋本数は総本数 `count`。帯筋は 1 リング 1 周分の長さ `π·d` で算定する
/// （`RcCircleColumnRebar::aw_mm2()` の 2 本はせん断耐力上の脚数であり、
/// 物理数量の周長ではない）。未入力なら数量 0。
fn push_circle_column_rebar(
    item: &mut MemberQuantity,
    rebar: &RcCircleColumnRebar,
    d: f64,
    h: f64,
) {
    if rebar.is_unset() {
        return;
    }
    if rebar.main_dia > 0.0 {
        let total_len = rebar.count as f64 * h;
        item.rebar.push(RebarItem {
            usage: RebarUsage::MainBar,
            dia: Some(rebar.main_dia),
            total_length_m: total_len / 1_000.0,
            weight_t: rebar::rebar_weight_t(total_len, rebar.main_dia),
        });
    }
    let sh = &rebar.hoop;
    if sh.dia > 0.0 && sh.pitch > 0.0 {
        let set_len = std::f64::consts::PI * d;
        let count = member::shear_bar_count(h, sh.pitch);
        let total_len = set_len * count;
        item.rebar.push(RebarItem {
            usage: RebarUsage::Hoop,
            dia: Some(sh.dia),
            total_length_m: total_len / 1_000.0,
            weight_t: rebar::rebar_weight_t(total_len, sh.dia),
        });
    }
    item.rebar_joints = rebar.count as f64 * member::column_joint_count(h);
}

/// 柱の数量。
#[allow(clippy::too_many_arguments)]
fn column_quantity(
    ctx: &Ctx,
    elem: &ElementData,
    sec: &sepika_core::model::Section,
    _mat: &sepika_core::model::Material,
    ni: usize,
    nj: usize,
    h: f64,
    structure: StructureKind,
) -> MemberQuantity {
    let lower = if ctx.model.nodes[ni].coord[2] <= ctx.model.nodes[nj].coord[2] {
        ni
    } else {
        nj
    };
    let story = ctx.story_name(lower);

    let mut item = MemberQuantity {
        secondary: None,
        elem: Some(elem.id),
        slab: None,
        wall_plate: None,
        section: Some(sec.id),
        label: sec.name.clone(),
        story,
        category: MemberCategory::Column,
        structure,
        concrete_m3: 0.0,
        formwork_m2: 0.0,
        rebar: Vec::new(),
        rebar_issues: Vec::new(),
        steel: None,
        rebar_joints: 0.0,
    };

    match (structure, sec.shape.as_ref()) {
        (StructureKind::Rc | StructureKind::Src, shape) => {
            let (vol, form, rebar): (f64, f64, Option<ColumnRebar>) = match shape {
                Some(SectionShape::RcColumnRect { b, d, rebar })
                | Some(SectionShape::SrcColumnRect { b, d, rebar, .. }) => (
                    member::column_concrete_volume(*b, *d, h),
                    member::column_formwork_area(*b, *d, h),
                    Some(ColumnRebar::Rect(rebar)),
                ),
                Some(SectionShape::RcColumnCircle { d, rebar }) => (
                    member::circle_column_concrete_volume(*d, h),
                    member::circle_column_formwork_area(*d, h),
                    Some(ColumnRebar::Circle(rebar)),
                ),
                _ => (
                    member::column_concrete_volume(sec.width, sec.depth, h),
                    member::column_formwork_area(sec.width, sec.depth, h),
                    None,
                ),
            };
            item.concrete_m3 = vol * 1e-9;
            item.formwork_m2 = form * 1e-6;

            match rebar {
                Some(ColumnRebar::Rect(rebar)) => {
                    let (b, d) = match shape {
                        Some(SectionShape::RcColumnRect { b, d, .. })
                        | Some(SectionShape::SrcColumnRect { b, d, .. }) => (*b, *d),
                        _ => (sec.width, sec.depth),
                    };
                    push_rect_column_rebar(&mut item, rebar, b, d, h);
                }
                Some(ColumnRebar::Circle(rebar)) => {
                    let d = match shape {
                        Some(SectionShape::RcColumnCircle { d, .. }) => *d,
                        _ => sec.depth,
                    };
                    push_circle_column_rebar(&mut item, rebar, d, h);
                }
                None => {}
            }

            if structure == StructureKind::Src {
                if let Some(a_s) = sec.shape.as_ref().and_then(src_steel_area) {
                    item.steel = Some(SteelItem {
                        section_name: sec.name.clone(),
                        length_m: h / 1_000.0,
                        weight_t: a_s * h * STEEL_UNIT_WEIGHT_T_PER_MM3,
                    });
                }
            }
        }
        (StructureKind::S, _) => {
            let a = quantity_area(sec).expect("数量積算の断面積を解決できません");
            item.steel = Some(SteelItem {
                section_name: sec.name.clone(),
                length_m: h / 1_000.0,
                weight_t: a * h * STEEL_UNIT_WEIGHT_T_PER_MM3,
            });
        }
        (StructureKind::Cft, shape) => {
            let a = quantity_area(sec).expect("数量積算の断面積を解決できません");
            item.steel = Some(SteelItem {
                section_name: sec.name.clone(),
                length_m: h / 1_000.0,
                weight_t: a * h * STEEL_UNIT_WEIGHT_T_PER_MM3,
            });
            if let Some(ai) = shape.and_then(cft_infill_area) {
                item.concrete_m3 = ai * h * 1e-9;
            }
        }
    }
    item
}

/// 水平梁（大梁・小梁・基礎梁）の数量。
#[allow(clippy::too_many_arguments)]
fn beam_quantity(
    ctx: &Ctx,
    elem_idx: usize,
    elem: &ElementData,
    sec: &sepika_core::model::Section,
    ni: usize,
    nj: usize,
    ci: [f64; 3],
    cj: [f64; 3],
    len: f64,
    structure: StructureKind,
) -> MemberQuantity {
    let model = ctx.model;
    let at_base =
        (ci[2] - ctx.min_z).abs() < LEVEL_TOL_MM && (cj[2] - ctx.min_z).abs() < LEVEL_TOL_MM;
    let touches_column = ctx.column_nodes.contains(&ni) || ctx.column_nodes.contains(&nj);
    let category = if at_base && touches_column {
        MemberCategory::FoundationGirder
    } else if touches_column {
        MemberCategory::Girder
    } else {
        MemberCategory::Beam
    };

    let mut item = MemberQuantity {
        secondary: None,
        elem: Some(elem.id),
        slab: None,
        wall_plate: None,
        section: Some(sec.id),
        label: sec.name.clone(),
        story: ctx.story_name(ni),
        category,
        structure,
        concrete_m3: 0.0,
        formwork_m2: 0.0,
        rebar: Vec::new(),
        rebar_issues: Vec::new(),
        steel: None,
        rebar_joints: 0.0,
    };

    if structure == StructureKind::S || structure == StructureKind::Cft {
        let a = quantity_area(sec).expect("数量積算の断面積を解決できません");
        item.steel = Some(SteelItem {
            section_name: sec.name.clone(),
            length_m: len / 1_000.0,
            weight_t: a * len * STEEL_UNIT_WEIGHT_T_PER_MM3,
        });
        return item;
    }

    let lo = elem.rigid_zone.clear_span_from(len).unwrap_or(len);
    let (b, d, rebar): (f64, f64, Option<BeamRebar>) = match sec.shape.as_ref() {
        Some(SectionShape::RcBeamRect { b, d, rebar }) => (*b, *d, Some(BeamRebar::Actual(rebar))),
        Some(SectionShape::SrcBeamRect { b, d, rebar, .. }) => {
            (*b, *d, Some(BeamRebar::Actual(rebar)))
        }
        _ => (sec.width, sec.depth, None),
    };

    if category == MemberCategory::Beam {
        let vol = member::beam_concrete_volume(b, d, lo);
        item.concrete_m3 = vol * 1e-9;
        item.formwork_m2 = member::beam_formwork_area(b, d, lo) * 1e-6;
        let vol_m3 = vol * 1e-9;
        item.rebar.push(RebarItem {
            usage: RebarUsage::BeamMain,
            dia: None,
            total_length_m: 0.0,
            weight_t: vol_m3 * ctx.cfg.beam_main_ratio * STEEL_UNIT_WEIGHT_T_PER_M3,
        });
        item.rebar.push(RebarItem {
            usage: RebarUsage::BeamStirrup,
            dia: None,
            total_length_m: 0.0,
            weight_t: vol_m3 * ctx.cfg.beam_stirrup_ratio * STEEL_UNIT_WEIGHT_T_PER_M3,
        });
    } else {
        let to_haunch = |h: &sepika_core::model::Haunch| Haunch {
            b: b + h.width_increase.max(0.0),
            d: d + h.depth_increase.max(0.0),
            len: h.length,
        };
        let detail = model.member_detail(elem.id);
        let haunch_i: Option<Haunch> = detail
            .and_then(|dt| dt.haunch_i.as_ref())
            .filter(|h| h.length > 0.0)
            .map(to_haunch);
        let haunch_j: Option<Haunch> = detail
            .and_then(|dt| dt.haunch_j.as_ref())
            .filter(|h| h.length > 0.0)
            .map(to_haunch);
        let vol = member::girder_concrete_volume(b, d, lo, haunch_i, haunch_j);
        item.concrete_m3 = vol * 1e-9;

        let form = if category == MemberCategory::FoundationGirder {
            member::foundation_girder_formwork_area(b, d, lo, haunch_i, haunch_j)
        } else {
            member::girder_formwork_area(b, d, d, lo, haunch_i, haunch_j)
        };
        item.formwork_m2 = form * 1e-6;

        match rebar {
            Some(BeamRebar::Actual(rebar)) if !rebar.is_unset() => {
                let main_bars: u32 =
                    rebar.top.iter().sum::<u32>() + rebar.bottom.iter().sum::<u32>();
                if main_bars > 0 && rebar.main_dia > 0.0 {
                    let dir_xy = {
                        let dx = cj[0] - ci[0];
                        let dy = cj[1] - ci[1];
                        let l = (dx * dx + dy * dy).sqrt();
                        [dx / l, dy / l]
                    };
                    let l2 = ctx.cfg.anchorage_dia_factor * rebar.main_dia;
                    let end_i = ctx.beam_bar_end(
                        elem_idx,
                        ni,
                        [-dir_xy[0], -dir_xy[1]],
                        elem.rigid_zone.face_i_or_zero(),
                        l2,
                    );
                    let end_j = ctx.beam_bar_end(
                        elem_idx,
                        nj,
                        dir_xy,
                        elem.rigid_zone.face_j_or_zero(),
                        l2,
                    );
                    let bar_len = member::girder_main_bar_length(lo, end_i, end_j);
                    let total_len = main_bars as f64 * bar_len;
                    item.rebar.push(RebarItem {
                        usage: RebarUsage::MainBar,
                        dia: Some(rebar.main_dia),
                        total_length_m: total_len / 1_000.0,
                        weight_t: rebar::rebar_weight_t(total_len, rebar.main_dia),
                    });
                }
                let sh = &rebar.stirrup;
                if sh.dia > 0.0 && sh.pitch > 0.0 {
                    let set_len = member::stirrup_set_length(b, d, sh.legs.max(1));
                    let count = member::shear_bar_count(lo, sh.pitch);
                    let total_len = set_len * count;
                    item.rebar.push(RebarItem {
                        usage: RebarUsage::Stirrup,
                        dia: Some(sh.dia),
                        total_length_m: total_len / 1_000.0,
                        weight_t: rebar::rebar_weight_t(total_len, sh.dia),
                    });
                }
                item.rebar_joints = main_bars as f64 * member::beam_joint_count(lo);
            }
            Some(BeamRebar::Actual(_)) | None => {}
        }

        if structure == StructureKind::Src {
            if let Some(a_s) = sec.shape.as_ref().and_then(src_steel_area) {
                item.steel = Some(SteelItem {
                    section_name: sec.name.clone(),
                    length_m: len / 1_000.0,
                    weight_t: a_s * len * STEEL_UNIT_WEIGHT_T_PER_MM3,
                });
            }
        }
    }
    item.formwork_m2 -= ctx
        .contacts
        .area(sepika_core::model::SupportMemberId::Primary(elem.id))
        * 1e-6;
    item
}

/// ブレースの数量。
///
/// 長さは節点間距離。接合部のプレート・ボルト等は考慮しない。
fn brace_quantity(ctx: &Ctx, elem: &ElementData) -> Option<MemberQuantity> {
    let model = ctx.model;
    let sec = model.sections.get(elem.section?.index())?;
    let mat = model.element_material(elem)?;
    let ni = elem.nodes[0].index();
    let nj = elem.nodes[1].index();
    let (ci, cj) = (model.nodes.get(ni)?.coord, model.nodes.get(nj)?.coord);
    let lb = dist3(ci, cj);
    if lb <= 0.0 {
        return None;
    }
    let structure = sepika_core::structure_kind::structure_kind_of(Some(sec), Some(mat.category));
    let lower = if ci[2] <= cj[2] { ni } else { nj };

    let mut item = MemberQuantity {
        secondary: None,
        elem: Some(elem.id),
        slab: None,
        wall_plate: None,
        section: Some(sec.id),
        label: sec.name.clone(),
        story: ctx.story_name(lower),
        category: MemberCategory::Brace,
        structure,
        concrete_m3: 0.0,
        formwork_m2: 0.0,
        rebar: Vec::new(),
        rebar_issues: Vec::new(),
        steel: None,
        rebar_joints: 0.0,
    };
    let a = quantity_area(sec).expect("数量積算の断面積を解決できません");
    if structure == StructureKind::Rc {
        item.concrete_m3 = a * lb * 1e-9;
    } else {
        item.steel = Some(SteelItem {
            section_name: sec.name.clone(),
            length_m: lb / 1_000.0,
            weight_t: a * lb * STEEL_UNIT_WEIGHT_T_PER_MM3,
        });
    }
    Some(item)
}

/// 4 節点壁の内法寸法 (長さ L, 高さ H) [mm]。
///
/// 周辺の柱・梁の断面寸法の半分を芯々寸法から控除する
/// （`sepika-load::story_gen::wall_clear_area_factor` と同じ規則。
/// 側柱は `min(width, depth)/2`、上下梁は `depth/2` を控除）。
/// 4 節点壁でない場合は None。
fn wall_clear_dims(model: &Model, elem: &ElementData, pts: &[[f64; 3]]) -> Option<(f64, f64)> {
    if elem.kind != ElementKind::Wall || elem.nodes.len() != 4 || pts.len() != 4 {
        return None;
    }
    let n = 4usize;
    let (mut l_len, mut l_cnt) = (0.0, 0u32);
    let (mut h_len, mut h_cnt) = (0.0, 0u32);
    let (mut l_deduct, mut h_deduct) = (0.0, 0.0);
    for i in 0..n {
        let (a, b) = (elem.nodes[i], elem.nodes[(i + 1) % n]);
        let (pa, pb) = (pts[i], pts[(i + 1) % n]);
        let dz = (pb[2] - pa[2]).abs();
        let dh = ((pb[0] - pa[0]).powi(2) + (pb[1] - pa[1]).powi(2)).sqrt();
        let len = (dz * dz + dh * dh).sqrt();
        if len <= 0.0 {
            continue;
        }
        let member_sec = model
            .elements
            .iter()
            .find(|e| {
                e.kind == ElementKind::Beam && e.nodes.len() >= 2 && {
                    let (m0, m1) = (e.nodes[0], e.nodes[e.nodes.len() - 1]);
                    (m0 == a && m1 == b) || (m0 == b && m1 == a)
                }
            })
            .and_then(|e| e.section)
            .and_then(|sid| model.sections.get(sid.index()));
        if dz > dh {
            h_len += len;
            h_cnt += 1;
            if let Some(sec) = member_sec {
                l_deduct += sec.width.min(sec.depth).max(0.0) / 2.0;
            }
        } else {
            l_len += len;
            l_cnt += 1;
            if let Some(sec) = member_sec {
                h_deduct += sec.depth.max(0.0) / 2.0;
            }
        }
    }
    if l_cnt == 0 || h_cnt == 0 {
        return None;
    }
    let l = (l_len / l_cnt as f64 - l_deduct).max(0.0);
    let h = (h_len / h_cnt as f64 - h_deduct).max(0.0);
    Some((l, h))
}

/// 壁（耐震壁・フレーム内雑壁）の数量。
fn wall_quantity(ctx: &Ctx, elem: &ElementData) -> Option<MemberQuantity> {
    let model = ctx.model;
    let sec = model.sections.get(elem.section?.index())?;
    let (t, pwh, pwv) = match sec.shape.as_ref() {
        Some(SectionShape::RcWall {
            thickness,
            pwh_ratio,
            pwv_ratio,
            ..
        }) => (*thickness, *pwh_ratio, *pwv_ratio),
        _ => (sec.thickness.unwrap_or(f64::NAN), None, None),
    };
    let pts: Vec<[f64; 3]> = elem
        .nodes
        .iter()
        .filter_map(|n| model.nodes.get(n.index()).map(|nd| nd.coord))
        .collect();
    if pts.len() < 3 {
        return None;
    }

    let (l_clear, h_clear, gross_area) = match wall_clear_dims(model, elem, &pts) {
        Some((l, h)) => (l, h, l * h),
        None => {
            let area = polygon_area_3d(&pts);
            let min_z = pts.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
            let max_z = pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
            let h = (max_z - min_z).max(0.0);
            let l = if h > 0.0 { area / h } else { 0.0 };
            (l, h, area)
        }
    };

    let opening = model
        .wall_attrs
        .iter()
        .find(|a| a.elem == elem.id)
        .map(|a| a.total_opening_area())
        .unwrap_or(0.0);
    let net_area = (gross_area - opening).max(0.0);

    let lower = elem
        .nodes
        .iter()
        .map(|n| n.index())
        .min_by(|&a, &b| {
            model.nodes[a].coord[2]
                .partial_cmp(&model.nodes[b].coord[2])
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(elem.nodes[0].index());

    let mut item = MemberQuantity {
        secondary: None,
        elem: Some(elem.id),
        slab: None,
        wall_plate: None,
        section: Some(sec.id),
        label: sec.name.clone(),
        story: ctx.story_name(lower),
        category: MemberCategory::Wall,
        structure: StructureKind::Rc,
        concrete_m3: if t.is_finite() && t > 0.0 && net_area.is_finite() {
            net_area * t * 1e-9
        } else {
            0.0
        },
        formwork_m2: net_area * 2.0 * 1e-6,
        rebar: Vec::new(),
        rebar_issues: Vec::new(),
        steel: None,
        rebar_joints: 0.0,
    };

    item.wall_plate = model.generated_wall_origins.get(&elem.id).copied();
    let dia_mm = ctx.cfg.assumed_wall_bar_dia;
    let anchorage_factor = ctx.cfg.anchorage_dia_factor;
    for (usage, ratio, span_mm, other_mm) in [
        (RebarUsage::WallHorizontal, pwh, l_clear, h_clear),
        (RebarUsage::WallVertical, pwv, h_clear, l_clear),
    ] {
        let issue = match ratio {
            None => Some((RebarIssueKind::Missing, "方向筋比が未入力")),
            Some(r) if !r.is_finite() || r < 0.0 => {
                Some((RebarIssueKind::Invalid, "方向筋比は有限・非負が必要"))
            }
            _ if !t.is_finite() || t <= 0.0 => {
                Some((RebarIssueKind::Invalid, "壁厚は有限・正が必要"))
            }
            _ if pts.iter().flatten().any(|v| !v.is_finite())
                || !span_mm.is_finite()
                || span_mm <= 0.0
                || !other_mm.is_finite()
                || other_mm <= 0.0 =>
            {
                Some((RebarIssueKind::Invalid, "算定内法寸法は有限・正が必要"))
            }
            _ if !dia_mm.is_finite() || dia_mm <= 0.0 => {
                Some((RebarIssueKind::Invalid, "仮定径は有限・正が必要"))
            }
            _ if !anchorage_factor.is_finite() || anchorage_factor < 0.0 => {
                Some((RebarIssueKind::Invalid, "定着係数は有限・非負が必要"))
            }
            _ => None,
        };
        if let Some((kind, reason)) = issue {
            item.rebar_issues.push(RebarIssue {
                elem: item.elem,
                wall_plate: item.wall_plate,
                section: item.section,
                usage,
                kind,
                reason: reason.into(),
            });
            continue;
        }
        let count =
            ratio.unwrap() * t * other_mm / sepika_core::section_shape::one_bar_area(dia_mm);
        let total_len_mm = member::wall_bar_length(span_mm, anchorage_factor * dia_mm, count);
        let weight_t = rebar::rebar_weight_t(total_len_mm, dia_mm);
        if !total_len_mm.is_finite() || !weight_t.is_finite() {
            item.rebar_issues.push(RebarIssue {
                elem: item.elem,
                wall_plate: item.wall_plate,
                section: item.section,
                usage,
                kind: RebarIssueKind::Invalid,
                reason: "数量換算結果が非有限".into(),
            });
            continue;
        }
        item.rebar.push(RebarItem {
            usage,
            dia: Some(dia_mm),
            total_length_m: total_len_mm / 1000.0,
            weight_t,
        });
    }
    Some(item)
}

/// 取り付く壁版（`WallPlateShape::Attached`。パラペット・腰壁・垂れ壁・自立壁）の数量。
///
/// 解析要素を持たない（D5）ため `WallPlate` から直接算定する（[`slab_quantity`] と
/// 同じ位置づけ。エレメント経由の [`wall_quantity`] は `Enclosed`（4 節点の耐震壁）
/// のみを数える）。取り付く壁版はフレーム外雑壁にあたる
/// ため、カテゴリは同じ `MemberCategory::MiscWall` を使う。
/// ラベルは囲まれた壁と同じく割り当てた断面の符号（`wall_quantity` と同じ）。
fn wall_plate_quantity(ctx: &Ctx, plate: &WallPlate) -> Option<MemberQuantity> {
    let model = ctx.model;
    let anchor_nodes: Vec<sepika_core::ids::NodeId> = match &plate.shape {
        WallPlateShape::Attached { anchor, .. } => match anchor {
            RegionAnchor::Line { nodes, .. } | RegionAnchor::FloorRegion { nodes, .. } => {
                nodes.to_vec()
            }
            RegionAnchor::Point(node) => vec![*node],
        },
        WallPlateShape::Enclosed => plate.boundary_nodes(model).unwrap_or_default(),
    };
    let sec = model.wall_plate_section(plate);
    let t_mm = sec.and_then(|s| match &s.shape {
        Some(SectionShape::RcWall { thickness, .. }) => Some(*thickness),
        _ => s.thickness,
    });
    let pts = plate.boundary_coords(model).unwrap_or_default();
    let area_mm2 = if pts.len() >= 3 {
        polygon_area_3d(&pts)
    } else {
        0.0
    };
    let valid_geometry = pts.len() >= 3
        && pts.iter().flatten().all(|v| v.is_finite())
        && area_mm2.is_finite()
        && area_mm2 > 0.0;
    let net_area_mm2 = if valid_geometry {
        (area_mm2 - plate.total_opening_area()).max(0.0)
    } else {
        0.0
    };
    let story = anchor_nodes
        .iter()
        .filter_map(|id| model.nodes.get(id.index()))
        .min_by(|a, b| a.coord[2].total_cmp(&b.coord[2]))
        .map(|n| ctx.story_name(n.id.index()))
        .unwrap_or_else(|| "-".into());
    let (kind, reason) = if sec.is_none() {
        (RebarIssueKind::Missing, "壁版の断面が未解決")
    } else if t_mm.is_none() {
        (RebarIssueKind::Missing, "壁版の壁厚が未入力")
    } else if t_mm.is_some_and(|t| !t.is_finite() || t <= 0.0) {
        (RebarIssueKind::Invalid, "壁厚は有限・正が必要")
    } else if !valid_geometry {
        (
            RebarIssueKind::Invalid,
            "壁版の算定寸法・境界が未解決または不正",
        )
    } else {
        (RebarIssueKind::Unsupported, "壁版の鉄筋積算は未実装")
    };
    Some(MemberQuantity {
        secondary: None,
        elem: None,
        slab: None,
        wall_plate: Some(plate.id),
        section: plate.section,
        label: sec.map(|s| s.name.clone()).unwrap_or_else(|| "-".into()),
        story,
        category: MemberCategory::MiscWall,
        structure: StructureKind::Rc,
        concrete_m3: t_mm
            .filter(|t| t.is_finite() && *t > 0.0)
            .map(|t| net_area_mm2 * t * 1e-9)
            .unwrap_or(0.0),
        formwork_m2: net_area_mm2 * 2.0 * 1e-6,
        rebar: Vec::new(),
        rebar_issues: [RebarUsage::WallHorizontal, RebarUsage::WallVertical]
            .into_iter()
            .map(|usage| RebarIssue {
                elem: None,
                wall_plate: Some(plate.id),
                section: plate.section,
                usage,
                kind,
                reason: reason.into(),
            })
            .collect(),
        steel: None,
        rebar_joints: 0.0,
    })
}

/// 床（一般・片持ち・出隅・入隅）の数量。
fn slab_quantity(ctx: &Ctx, slab: &Slab) -> Option<MemberQuantity> {
    let model = ctx.model;
    let pts: Vec<[f64; 3]> = slab.boundary_coords(model)?;
    if pts.len() < 3 {
        return None;
    }
    let area = polygon_area_3d(&pts);
    let t = model.slab_plate_thickness(slab).unwrap_or(0.0);
    let category = if slab.is_attached() {
        MemberCategory::CantileverSlab
    } else {
        MemberCategory::Slab
    };
    let vol = area * t;
    let vol_m3 = vol * 1e-9;

    let mut item = MemberQuantity {
        secondary: None,
        elem: None,
        slab: Some(slab.id),
        wall_plate: None,
        section: slab.section(),
        label: format!("S{}", slab.id.0),
        story: slab
            .reference_node(model)
            .map(|n| ctx.story_name(n.index()))
            .unwrap_or_else(|| "-".to_string()),
        category,
        structure: StructureKind::Rc,
        concrete_m3: vol_m3,
        formwork_m2: area * 1e-6,
        rebar: Vec::new(),
        rebar_issues: Vec::new(),
        steel: None,
        rebar_joints: 0.0,
    };
    if vol_m3 > 0.0 {
        item.rebar.push(RebarItem {
            usage: RebarUsage::SlabBar,
            dia: None,
            total_length_m: 0.0,
            weight_t: vol_m3 * ctx.cfg.slab_rebar_ratio * STEEL_UNIT_WEIGHT_T_PER_M3,
        });
    }
    Some(item)
}
