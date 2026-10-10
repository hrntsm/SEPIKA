//! 材料の型。

use super::*;

/// 材料の区分。
///
/// 部材が S 造か RC 造かは、断面形状ではなくこの区分で判定する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MaterialCategory {
    /// 構造用鋼材。
    Steel,
    /// 鉄筋。RC 部材の主筋・せん断補強筋に用いる。
    Rebar,
    /// コンクリート。
    Concrete,
}

impl MaterialCategory {
    /// UI 表示名。
    pub fn label(&self) -> &'static str {
        match self {
            MaterialCategory::Steel => "鋼材",
            MaterialCategory::Rebar => "鉄筋",
            MaterialCategory::Concrete => "コンクリート",
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Material {
    pub id: MaterialId,
    pub name: String,
    /// 材料の区分。
    pub category: MaterialCategory,
    pub young: f64,
    pub poisson: f64,
    pub density: f64,
    #[serde(default)]
    pub shear: Option<f64>,
    /// コンクリート設計基準強度 Fc [N/mm²]。鋼材では `None`。
    #[serde(default)]
    pub fc: Option<f64>,
    /// 降伏応力 fy [N/mm²]。
    /// `None` の場合、ファイバ材料は弾性（降伏しない）として扱う。
    #[serde(default)]
    pub fy: Option<f64>,
    /// コンクリートの種類（普通/軽量1種/軽量2種）。鋼材では意味を持たない（既定 Normal）。
    #[serde(default)]
    pub concrete_class: crate::units::ConcreteClass,
    /// 保有水平耐力計算用の材料強度割増係数の直接入力。
    /// `None`（既定）の場合は自動判定。
    #[serde(default)]
    pub strength_factor: Option<f64>,
}

impl Material {
    /// 標準生成経路のコンクリートで、ヤング係数を標準式から得る材料か。
    pub fn is_standard_concrete(&self) -> bool {
        self.category == MaterialCategory::Concrete
            && self.concrete_class == crate::units::ConcreteClass::Normal
    }

    pub fn shear_modulus(&self) -> f64 {
        self.shear
            .unwrap_or_else(|| self.young / (2.0 * (1.0 + self.poisson)))
    }

    /// 固定荷重（DL・地震用重量）算定に用いる単位体積重量 [N/mm³]。
    /// 鋼材は密度 > 0 なら基準資料の 78.5 kN/m³ 固定（保存された密度に依存しない）。
    /// 質量密度 0 の材料は自重 0。その他は質量密度×g（内部単位 N-mm-s）。
    ///
    /// 鉄筋は鋼材に含めない。RC/SRC の主材料はコンクリートであり、鉄筋の自重は
    /// コンクリートの単位体積重量（γRC/γSRC）に内包されるため別加算しない。
    pub fn design_unit_weight_n_per_mm3(&self) -> f64 {
        if self.category == MaterialCategory::Steel {
            if self.density > 0.0 {
                crate::units::to_internal::unit_weight_kn_per_m3(
                    crate::units::STEEL_UNIT_WEIGHT_KN_M3,
                )
            } else {
                0.0
            }
        } else {
            self.density * crate::units::GRAVITY_MM_S2
        }
    }

    /// 参照された鋼材の設計自重を検査する。参照部材と計算目的を診断に含める。
    /// 非有限・負の密度、または物理重量が固定設計重量を超える場合はエラー。
    pub fn validate_design_self_weight(&self, member: &str, purpose: &str) -> Result<(), String> {
        if self.category != MaterialCategory::Steel {
            return Ok(());
        }
        let context = format!(
            "材料 {}、{member}、{purpose}: 入力ρ={} t/mm³",
            self.id.0, self.density
        );
        if !self.density.is_finite() {
            return Err(format!("{context}: 物理密度が非有限です"));
        }
        if self.density < 0.0 {
            return Err(format!("{context}: 物理密度が負です"));
        }
        let physical_n_per_mm3 = self.density * crate::units::GRAVITY_MM_S2;
        let design_n_per_mm3 =
            crate::units::to_internal::unit_weight_kn_per_m3(crate::units::STEEL_UNIT_WEIGHT_KN_M3);
        if physical_n_per_mm3 > design_n_per_mm3 {
            return Err(format!("{context}: ρg={physical_n_per_mm3} N/mm³ が固定設計単位重量 {design_n_per_mm3} N/mm³（78.5 kN/m³）を超え、設計自重を過小評価するため計算できません"));
        }
        Ok(())
    }

    /// CFT 充填コンクリートの単位体積重量 [kN/m³]（無筋 `ConcreteComposition::Plain`）。
    /// `fc` 未設定または 0 以下なら 0。
    fn cft_filling_unit_weight_kn_m3(&self) -> f64 {
        self.fc.filter(|fc| *fc > 0.0).map_or(0.0, |fc| {
            crate::units::concrete_unit_weight_kn_m3(
                fc,
                self.concrete_class,
                crate::units::ConcreteComposition::Plain,
            )
        })
    }

    /// CFT 充填コンクリート部の設計用単位体積重量 [N/mm³]（γC）。
    /// `fc` 未設定または 0 以下なら 0。
    pub fn cft_core_design_unit_weight_n_per_mm3(&self) -> f64 {
        crate::units::to_internal::unit_weight_kn_per_m3(self.cft_filling_unit_weight_kn_m3())
    }

    /// CFT 充填コンクリート部の物理質量密度 [t/mm³]（γC を質量密度へ換算）。
    /// `fc` 未設定または 0 以下なら 0。
    pub fn cft_core_mass_density(&self) -> f64 {
        crate::units::to_internal::mass_density_from_unit_weight_kn_m3(
            self.cft_filling_unit_weight_kn_m3(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn steel(density: f64) -> Material {
        Material {
            id: MaterialId(0),
            name: "steel".into(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density,
            shear: None,
            fc: None,
            fy: None,
            concrete_class: Default::default(),
            strength_factor: None,
        }
    }

    #[test]
    fn test_steel_design_unit_weight_is_fixed_regardless_of_density() {
        // 標準密度、非標準の正密度のいずれでも設計単位体積重量は 78.5 kN/m³ 固定。
        for density in [crate::units::STEEL_MASS_DENSITY_TON_MM3, 5.0e-9, 12.0e-9] {
            assert_relative_eq!(
                steel(density).design_unit_weight_n_per_mm3(),
                78.5e-6,
                max_relative = 1e-12
            );
        }

        // 質量密度 0 の材料は自重 0（密度に依らず固定すると荷重が発生してしまうため）。
        assert_eq!(steel(0.0).design_unit_weight_n_per_mm3(), 0.0);
        assert_eq!(steel(-1.0e-9).design_unit_weight_n_per_mm3(), 0.0);
    }

    #[test]
    fn test_concrete_design_unit_weight_scales_with_density() {
        let mut concrete = steel(2.4e-9);
        concrete.category = MaterialCategory::Concrete;
        assert_relative_eq!(
            concrete.design_unit_weight_n_per_mm3(),
            2.4e-9 * crate::units::GRAVITY_MM_S2,
            max_relative = 1e-12
        );
    }
    #[test]
    fn design_density_guard_uses_unrounded_boundary_and_separate_invalid_diagnostics() {
        let boundary = 78.5e-6 / 9806.65;
        for density in [0.0, 7.85e-9, boundary * (1.0 - 1e-8), boundary] {
            assert!(steel(density)
                .validate_design_self_weight("要素 12", "DL")
                .is_ok());
        }
        for density in [boundary * (1.0 + 1e-8), 85e-6 / 9806.65] {
            let error = steel(density)
                .validate_design_self_weight("要素 12", "DL")
                .unwrap_err();
            for token in [
                "材料 0",
                "要素 12",
                "入力ρ=",
                "ρg=",
                "78.5",
                "DL",
                "過小評価",
            ] {
                assert!(error.contains(token), "{error}");
            }
        }
        for density in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let error = steel(density)
                .validate_design_self_weight("要素 12", "DL")
                .unwrap_err();
            assert!(error.contains("非有限") && !error.contains("過小評価"));
        }
        assert!(steel(-1e-9)
            .validate_design_self_weight("要素 12", "DL")
            .unwrap_err()
            .contains("負"));
        for category in [MaterialCategory::Concrete, MaterialCategory::Rebar] {
            let mut material = steel(85e-6 / 9806.65);
            material.category = category;
            assert!(material
                .validate_design_self_weight("要素 12", "DL")
                .is_ok());
        }
        assert_relative_eq!(
            steel(7.85e-9).density * 9806.65 * 1e9 / 1000.0,
            76.9822025,
            epsilon = 1e-10
        );
        assert_relative_eq!(
            steel(85e-6 / 9806.65).density * 9806.65 * 1e9 / 1000.0 - 78.5,
            6.5,
            epsilon = 1e-10
        );
    }
}
