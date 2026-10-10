//! 標準グレードから復元するアプリの物性既定値。

use crate::material_grade::{parse_concrete_fc, rebar_grade_f_value, steel_f_value, STEEL_GRADES};
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
/// 鉄筋（`SD`/`SR`）は鋼材の前方一致より先に判定する（`SD` は鋼材グレード表と
/// 前方一致しないため順序自体は結果に影響しないが、意図を明示するため維持する）。
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
    let base = STEEL_GRADES.iter().find(|base| {
        n == **base
            || n.strip_prefix(**base).is_some_and(|suffix| match **base {
                "SN400" => matches!(suffix, "A" | "B" | "C"),
                "SN490" => matches!(suffix, "B" | "C"),
                "SM400" | "SM490" => matches!(suffix, "A" | "B" | "C" | "YA" | "YB"),
                "SM520" => matches!(suffix, "B" | "C"),
                "STKN400" | "STKN490" => matches!(suffix, "B" | "W"),
                "SNR400" => matches!(suffix, "A" | "B"),
                "SNR490" => suffix == "B",
                _ => false,
            })
    })?;
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
