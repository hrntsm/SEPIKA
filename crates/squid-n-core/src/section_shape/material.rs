//! 材料に関する換算関数。
//!
//! - [`concrete_young_modulus`] — コンクリート強度 Fc からヤング係数 Ec を算定
//! - [`wall_shear_shape_factor_isection`] — 耐震壁のせん断形状係数

use super::constants::{GAMMA_CONCRETE, KAPPA_RC};
use crate::units::to_internal::unit_weight_kn_m3_from_mass_density;

/// コンクリート強度 Fc [N/mm²] からヤング係数 Ec [N/mm²] を算定する
/// （RC 規準の Ec=3.35·10⁴·(γC/24)²·(Fc/60)^(1/3)。密度を持たない経路は γC=23 を用いる）。
pub fn concrete_young_modulus(fc: f64) -> f64 {
    concrete_young_modulus_gamma(fc, GAMMA_CONCRETE)
}

/// コンクリート強度 Fc [N/mm²]・気乾単位体積重量 γ [kN/m³] から
/// ヤング係数 Ec [N/mm²] を算定する（RC 規準の Ec=3.35·10⁴·(γ/24)²·(Fc/60)^(1/3)）。
pub fn concrete_young_modulus_gamma(fc: f64, gamma_kn_m3: f64) -> f64 {
    if fc <= 0.0 {
        return 0.0;
    }
    3.35e4 * (gamma_kn_m3 / 24.0).powi(2) * (fc / 60.0).powf(1.0 / 3.0)
}

/// コンクリート強度 Fc [N/mm²]・RC 材料の質量密度 [ton/mm³] から
/// ヤング係数 Ec [N/mm²] を算定する。質量密度から γRC を復元し、γC = γRC - 1.0
/// とする。不正な密度は既定の γC=23.0 kN/m³ にフォールバックする。
pub fn concrete_young_modulus_from_density(fc: f64, density_ton_per_mm3: f64) -> f64 {
    let gamma_rc = unit_weight_kn_m3_from_mass_density(density_ton_per_mm3);
    let gamma_c = if density_ton_per_mm3.is_finite() && gamma_rc.is_finite() && gamma_rc > 1.0 {
        gamma_rc - 1.0
    } else {
        GAMMA_CONCRETE
    };
    concrete_young_modulus_gamma(fc, gamma_c)
}

/// 平面I形断面のせん断形状係数を区分多項式の積分で求める。寸法は [mm]。
/// 引数は全せい・片側フランジせい・フランジ幅・ウェブ幅の順。
/// 非有限・非正の全せい/幅、側柱なし、フランジ幅が壁厚以下なら1.2を返す。
pub fn wall_shear_shape_factor_isection(d_total: f64, dc_each: f64, bc: f64, t: f64) -> f64 {
    if !(d_total.is_finite() && dc_each.is_finite() && bc.is_finite() && t.is_finite())
        || d_total <= 0.0
        || t <= 0.0
        || bc <= 0.0
    {
        return KAPPA_RC;
    }
    let c = d_total / 2.0;
    let dc = dc_each.clamp(0.0, c);
    let a = c - dc;
    if dc <= 0.0 || bc <= t {
        return KAPPA_RC;
    }
    let strips = if a > 0.0 {
        vec![[dc, bc], [2.0 * a, t], [dc, bc]]
    } else {
        vec![[2.0 * dc, bc]]
    };
    let Some(properties) = super::shear::strip_section_properties(&strips) else {
        return KAPPA_RC;
    };
    let kappa = properties.area / properties.shear_area;
    if kappa.is_finite() && kappa >= 1.0 {
        kappa
    } else {
        KAPPA_RC
    }
}

#[cfg(test)]
mod isection_kappa_tests {
    use super::*;

    #[test]
    fn test_kappa_matches_exact_polynomial_integral() {
        // D=4, dc=1, bc=2, t=1: A=6, I=10。
        // 半断面の積分はウェブ167/15、フランジ53/30。κ=387/250。
        let expected = 387.0 / 250.0;
        for scale in [0.001, 1.0, 1000.0] {
            let actual = wall_shear_shape_factor_isection(4.0 * scale, scale, 2.0 * scale, scale);
            assert!(
                (actual - expected).abs() < 1e-12,
                "κ={actual}, expected={expected}"
            );
        }
    }

    /// 一様矩形（側柱なし、または側柱幅＝壁厚）では厳密に 1.2。
    #[test]
    fn test_kappa_uniform_rectangle_is_12() {
        assert!((wall_shear_shape_factor_isection(4000.0, 0.0, 150.0, 150.0) - 1.2).abs() < 1e-9);
        assert!((wall_shear_shape_factor_isection(4000.0, 600.0, 150.0, 150.0) - 1.2).abs() < 1e-9);
        assert!(
            (wall_shear_shape_factor_isection(4000.0, 2000.0, 600.0, 150.0) - 1.2).abs() < 1e-12
        );
    }

    /// 代表的な3断面では κ > 1.2 となり、選んだ側柱寸法の範囲では増大する。
    /// 従来の閉形式は逆に 1.2 から減少していた（非物理）。
    #[test]
    fn test_kappa_increases_with_flange_size() {
        let t = 150.0;
        let k300 = wall_shear_shape_factor_isection(4000.0, 300.0, 300.0, t);
        let k600 = wall_shear_shape_factor_isection(4000.0, 600.0, 600.0, t);
        let k900 = wall_shear_shape_factor_isection(4000.0, 900.0, 900.0, t);
        assert!(k300 > 1.2, "k300={}", k300);
        assert!(k600 > k300, "k600={} k300={}", k600, k300);
        assert!(k900 > k600, "k900={} k600={}", k900, k600);
    }

    /// せん断断面積 A/κ は総断面積を超えず、ウェブ断面積を下回らない
    /// （I 形断面のせん断はウェブがほぼ全負担する）。従来式は A/κ > A となり破綻していた。
    #[test]
    fn test_shear_area_is_between_web_and_gross() {
        let (d, dc, bc, t) = (4000.0, 600.0, 600.0, 150.0_f64);
        let kappa = wall_shear_shape_factor_isection(d, dc, bc, t);
        let area = 2.0 * dc * bc + 2.0 * (d / 2.0 - dc) * t;
        let web = 2.0 * (d / 2.0 - dc) * t;
        let as_shear = area / kappa;
        assert!(as_shear <= area, "As={} > A={}", as_shear, area);
        assert!(as_shear >= web * 0.8, "As={} << Aweb={}", as_shear, web);
    }

    /// 退化入力は 1.2 へフォールバック。
    #[test]
    fn test_kappa_degenerate_inputs() {
        assert!((wall_shear_shape_factor_isection(0.0, 100.0, 300.0, 150.0) - 1.2).abs() < 1e-12);
        assert!((wall_shear_shape_factor_isection(4000.0, 100.0, 300.0, 0.0) - 1.2).abs() < 1e-12);
    }
}

#[cfg(test)]
mod concrete_young_modulus_tests {
    use super::*;
    use crate::units::to_internal::mass_density_from_unit_weight_kn_m3;

    #[test]
    fn rc_unit_weight_24_gives_concrete_unit_weight_23() {
        let density = mass_density_from_unit_weight_kn_m3(24.0);
        let expected = concrete_young_modulus_gamma(21.0, 23.0);
        assert_eq!(concrete_young_modulus_from_density(21.0, density), expected);
    }

    #[test]
    fn invalid_density_uses_existing_default() {
        assert_eq!(
            concrete_young_modulus_from_density(21.0, 0.0),
            concrete_young_modulus(21.0)
        );
        assert_eq!(
            concrete_young_modulus_from_density(21.0, f64::NAN),
            concrete_young_modulus(21.0)
        );
    }
}
