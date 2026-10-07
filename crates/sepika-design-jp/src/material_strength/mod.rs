//! 材料強度・許容応力度（許容応力度検定で用いる材料の許容応力度・材料定数）。
//! コンクリート・鉄筋は RC 規準・構造規定、鋼材は鋼構造設計規準 1973・構造規定に準拠する。

mod concrete;
mod rebar;
mod steel;

pub(crate) use concrete::concrete_allowable_bond_for_rebar;
pub use concrete::{
    concrete_allowable_bond, concrete_allowable_compression, concrete_allowable_shear,
    concrete_allowable_shear_class, concrete_young_modulus, young_ratio_n,
};
pub use rebar::{
    main_rebar_grade, rebar_allowable_shear, rebar_allowable_tension, rebar_sigma_y_of,
    shear_rebar_grade,
};
pub use steel::{
    big_lambda, plate_thickness, steel_f_value, steel_f_value_prefix, steel_fc, steel_fs, steel_ft,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LoadTerm;
    use sepika_core::model::{Material, MaterialCategory};
    use sepika_core::units::ConcreteClass;

    #[test]
    fn test_concrete_shear_long_term_min_branch() {
        // Fc=21: Fc/30=0.7, 0.5+Fc/100=0.71 → Fc/30 側が支配。
        assert!((concrete_allowable_shear(21.0, true) - 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_concrete_fc24_representative_values() {
        for (long, compression, normal_shear, lightweight_shear) in
            [(true, 8.0, 0.74, 0.666), (false, 16.0, 1.11, 0.999)]
        {
            assert!((concrete_allowable_compression(24.0, long) - compression).abs() < 1e-12);
            assert!((concrete_allowable_shear(24.0, long) - normal_shear).abs() < 1e-12);
            for (class, expected) in [
                (ConcreteClass::Normal, normal_shear),
                (ConcreteClass::Lightweight1, lightweight_shear),
                (ConcreteClass::Lightweight2, lightweight_shear),
            ] {
                assert!(
                    (concrete_allowable_shear_class(24.0, class, long) - expected).abs() < 1e-12,
                    "class={class:?} long={long}"
                );
            }
        }
    }

    #[test]
    fn test_young_ratio_n_buckets() {
        assert_eq!(young_ratio_n(24.0), 15.0);
        assert_eq!(young_ratio_n(27.0), 15.0);
        assert_eq!(young_ratio_n(30.0), 13.0);
        assert_eq!(young_ratio_n(42.0), 11.0);
        assert_eq!(young_ratio_n(60.0), 9.0);
        assert_eq!(young_ratio_n(80.0), 7.0);
    }

    #[test]
    fn test_concrete_allowable_bond_table() {
        // Fc=24 上端筋: min(24/15, 0.9+2/75×24) = min(1.6, 1.54) = 1.54
        assert!((concrete_allowable_bond(24.0, true, true) - 1.54).abs() < 1e-9);
        // Fc=24 その他: min(24/10, 1.355+24/25) = min(2.4, 2.315) = 2.315
        assert!((concrete_allowable_bond(24.0, false, true) - 2.315).abs() < 1e-9);
        assert!(
            (concrete_allowable_bond(24.0, true, false)
                - concrete_allowable_bond(24.0, true, true) * 1.5)
                .abs()
                < 1e-9
        );
        // 低強度側の分岐: Fc=15 上端筋 min(1.0, 1.3) = 1.0（Fc/15 側が支配）。
        assert!((concrete_allowable_bond(15.0, true, true) - 1.0).abs() < 1e-9);
        assert!((concrete_allowable_bond(48.0, false, true) - 2.795).abs() < 1e-9);
    }

    #[test]
    fn test_round_rebar_allowable_bond_fc24() {
        assert!((concrete_allowable_bond_for_rebar(24.0, true, true, false) - 0.9).abs() < 1e-9);
        assert!((concrete_allowable_bond_for_rebar(24.0, false, true, false) - 1.35).abs() < 1e-9);
        assert!((concrete_allowable_bond_for_rebar(24.0, true, false, false) - 1.35).abs() < 1e-9);
        assert!(
            (concrete_allowable_bond_for_rebar(24.0, false, false, false) - 2.025).abs() < 1e-9
        );
    }

    #[test]
    fn test_rebar_tension_sd345_d29_reduction() {
        assert!((rebar_allowable_tension("SD345", 25.0, true) - 215.0).abs() < 1e-9);
        assert!((rebar_allowable_tension("SD345", 29.0, true) - 195.0).abs() < 1e-9);
        assert!((rebar_allowable_tension("SD345", 25.0, false) - 345.0).abs() < 1e-9);
    }

    #[test]
    fn test_rebar_usd685() {
        assert!((rebar_allowable_tension("USD685", 32.0, true) - 215.0).abs() < 1e-9);
        assert!((rebar_allowable_tension("USD685", 32.0, false) - 685.0).abs() < 1e-9);
    }

    /// σy は断面の主筋材料の `fy` だけから決まる。材料名からの推定は行わない
    /// （材料名は許容応力度表の引き当てにのみ用いる）。
    #[test]
    fn test_rebar_sigma_y_sources() {
        let mut m = Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: sepika_core::ids::MaterialId(0),
            name: "SD390".to_string(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: Some(24.0),
            fy: None,
        };
        // fy が無ければ 0（検定の入口で止まるため耐力算定へは流れない）。
        assert!(rebar_sigma_y_of(Some(&m)).abs() < 1e-9);
        m.fy = Some(400.0);
        assert!((rebar_sigma_y_of(Some(&m)) - 400.0).abs() < 1e-9);
        // 主筋の材料が未割当でも 0 とし、既定値をでっち上げない。
        assert!(rebar_sigma_y_of(None).abs() < 1e-9);
    }

    #[test]
    fn test_steel_tension_and_shear_reference_values() {
        for (term, tension, shear) in [
            (
                LoadTerm::Long,
                156.666_666_666_666_66,
                90.451_542_173_041_36,
            ),
            (LoadTerm::Short, 235.0, 135.677_313_259_562_06),
        ] {
            assert!((steel_ft(235.0, term) - tension).abs() < 1e-9);
            assert!((steel_fs(235.0, term) - shear).abs() < 1e-9);
        }
    }

    #[test]
    fn test_steel_fc_continuous_at_lambda() {
        // λ=0 で fc = F/1.5（=ft長期）、λ=Λ で両分岐が連続（0.277F 近傍）。
        let f = 235.0;
        let e = 205_000.0;
        assert!((steel_fc(f, e, 0.0, LoadTerm::Long) - f / 1.5).abs() < 1e-6);
        let big_l = big_lambda(f, e);
        let below = steel_fc(f, e, big_l - 1e-9, LoadTerm::Long);
        let above = steel_fc(f, e, big_l + 1e-9, LoadTerm::Long);
        // 両分岐の差は 0.277 と 3.6/13 の丸め分のみ（F の 1e-4 未満）。
        assert!(
            (below - above).abs() < 1e-4 * f,
            "below={} above={}",
            below,
            above
        );
        // λ>Λ 側は λ=Λ（r=1）で 0.277F に一致する。
        assert!((above - 0.277 * f).abs() < 1e-6, "above={}", above);
    }

    #[test]
    fn test_big_lambda_representative_value() {
        assert!((big_lambda(235.0, 205_000.0) - 119.789_084_805_182_68).abs() < 1e-9);
    }

    #[test]
    fn test_steel_fc_independent_reference_values() {
        for (young, lambda, long, short) in [
            (205_000.0, 0.0, 156.666_666_666_666_66, 235.0),
            (
                205_000.0,
                50.0,
                135.274_087_404_248_12,
                202.911_131_106_372_18,
            ),
            (
                205_000.0,
                300.0,
                10.378_620_109_552_948,
                15.567_930_164_329_422,
            ),
            (
                100_000.0,
                50.0,
                115.889_000_442_990_08,
                173.833_500_664_485_11,
            ),
        ] {
            assert!(
                (steel_fc(235.0, young, lambda, LoadTerm::Long) - long).abs() < 1e-9,
                "E={young} λ={lambda}"
            );
            assert!(
                (steel_fc(235.0, young, lambda, LoadTerm::Short) - short).abs() < 1e-9,
                "E={young} λ={lambda}"
            );
        }
    }
}
