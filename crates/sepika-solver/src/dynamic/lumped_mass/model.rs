//! 串団子モデル生成（せん断型多質点系、構造力学）。
//!
//! プッシュオーバー結果の層 Q-δ 関係を等包絡面積則でトリリニア骨格へ縮約する。
//!
//! - [`StoryTrilinear`] — 層のトリリニア骨格（Q-δ）。
//! - [`StoryStick`] — 串団子モデルの1質点（層）。
//! - [`LumpedMassType`] — モデル化タイプ（せん断型多質点系）。
//! - [`LumpedMassModel`] — 串団子モデル。
//! - [`build_lumped_mass_model`] — 指定骨格と物理質量から質点系を構成する。

use crate::statics::analysis::SeismicDir;
use sepika_core::ids::StoryId;
use sepika_core::model::Model;
use sepika_core::units::GRAVITY_MM_S2;
use sepika_math::solver::SolveError;

/// 質点系の次元。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum StickDim {
    /// 2 次元（加力方向の並進。曲げせん断型は曲げ回転も持つ）。
    #[default]
    Planar,
    /// 3 次元（各階 Ux, Uy, θz。曲げせん断型は θx, θy を追加）。
    Spatial,
}

impl StickDim {
    pub fn label(self) -> &'static str {
        match self {
            Self::Planar => "2次元",
            Self::Spatial => "3次元",
        }
    }
}

/// 層並進剛性の定義。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LumpedStiffnessSource {
    /// 地震静 EX/EY の層せん断 Q と層間変位 δ から K = Q/δ。
    #[default]
    StoryQd,
    /// 柱の ki = Qi/δi の合計（偏心率精算と同じ）。
    ColumnKi,
}

impl LumpedStiffnessSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::StoryQd => "層 Q/δ（EX/EY）",
            Self::ColumnKi => "柱 ki",
        }
    }
}

/// 3 次元質点の層データ（剛心・ねじり・方向別骨格）。
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct StorySpatial {
    /// 回転慣性 J [t·mm²]（質量重心まわり。物理質量分布から直接算定）。
    pub j: f64,
    /// 質量重心 (x, y) `[mm]`。
    pub mass_xy: [f64; 2],
    /// 剛心 (x, y) `[mm]`。
    pub rigidity_xy: [f64; 2],
    /// X 方向初期剛性 [N/mm]。
    pub k1_x: f64,
    /// Y 方向初期剛性 [N/mm]。
    pub k1_y: f64,
    /// 剛心まわりのねじり剛性 KR [N·mm/rad]。
    pub kr: f64,
    /// X 方向トリリニア（非線形時。線形なら弾性相当）。
    pub skeleton_x: StoryTrilinear,
    /// Y 方向トリリニア。
    pub skeleton_y: StoryTrilinear,
}

/// 層のトリリニア骨格（Q-δ）。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoryTrilinear {
    /// 初期剛性 K1 [N/mm]。
    pub k1: f64,
    /// 第1折点 (δ1`[mm]`, Q1`[N]`)。
    pub d1: f64,
    pub q1: f64,
    /// 第2折点 (δ2, Q2)。
    pub d2: f64,
    pub q2: f64,
    /// 第3折点＝終局 (δ3, Q3)。
    pub d3: f64,
    pub q3: f64,
}

impl StoryTrilinear {
    /// 第2勾配 K2 = (Q2−Q1)/(δ2−δ1)。
    pub fn k2(&self) -> f64 {
        if self.d2 > self.d1 {
            (self.q2 - self.q1) / (self.d2 - self.d1)
        } else {
            0.0
        }
    }
    /// 第3勾配 K3 = (Q3−Q2)/(δ3−δ2)。
    pub fn k3(&self) -> f64 {
        if self.d3 > self.d2 {
            (self.q3 - self.q2) / (self.d3 - self.d2)
        } else {
            0.0
        }
    }

    /// 正の初期剛性・有限な非減少骨格と第1折点の整合を検証する。
    /// 同一変位の折点は、力が相対64ε以内で一致する場合に限り許容する。値は変更しない。
    pub fn is_valid_spring(&self) -> bool {
        let same = |a: f64, b: f64| {
            a.is_finite()
                && b.is_finite()
                && (a - b).abs() <= 64.0 * f64::EPSILON * a.abs().max(b.abs())
        };
        if [
            self.k1, self.d1, self.q1, self.d2, self.q2, self.d3, self.q3,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
            || !same(self.k1, self.q1 / self.d1)
        {
            return false;
        }
        [(self.d1, self.q1), (self.d2, self.q2), (self.d3, self.q3)]
            .windows(2)
            .all(|points| {
                let [(d0, q0), (d1, q1)] = [points[0], points[1]];
                if d1 == d0 {
                    same(q0, q1)
                } else {
                    d1 > d0 && q1 >= q0 && ((q1 - q0) / (d1 - d0)).is_finite()
                }
            })
    }

    /// 線形ばね相当（折点を十分遠くに置いた弾性トリリニア）。
    pub fn elastic(k1: f64) -> Self {
        let k = k1.max(0.0);
        let d = 1.0e9;
        let q = k * d;
        Self {
            k1: k,
            d1: d,
            q1: q,
            d2: d,
            q2: q,
            d3: d,
            q3: q,
        }
    }
}

/// 串団子モデルの1質点（層）。
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoryStick {
    pub story: StoryId,
    /// 質量 `[t]`（= 物理質量相当重量 / g）。
    pub mass: f64,
    /// 階高 `[mm]`。
    pub height: f64,
    /// 層の復元力特性（トリリニア）。
    pub skeleton: StoryTrilinear,
}

/// モデル化タイプ（せん断型多質点系、構造力学）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LumpedMassType {
    /// 等価せん断型（曲げ剛性を剛とする）。
    #[default]
    EquivalentShear,
    /// 等価曲げせん断型（曲げ剛性を梁要素として考慮）。
    EquivalentBendingShear,
}

impl LumpedMassType {
    pub fn label(&self) -> &'static str {
        match self {
            LumpedMassType::EquivalentShear => "等価せん断型",
            LumpedMassType::EquivalentBendingShear => "等価曲げせん断型",
        }
    }
}

/// 串団子モデル。層ごとの質点と復元力特性を保持する。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LumpedMassModel {
    /// 層・方向別の等価骨格同定条件と結果。弾性解析時は空。
    pub identifications: Vec<StoryIdentification>,
    pub model_type: LumpedMassType,
    pub stories: Vec<StoryStick>,
    #[serde(default)]
    pub dim: StickDim,
    #[serde(default)]
    pub stiffness_source: LumpedStiffnessSource,
    #[serde(default)]
    pub dir: SeismicDir,
    /// 非線形骨格（トリリニア）を使うか。false なら初期剛性の線形ばね。
    #[serde(default)]
    pub nonlinear: bool,
    /// 3 次元の層データ（`dim == Spatial` のとき `stories` と同順・同長）。
    #[serde(default)]
    pub spatial: Vec<StorySpatial>,
    /// 曲げせん断型の層別諸元。せん断型では空。
    pub bending: Vec<StoryBending>,
}

/// 層・方向に対応付けた同定結果と元解析条件。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoryIdentification {
    pub story: StoryId,
    pub dir: SeismicDir,
    pub fit: super::StoryFit,
    pub source_target: crate::nonlinear::pushover::PushoverTarget,
    /// 同じ水平力分布の等価降伏荷重時の弾性曲げ層間変位 [mm]。
    pub bending_yield_drift: f64,
}

/// 弾性曲げ剛性 [N·mm²] と重心まわりの物理回転慣性 [t·mm²]。
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoryBending {
    pub ei_x: f64,
    pub ei_y: f64,
    /// [Jx, Jy, Σm dx dy]。
    pub inertia: [f64; 3],
}

impl LumpedMassModel {
    /// 2 次元せん断串（増分 1 方向からの生成）。
    pub fn from_stories(model_type: LumpedMassType, stories: Vec<StoryStick>) -> Self {
        Self {
            model_type,
            stories,
            dim: StickDim::Planar,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: Vec::new(),
            identifications: Vec::new(),
            bending: Vec::new(),
        }
    }

    pub fn is_spatial(&self) -> bool {
        self.dim == StickDim::Spatial && self.spatial.len() == self.stories.len()
    }
}

/// 指定済みの層骨格と動的物理質量から2次元質点系を構成する。
/// 骨格は下層から順番に与え、層数不一致・非正質量・不正骨格は InvalidInput。
pub fn build_lumped_mass_model(
    model: &Model,
    skeletons: &[StoryTrilinear],
    model_type: LumpedMassType,
) -> Result<LumpedMassModel, SolveError> {
    model
        .validate_rc_section_usage()
        .map_err(|e| sepika_math::solver::SolveError::InvalidInput(e.to_string()))?;
    model
        .validate_secondary_section_usage()
        .map_err(|e| sepika_math::solver::SolveError::InvalidInput(e.to_string()))?;
    model
        .validate_damper_mass_placement()
        .map_err(SolveError::InvalidInput)?;
    let layers = model.layers();
    let mut sticks = Vec::with_capacity(layers.len());
    for layer in &layers {
        let dynamic_mass = layer.dynamic_mass.ok_or_else(|| {
            SolveError::InvalidInput(format!("層 {:?} の動的質量が未算定です", layer.bottom))
        })?;
        let mass = dynamic_mass.mass_equiv_weight_n / GRAVITY_MM_S2;
        if !mass.is_finite() || mass <= 0.0 {
            return Err(SolveError::InvalidInput(format!(
                "層 {:?} の質量が 0 以下です（物理質量相当重量が 0 以下）",
                layer.bottom
            )));
        }

        let skeleton = *skeletons
            .get(layer.index)
            .ok_or_else(|| SolveError::InvalidInput("層数と指定骨格数が一致しません".into()))?;
        if !skeleton.is_valid_spring() {
            return Err(SolveError::InvalidInput(format!(
                "層 {:?} の指定骨格が不正です",
                layer.bottom
            )));
        }
        sticks.push(StoryStick {
            story: layer.bottom,
            mass,
            height: layer.height.max(0.0),
            skeleton,
        });
    }
    if skeletons.len() != sticks.len() {
        return Err(SolveError::InvalidInput(
            "層数と指定骨格数が一致しません".into(),
        ));
    }
    Ok(LumpedMassModel::from_stories(model_type, sticks))
}
