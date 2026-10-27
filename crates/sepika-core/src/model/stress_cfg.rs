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

/// 自立壁の基準剛性の算定方式。既定は層内柱合算。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MiscWallStiffnessMethod {
    #[default]
    StoryColumns,
    RepresentativeColumn,
}

impl MiscWallStiffnessMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::StoryColumns => "層内柱合算方式",
            Self::RepresentativeColumn => "代表内柱方式",
        }
    }
}

/// 代表内柱の指定。storyは層の上端階、columnはその層の柱を構成する部材。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MiscWallRepresentativeColumn {
    pub story: crate::ids::StoryId,
    pub column: crate::ids::ElemId,
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
    /// 自立壁のn倍法係数（無次元）。1以上の有限値に限り、入出力時に不正値を拒否する。
    /// `None`（既定）は自立壁の付加剛性を考慮しない。
    #[serde(default, with = "misc_wall_coefficient")]
    pub misc_wall_n: Option<f64>,
    /// 自立壁の基準剛性の建物共通の算定方式。
    #[serde(default)]
    pub misc_wall_method: MiscWallStiffnessMethod,
    /// 代表内柱方式で各層に指定する柱。X/Y方向で同じ柱を使う。
    #[serde(default)]
    pub misc_wall_representatives: Vec<MiscWallRepresentativeColumn>,
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

impl StressAnalysisCfg {
    /// 自立壁のn倍法係数が未指定又は1以上の有限値でなければErr。
    pub fn validate_misc_wall_n(&self) -> Result<(), String> {
        misc_wall_coefficient::validate(self.misc_wall_n)
    }
}

mod misc_wall_coefficient {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn validate(value: Option<f64>) -> Result<(), String> {
        if value.is_some_and(|n| !n.is_finite() || n < 1.0) {
            return Err("n倍法: 係数nは1以上の有限値にしてください".into());
        }
        Ok(())
    }

    pub fn serialize<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        validate(*value).map_err(serde::ser::Error::custom)?;
        value.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<f64>, D::Error> {
        let value = Option::<f64>::deserialize(deserializer)?;
        validate(value).map_err(serde::de::Error::custom)?;
        Ok(value)
    }
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
            misc_wall_method: MiscWallStiffnessMethod::default(),
            misc_wall_representatives: Vec::new(),
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

#[cfg(test)]
mod misc889_tests {
    use super::*;

    #[derive(serde::Serialize)]
    struct UncheckedCfg {
        #[serde(flatten)]
        fields: serde_json::Map<String, serde_json::Value>,
        misc_wall_n: Option<f64>,
    }

    #[test]
    fn misc889_input_serialization_and_model_validation_share_lower_bound() {
        for n in [None, Some(1.0), Some(2.0)] {
            let cfg = StressAnalysisCfg {
                misc_wall_n: n,
                ..Default::default()
            };
            let json = serde_json::to_string(&cfg).unwrap();
            let binary = rmp_serde::to_vec_named(&cfg).unwrap();
            assert_eq!(
                serde_json::from_str::<StressAnalysisCfg>(&json)
                    .unwrap()
                    .misc_wall_n,
                n
            );
            assert_eq!(
                rmp_serde::from_slice::<StressAnalysisCfg>(&binary)
                    .unwrap()
                    .misc_wall_n,
                n
            );
            let model = crate::Model {
                stress_cfg: cfg,
                ..Default::default()
            };
            assert!(model.validate().is_ok());
        }
        let mut fields = serde_json::to_value(StressAnalysisCfg::default())
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        fields.remove("misc_wall_n");
        assert_eq!(
            serde_json::from_value::<StressAnalysisCfg>(fields.clone().into())
                .unwrap()
                .misc_wall_n,
            None
        );
        for n in [
            f64::MIN_POSITIVE,
            0.5,
            1.0 - f64::EPSILON,
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let cfg = StressAnalysisCfg {
                misc_wall_n: Some(n),
                ..Default::default()
            };
            assert!(serde_json::to_string(&cfg)
                .unwrap_err()
                .to_string()
                .contains("係数n"));
            assert!(rmp_serde::to_vec_named(&cfg)
                .unwrap_err()
                .to_string()
                .contains("係数n"));
            let binary = rmp_serde::to_vec_named(&UncheckedCfg {
                fields: fields.clone(),
                misc_wall_n: Some(n),
            })
            .unwrap();
            assert!(rmp_serde::from_slice::<StressAnalysisCfg>(&binary)
                .unwrap_err()
                .to_string()
                .contains("係数n"));
            if n.is_finite() {
                let mut value = fields.clone();
                value.insert("misc_wall_n".into(), n.into());
                assert!(serde_json::from_value::<StressAnalysisCfg>(value.into())
                    .unwrap_err()
                    .to_string()
                    .contains("係数n"));
            }
            let model = crate::Model {
                stress_cfg: cfg,
                ..Default::default()
            };
            assert!(model.validate().unwrap_err().to_string().contains("係数n"));
            assert_eq!(model.stress_cfg.misc_wall_n.unwrap().to_bits(), n.to_bits());
        }
    }
}

#[cfg(test)]
mod misc_wall_method_tests {
    use super::*;
    use crate::ids::{ElemId, StoryId};

    #[test]
    fn 代表内柱方式と層ごとの指定柱を保存復元する() {
        let cfg = StressAnalysisCfg {
            misc_wall_n: Some(1.5),
            misc_wall_method: MiscWallStiffnessMethod::RepresentativeColumn,
            misc_wall_representatives: vec![
                MiscWallRepresentativeColumn {
                    story: StoryId(1),
                    column: ElemId(3),
                },
                MiscWallRepresentativeColumn {
                    story: StoryId(2),
                    column: ElemId(8),
                },
            ],
            ..Default::default()
        };
        let binary = rmp_serde::to_vec_named(&cfg).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<StressAnalysisCfg>(&binary).unwrap(),
            cfg
        );
        assert_eq!(
            serde_json::from_str::<StressAnalysisCfg>(&serde_json::to_string(&cfg).unwrap())
                .unwrap(),
            cfg
        );
        assert_eq!(
            StressAnalysisCfg::default().misc_wall_method,
            MiscWallStiffnessMethod::StoryColumns
        );
    }

    #[test]
    fn 不正な代表内柱参照はモデル検証で拒否する() {
        let mut model = crate::Model::default();
        model
            .stress_cfg
            .misc_wall_representatives
            .push(MiscWallRepresentativeColumn {
                story: StoryId(999),
                column: ElemId(999),
            });
        assert!(model
            .validate()
            .unwrap_err()
            .to_string()
            .contains("代表内柱"));
    }
}
