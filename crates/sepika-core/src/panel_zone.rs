//! 仕口パネル（柱梁接合部パネル）の諸元解決。
//!
//! 断面用途が柱・梁である線材だけを S/CFT 等の仕口パネル候補として扱い、
//! Wall/Shell および斜材は対象外とする。断面用途（柱・梁という意味論）による判定と、
//! 部材軸が水平・鉛直など既存パネル式の幾何適用条件は分離する。
//! 候補の断面形状からパネルの寸法（柱せい `dc`・板厚 `tp`・梁せい `db`）と、
//! そこから定まる形状係数 κ・実効体積 `Ve` を解決する。
//!
//! # 単一の出所とする理由
//!
//! パネルの諸元は次の 2 箇所で必要になる。
//!
//! - **モデル化**（`sepika_element::springs::panel`）— せん断剛性 `Kxp = Kyp = G・Ve`
//! - **断面検定**（`sepika_design_jp::steel::panel_zone`）— 降伏モーメント
//!   `pMy = (Ve/κ)・√(1−n²)・Fy/√3`
//!
//! 両者が別々に断面形状を解釈すると、同じ接合部に対して剛性と耐力が食い違う
//! 諸元で算定されうる。本モジュールを唯一の出所とし、双方がここを呼ぶ。
//!
//! # パネル板厚 `tp` の解決順
//!
//! 1. [`Section::panel_thickness`] が入力されていればその値（ダイアフラム補強・
//!    ダブラープレートによる増厚の明示指定）
//! 2. 未入力なら柱の断面形状から算出する（H 形＝ウェブ厚、角形・円形＝板厚）
//!
//! # 対象とする接合部
//!
//! 節点に次のすべてが揃うとき、その接合部を仕口パネルの対象とする
//! （[`resolve_panel_joint`]）。モデル化と断面検定は同じ判定を通る。
//!
//! - 用途が柱の部材が 1 本以上・用途が梁の部材が 1 本以上取り付く
//! - 取り付く部材がすべて S/CFT 系（`StructureKind::is_steel_like`）
//! - 採用柱の諸元を解決でき、実効体積 `Ve` が有限の正値
//!
//! 斜材（ブレース等）は資料が接合位置と係数 ζ を定めておらずパネル自由度と
//! 連成しないため、種別判定の対象にもしない。
//!
//! # 柱が複数取り付く場合
//!
//! 接合部から他端への座標で下側の柱を優先し、下側が存在しない場合だけ上側を採る。
//! 採用側を一意に特定できない場合や採用柱の情報が不正な場合は理由付きエラーとする。
//!
//! # 柱断面ごとの扱い
//!
//! 諸元を解決できるのは H 形鋼・角形鋼管・円形鋼管・CFT（角形・円形）である。
//! 組立 H 形（`SteelBuiltH`）は上下フランジが異なりパネル形状係数 κ の標準式が
//! 適用できないため対象外とする。
//!
//! **モデル化と断面検定で対象範囲が異なる**点に注意する。
//!
//! | 柱断面 | モデル化 | 断面検定 |
//! |---|---|---|
//! | H 形鋼・角形鋼管・円形鋼管 | 対象 | 対象 |
//! | CFT（角形・円形） | **対象外** | 対象 |
//! | RC・SRC・組立 H 形 | 対象外 | 対象外 |
//!
//! CFT を諸元解決の対象に残したまま、モデル化からは
//! [`PanelJoint::has_filled_column`] で除外する。CFT の接合部では充填コンクリートと
//! 通しダイアフラムがせん断挙動に関与し、鋼管のみの実効体積による弾性せん断パネル
//! `G・Ve` では剛性を表せないため、接合部を剛節点として扱う。

use crate::geom;
use crate::ids::{ElemId, NodeId};
use crate::model::{ElementData, FrameSectionUse, Model, Section};
use crate::section_shape::SectionShape;
use crate::structure_kind::member_structure_kind;

/// 仕口パネルに対する部材の向き。
///
/// 斜材（鉛直成分が 0.2 と 0.8 の間）は、資料が接合位置と係数 ζ を定めていない
/// ため、いずれにも分類しない（`None`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberOrientation {
    /// 用途が柱。パネルの上下面で接合する。
    Column,
    /// 用途が梁。パネルの左右面（柱フェース）で接合する。
    Beam,
}

/// 線材要素の単位材軸ベクトル（i 端 → j 端）。線材以外・退化長さは `None`。
pub fn member_unit_axis(model: &Model, elem: &ElementData) -> Option<[f64; 3]> {
    if !elem.kind.is_primary_frame_support() || elem.nodes.len() < 2 {
        return None;
    }
    let p0 = model.nodes.get(elem.nodes[0].index())?.coord;
    let p1 = model.nodes.get(elem.nodes[1].index())?.coord;
    geom::vec3::unit_from(p0, p1)
}

/// 既存の仕口パネル式を適用できる水平・鉛直材かを判定する。
pub fn is_horizontal_or_vertical_member(model: &Model, elem: &ElementData) -> bool {
    let Some(axis) = member_unit_axis(model, elem) else {
        return false;
    };
    !(0.2..0.8).contains(&axis[2].abs())
}

/// 断面用途から柱・はりを判定する。用途未設定・ブレース・線材以外は `None`。
pub fn member_orientation(model: &Model, elem: &ElementData) -> Option<MemberOrientation> {
    if !elem.kind.is_primary_frame_support() || elem.nodes.len() < 2 {
        return None;
    }
    let section = elem.section.and_then(|id| model.sections.get(id.index()))?;
    match section.frame_use? {
        FrameSectionUse::Column => Some(MemberOrientation::Column),
        FrameSectionUse::Girder => Some(MemberOrientation::Beam),
        FrameSectionUse::Brace => None,
    }
}

/// 接合部が占める領域の半寸法 `[mm]`。
///
/// 部材は節点そのものではなく、パネルの面で接合する。その面までの距離が
/// パネル分のオフセットになる。
///
/// - はりは**柱フェース**で接合するため、オフセットは [`Self::column_half`]
/// - 柱は**梁フェース**で接合するため、オフセットは [`Self::beam_half`]
///
/// # 危険断面位置（`RigidZone::face_i` / `face_j`）と分けている理由
///
/// フェース距離は現在「接合する直交部材せいの 1/2」だが、これは断面算定の
/// **既定の**危険断面位置でもあり、将来は任意位置を取りうる。一方パネルの
/// オフセットは接合部の物理的な寸法そのもので、設計上の評価位置がどこへ動いても
/// 変わってはいけない。両者を別の量として扱う。
///
/// また `face_i` は「概ね直交する全部材の最大せい/2」であり、はりにとっては
/// 直交する**はり**も候補に入る。本構造体は柱・はりを明示的に区別するため、
/// はりのオフセットが直交ばりのせいで決まることがない。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanelHalfExtent {
    /// 柱せいの最大値の 1/2 `[mm]`（＝はりのオフセット、パネルの水平半寸法）。
    pub column_half: f64,
    /// はりせいの最大値の 1/2 `[mm]`（＝柱のオフセット、パネルの鉛直半寸法）。
    pub beam_half: f64,
}

impl PanelHalfExtent {
    /// 向き `orientation` の部材が接合する面までのオフセット `[mm]`。
    pub fn offset_for(&self, orientation: MemberOrientation) -> f64 {
        match orientation {
            MemberOrientation::Beam => self.column_half,
            MemberOrientation::Column => self.beam_half,
        }
    }
}

/// 節点 `node` に取り付く部材 `members` から接合部の半寸法を集める。
///
/// `members` は当該節点へ接続する要素（隣接マップを持つ呼び出し側はそれを渡し、
/// 持たない側は全要素を渡してよい。節点を端点に持たない要素は読み飛ばす）。
pub fn panel_half_extent<'a>(
    model: &Model,
    node: NodeId,
    members: impl IntoIterator<Item = &'a ElementData>,
) -> PanelHalfExtent {
    let mut extent = PanelHalfExtent::default();
    for e in members {
        if !e.nodes.iter().take(2).any(|n| *n == node) {
            continue;
        }
        if !is_horizontal_or_vertical_member(model, e) {
            continue;
        }
        let Some(orientation) = member_orientation(model, e) else {
            continue;
        };
        let Some(sec) = e.section.and_then(|sid| model.sections.get(sid.index())) else {
            continue;
        };
        let half = sec.depth / 2.0;
        match orientation {
            MemberOrientation::Column => extent.column_half = extent.column_half.max(half),
            MemberOrientation::Beam => extent.beam_half = extent.beam_half.max(half),
        }
    }
    extent
}

/// パネルの断面区分（形状係数 κ・実効体積 `Ve` の算定式を分ける）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanelShapeKind {
    /// H 形鋼柱。`bc`: フランジ幅、`tf`: フランジ厚。パネルはウェブ 1 枚。
    H { bc: f64, tf: f64 },
    /// 角形鋼管柱（CFT 角形を含む）。`bc`: 柱幅。パネルはウェブ 2 枚。
    Box { bc: f64 },
    /// 円形鋼管柱（CFT 円形を含む）。パネルは円筒。
    Pipe,
}

/// 柱断面から解決した仕口パネルの諸元。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelGeometry {
    pub kind: PanelShapeKind,
    /// 柱せい方向のパネル寸法 `dc` `[mm]`（H 形・角形は板厚中心間、円形は径 − 板厚）。
    pub dc: f64,
    /// パネル板厚 `tp` `[mm]`。
    pub tp: f64,
    /// 充填コンクリートを持つ柱（CFT）か。
    ///
    /// 断面検定では鋼管部を S 造と同じ式で評価するため区別しないが、仕口パネルの
    /// **モデル化からは除外する**（[`PanelJoint::has_filled_column`]）。
    pub filled: bool,
}

impl PanelGeometry {
    /// 柱の断面からパネル諸元を解決する。対象外の断面（RC・SRC・組立 H 形・
    /// 形状未定義）は `None`。
    ///
    /// `tp` は [`Section::panel_thickness`] が正値で入力されていればそちらを
    /// 優先し、未入力なら断面形状から算出する（モジュール冒頭「パネル板厚 `tp`
    /// の解決順」）。
    pub fn from_column(sec: &Section) -> Option<Self> {
        let (kind, dc, tp, filled) = match sec.shape {
            Some(SectionShape::SteelH {
                height,
                width,
                web_thick,
                flange_thick,
                ..
            }) => (
                PanelShapeKind::H {
                    bc: width,
                    tf: flange_thick,
                },
                height - flange_thick,
                web_thick,
                false,
            ),
            Some(SectionShape::SteelBox {
                height,
                width,
                thick,
                ..
            }) => (
                PanelShapeKind::Box { bc: width },
                height - thick,
                thick,
                false,
            ),
            Some(SectionShape::CftBox {
                height,
                width,
                thick,
                ..
            }) => (
                PanelShapeKind::Box { bc: width },
                height - thick,
                thick,
                true,
            ),
            Some(SectionShape::SteelPipe { outer_dia, thick }) => {
                (PanelShapeKind::Pipe, outer_dia - thick, thick, false)
            }
            Some(SectionShape::CftPipe { outer_dia, thick }) => {
                (PanelShapeKind::Pipe, outer_dia - thick, thick, true)
            }
            _ => return None,
        };
        let tp = match sec.panel_thickness {
            Some(t) if t > 0.0 => t,
            _ => tp,
        };
        Some(Self {
            kind,
            dc,
            tp,
            filled,
        })
    }

    /// パネルの形状係数 κ（鋼構造接合部設計指針）。
    ///
    /// - H 形: `κ = 1/(2/3 + 4・bc・tf/(dc・tp)) + 1/(1 + dc・tp/(6・bc・tf))`
    /// - 角形: `κ = 1/(2/3 + 2・bc/dc) + 1/(1 + dc/(3・bc))`
    /// - 円形: `κ = 4/π`
    pub fn kappa(&self) -> f64 {
        match self.kind {
            PanelShapeKind::H { bc, tf } => {
                1.0 / (2.0 / 3.0 + (4.0 * bc * tf) / (self.dc * self.tp))
                    + 1.0 / (1.0 + (self.dc * self.tp) / (6.0 * bc * tf))
            }
            PanelShapeKind::Box { bc } => {
                1.0 / (2.0 / 3.0 + 2.0 * bc / self.dc) + 1.0 / (1.0 + self.dc / (3.0 * bc))
            }
            PanelShapeKind::Pipe => 4.0 / std::f64::consts::PI,
        }
    }

    /// パネルの実効体積 `Ve` `[mm³]`（`db` は梁フランジ板厚中心間距離）。
    ///
    /// - H 形: `Ve = dc・db・tp`（ウェブ 1 枚）
    /// - 角形・円形: `Ve = 2・dc・db・tp`（ウェブ 2 枚相当）
    ///
    /// せん断剛性 `Kxp = Kyp = G・Ve` と降伏モーメント
    /// `pMy = (Ve/κ)・√(1−n²)・Fy/√3` の双方がこの体積を用いる。
    /// 資料の 6 面体体積 `V = Bx・By・Dz` に対し、H 形柱ではウェブ厚方向の寸法を
    /// `By = tp` と対応させたものであり、中実断面ではなく板厚分の実効体積となる。
    pub fn effective_volume(&self, db: f64) -> f64 {
        let base = self.dc * db * self.tp;
        match self.kind {
            PanelShapeKind::H { .. } => base,
            PanelShapeKind::Box { .. } | PanelShapeKind::Pipe => 2.0 * base,
        }
    }
}

/// 仕口パネルを設ける接合部の諸元。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelJoint {
    /// 諸元を採った柱の断面区分・`dc`・`tp`。
    pub geometry: PanelGeometry,
    /// 梁フランジ板厚中心間距離 `db` `[mm]`（取り付くはりの最大）。
    pub db: f64,
    /// 実効体積 `Ve` `[mm³]`。
    pub ve: f64,
    /// 諸元を採った柱の要素 ID（軸力比 `n`・基準強度 `F` の解決に用いる）。
    pub column: ElemId,
    /// 取り付く柱に充填断面（CFT）が 1 本でもあるか。
    ///
    /// モデル化はこれが `true` の接合部を対象外とする（モジュール冒頭
    /// 「柱断面ごとの扱い」）。断面検定は鋼管部を S 造と同じ式で評価するため
    /// 区別しない。
    pub has_filled_column: bool,
}

/// 局所座標のフランジ位置比較用公差 `[mm]`。閉区間で比較する。
pub const FLANGE_POSITION_TOL_MM: f64 = 1e-6;

/// 接合部を原点とする上・下フランジ板厚中心位置 `[mm]`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeamFlangePositions {
    pub upper: f64,
    pub lower: f64,
}

impl BeamFlangePositions {
    /// 形状外寸・上下板厚・局所鉛直配置から解決する。不正寸法はエラー。
    pub fn from_dimensions(
        height: f64,
        upper_tf: f64,
        lower_tf: f64,
        offset_z: f64,
    ) -> Result<Self, String> {
        if [height, upper_tf, lower_tf]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
            || !offset_z.is_finite()
            || upper_tf + lower_tf >= height
        {
            return Err("フランジ位置には有限の正寸法・非干渉板厚・有限の局所配置が必要".into());
        }
        Ok(Self {
            upper: offset_z + height / 2.0 - upper_tf / 2.0,
            lower: offset_z - height / 2.0 + lower_tf / 2.0,
        })
    }

    pub fn db(self) -> f64 {
        self.upper - self.lower
    }
}

/// 上下両方の局所位置差 `[mm]` が公差内なら標準形式。非有限値は対象外。
pub fn standard_flange_differences(upper_difference: f64, lower_difference: f64) -> bool {
    [upper_difference, lower_difference]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= FLANGE_POSITION_TOL_MM)
}

/// 同一検討面の梁。材軸は接合部から外向き、位置は接合部原点 `[mm]`。
#[derive(Clone, Debug, PartialEq)]
pub struct PanelPlaneBeam {
    pub element: ElemId,
    pub outward_axis: [f64; 3],
    pub outer_depth: Option<f64>,
    pub flanges: Result<BeamFlangePositions, String>,
}

/// 水平検討面の正方向材軸と、その面に取り付く梁。
#[derive(Clone, Debug, PartialEq)]
pub struct PanelPlane {
    pub axis: [f64; 3],
    pub beams: Vec<PanelPlaneBeam>,
}

impl PanelPlane {
    pub fn label(&self) -> String {
        format!(
            "検討面 [{:.3}, {:.3}, {:.3}]",
            self.axis[0], self.axis[1], self.axis[2]
        )
    }

    /// 片側梁は保持し、相対する梁の上下位置一致を確認する。
    pub fn standard_applicability(&self, node: NodeId) -> Result<(), String> {
        for beam in &self.beams {
            beam.flanges.as_ref().map_err(|reason| {
                format!(
                    "接合部 ID {} のパネル未算定: 梁 ID {} {}: {reason}",
                    node.0,
                    beam.element.0,
                    self.label()
                )
            })?;
        }
        if self.beams.len() > 2
            || (self.beams.len() == 2
                && geom::vec3::dot(self.beams[0].outward_axis, self.beams[1].outward_axis) >= 0.0)
        {
            return Err(format!(
                "接合部 ID {} のパネル未算定: {} 同じ側の複数梁の標準形式は未対応",
                node.0,
                self.label()
            ));
        }
        if let [left, right] = self.beams.as_slice() {
            let a = *left.flanges.as_ref().unwrap();
            let b = *right.flanges.as_ref().unwrap();
            let du = a.upper - b.upper;
            let dl = a.lower - b.lower;
            if !standard_flange_differences(du, dl) {
                return Err(format!("接合部 ID {} のパネル未算定: {} 梁 ID {} db={} mm / 梁 ID {} db={} mm、上位置差={} mm、下位置差={} mm、外寸差={} mm、db差={} mm。段違い接合の耐力・剛性式は未対応", node.0, self.label(), left.element.0, a.db(), right.element.0, b.db(), du, dl, left.outer_depth.unwrap_or(0.0)-right.outer_depth.unwrap_or(0.0), a.db()-b.db()));
            }
        }
        Ok(())
    }
}

/// 標準形式の方向確認用の無次元数値公差。施工・設計の傾斜許容値ではない。
pub const PANEL_DIRECTION_TOL: f64 = 1e-12;

fn model_beam_flanges(model: &Model, beam: &ElementData) -> Result<BeamFlangePositions, String> {
    let sec = model.element_section(beam).ok_or("断面未設定")?;
    beam_panel_depth(sec)?;
    let ex = member_unit_axis(model, beam).ok_or("梁材軸未解決")?;
    if !ex[2].is_finite() || ex[2].abs() > PANEL_DIRECTION_TOL {
        return Err("勾配梁・鉛直Girderのフランジ位置と標準形式は未対応（材軸水平が必要）".into());
    }
    let reject_ex = |v| geom::vec3::sub(v, geom::vec3::scale(ex, geom::vec3::dot(v, ex)));
    let mut ey = reject_ex(beam.local_axis.ref_vector);
    let mut length = geom::vec3::norm(ey);
    if length <= 1e-12 {
        ey = reject_ex(if ex[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        });
        length = geom::vec3::norm(ey);
    }
    if !length.is_finite() || length <= 1e-12 {
        return Err("梁せい方向未解決".into());
    }
    ey = geom::vec3::scale(ey, 1.0 / length);
    if ey[0].abs() > PANEL_DIRECTION_TOL
        || ey[1].abs() > PANEL_DIRECTION_TOL
        || (ey[2].abs() - 1.0).abs() > PANEL_DIRECTION_TOL
    {
        return Err(
            "横倒し・斜め断面回転のフランジ位置と標準形式は未対応（直交化した梁せい方向±Zが必要）"
                .into(),
        );
    }
    let (h, mut upper, mut lower) = match sec.shape {
        Some(SectionShape::SteelH {
            height,
            flange_thick,
            ..
        }) => (height, flange_thick, flange_thick),
        Some(SectionShape::SteelBuiltH {
            height,
            upper_thick,
            lower_thick,
            ..
        }) => (height, upper_thick, lower_thick),
        _ => unreachable!(),
    };
    if ey[2] < 0.0 {
        std::mem::swap(&mut upper, &mut lower);
    }
    BeamFlangePositions::from_dimensions(h, upper, lower, 0.0)
}

/// 相対する材軸で梁を検討面へ分ける。端部反転・要素順は結果に影響しない。
pub fn panel_joint_planes<'a>(
    model: &Model,
    node: NodeId,
    members: impl IntoIterator<Item = &'a ElementData>,
) -> Vec<PanelPlane> {
    let mut beams: Vec<_> = members
        .into_iter()
        .filter(|e| {
            e.nodes.iter().take(2).any(|n| *n == node)
                && member_orientation(model, e) == Some(MemberOrientation::Beam)
        })
        .collect();
    beams.sort_by_key(|e| e.id);
    let mut planes: Vec<PanelPlane> = Vec::new();
    for beam in beams {
        let mut outward_axis = member_unit_axis(model, beam).unwrap_or([0.0; 3]);
        if beam.nodes[0] != node {
            outward_axis = outward_axis.map(|v| -v);
        }
        let horizontal_length = outward_axis[0].hypot(outward_axis[1]);
        let mut axis = if horizontal_length > 1e-12 {
            [
                outward_axis[0] / horizontal_length,
                outward_axis[1] / horizontal_length,
                0.0,
            ]
        } else {
            outward_axis
        };
        if axis
            .iter()
            .find(|v| v.abs() > 1e-12)
            .is_some_and(|v| *v < 0.0)
        {
            axis = axis.map(|v| -v);
        }
        axis = axis.map(|v| if v.abs() < 1e-12 { 0.0 } else { v });
        let flanges = model_beam_flanges(model, beam);
        let outer_depth = model.element_section(beam).and_then(|sec| match sec.shape {
            Some(
                SectionShape::SteelH { height, .. } | SectionShape::SteelBuiltH { height, .. },
            ) => Some(height),
            _ => None,
        });
        let entry = PanelPlaneBeam {
            element: beam.id,
            outward_axis,
            outer_depth,
            flanges,
        };
        if let Some(plane) = planes
            .iter_mut()
            .find(|p| geom::vec3::dot(p.axis, axis) > 1.0 - 1e-12)
        {
            plane.beams.push(entry);
        } else {
            planes.push(PanelPlane {
                axis,
                beams: vec![entry],
            });
        }
    }
    planes.sort_by(|a, b| {
        a.axis[0]
            .total_cmp(&b.axis[0])
            .reverse()
            .then(a.axis[1].total_cmp(&b.axis[1]))
    });
    planes
}

/// 節点 `node` に仕口パネルを設けられるか判定し、諸元を解決する。
///
/// `members` は当該節点へ接続する要素（隣接マップを持つ呼び出し側はそれを渡し、
/// 持たない側は全要素を渡してよい）。
///
/// 判定規則はモジュール冒頭「対象とする接合部」のとおりで、モデル化と断面検定の
/// 双方がこの関数を通る。採用柱・梁の不正や柱の識別不能は接合部/部材ID付きエラー、候補外は `Ok(None)`。
/// モデル化はさらに [`PanelJoint::has_filled_column`] が
/// `false` であることを要求する。
pub fn resolve_panel_joint<'a>(
    model: &Model,
    node: NodeId,
    members: impl IntoIterator<Item = &'a ElementData>,
) -> Result<Option<PanelJoint>, String> {
    resolve_panel_joint_impl(model, node, members, false)
}

/// 明示パネルの参照柱を要求して解決する。柱不在は接合部 ID 付きエラー、既存の材種対象外は `Ok(None)`。
pub fn resolve_explicit_panel_joint<'a>(
    model: &Model,
    node: NodeId,
    members: impl IntoIterator<Item = &'a ElementData>,
) -> Result<Option<PanelJoint>, String> {
    resolve_panel_joint_impl(model, node, members, true)
}

fn resolve_panel_joint_impl<'a>(
    model: &Model,
    node: NodeId,
    members: impl IntoIterator<Item = &'a ElementData>,
    explicit_panel: bool,
) -> Result<Option<PanelJoint>, String> {
    let mut columns: Vec<&ElementData> = Vec::new();
    let mut beams: Vec<&ElementData> = Vec::new();
    let mut unidentified: Vec<&ElementData> = Vec::new();
    for e in members {
        if !e.kind.is_primary_frame_support() || !e.nodes.iter().take(2).any(|n| *n == node) {
            continue;
        }
        match e
            .section
            .and_then(|sid| model.sections.get(sid.index()))
            .and_then(|s| s.frame_use)
        {
            Some(FrameSectionUse::Column) => columns.push(e),
            Some(FrameSectionUse::Girder) => beams.push(e),
            Some(FrameSectionUse::Brace) => {}
            None => unidentified.push(e),
        }
    }
    if columns.is_empty()
        && unidentified.is_empty()
        && (explicit_panel
            || model.elements.iter().any(|e| {
                e.kind == crate::model::ElementKind::PanelZone && e.nodes.first() == Some(&node)
            }))
        && beams
            .iter()
            .all(|e| member_structure_kind(model, e).is_steel_like())
    {
        return Err(format!(
            "接合部 ID {} のパネル未算定: 明示パネルの参照柱が存在しません",
            node.0
        ));
    }
    if beams.is_empty() {
        return Ok(None);
    }
    let invalid_column = |reason: String| format!("接合部 ID {} のパネル未算定: {reason}", node.0);
    unidentified.sort_by_key(|e| e.id);
    if !unidentified.is_empty() {
        return Err(invalid_column(format!(
            "部材 ID {}: 断面未設定・参照不明または用途未設定のため柱の存在を確定できません",
            unidentified
                .iter()
                .map(|e| e.id.0.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if columns.is_empty() {
        return Ok(None);
    }
    columns.sort_by_key(|e| e.id);
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    for column in &columns {
        let far = column
            .nodes
            .iter()
            .take(2)
            .find(|id| **id != node)
            .and_then(|id| model.nodes.get(id.index()));
        let center = model.nodes.get(node.index());
        let Some((far, center)) = far.zip(center) else {
            return Err(invalid_column(format!(
                "柱 ID {}: 接続節点を解決できません",
                column.id.0
            )));
        };
        let Some(axis) =
            member_unit_axis(model, column).filter(|axis| axis.iter().all(|v| v.is_finite()))
        else {
            return Err(invalid_column(format!(
                "柱 ID {}: 有限の非零材軸が必要です",
                column.id.0
            )));
        };
        let dz = far.coord[2] - center.coord[2];
        if !dz.is_finite() || dz == 0.0 || axis[2] == 0.0 {
            return Err(invalid_column(format!(
                "柱 ID {}: 接合部に対する上下を特定できません",
                column.id.0
            )));
        }
        if dz < 0.0 {
            lower.push(*column);
        } else {
            upper.push(*column);
        }
    }
    let (side, candidates) = if lower.is_empty() {
        ("上側", upper)
    } else {
        ("下側", lower)
    };
    if candidates.len() != 1 {
        return Err(invalid_column(format!(
            "{side}柱を一意に特定できません: 柱 ID {}",
            candidates
                .iter()
                .map(|e| e.id.0.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let selected = candidates[0];
    let selected_section = model.element_section(selected).ok_or_else(|| {
        invalid_column(format!("柱 ID {}: 断面未設定または参照不明", selected.id.0))
    })?;
    if model.element_material(selected).is_none() {
        return Err(invalid_column(format!(
            "柱 ID {}: 材料未設定または参照不明",
            selected.id.0
        )));
    }
    if columns
        .iter()
        .chain(beams.iter())
        .any(|e| !member_structure_kind(model, e).is_steel_like())
    {
        return Ok(None);
    }

    let section_of = |e: &ElementData| e.section.and_then(|sid| model.sections.get(sid.index()));
    beams.sort_by_key(|e| e.id);
    let mut db = 0.0_f64;
    let mut reasons = Vec::new();
    for beam in &beams {
        let Some(section) = section_of(beam) else {
            reasons.push(format!("梁 ID {}: 断面未設定", beam.id.0));
            continue;
        };
        let properties = beam_panel_properties(section);
        let shape = section
            .shape
            .as_ref()
            .map(SectionShape::dimension_label)
            .unwrap_or_else(|| "形状未定義".into());
        match (&properties.depth, &properties.applicability) {
            (Ok(depth), Ok(())) => db = db.max(*depth),
            _ => {
                let conditions = properties
                    .depth
                    .err()
                    .into_iter()
                    .chain(properties.applicability.err())
                    .collect::<Vec<_>>()
                    .join("; ");
                let mut axis = member_unit_axis(model, beam).unwrap_or([0.0; 3]);
                if axis
                    .iter()
                    .find(|v| v.abs() > 1e-12)
                    .is_some_and(|v| *v < 0.0)
                {
                    axis = axis.map(|v| -v);
                }
                let axis = axis.map(|v| if v.abs() < 1e-12 { 0.0 } else { v });
                reasons.push(format!(
                    "梁 ID {} ({shape}, 検討面の梁材軸 [{:.3}, {:.3}, {:.3}]): {conditions}",
                    beam.id.0, axis[0], axis[1], axis[2]
                ));
            }
        }
    }
    if !reasons.is_empty() {
        return Err(format!(
            "接合部 ID {} のパネル未算定: {}",
            node.0,
            reasons.join(" / ")
        ));
    }

    for plane in panel_joint_planes(model, node, beams.iter().copied()) {
        plane.standard_applicability(node)?;
    }

    let geometry_of = |e: &&ElementData| section_of(e).and_then(PanelGeometry::from_column);
    let has_filled_column = columns.iter().filter_map(geometry_of).any(|g| g.filled);

    if !is_horizontal_or_vertical_member(model, selected) {
        return Err(invalid_column(format!(
            "柱 ID {}: 斜め柱のパネル式は未対応",
            selected.id.0
        )));
    }
    let geometry = PanelGeometry::from_column(selected_section).ok_or_else(|| {
        invalid_column(format!(
            "柱 ID {}: 断面形状が未定義またはパネル諸元に未対応",
            selected.id.0
        ))
    })?;
    let dimensions = match geometry.kind {
        PanelShapeKind::H { bc, tf } => vec![geometry.dc, geometry.tp, bc, tf],
        PanelShapeKind::Box { bc } => vec![geometry.dc, geometry.tp, bc],
        PanelShapeKind::Pipe => vec![geometry.dc, geometry.tp],
    };
    let ve = geometry.effective_volume(db);
    if dimensions.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || selected_section
            .panel_thickness
            .is_some_and(|v| !v.is_finite() || v <= 0.0)
        || !ve.is_finite()
        || ve <= 0.0
    {
        return Err(invalid_column(format!(
            "柱 ID {}: パネル寸法・板厚・有効体積Veは有限の正値が必要です",
            selected.id.0
        )));
    }

    let selected_material = model.element_material(selected).unwrap();
    let axial_stiffness =
        selected_material.young * selected_section.area / model.member_length(selected);
    if !selected_section.area.is_finite()
        || selected_section.area <= 0.0
        || !axial_stiffness.is_finite()
        || axial_stiffness <= 0.0
    {
        return Err(invalid_column(format!(
            "柱 ID {}: 断面積Aと軸剛性E・A/Lは有限の正値が必要です",
            selected.id.0
        )));
    }
    Ok(Some(PanelJoint {
        geometry,
        db,
        ve,
        column: selected.id,
        has_filled_column,
    }))
}

/// 梁の幾何解決 `[mm]` とパネル式適用の状態。保存断面諸元の代用は行わない。
#[derive(Clone, Debug, PartialEq)]
pub struct BeamPanelProperties {
    pub depth: Result<f64, String>,
    pub applicability: Result<(), String>,
}

/// 梁のフランジ板厚中心間距離 `[mm]` と式の適用可否を別々に返す。
pub fn beam_panel_properties(sec: &Section) -> BeamPanelProperties {
    let applicability = match sec.shape {
        Some(SectionShape::SteelH { .. } | SectionShape::SteelBuiltH { .. }) => Ok(()),
        _ => Err("この梁形状・接合詳細へのH梁パネル式の適用根拠未確認".into()),
    };
    BeamPanelProperties {
        depth: beam_panel_depth(sec),
        applicability,
    }
}

/// 形状寸法から梁フランジ板厚中心間距離 `db` `[mm]` を返す。
/// 不正寸法・未定義のフランジ対は理由付きエラー。式適用可否は別途確認する。
pub fn beam_panel_depth(sec: &Section) -> Result<f64, String> {
    let (height, upper, lower, dimensions) = match sec.shape {
        Some(SectionShape::SteelH {
            height,
            width,
            web_thick,
            flange_thick,
            ..
        }) => (
            height,
            flange_thick,
            flange_thick,
            vec![height, width, web_thick, flange_thick],
        ),
        Some(SectionShape::SteelBuiltH {
            height,
            upper_width,
            upper_thick,
            lower_width,
            lower_thick,
            web_thick,
        }) => (
            height,
            upper_thick,
            lower_thick,
            vec![
                height,
                upper_width,
                upper_thick,
                lower_width,
                lower_thick,
                web_thick,
            ],
        ),
        None => return Err("断面形状未定義: フランジ対を解決できません".into()),
        _ => return Err("梁フランジ対とその板厚中心間距離の定義が未対応".into()),
    };
    if dimensions.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err("外寸・幅・上下フランジ板厚・ウェブ厚は有限の正値 [mm] が必要".into());
    }
    if upper + lower >= height {
        return Err("上下フランジが干渉: 板厚合計は外寸未満が必要".into());
    }
    let db = height - (upper / 2.0 + lower / 2.0);
    if !db.is_finite() || db <= 0.0 {
        return Err("板厚中心間距離は有限の正値 [mm] が必要".into());
    }
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_member_axes_and_orientation_do_not_depend_on_frame_representation() {
        let mut model = Model {
            nodes: vec![node(0, [0.0; 3]), node(1, [3000.0, 0.0, 0.0])],
            ..Default::default()
        };
        let mut section = Section::zero(crate::ids::SectionId(0), "G1".into());
        section.frame_use = Some(FrameSectionUse::Girder);
        model.sections.push(section);
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            let element = ElementData {
                id: ElemId(0),
                kind,
                nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
                section: Some(crate::ids::SectionId(0)),
                local_axis: crate::model::LocalAxis {
                    ref_vector: [0.0, 1.0, 0.0],
                },
                end_cond: [crate::model::EndCondition::Fixed; 2],
                force_regime: crate::model::ForceRegime::Auto,
                rigid_zone: Default::default(),
                plastic_zone: None,
                spring: None,
            };
            assert_eq!(member_unit_axis(&model, &element), Some([1.0, 0.0, 0.0]));
            assert_eq!(
                member_orientation(&model, &element),
                Some(MemberOrientation::Beam)
            );
            assert!(is_horizontal_or_vertical_member(&model, &element));
            model.sections[0].frame_use = Some(FrameSectionUse::Column);
            assert_eq!(
                member_orientation(&model, &element),
                Some(MemberOrientation::Column)
            );
            model.sections[0].frame_use = Some(FrameSectionUse::Girder);
        }
    }
    use crate::ids::{MaterialId, SectionId};
    use crate::model::{ElementKind, MaterialCategory};

    fn sec(shape: SectionShape, depth: f64, panel_thickness: Option<f64>) -> Section {
        Section {
            frame_use: None,
            id: SectionId(0),
            name: String::new(),
            floor: None,
            area: 1.0e4,
            iy: 1.0e8,
            iz: 1.0e8,
            j: 1.0e8,
            depth,
            width: depth,
            as_y: 0.0,
            as_z: 0.0,
            panel_thickness,
            thickness: None,
            shape: Some(shape),
            material: None,
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }
    }

    /// [`sec`] の id と主材料も指定できる版。
    fn sec_with_mat(
        shape: SectionShape,
        depth: f64,
        panel_thickness: Option<f64>,
        id: u32,
        mat: u32,
    ) -> Section {
        Section {
            id: SectionId(id),
            material: Some(crate::ids::MaterialId(mat)),
            ..sec(shape, depth, panel_thickness)
        }
    }

    /// H 形柱: dc = せい − フランジ厚、tp = ウェブ厚、Ve = dc・db・tp。
    #[test]
    fn test_h_column_geometry() {
        let s = sec(
            SectionShape::SteelH {
                root_r: Some(0.0),
                height: 400.0,
                width: 400.0,
                web_thick: 13.0,
                flange_thick: 21.0,
            },
            400.0,
            None,
        );
        let g = PanelGeometry::from_column(&s).expect("H 形は対象");
        assert!((g.dc - (400.0 - 21.0)).abs() < 1e-9);
        assert!((g.tp - 13.0).abs() < 1e-9);
        let db = 600.0;
        assert!((g.effective_volume(db) - g.dc * db * g.tp).abs() < 1e-6);
    }

    /// 角形・円形はウェブ 2 枚相当で Ve が 2 倍になる。
    #[test]
    fn test_box_and_pipe_double_volume() {
        let b = sec(
            SectionShape::SteelBox {
                height: 400.0,
                width: 400.0,
                thick: 16.0,
                corner_r: Some(0.0),
            },
            400.0,
            None,
        );
        let g = PanelGeometry::from_column(&b).expect("角形は対象");
        let db = 500.0;
        assert!((g.effective_volume(db) - 2.0 * g.dc * db * g.tp).abs() < 1e-6);

        let p = sec(
            SectionShape::SteelPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
            400.0,
            None,
        );
        let gp = PanelGeometry::from_column(&p).expect("円形は対象");
        assert!((gp.dc - (400.0 - 12.0)).abs() < 1e-9);
        assert!((gp.effective_volume(db) - 2.0 * gp.dc * db * gp.tp).abs() < 1e-6);
        assert!((gp.kappa() - 4.0 / std::f64::consts::PI).abs() < 1e-12);
    }

    /// `panel_thickness` の明示入力は断面形状から算出した板厚より優先される
    /// （ダイアフラム補強・ダブラープレートによる増厚）。
    #[test]
    fn test_panel_thickness_overrides_shape() {
        let s = sec(
            SectionShape::SteelH {
                root_r: Some(0.0),
                height: 400.0,
                width: 400.0,
                web_thick: 13.0,
                flange_thick: 21.0,
            },
            400.0,
            Some(25.0),
        );
        let g = PanelGeometry::from_column(&s).expect("H 形は対象");
        assert!((g.tp - 25.0).abs() < 1e-9, "明示入力が優先される: {}", g.tp);

        // 0 以下は未入力扱いとし、断面形状の値へフォールバックする。
        let z = sec(
            SectionShape::SteelH {
                root_r: Some(0.0),
                height: 400.0,
                width: 400.0,
                web_thick: 13.0,
                flange_thick: 21.0,
            },
            400.0,
            Some(0.0),
        );
        let gz = PanelGeometry::from_column(&z).expect("H 形は対象");
        assert!((gz.tp - 13.0).abs() < 1e-9);
    }

    /// CFT は諸元を解決できるが、モデル化の対象外とする。断面検定は鋼管部を
    /// S 造と同じ式で評価するため、諸元解決自体は成功させる必要がある。
    #[test]
    fn test_cft_resolves_but_is_not_modeling_target() {
        let cases = [
            (
                SectionShape::CftBox {
                    corner_r: Some(0.0),
                    height: 400.0,
                    width: 400.0,
                    thick: 16.0,
                },
                PanelShapeKind::Box { bc: 400.0 },
            ),
            (
                SectionShape::CftPipe {
                    outer_dia: 400.0,
                    thick: 12.0,
                },
                PanelShapeKind::Pipe,
            ),
        ];
        for (shape, kind) in cases {
            let s = sec(shape, 400.0, None);
            let g = PanelGeometry::from_column(&s).expect("CFT も諸元は解決できる");
            assert_eq!(g.kind, kind);
            assert!(g.filled, "CFT は充填断面");
            assert!(g.filled, "CFT はモデル化の対象外");
            // 検定に使う Ve・κ は鋼管と同じ式で求まる。
            assert!(g.effective_volume(500.0) > 0.0);
            assert!(g.kappa() > 0.0);
        }
    }

    /// CFT でない鋼管（角形・円形）と H 形はモデル化の対象。
    #[test]
    fn test_steel_sections_are_modeling_targets() {
        let shapes = [
            SectionShape::SteelH {
                root_r: Some(0.0),
                height: 400.0,
                width: 400.0,
                web_thick: 13.0,
                flange_thick: 21.0,
            },
            SectionShape::SteelBox {
                height: 400.0,
                width: 400.0,
                thick: 16.0,
                corner_r: Some(0.0),
            },
            SectionShape::SteelPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
        ];
        for shape in shapes {
            let s = sec(shape, 400.0, None);
            let g = PanelGeometry::from_column(&s).expect("S 造は対象");
            assert!(!g.filled);
            assert!(!g.filled);
        }
    }

    /// CFT と対応する鋼管は、断面検定に使う Ve・κ が同一になる
    /// （CFT を検定対象から外していないことの裏付け）。
    #[test]
    fn test_cft_and_steel_tube_share_check_properties() {
        let steel = sec(
            SectionShape::SteelBox {
                height: 400.0,
                width: 400.0,
                thick: 16.0,
                corner_r: Some(0.0),
            },
            400.0,
            None,
        );
        let cft = sec(
            SectionShape::CftBox {
                corner_r: Some(0.0),
                height: 400.0,
                width: 400.0,
                thick: 16.0,
            },
            400.0,
            None,
        );
        let (gs, gc) = (
            PanelGeometry::from_column(&steel).expect("角形"),
            PanelGeometry::from_column(&cft).expect("CFT 角形"),
        );
        assert!((gs.dc - gc.dc).abs() < 1e-12);
        assert!((gs.tp - gc.tp).abs() < 1e-12);
        assert!((gs.kappa() - gc.kappa()).abs() < 1e-12);
        assert!((gs.effective_volume(500.0) - gc.effective_volume(500.0)).abs() < 1e-9);
    }

    /// RC 柱はパネルの対象外（剛域と RC 柱梁接合部検定で扱う）。
    #[test]
    fn test_rc_column_is_not_panel_target() {
        let s = sec(rc_rect_shape(700.0, 700.0), 700.0, None);
        assert!(PanelGeometry::from_column(&s).is_none());
    }

    // ===== 接合部の判定（モデル化・断面検定の共通規則）=====

    fn node(id: u32, coord: [f64; 3]) -> crate::model::Node {
        crate::model::Node {
            id: NodeId(id),
            coord,
            restraint: crate::dof::Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        }
    }

    fn member(id: u32, n0: u32, n1: u32, sec: u32) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(n0), NodeId(n1)],
            section: Some(crate::ids::SectionId(sec)),
            local_axis: crate::model::LocalAxis {
                ref_vector: if sec == 1 {
                    [0.0, 1.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                },
            },
            end_cond: [
                crate::model::EndCondition::Fixed,
                crate::model::EndCondition::Fixed,
            ],
            force_regime: crate::model::ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }
    }

    fn h_col() -> SectionShape {
        SectionShape::SteelH {
            root_r: Some(0.0),
            height: 400.0,
            width: 400.0,
            web_thick: 13.0,
            flange_thick: 21.0,
        }
    }

    fn h_beam() -> SectionShape {
        SectionShape::SteelH {
            root_r: Some(0.0),
            height: 600.0,
            width: 200.0,
            web_thick: 11.0,
            flange_thick: 17.0,
        }
    }

    /// RC 矩形断面。
    fn rc_rect_shape(b: f64, d: f64) -> SectionShape {
        SectionShape::RcColumnRect {
            b,
            d,
            rebar: crate::section_shape::RcRectColumnRebar {
                main_dia: 25.0,
                x: vec![4],
                y: vec![4],
                cover: 40.0,
                hoop: crate::section_shape::RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        }
    }

    fn mat(id: u32, category: MaterialCategory) -> crate::model::Material {
        crate::model::Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: crate::ids::MaterialId(id),
            name: String::new(),
            category,
            young: 205_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }
    }

    /// 節点 0 を接合部とする T 型（梁 1 本・柱 1 本）のモデル。
    /// 断面 0 が梁、断面 1 が柱。材料 0 が鋼材、材料 1 がコンクリート。
    fn joint_model(beam: SectionShape, beam_depth: f64, col: SectionShape) -> Model {
        joint_model_with_mats(beam, beam_depth, col, 0, 0)
    }

    /// [`joint_model`] の材料も指定できる版（0 = 鋼材、1 = コンクリート）。
    fn joint_model_with_mats(
        beam: SectionShape,
        beam_depth: f64,
        col: SectionShape,
        beam_mat: u32,
        col_mat: u32,
    ) -> Model {
        let mut beam_sec = sec_with_mat(beam, beam_depth, None, 0, beam_mat);
        beam_sec.frame_use = Some(FrameSectionUse::Girder);
        let mut col_sec = sec_with_mat(col, 400.0, None, 1, col_mat);
        col_sec.frame_use = Some(FrameSectionUse::Column);
        Model {
            nodes: vec![
                node(0, [0.0, 0.0, 3000.0]),
                node(1, [6000.0, 0.0, 3000.0]),
                node(2, [0.0, 0.0, 0.0]),
            ],
            // 材料は断面が持つ。断面 0（梁）・断面 1（柱）へそれぞれ割り当てる。
            sections: vec![beam_sec, col_sec],
            materials: vec![
                mat(0, MaterialCategory::Steel),
                mat(1, MaterialCategory::Concrete),
            ],
            elements: vec![member(0, 0, 1, 0), member(1, 2, 0, 1)],
            ..Default::default()
        }
    }

    /// 柱・はりがすべて S なら対象になる。
    #[test]
    fn test_all_steel_joint_is_target() {
        let m = joint_model(h_beam(), 600.0, h_col());
        let joint = resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .expect("S 造接合部");
        assert!((joint.db - (600.0 - 17.0)).abs() < 1e-9);
        assert_eq!(joint.column, ElemId(1));
        assert!(!joint.has_filled_column);
    }

    /// RC 梁が 1 本でもあれば対象外（柱が S でも接合部は RC になる）。
    /// 判定は材料の区分によるため、断面形状ではなく材料で RC にする。
    #[test]
    fn test_rc_beam_disqualifies_joint() {
        let m = joint_model_with_mats(rc_rect_shape(400.0, 700.0), 700.0, h_col(), 1, 0);
        assert!(resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .is_none());
    }

    /// RC 柱が 1 本でもあれば対象外。
    #[test]
    fn test_rc_column_disqualifies_joint() {
        let m = joint_model_with_mats(h_beam(), 600.0, rc_rect_shape(400.0, 700.0), 0, 1);
        assert!(resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .is_none());
    }

    /// H 形の断面でも材料がコンクリートなら対象外になる。
    /// 断面形状ではなく材料の区分で判定していることの裏付け。
    #[test]
    fn test_steel_shape_with_concrete_material_is_excluded() {
        let m = joint_model_with_mats(h_beam(), 600.0, h_col(), 1, 0);
        assert!(
            resolve_panel_joint(&m, NodeId(0), &m.elements)
                .unwrap()
                .is_none(),
            "H 形でも材料がコンクリートなら S 造の接合部ではない"
        );
    }

    /// 柱だけ・はりだけの節点は対象外。
    #[test]
    fn test_column_or_beam_only_node_is_not_target() {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        let beam_only = {
            let mut mm = m.clone();
            mm.elements.retain(|e| e.id != ElemId(1));
            mm
        };
        assert!(
            resolve_panel_joint(&beam_only, NodeId(0), &beam_only.elements)
                .unwrap()
                .is_none()
        );
        m.elements.retain(|e| e.id != ElemId(0));
        assert!(resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .is_none());
    }

    #[test]
    fn lower_column_is_selected_independently_of_volume_order_and_ends() {
        let thin = SectionShape::SteelH {
            root_r: Some(0.0),
            height: 400.0,
            width: 400.0,
            web_thick: 9.0,
            flange_thick: 21.0,
        };
        let build = |upper_first: bool, web_thick: f64| {
            let mut m = joint_model(h_beam(), 600.0, h_col());
            m.nodes.push(node(3, [0.0, 0.0, 6000.0]));
            let mut thin_sec = sec(thin.clone(), 400.0, None);
            thin_sec.id = SectionId(2);
            thin_sec.frame_use = Some(FrameSectionUse::Column);
            if let Some(SectionShape::SteelH {
                web_thick: value, ..
            }) = &mut thin_sec.shape
            {
                *value = web_thick;
            }
            m.sections.push(thin_sec);
            let upper = member(2, 0, 3, 2);
            if upper_first {
                m.elements.insert(0, upper);
            } else {
                m.elements.push(upper);
            }
            m
        };
        for web_thick in [9.0, 13.0, 20.0] {
            for upper_first in [false, true] {
                let mut m = build(upper_first, web_thick);
                let joint = resolve_panel_joint(&m, NodeId(0), &m.elements)
                    .unwrap()
                    .unwrap();
                assert_eq!(joint.column, ElemId(1));
                assert_eq!(joint.geometry.tp, 13.0);
                assert_eq!(joint.ve, 2_872_441.0);
                m.elements.reverse();
                for e in &mut m.elements {
                    e.nodes.reverse();
                }
                assert_eq!(
                    resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap(),
                    Some(joint)
                );
            }
        }
    }

    fn two_column_joint() -> Model {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        m.nodes.push(node(3, [0.0, 0.0, 6000.0]));
        let mut upper = m.sections[1].clone();
        upper.id = SectionId(2);
        upper.panel_thickness = Some(9.0);
        m.sections.push(upper);
        m.elements.push(member(2, 0, 3, 2));
        m
    }

    #[test]
    fn only_physically_absent_lower_column_allows_upper_column() {
        for remaining in [ElemId(1), ElemId(2)] {
            let mut m = two_column_joint();
            m.elements
                .retain(|e| e.id == ElemId(0) || e.id == remaining);
            let joint = resolve_panel_joint(&m, NodeId(0), &m.elements)
                .unwrap()
                .unwrap();
            assert_eq!(joint.column, remaining);
            assert_eq!(
                joint.geometry.tp,
                if remaining == ElemId(1) { 13.0 } else { 9.0 }
            );
        }
    }

    #[test]
    fn invalid_lower_column_never_falls_back_to_valid_upper_column() {
        for invalid in 0..14 {
            let mut m = two_column_joint();
            match invalid {
                0 => m.elements[1].section = None,
                1 => m.elements[1].section = Some(SectionId(99)),
                2 => m.sections[1].frame_use = None,
                3 => m.sections[1].material = None,
                4 => m.sections[1].material = Some(MaterialId(99)),
                5 => m.sections[1].shape = None,
                6 => m.sections[1].panel_thickness = Some(f64::NAN),
                7 => m.sections[1].panel_thickness = Some(0.0),
                8 => m.sections[1].panel_thickness = Some(f64::INFINITY),
                9 => {
                    m.sections[1].shape = Some(SectionShape::SteelBuiltH {
                        height: 400.0,
                        upper_width: 400.0,
                        upper_thick: 21.0,
                        lower_width: 400.0,
                        lower_thick: 21.0,
                        web_thick: 13.0,
                    })
                }
                10 => m.nodes[2].coord = [3000.0, 0.0, 0.0],
                11 => {
                    m.sections[1].shape = Some(SectionShape::SteelH {
                        height: 400.0,
                        width: 400.0,
                        web_thick: f64::NAN,
                        flange_thick: 21.0,
                        root_r: None,
                    })
                }
                12 | 13 => {
                    m.sections[1].shape = Some(SectionShape::SteelH {
                        height: if invalid == 12 { 21.0 } else { 20.0 },
                        width: 400.0,
                        web_thick: 13.0,
                        flange_thick: 21.0,
                        root_r: None,
                    })
                }
                _ => unreachable!(),
            }
            let reason = resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err();
            assert!(reason.contains("接合部 ID 0"), "{invalid}: {reason}");
            assert!(reason.contains("ID 1"), "{invalid}: {reason}");
            assert!(reason.contains(':'), "{invalid}: {reason}");
            m.elements.reverse();
            for e in &mut m.elements {
                e.nodes.reverse();
            }
            assert_eq!(
                resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err(),
                reason
            );
        }
    }

    #[test]
    fn ambiguous_or_unresolved_column_connection_reports_joint_and_reason() {
        for invalid in 0..6 {
            let mut m = two_column_joint();
            match invalid {
                0 => m.elements.push(member(3, 0, 2, 1)),
                1 => {
                    m.elements.retain(|e| e.id != ElemId(1));
                    m.elements.push(member(3, 0, 3, 2));
                }
                2 => m.nodes[2].coord = [1000.0, 0.0, 3000.0],
                3 => m.elements[1].nodes[0] = NodeId(99),
                4 => m.nodes[2].coord[2] = f64::NAN,
                5 => m.elements[1].nodes = smallvec::smallvec![NodeId(0)],
                _ => unreachable!(),
            }
            let reason = resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err();
            assert!(reason.contains("接合部 ID 0"), "{invalid}: {reason}");
            assert!(reason.contains("柱 ID"), "{invalid}: {reason}");
            m.elements.reverse();
            for e in &mut m.elements {
                e.nodes.reverse();
            }
            assert_eq!(
                resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err(),
                reason
            );
        }
    }

    #[test]
    fn legal_model_permutation_keeps_physical_lower_column() {
        let mut m = two_column_joint();
        let before = resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .unwrap();
        m.nodes.reverse();
        for (i, n) in m.nodes.iter_mut().enumerate() {
            n.id = NodeId(i as u32);
        }
        m.sections.reverse();
        for (i, section) in m.sections.iter_mut().enumerate() {
            section.id = SectionId(i as u32);
        }
        m.materials.reverse();
        for (i, material) in m.materials.iter_mut().enumerate() {
            material.id = MaterialId(i as u32);
        }
        for section in &mut m.sections {
            section.material = section.material.map(|id| MaterialId(1 - id.0));
        }
        m.elements.rotate_left(1);
        for (i, e) in m.elements.iter_mut().enumerate() {
            e.id = ElemId(i as u32);
            e.section = e.section.map(|id| SectionId(2 - id.0));
            for n in &mut e.nodes {
                *n = NodeId(3 - n.0);
            }
            e.nodes.reverse();
        }
        let after = resolve_panel_joint(&m, NodeId(3), &m.elements)
            .unwrap()
            .unwrap();
        assert_eq!(after.column, ElemId(0));
        assert_eq!(after.geometry, before.geometry);
        assert_eq!(after.ve, before.ve);
        assert_eq!(after.db, before.db);
        assert_eq!(m.elements[0].nodes.as_slice(), &[NodeId(3), NodeId(1)]);
    }

    /// CFT 柱の接合部は解決できるが、モデル化からは除外する目印が立つ。
    #[test]
    fn test_cft_column_flags_filled() {
        let m = joint_model(
            h_beam(),
            600.0,
            SectionShape::CftBox {
                corner_r: Some(0.0),
                height: 400.0,
                width: 400.0,
                thick: 16.0,
            },
        );
        let joint = resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .expect("検定の対象にはなる");
        assert!(joint.has_filled_column, "モデル化からは除外する");
    }

    // ===== 接合部の半寸法 =====

    /// 半寸法は柱せい・梁せいの 1/2。危険断面位置（face）は参照しない。
    #[test]
    fn test_panel_half_extent_uses_member_depths() {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        for e in &mut m.elements {
            e.rigid_zone.face_i = Some(9999.0);
            e.rigid_zone.face_j = Some(9999.0);
        }
        let extent = panel_half_extent(&m, NodeId(0), &m.elements);
        assert!((extent.column_half - 200.0).abs() < 1e-9);
        assert!((extent.beam_half - 300.0).abs() < 1e-9);
        assert!((extent.offset_for(MemberOrientation::Beam) - 200.0).abs() < 1e-9);
        assert!((extent.offset_for(MemberOrientation::Column) - 300.0).abs() < 1e-9);
    }

    #[test]
    fn test_diagonal_member_depth_is_excluded_from_panel_dimensions() {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        m.nodes.push(node(3, [4000.0, 0.0, 6000.0]));
        let mut diagonal_section = m.sections[0].clone();
        diagonal_section.id = SectionId(2);
        diagonal_section.frame_use = Some(FrameSectionUse::Brace);
        diagonal_section.depth = 2000.0;
        m.sections.push(diagonal_section);
        m.elements.push(member(2, 0, 3, 2));

        let joint = resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .expect("接合部");
        assert!((joint.db - (600.0 - 17.0)).abs() < 1e-9);
        let extent = panel_half_extent(&m, NodeId(0), &m.elements);
        assert!((extent.beam_half - 300.0).abs() < 1e-9);
    }

    /// 用途が梁の傾斜部材は梁として扱い、幾何による再分類を行わない。
    #[test]
    fn test_diagonal_beam_uses_section_use() {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        m.nodes[1].coord = [4000.0, 0.0, 6000.0];
        assert_eq!(
            member_orientation(&m, &m.elements[0]),
            Some(MemberOrientation::Beam)
        );
    }

    #[test]
    fn test_diagonal_column_uses_section_use() {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        m.nodes[2].coord = [1000.0, 0.0, 0.0];
        assert_eq!(
            member_orientation(&m, &m.elements[1]),
            Some(MemberOrientation::Column)
        );
    }

    #[test]
    fn test_non_line_element_is_not_classified_by_section_use() {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        m.elements[0].kind = ElementKind::Wall;
        assert_eq!(member_orientation(&m, &m.elements[0]), None);
        assert!(resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .is_none());
    }

    /// H 梁の中心間距離を解決し、箱形の未定義フランジ対は拒否する。
    #[test]
    fn test_beam_panel_depth() {
        let h = sec(
            SectionShape::SteelH {
                root_r: Some(0.0),
                height: 600.0,
                width: 200.0,
                web_thick: 11.0,
                flange_thick: 17.0,
            },
            600.0,
            None,
        );
        assert!((beam_panel_depth(&h).unwrap() - (600.0 - 17.0)).abs() < 1e-9);

        let b = sec(
            SectionShape::SteelBox {
                height: 500.0,
                width: 300.0,
                thick: 12.0,
                corner_r: Some(0.0),
            },
            500.0,
            None,
        );
        assert!(beam_panel_depth(&b).is_err());
    }
    #[test]
    fn shape_dimensions_define_h_and_built_h_depth() {
        let mut h = sec(h_beam(), 650.0, None);
        if let Some(SectionShape::SteelH { flange_thick, .. }) = &mut h.shape {
            *flange_thick = 20.0;
        }
        assert_eq!(beam_panel_depth(&h), Ok(580.0));
        for (upper_thick, lower_thick) in [(20.0, 30.0), (30.0, 20.0)] {
            h.shape = Some(SectionShape::SteelBuiltH {
                height: 600.0,
                upper_width: 200.0,
                upper_thick,
                lower_width: 250.0,
                lower_thick,
                web_thick: 12.0,
            });
            assert_eq!(beam_panel_depth(&h), Ok(575.0));
            assert_eq!(beam_panel_properties(&h).applicability, Ok(()));
        }
    }

    #[test]
    fn invalid_flange_dimensions_are_not_calculated() {
        for (height, flange_thick) in [
            (f64::NAN, 20.0),
            (600.0, f64::INFINITY),
            (0.0, 20.0),
            (600.0, 0.0),
            (600.0, -20.0),
            (600.0, 300.0),
            (600.0, 301.0),
        ] {
            let h = sec(
                SectionShape::SteelH {
                    height,
                    flange_thick,
                    width: 200.0,
                    web_thick: 12.0,
                    root_r: None,
                },
                650.0,
                None,
            );
            assert!(beam_panel_depth(&h).is_err(), "{height}, {flange_thick}");
            assert_eq!(beam_panel_properties(&h).applicability, Ok(()));
        }
        let mut h = sec(h_beam(), 650.0, None);
        h.shape = None;
        assert!(beam_panel_depth(&h).is_err());
        assert!(beam_panel_properties(&h).applicability.is_err());
        h.shape = Some(SectionShape::SteelPipe {
            outer_dia: 600.0,
            thick: 20.0,
        });
        assert!(beam_panel_depth(&h).is_err());
        assert!(beam_panel_properties(&h).applicability.is_err());
    }

    #[test]
    fn mixed_beams_preserve_joint_beam_shape_and_plane_reasons() {
        for (coord, axis) in [
            ([-6000.0, 0.0, 3000.0], "[1.000, 0.000, 0.000]"),
            ([0.0, 6000.0, 3000.0], "[0.000, 1.000, 0.000]"),
            ([6000.0, 6000.0, 3000.0], "[0.707, 0.707, 0.000]"),
            ([6000.0, -6000.0, 3000.0], "[0.707, -0.707, 0.000]"),
        ] {
            let mut m = joint_model(h_beam(), 650.0, h_col());
            m.nodes.push(node(3, coord));
            let mut unsupported = m.sections[0].clone();
            unsupported.id = SectionId(2);
            unsupported.shape = Some(SectionShape::SteelBox {
                height: 600.0,
                width: 200.0,
                thick: 20.0,
                corner_r: None,
            });
            m.sections.push(unsupported);
            m.elements.push(member(2, 0, 3, 2));
            let reason = resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err();
            assert!(reason.contains("接合部 ID 0"));
            assert!(reason.contains("梁 ID 2 (BOX-"));
            assert!(reason.contains(axis));
            m.elements.reverse();
            for e in &mut m.elements {
                e.nodes.reverse();
            }
            assert_eq!(
                resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err(),
                reason
            );
        }
    }

    #[test]
    fn one_sided_h_joint_uses_580_mm_independent_of_member_order_and_ends() {
        let mut m = joint_model(h_beam(), 650.0, h_col());
        if let Some(SectionShape::SteelH { flange_thick, .. }) = &mut m.sections[0].shape {
            *flange_thick = 20.0;
        }
        let a = resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .unwrap();
        assert_eq!(a.db, 580.0);
        assert_eq!(a.ve, 2_857_660.0);
        m.elements.reverse();
        for e in &mut m.elements {
            e.nodes.reverse();
        }
        assert_eq!(
            resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap(),
            Some(a)
        );
    }
    #[test]
    fn beam_504_all_shape_dimensions_and_built_column_scope_are_checked() {
        let shape = SectionShape::SteelBuiltH {
            height: 600.0,
            upper_width: 200.0,
            upper_thick: 20.0,
            lower_width: 250.0,
            lower_thick: 30.0,
            web_thick: 12.0,
        };
        let mut section = sec(shape, 650.0, None);
        assert!(PanelGeometry::from_column(&section).is_none());
        for invalid in [f64::NAN, 0.0, -1.0] {
            for dim in 0..3 {
                section.shape = Some(SectionShape::SteelBuiltH {
                    height: 600.0,
                    upper_width: if dim == 0 { invalid } else { 200.0 },
                    upper_thick: 20.0,
                    lower_width: if dim == 1 { invalid } else { 250.0 },
                    lower_thick: 30.0,
                    web_thick: if dim == 2 { invalid } else { 12.0 },
                });
                assert!(beam_panel_depth(&section).is_err());
            }
            section.shape = Some(SectionShape::SteelH {
                height: 600.0,
                width: invalid,
                web_thick: 12.0,
                flange_thick: 20.0,
                root_r: None,
            });
            assert!(beam_panel_depth(&section).is_err());
            section.shape = Some(SectionShape::SteelH {
                height: 600.0,
                width: 200.0,
                web_thick: invalid,
                flange_thick: 20.0,
                root_r: None,
            });
            assert!(beam_panel_depth(&section).is_err());
        }
        section.shape = Some(SectionShape::SteelBuiltH {
            height: 600.0,
            upper_width: 200.0,
            upper_thick: 20.0,
            lower_width: 250.0,
            lower_thick: 580.0,
            web_thick: 12.0,
        });
        assert!(beam_panel_depth(&section).is_err());
    }
    #[test]
    fn panel_505_pure_flange_positions_and_closed_tolerance() {
        let delta = FLANGE_POSITION_TOL_MM;
        for sign in [-1.0, 1.0] {
            for factor in [0.5, 1.0, 2.0] {
                assert_eq!(
                    standard_flange_differences(sign * factor * delta, 0.0),
                    factor <= 1.0
                );
                assert_eq!(
                    standard_flange_differences(0.0, sign * factor * delta),
                    factor <= 1.0
                );
            }
        }
        let h600 = BeamFlangePositions::from_dimensions(600.0, 20.0, 20.0, 0.0).unwrap();
        assert_eq!(
            h600,
            BeamFlangePositions {
                upper: 290.0,
                lower: -290.0
            }
        );
        assert_eq!(h600.db(), 580.0);
        for (h, offset, du, dl) in [
            (600.0, 20.0, -20.0, -20.0),
            (500.0, 0.0, 50.0, -50.0),
            (500.0, 50.0, 0.0, -100.0),
        ] {
            let b = BeamFlangePositions::from_dimensions(h, 20.0, 20.0, offset).unwrap();
            assert_eq!((h600.upper - b.upper, h600.lower - b.lower), (du, dl));
            assert!(!standard_flange_differences(du, dl));
        }
        for depth_difference in [149.0, 150.0, 151.0] {
            let b = BeamFlangePositions::from_dimensions(600.0 - depth_difference, 20.0, 20.0, 0.0)
                .unwrap();
            assert!(!standard_flange_differences(
                h600.upper - b.upper,
                h600.lower - b.lower
            ));
        }
        let thicker = BeamFlangePositions::from_dimensions(610.0, 30.0, 30.0, 0.0).unwrap();
        assert_eq!(h600, thicker);
        assert!(!standard_flange_differences(f64::NAN, 0.0));
    }

    #[test]
    fn panel_505_model_pairs_are_local_and_independent_of_order_and_ends() {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        m.nodes.push(node(3, [-6000.0, 0.0, 3000.0]));
        m.elements.push(member(2, 3, 0, 0));
        assert!(resolve_panel_joint(&m, NodeId(0), &m.elements).is_ok());
        let mut small = m.sections[0].clone();
        small.id = SectionId(2);
        if let Some(SectionShape::SteelH { height, .. }) = &mut small.shape {
            *height = 500.0;
        }
        m.sections.push(small);
        m.elements[2].section = Some(SectionId(2));
        let reason = resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err();
        assert!(reason.contains("上位置差=50 mm"));
        assert!(reason.contains("下位置差=-50 mm"));
        let planes = panel_joint_planes(&m, NodeId(0), &m.elements);
        m.elements.reverse();
        for e in &mut m.elements {
            e.nodes.reverse();
        }
        for n in &mut m.nodes {
            for c in &mut n.coord {
                *c += 1e12;
            }
        }
        assert_eq!(panel_joint_planes(&m, NodeId(0), &m.elements), planes);
        assert_eq!(
            resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err(),
            reason
        );
        m.nodes[3].coord = [1e12, 1e12 + 6000.0, 1e12 + 3000.0];
        assert_eq!(panel_joint_planes(&m, NodeId(0), &m.elements).len(), 2);
        assert!(resolve_panel_joint(&m, NodeId(0), &m.elements).is_ok());
    }
    #[test]
    fn panel_505_orientation_retains_unsupported_girders_and_orthogonalizes_reference() {
        let mut m = joint_model(h_beam(), 600.0, h_col());
        m.elements[0].local_axis.ref_vector = [1.0, 0.0, 1.0];
        assert!(resolve_panel_joint(&m, NodeId(0), &m.elements)
            .unwrap()
            .is_some());
        for reference in [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 0.0, 0.0]] {
            m.elements[0].local_axis.ref_vector = reference;
            let reason = resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err();
            assert!(reason.contains("横倒し・斜め断面回転"));
            assert_eq!(
                panel_joint_planes(&m, NodeId(0), &m.elements)[0]
                    .beams
                    .len(),
                1
            );
            m.elements[0].nodes.reverse();
            assert_eq!(
                resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err(),
                reason
            );
        }
        m.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
        for far in [
            [6000.0, 0.0, 3600.0],
            [6000.0, 0.0, 9000.0],
            [0.0, 0.0, 9000.0],
        ] {
            m.nodes[1].coord = far;
            assert!(resolve_panel_joint(&m, NodeId(0), &m.elements)
                .unwrap_err()
                .contains("勾配梁・鉛直Girder"));
            assert_eq!(
                panel_joint_planes(&m, NodeId(0), &m.elements)[0]
                    .beams
                    .len(),
                1
            );
        }
    }

    #[test]
    fn panel_505_reversed_built_h_maps_flange_thicknesses_to_global_up_and_down() {
        let shape = SectionShape::SteelBuiltH {
            height: 600.0,
            upper_width: 200.0,
            upper_thick: 20.0,
            lower_width: 250.0,
            lower_thick: 30.0,
            web_thick: 12.0,
        };
        let mut m = joint_model(shape, 600.0, h_col());
        let upright = panel_joint_planes(&m, NodeId(0), &m.elements)[0].beams[0]
            .flanges
            .clone()
            .unwrap();
        assert_eq!(
            upright,
            BeamFlangePositions {
                upper: 290.0,
                lower: -285.0
            }
        );
        m.elements[0].local_axis.ref_vector = [0.0, 0.0, -1.0];
        let reversed = panel_joint_planes(&m, NodeId(0), &m.elements)[0].beams[0]
            .flanges
            .clone()
            .unwrap();
        assert_eq!(
            reversed,
            BeamFlangePositions {
                upper: 285.0,
                lower: -290.0
            }
        );
        assert_eq!(upright.db(), reversed.db());
        m.elements[0].nodes.reverse();
        assert_eq!(
            panel_joint_planes(&m, NodeId(0), &m.elements)[0].beams[0]
                .flanges
                .clone()
                .unwrap(),
            reversed
        );
        m.nodes.push(node(3, [-6000.0, 0.0, 3000.0]));
        m.elements.push(member(2, 3, 0, 0));
        let reason = resolve_panel_joint(&m, NodeId(0), &m.elements).unwrap_err();
        assert!(reason.contains("上位置差=-5 mm"));
        assert!(reason.contains("下位置差=-5 mm"));
    }
}
