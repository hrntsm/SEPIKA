//! ST-Bridge のグレード名からアプリの物性既定値を復元する。

pub(super) use sepika_core::standard_material::standard_material_properties as resolve_grade;

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::section_shape::concrete_young_modulus_gamma;
    use sepika_core::units::{
        concrete_unit_weight_kn_m3, to_internal::mass_density_from_unit_weight_kn_m3,
        ConcreteClass, ConcreteComposition,
    };

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
