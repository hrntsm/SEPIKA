//! 標準グレードから復元するアプリの物性既定値。

use crate::material_grade::{
    parse_concrete_fc, rebar_grade_f_value, steel_f_value, steel_grade_base,
};
use crate::section_shape::{concrete_young_modulus_gamma, E_STEEL};
use crate::units::STEEL_MASS_DENSITY_TON_MM3;
use crate::units::{
    concrete_unit_weight_kn_m3, to_internal::mass_density_from_unit_weight_kn_m3, ConcreteClass,
    ConcreteComposition,
};

/// グレード名から解決した標準材料物性（内部単位系 N-mm-s。密度は ton/mm³）。
pub struct StandardMaterialProperties {
    pub young: f64,
    pub poisson: f64,
    pub density: f64,
    pub fc: Option<f64>,
    pub fy: Option<f64>,
}

/// グレード名から標準材料物性を解決する。認識できない名前は `None`。
///
/// - コンクリート `FcXX`（`Fc21`・`Fc24` 等）→ 圧縮強度 Fc=XX、Ec は RC 規準式で算定。
/// - 鉄筋（`SD295A`・`SD345`・`SD390`・`SR235` 等）→ E=205000、規格降伏点。
/// - 構造用鋼材（`SN400B`・`SS400`・`STKR400`・`SM490` 等）→ E=205000、基準強度 F を降伏点に
///   （板厚 40mm 以下の値。板厚区分は ST-Bridge の材料テーブルにないため一律 40mm 以下とみなす）。
///
/// 鋼材は対応済みの分類名・種別記号付き名称のみ解決する。
/// 前後空白は除き、鋼材名の大小文字は区別する。
pub fn standard_material_properties(name: &str) -> Option<StandardMaterialProperties> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    if let Some(fc) = concrete_grade_strength(n) {
        let gamma_rc =
            concrete_unit_weight_kn_m3(fc, ConcreteClass::Normal, ConcreteComposition::Rc);
        return Some(StandardMaterialProperties {
            young: concrete_young_modulus_gamma(fc, gamma_rc - 1.0),
            poisson: 0.2,
            density: mass_density_from_unit_weight_kn_m3(gamma_rc),
            fc: Some(fc),
            fy: None,
        });
    }
    if let Some(fy) = rebar_grade_strength(n) {
        return Some(StandardMaterialProperties {
            young: E_STEEL,
            poisson: 0.3,
            density: STEEL_MASS_DENSITY_TON_MM3,
            fc: None,
            fy: Some(fy),
        });
    }
    let base = steel_grade_base(n)?;
    steel_f_value(base, 40.0).map(|fy| StandardMaterialProperties {
        young: E_STEEL,
        poisson: 0.3,
        density: STEEL_MASS_DENSITY_TON_MM3,
        fc: None,
        fy: Some(fy),
    })
}

/// Fc 名を完全に解釈する。非正値・非有限値・不明な接尾辞は未解決。
pub fn concrete_grade_strength(name: &str) -> Option<f64> {
    let n = name.trim();
    let rest = n
        .strip_prefix("Fc")
        .or_else(|| n.strip_prefix("FC"))
        .or_else(|| n.strip_prefix("fc"))?;
    if !rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    parse_concrete_fc(n).filter(|v| v.is_finite() && *v > 0.0)
}

/// 対応済み鉄筋名を完全一致で解決する。未知名からの数値推定はしない。
pub fn rebar_grade_strength(name: &str) -> Option<f64> {
    let n = name.trim();
    if !matches!(
        n,
        "SR235"
            | "SR295"
            | "SD295"
            | "SD295A"
            | "SD295B"
            | "SD345"
            | "SD390"
            | "SD490"
            | "USD685"
            | "KH785"
            | "UB785"
            | "SBPD1275"
    ) {
        return None;
    }
    rebar_grade_f_value(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn rebar_standard_properties_preserve_strength_and_mass_density() {
        for (name, fy) in [("SD345", 345.0), ("SR235", 235.0)] {
            let material = standard_material_properties(name).unwrap();
            assert_eq!(material.fy, Some(fy), "{name}");
            assert_eq!(material.density, 7.85e-9, "{name}");
        }
    }

    #[test]
    fn concrete_standard_properties_follow_fc_dependent_unit_weight() {
        for (name, fc, young_n_mm2, density_ton_mm3) in [
            (
                "Fc24",
                24.0,
                22_668.945_903_873_48,
                2.447_318_911_147_028e-9,
            ),
            (
                "Fc36",
                36.0,
                25_949.465_239_834_98,
                2.447_318_911_147_028e-9,
            ),
            (
                "Fc40",
                40.0,
                28_058.274_635_930_21,
                2.498_304_721_795_924e-9,
            ),
            ("Fc50", 50.0, 31_524.706_967_514_46, 2.549_290_532_444_82e-9),
            ("Fc60", 60.0, 33_500.0, 2.549_290_532_444_82e-9),
        ] {
            let material = standard_material_properties(name).unwrap();
            assert_eq!(material.fc, Some(fc), "{name}");
            assert_relative_eq!(
                material.young,
                young_n_mm2,
                epsilon = 0.0,
                max_relative = 1e-14
            );
            assert_relative_eq!(
                material.density,
                density_ton_mm3,
                epsilon = 0.0,
                max_relative = 1e-14
            );
        }
    }

    #[test]
    fn unknown_numeric_name_has_no_standard_properties() {
        assert!(standard_material_properties("UNKNOWN999").is_none());
    }
}
