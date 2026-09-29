//! ST-Bridge のグレード名（`Fc21`・`SN400B`・`SD345` 等）から材料物性を一意に解決する。
//!
//! ST-Bridge 2.0 の `StbModel` は材料テーブル（E・ν・密度）を持たず、材料は断面に付く
//! グレード名の文字列で表す。日本の建築構造材料は法令・JIS で規格化されており、
//! グレード名が決まれば物性（ヤング係数・ポアソン比・単位体積重量・基準強度）は一意に定まる。
//! 本モジュールは代表的な規格材のグレード名から [`StdMat`] を返す（未知の名前は `None`）。
//!
//! グレード名→強度の対応表は `sepika_core::material_grade` に一本化されており、
//! 本モジュールはそれを利用する（鉄筋・鋼材・コンクリートの表を独自に持たない）。

use sepika_core::material_grade::{parse_concrete_fc, rebar_f_value, steel_f_value_prefix};
use sepika_core::section_shape::{concrete_young_modulus_gamma, E_STEEL};
use sepika_core::units::STEEL_MASS_DENSITY_TON_MM3;
use sepika_core::units::{
    concrete_unit_weight_kn_m3, to_internal::mass_density_from_unit_weight_kn_m3, ConcreteClass,
    ConcreteComposition,
};

/// グレード名から解決した標準材料物性（内部単位系 N-mm-s。密度は ton/mm³）。
pub(super) struct StdMat {
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
pub(super) fn resolve_grade(name: &str) -> Option<StdMat> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    if let Some(fc) = parse_concrete_fc(n) {
        let gamma_rc =
            concrete_unit_weight_kn_m3(fc, ConcreteClass::Normal, ConcreteComposition::Rc);
        return Some(StdMat {
            young: concrete_young_modulus_gamma(fc, gamma_rc - 1.0),
            poisson: 0.2,
            density: mass_density_from_unit_weight_kn_m3(gamma_rc),
            fc: Some(fc),
            fy: None,
        });
    }
    if let Some(fy) = rebar_f_value(n) {
        return Some(StdMat {
            young: E_STEEL,
            poisson: 0.3,
            density: STEEL_MASS_DENSITY_TON_MM3,
            fc: None,
            fy: Some(fy),
        });
    }
    steel_f_value_prefix(n, 40.0).map(|fy| StdMat {
        young: E_STEEL,
        poisson: 0.3,
        density: STEEL_MASS_DENSITY_TON_MM3,
        fc: None,
        fy: Some(fy),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 接尾辞なし "SN490" は SM520 系（F=355）ではなく 490 N/mm² 級（F=325）に
    /// 解決されること（core 委譲前は SM520 の腕に誤って一致していた回帰）。
    #[test]
    fn test_resolve_grade_sn490_is_325() {
        let m = resolve_grade("SN490").unwrap();
        assert_eq!(m.fy, Some(325.0));
    }

    /// グレード名→強度の網羅表は `sepika_core::material_grade` を正とする。
    /// 本クレートは core の解決表へ委譲している配線を代表ケースで確認する。
    #[test]
    fn test_resolve_grade_wires_to_core() {
        for (name, fy) in [("SS400", 235.0), ("SN490B", 325.0), ("SM520", 355.0)] {
            let m = resolve_grade(name).unwrap_or_else(|| panic!("{name} が解決できませんでした"));
            assert_eq!(m.fy, Some(fy), "{name}");
        }
    }

    #[test]
    fn test_resolve_grade_rebar_and_concrete() {
        assert_eq!(resolve_grade("SD345").unwrap().fy, Some(345.0));
        assert_eq!(resolve_grade("SR235").unwrap().fy, Some(235.0));
        assert_eq!(resolve_grade("Fc24").unwrap().fc, Some(24.0));
        assert!(resolve_grade("UNKNOWN999").is_none());
        assert!(resolve_grade("").is_none());
    }

    #[test]
    fn test_resolve_grade_concrete_uses_fc_dependent_unit_weight() {
        for fc in [36.0, 40.0, 50.0, 60.0] {
            let material = resolve_grade(&format!("Fc{fc:.0}")).unwrap();
            let gamma_rc =
                concrete_unit_weight_kn_m3(fc, ConcreteClass::Normal, ConcreteComposition::Rc);
            assert_eq!(
                material.young,
                concrete_young_modulus_gamma(fc, gamma_rc - 1.0)
            );
            assert_eq!(
                material.density,
                mass_density_from_unit_weight_kn_m3(gamma_rc)
            );
        }
    }

    /// 鋼材・鉄筋の質量密度は物理密度 7.85 t/m³（= 7.85e-9 t/mm³）。
    /// 設計用単位体積重量 78.5 kN/m³ からは導出しない。
    #[test]
    fn test_resolve_grade_steel_mass_density() {
        assert_eq!(resolve_grade("SN400B").unwrap().density, 7.85e-9);
        assert_eq!(resolve_grade("SS400").unwrap().density, 7.85e-9);
        assert_eq!(resolve_grade("SD345").unwrap().density, 7.85e-9);
        assert_eq!(resolve_grade("SR235").unwrap().density, 7.85e-9);
    }
}
