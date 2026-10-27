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
    big_lambda, plate_thickness, section_steel_f, steel_f_value, steel_f_value_prefix, steel_fc,
    steel_fs, steel_ft,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LoadTerm;
    use sepika_core::model::{Material, MaterialCategory};
    use sepika_core::units::ConcreteClass;

    #[test]
    fn concrete_shear_922_matches_independent_legal_boundary_values() {
        for (fc, long, short) in [
            (20.999, 0.6999666666666667, 1.04995),
            (21.0, 0.7, 1.05),
            (21.001, 0.70001, 1.050015),
            (24.0, 0.73, 1.095),
            (36.0, 0.85, 1.275),
        ] {
            assert!((concrete_allowable_shear(fc, true) - long).abs() < 1e-12);
            assert!((concrete_allowable_shear(fc, false) - short).abs() < 1e-12);
        }
    }

    #[test]
    fn test_concrete_fc24_representative_values() {
        for (long, compression, normal_shear, lightweight_shear) in
            [(true, 8.0, 0.73, 0.657), (false, 16.0, 1.095, 0.9855)]
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
    fn allowable_bending_ratio_boundaries_are_separate_from_analysis_ratio() {
        for (fc, expected) in [
            (27.0, 15.0),
            (27.0001, 13.0),
            (36.0, 13.0),
            (36.0001, 11.0),
            (48.0, 11.0),
            (48.0001, 9.0),
            (60.0, 9.0),
        ] {
            assert_eq!(young_ratio_n(fc), expected);
            assert_ne!(young_ratio_n(fc), 205000.0 / 20500.0);
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
        for (fc, top_long, other_long, top_short, other_short) in [
            (15.0, 1.0, 1.5, 1.5, 2.25),
            (22.49, 1.4993333333333334, 2.249, 2.249, 3.3735),
            (22.5, 1.5, 2.25, 2.25, 3.375),
            (22.51, 1.5002666666666666, 2.2504, 2.2504, 3.3756),
            (24.0, 1.54, 2.31, 2.31, 3.465),
            (36.0, 1.86, 2.79, 2.79, 4.185),
            (48.0, 1.86, 2.79, 2.79, 4.185),
        ] {
            for (top_bar, long_term, expected) in [
                (true, true, top_long),
                (false, true, other_long),
                (true, false, top_short),
                (false, false, other_short),
            ] {
                let actual = concrete_allowable_bond(fc, top_bar, long_term);
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "Fc={fc}, 上端={top_bar}, 長期={long_term}: {actual} != {expected}"
                );
            }
        }
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
    fn rebar_long_term_tension_follows_article_90_diameter_boundary_mm() {
        for (grade, short_term) in [("SD345", 345.0), ("SD390", 390.0), ("SD490", 490.0)] {
            for (dia_mm, expected_long_term) in [
                (25.0, 215.0),
                (25.4, 215.0),
                (27.999, 215.0),
                (28.0, 215.0),
                (28.001, 195.0),
                (28.6, 195.0),
                (29.0, 195.0),
            ] {
                assert_eq!(
                    rebar_allowable_tension(grade, dia_mm, true),
                    expected_long_term,
                    "{grade}, {dia_mm} mm"
                );
                assert_eq!(rebar_allowable_tension(grade, dia_mm, false), short_term);
            }
        }
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
