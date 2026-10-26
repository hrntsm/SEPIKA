//! 応力解析の計算条件の型。
//!
//! - [`StressAnalysisCfg`] — 長期応力解析の計算条件（令82条）。

/// 建物共通のRC壁終局せん断耐力の算定方式。既定は荒川mean。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RcWallShearMethod {
    ArakawaMin,
    #[default]
    ArakawaMean,
}

impl RcWallShearMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::ArakawaMin => "荒川min",
            Self::ArakawaMean => "荒川mean",
        }
    }
}

/// 長期応力解析の計算条件（令82条の応力解析）。
///
/// 計算条件の指定により、一部の部材（ブレース・柱・制振間柱）について
/// 長期軸力を負担させないことが可能である。
///
/// 制振間柱（damper-equipped mullion column）は本リポジトリに要素種別が未実装のため、
/// 対象外（既知の制約）。ブレースと柱（鉛直部材）のみ対応する。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StressAnalysisCfg {
    /// RC壁の終局せん断耐力に用いる建物共通の方式。
    pub rc_wall_shear_method: RcWallShearMethod,
    /// 長期応力解析でブレース（`ElementKind::Brace`）に軸力を負担させない。
    pub no_long_axial_brace: bool,
    /// 長期応力解析で柱（鉛直な `ElementKind::Beam`）に軸力を負担させない。
    pub no_long_axial_column: bool,
    /// 剛性率・偏心率算定時の雑壁剛性の n 倍法係数（雑壁の剛性評価。RC 規準。
    /// `Kw' = n·Aw'·ΣKc/ΣAc` の n。入力値）。`None` は雑壁剛性を考慮しない。
    #[serde(default)]
    pub misc_wall_n: Option<f64>,
    /// 層間変形角の設定値との参考比較に使う正の有限な分母。既定200。
    /// 法令適合や緩和適用資格は判定せず、不正値では参考比較を行わない。
    #[serde(default = "default_drift_limit_denom")]
    pub drift_limit_denom: f64,
    /// 線形（弾性）応力解析で引張専用ブレースを active-set 反復で扱うか（既定 `false`）。
    #[serde(default)]
    pub tension_only_iteration: bool,
    /// 剛域の算定で、部材フェース・部材せいに取り付く壁を考慮するか（技術基準
    /// 「剛域の計算」。既定は考慮する）。
    ///
    /// `true`（既定）: 柱には袖壁、梁には腰壁・垂壁を含めた寸法で算定する。
    /// 対象は現場打ちコンクリート壁で厚さ 100 mm 以上のもの（耐震壁・雑壁を問わない）。
    /// `false`: 部材の原断面だけで算定する。
    #[serde(default = "default_rigid_zone_consider_walls")]
    pub rigid_zone_consider_walls: bool,
    /// 部材の剛域長を剛性・荷重応答へ反映する。既定true。仕口パネルの偏心は保持する。
    #[serde(default = "default_rigid_zone_consider_walls")]
    pub consider_rigid_zones: bool,
}

fn default_rigid_zone_consider_walls() -> bool {
    true
}

fn default_drift_limit_denom() -> f64 {
    200.0
}

impl Default for StressAnalysisCfg {
    fn default() -> Self {
        StressAnalysisCfg {
            rc_wall_shear_method: RcWallShearMethod::default(),
            no_long_axial_brace: false,
            no_long_axial_column: false,
            misc_wall_n: None,
            drift_limit_denom: default_drift_limit_denom(),
            tension_only_iteration: false,
            rigid_zone_consider_walls: default_rigid_zone_consider_walls(),
            consider_rigid_zones: true,
        }
    }
}

#[cfg(test)]
mod rigid_zone_switch_tests {
    use super::*;

    #[test]
    fn rigid_zone_switch_defaults_on_and_roundtrips_off() {
        let mut cfg = StressAnalysisCfg::default();
        assert!(cfg.consider_rigid_zones);
        let mut value = serde_json::to_value(&cfg).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("consider_rigid_zones");
        assert!(
            serde_json::from_value::<StressAnalysisCfg>(value)
                .unwrap()
                .consider_rigid_zones
        );
        cfg.consider_rigid_zones = false;
        let restored: StressAnalysisCfg =
            serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert!(!restored.consider_rigid_zones);
        assert!(restored.rigid_zone_consider_walls);
    }
}
