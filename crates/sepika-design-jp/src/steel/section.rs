//! 鉄骨の断面検定における断面性能（鋼構造設計規準の
//! 鉄骨の断面検定における断面性能）。
//!
//! 横座屈を考慮した許容曲げ応力度 fb（鋼構造設計規準 1973）・断面二次半径・
//! 断面欠損（継手部・スカラップ）を考慮した断面係数 Z'・横座屈長さ lb の
//! 解決を扱う。

use crate::material_strength::{big_lambda, steel_ft};
use crate::{DesignCtx, LoadTerm};
use sepika_core::model::Section;
use sepika_core::section_shape::{SectionShape, E_STEEL};

use super::section_modulus;

/// 圧縮フランジとウェブからなるT形断面の断面二次半径 `[mm]` を返す（1973年版の横座屈用）。
/// b・tf・h・twはmm。対象断面積が微小または非正の場合は0。
pub fn steel_i_t(b: f64, tf: f64, h: f64, tw: f64) -> f64 {
    let hw = (h / 6.0 - tf).max(0.0);
    let i_t = tf * b.powi(3) / 12.0 + hw * tw.powi(3) / 12.0;
    let a_t = b * tf + hw * tw;
    if a_t > 1e-12 {
        (i_t / a_t).sqrt()
    } else {
        0.0
    }
}

/// 横座屈用の断面二次半径 `[mm]`・圧縮フランジ面積 `[mm²]` をこの順で返す。
/// H形はshape寸法、非対称組立H形は二次半径の小さいフランジ側を採用する。
/// それ以外はsecの幅・せいと引数tf・tw `[mm]` を用いる。
pub(crate) fn steel_lateral_buckling_i_af(sec: &Section, tf: f64, tw: f64) -> (f64, f64) {
    match &sec.shape {
        Some(SectionShape::SteelH {
            height,
            width,
            web_thick,
            flange_thick,
            ..
        }) => {
            let i = steel_i_t(*width, *flange_thick, *height, *web_thick);
            (i, width * flange_thick)
        }
        Some(SectionShape::SteelBuiltH {
            height,
            upper_width,
            upper_thick,
            lower_width,
            lower_thick,
            web_thick,
        }) => {
            let i_upper = steel_i_t(*upper_width, *upper_thick, *height, *web_thick);
            let i_lower = steel_i_t(*lower_width, *lower_thick, *height, *web_thick);
            if i_upper <= i_lower {
                (i_upper, upper_width * upper_thick)
            } else {
                (i_lower, lower_width * lower_thick)
            }
        }
        _ => {
            let b = sec.width;
            let h = sec.depth;
            (steel_i_t(b, tf, h, tw), b * tf)
        }
    }
}

/// 1973年版のH形鋼強軸の許容曲げ応力度 `[N/mm²]` を返す。
/// FはN/mm²、lb・i・hはmm、afはmm²、cは確定済みの無次元修正係数。
/// 長期の上限はF/1.5、短期は長期の1.5倍で上限F。
pub fn steel_fb_h(f: f64, term: LoadTerm, lb: f64, i: f64, h: f64, af: f64, c: f64) -> f64 {
    let c = if c > 1e-9 { c } else { 1.0 };
    let i = i.max(1e-9);
    let af = af.max(1e-9);
    let big_l = big_lambda(f, E_STEEL);
    let big_l2 = (big_l.max(1e-9)).powi(2);

    let fb1 = f * (2.0 / 3.0 - (4.0 / 15.0) * (lb / i).powi(2) / (c * big_l2));
    let denom2 = lb * h / af;
    let fb2 = if denom2 > 0.0 {
        89_000.0 / denom2
    } else {
        f64::INFINITY
    };

    let cap_long = f / 1.5;
    let fb_long = fb1.max(fb2).min(cap_long).max(0.0);
    match term {
        LoadTerm::Long => fb_long,
        LoadTerm::Short => (fb_long * 1.5).min(f),
    }
}

/// 座屈区間の強軸モーメントから横座屈修正係数Cを返す。自動算定の上限は2.3。
/// 端部モーメントが未設定、または中央の絶対値が両端より大きい場合は1。
pub(crate) fn steel_lateral_buckling_c(ctx: &DesignCtx) -> f64 {
    let Some(m2_over_m1) = end_moment_ratio_m2_m1(ctx) else {
        return 1.0;
    };
    let c = 1.75 + 1.05 * m2_over_m1 + 0.3 * m2_over_m1 * m2_over_m1;
    c.min(2.3)
}

/// 端部強軸モーメントの絶対値が大きい方をM1、小さい方をM2とした比を返す。
/// 両端異符号なら正、同符号なら負。端部未設定・中央の絶対値が両端より大きい場合はNone。
/// 両端とも微小な場合は `Some(0.0)`。
fn end_moment_ratio_m2_m1(ctx: &DesignCtx) -> Option<f64> {
    let (m_i, m_j) = ctx.end_moments_z?;
    let abs_i = m_i.abs();
    let abs_j = m_j.abs();
    let m1 = abs_i.max(abs_j);
    let m2 = abs_i.min(abs_j);

    if let Some(mid) = ctx.mid_moment_z {
        if mid.abs() > m1 + 1e-9 {
            return None;
        }
    }
    if m1 <= 1e-9 {
        return Some(0.0);
    }
    let ratio_abs = m2 / m1;
    let double_curvature = m_i * m_j < 0.0;
    Some(if double_curvature {
        ratio_abs
    } else {
        -ratio_abs
    })
}

/// 有効なC直接入力を最優先し、自動算定の上限を適用せず返す。
/// 直接入力が無効または未設定なら、部分座屈区間は1、それ以外は端部モーメントから自動算定する。
pub(crate) fn steel_c_factor(ctx: &DesignCtx, lb_is_partial: bool) -> f64 {
    if let Some(c) = ctx.steel_attr.as_ref().and_then(|a| a.c_direct) {
        if c > 1e-9 {
            return c;
        }
    }
    if lb_is_partial {
        return 1.0;
    }
    steel_lateral_buckling_c(ctx)
}

/// 2019年版のH形鋼強軸の許容曲げ応力度 `[N/mm²]` を返す。
/// F・E・GはN/mm²、lbはmm、Iz・Jはmm⁴、Iwはmm⁶、Zはmm³、C・pλbは無次元。
/// 長期の上限はF/1.5、短期は長期の1.5倍で上限F。lbが0以下なら許容引張応力度を返す。
#[allow(clippy::too_many_arguments)]
pub fn steel_fb_h_asd2019(
    f: f64,
    term: LoadTerm,
    lb: f64,
    iz: f64,
    iw: f64,
    j: f64,
    e: f64,
    g: f64,
    z_strong: f64,
    c: f64,
    p_lambda_b: f64,
) -> f64 {
    if lb <= 1e-9 {
        return steel_ft(f, term);
    }
    let c = if c > 1e-9 { c } else { 1.0 };

    let my = f * z_strong;
    let pi2 = std::f64::consts::PI.powi(2);
    let pi4 = std::f64::consts::PI.powi(4);
    let me_sq = pi4 * e * iz * e * iw / lb.powi(4) + pi2 * e * iz * g * j / lb.powi(2);
    let me = (c * me_sq.max(0.0).sqrt()).max(1e-9);

    let lambda_b = (my / me).max(0.0).sqrt();
    let e_lambda_b = 1.29;
    let nu = 1.5 + (2.0 / 3.0) * (lambda_b / e_lambda_b).powi(2);

    let fb_long = if lambda_b <= p_lambda_b {
        f / nu
    } else if lambda_b <= e_lambda_b {
        let gap = (e_lambda_b - p_lambda_b).max(1e-9);
        (1.0 - 0.4 * (lambda_b - p_lambda_b) / gap) * f / nu
    } else {
        f / (2.17 * lambda_b.powi(2))
    };
    let cap_long = f / 1.5;
    let fb_long = fb_long.min(cap_long).max(0.0);

    match term {
        LoadTerm::Long => fb_long,
        LoadTerm::Short => (fb_long * 1.5).min(f),
    }
}

/// 2019年版の塑性限界細長比pλbを返す。端部比は両端異符号で正、同符号で負。
/// 部分座屈区間・端部モーメント未設定・中央の絶対値が両端より大きい場合は0.3。
/// 両端とも微小な場合は0.6。
pub(crate) fn steel_p_lambda_b(ctx: &DesignCtx, lb_is_partial: bool) -> f64 {
    if lb_is_partial {
        return 0.3;
    }
    let Some(m2_over_m1) = end_moment_ratio_m2_m1(ctx) else {
        return 0.3;
    };
    0.6 + 0.3 * m2_over_m1
}

/// H形鋼の曲げねじり定数Iw `[mm⁶]` を返す。tfはmm。
/// 非対称組立H形はshape寸法、それ以外はsecのせい・弱軸断面二次モーメントとtfを用いる。
pub(crate) fn steel_warping_constant(sec: &Section, tf: f64) -> f64 {
    match &sec.shape {
        Some(SectionShape::SteelBuiltH {
            height,
            upper_width,
            upper_thick,
            lower_width,
            lower_thick,
            ..
        }) => {
            let hf = height - (upper_thick + lower_thick) / 2.0;
            let i_u = upper_thick * upper_width.powi(3) / 12.0;
            let i_l = lower_thick * lower_width.powi(3) / 12.0;
            let denom = i_u + i_l;
            if denom > 1e-12 {
                hf * hf * i_u * i_l / denom
            } else {
                0.0
            }
        }
        _ => {
            let h = sec.depth;
            sec.iz * (h - tf).powi(2) / 4.0
        }
    }
}

/// 欠損を考慮したH形鋼の強軸断面係数 `[mm³]` を返す。寸法はmm、欠損率は%。
/// フランジ/ウェブ継手欠損を適用し、`is_end=true` の場合だけスカラップ欠損も併用する。
/// 全欠損率が0なら全断面の断面係数に一致する。
#[allow(clippy::too_many_arguments)]
pub fn steel_h_z_with_loss(
    h: f64,
    b: f64,
    tw: f64,
    tf: f64,
    beta_f: f64,
    beta_w: f64,
    alpha_w: f64,
    is_end: bool,
) -> f64 {
    let hw = (h - 2.0 * tf).max(0.0);
    let i_f = (b * h.powi(3) - b * hw.powi(3)) / 12.0;
    let i_f_prime = i_f * (1.0 - beta_f / 100.0).max(0.0);

    let i_w_prime = if is_end {
        let hw_scallop = hw * (1.0 - alpha_w / 100.0).max(0.0);
        let i_w_scallop = tw * hw_scallop.powi(3) / 12.0;
        i_w_scallop * (1.0 - beta_w / 100.0).max(0.0)
    } else {
        let i_w = tw * hw.powi(3) / 12.0;
        i_w * (1.0 - beta_w / 100.0).max(0.0)
    };

    section_modulus(i_f_prime + i_w_prime, h / 2.0)
}

/// 横座屈長さ `[mm]` を、直接入力・等間隔横補剛・部材長の優先順で返す。
/// 直接入力tupleは始端・中央・終端の順。無次元位置posが0.25未満は始端、0.75未満は中央、それ以外は終端。
/// 直接入力も補剛本数もなければlength `[mm]` を用いる。
pub fn resolve_lb(
    pos: f64,
    length: f64,
    lb_direct: Option<(f64, f64, f64)>,
    brace_count: Option<u32>,
) -> f64 {
    if let Some((start, mid, end)) = lb_direct {
        return if pos < 0.25 {
            start
        } else if pos < 0.75 {
            mid
        } else {
            end
        };
    }
    if let Some(n) = brace_count {
        return length / (n as f64 + 1.0);
    }
    length
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_strength::steel_ft;
    use sepika_core::ids::ElemId;
    use sepika_core::model::SteelDesignAttr;

    #[test]
    fn test_fb_lb_zero_equals_ft() {
        let f = 235.0;
        let fb = steel_fb_h(f, LoadTerm::Long, 0.0, 100.0, 500.0, 4000.0, 1.0);
        let ft = steel_ft(f, LoadTerm::Long);
        assert!((fb - ft).abs() < 1e-6, "fb={} ft={}", fb, ft);
    }

    #[test]
    fn test_fb_large_lb_fb2_governs() {
        let f = 235.0;
        let i = 60.0;
        let h = 500.0;
        let af = 4000.0;
        let lb = 20_000.0;
        let fb = steel_fb_h(f, LoadTerm::Long, lb, i, h, af, 1.0);
        let fb2 = 89_000.0 / (lb * h / af);
        assert!(
            (fb - fb2).abs() < 1e-6,
            "fb2 should govern: fb={} fb2={}",
            fb,
            fb2
        );
    }

    #[test]
    fn test_i_t_helper() {
        let i = steel_i_t(200.0, 15.0, 400.0, 10.0);
        // hw = max(h/6 - tf, 0) = max(400/6 - 15, 0) = 51.6666...
        let hw: f64 = (400.0 / 6.0 - 15.0_f64).max(0.0);
        let expected_it = 15.0 * 200.0_f64.powi(3) / 12.0 + hw * 10.0_f64.powi(3) / 12.0;
        let expected_at = 200.0 * 15.0 + hw * 10.0;
        let expected = (expected_it / expected_at).sqrt();
        assert!((i - expected).abs() < 1e-9);
    }

    /// h/6 ≤ tf（薄せい・厚フランジ）の場合はウェブ寄与分が 0 にガードされ、
    /// T 形はフランジ矩形単独の断面二次半径 `i = √((tf·b³/12)/(b·tf)) = b/√12`
    /// に一致する。
    #[test]
    fn test_i_t_helper_hw_clamped_to_zero() {
        // h/6 = 100/6 = 16.67 < tf=20 → hw=0。
        let i = steel_i_t(200.0, 20.0, 100.0, 10.0);
        let expected = 200.0 / 12.0_f64.sqrt();
        assert!((i - expected).abs() < 1e-9, "i={} expected={}", i, expected);
    }

    #[test]
    fn test_asd2019_e_lambda_b_and_short_term_factor() {
        let f = 235.0;
        let long = steel_fb_h_asd2019(
            f,
            LoadTerm::Long,
            20_000.0,
            1.0e7,
            1.0e12,
            1.0e5,
            205_000.0,
            79_000.0,
            1.0e6,
            1.0,
            0.3,
        );
        let short = steel_fb_h_asd2019(
            f,
            LoadTerm::Short,
            20_000.0,
            1.0e7,
            1.0e12,
            1.0e5,
            205_000.0,
            79_000.0,
            1.0e6,
            1.0,
            0.3,
        );
        assert!((short - (long * 1.5).min(f)).abs() < 1e-9);
    }

    #[test]
    fn test_asd2019_fixed_values_cover_elastic_inelastic_and_caps() {
        let f = 235.0;
        let iz = 1.0e7;
        let iw = 1.0e12;
        let j = 1.0e5;
        let e = 205_000.0;
        let g = 79_000.0;
        let z = 1.0e6;
        let c = 1.0;
        let p_lambda_b = 0.3;
        let fb_elastic = steel_fb_h_asd2019(
            f,
            LoadTerm::Long,
            20_000.0,
            iz,
            iw,
            j,
            e,
            g,
            z,
            c,
            p_lambda_b,
        );
        assert!((fb_elastic - 11.798015111900156).abs() < 1e-12);

        let fb_inelastic = steel_fb_h_asd2019(
            f,
            LoadTerm::Long,
            5_000.0,
            iz,
            iw,
            j,
            e,
            g,
            z,
            c,
            p_lambda_b,
        );
        assert!((fb_inelastic - 94.31787217895743).abs() < 1e-12);

        let fb_cap_long =
            steel_fb_h_asd2019(f, LoadTerm::Long, 0.0, iz, iw, j, e, g, z, c, p_lambda_b);
        let fb_cap_short =
            steel_fb_h_asd2019(f, LoadTerm::Short, 0.0, iz, iw, j, e, g, z, c, p_lambda_b);
        assert!((fb_cap_long - 156.66666666666666).abs() < 1e-9);
        assert!((fb_cap_short - 235.0).abs() < 1e-9);
    }

    /// 複曲率（端部モーメント異符号＝反曲点あり）・等モーメント逆向き相当
    /// （M2/M1=+1）で C=1.75+1.05+0.3=3.1 → 上限 2.3 にクランプされる。
    #[test]
    fn test_c_factor_double_curvature_equal_clamps_to_2_3() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, -100.0)),
            ..Default::default()
        };
        let c = steel_lateral_buckling_c(&ctx);
        assert!((c - 2.3).abs() < 1e-9, "c={}", c);
    }

    /// 単曲率（端部モーメント同符号＝反曲点なし）・一様分布相当（M2/M1=-1）で
    /// C=1.75-1.05+0.3=1.0。
    #[test]
    fn test_c_factor_single_curvature_uniform_is_1_0() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, 100.0)),
            ..Default::default()
        };
        let c = steel_lateral_buckling_c(&ctx);
        assert!((c - 1.0).abs() < 1e-9, "c={}", c);
    }

    /// 片端モーメントが 0（M2=0）の場合は M2/M1=0 なので C=1.75。
    #[test]
    fn test_c_factor_m2_zero_is_1_75() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, 0.0)),
            ..Default::default()
        };
        let c = steel_lateral_buckling_c(&ctx);
        assert!((c - 1.75).abs() < 1e-9, "c={}", c);
    }

    /// 座屈区間中央の曲げが両端部より大きい場合は安全側 C=1.0。
    #[test]
    fn test_c_factor_mid_moment_dominant_is_1_0() {
        let ctx = DesignCtx {
            end_moments_z: Some((50.0, 50.0)),
            mid_moment_z: Some(200.0),
            ..Default::default()
        };
        let c = steel_lateral_buckling_c(&ctx);
        assert!((c - 1.0).abs() < 1e-9, "c={}", c);
    }

    /// end_moments_z が None の場合は C=1.0。
    #[test]
    fn test_c_factor_none_end_moments_is_1_0() {
        let ctx = DesignCtx {
            end_moments_z: None,
            ..Default::default()
        };
        let c = steel_lateral_buckling_c(&ctx);
        assert!((c - 1.0).abs() < 1e-9, "c={}", c);
    }

    /// 符号判定の検証: SEPIKA の内力（断面力）符号規約では mz(0)/mz(1) が
    /// 異符号なら複曲率扱い（C が大きくなる）、同符号なら単曲率扱い
    /// （C が小さくなる）。|M2/M1| が同じでも符号が異なれば C の値が変わる
    /// ことを確認する。
    #[test]
    fn test_c_factor_diff_sign_gt_same_sign_for_same_ratio() {
        let ctx_diff = DesignCtx {
            end_moments_z: Some((100.0, -30.0)),
            ..Default::default()
        };
        let ctx_same = DesignCtx {
            end_moments_z: Some((100.0, 30.0)),
            ..Default::default()
        };
        let c_diff = steel_lateral_buckling_c(&ctx_diff);
        let c_same = steel_lateral_buckling_c(&ctx_same);
        assert!(
            c_diff > c_same,
            "異符号(複曲率)の方が C が大きいはず: c_diff={} c_same={}",
            c_diff,
            c_same
        );
    }

    /// c_direct（正値）があれば、自動算定結果や上限 2.3 を無視してその値を
    /// そのまま採用する（自動算定なら異符号端モーメントで C=2.3 になる状況
    /// でも、c_direct=1.5 が優先されること）。
    #[test]
    fn test_c_factor_direct_input_overrides_auto() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, -100.0)),
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: Some(1.5),
            }),
            ..Default::default()
        };
        let auto = steel_lateral_buckling_c(&ctx);
        assert!((auto - 2.3).abs() < 1e-9, "auto={auto}");
        assert_eq!(steel_c_factor(&ctx, false), 1.5);
        assert_eq!(steel_c_factor(&ctx, true), 1.5);
    }

    /// c_direct がない場合、部分区間（lb_is_partial=true。横補剛により座屈
    /// 区間が部材の部分区間となり区間端モーメント比が不明）では安全側の
    /// C=1.0 とする（自動算定なら異なる値になる状況でも 1.0 に落ちること）。
    #[test]
    fn test_c_factor_partial_lb_without_direct_is_1_0() {
        let ctx = DesignCtx {
            end_moments_z: Some((100.0, -100.0)),
            ..Default::default()
        };
        assert_eq!(steel_c_factor(&ctx, true), 1.0);
        let auto = steel_c_factor(&ctx, false);
        assert!((auto - 2.3).abs() < 1e-9, "auto={auto}");
    }

    /// c_direct ≤ 0 は無効な入力（未入力相当）として無視し、2./3. の規定へ
    /// フォールバックすること。
    #[test]
    fn test_c_factor_direct_non_positive_falls_back() {
        let ctx_zero = DesignCtx {
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: Some(0.0),
            }),
            ..Default::default()
        };
        assert_eq!(steel_c_factor(&ctx_zero, true), 1.0);
        assert_eq!(steel_c_factor(&ctx_zero, false), 1.0);

        let ctx_neg = DesignCtx {
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: Some(-1.5),
            }),
            ..Default::default()
        };
        assert_eq!(steel_c_factor(&ctx_neg, true), 1.0);
    }

    /// C が増加すると fb1 の分母 `C·Λ²` が大きくなり、fb（=fb1 が支配的な
    /// 場合）が増加することを確認する。
    /// lb=5200 は fb1 が fb2 より大きく（fb1 が支配）、かつ長期許容引張
    /// 上限（cap=F/1.5）未満に収まるよう選定した値（fb2, cap によるクランプ
    /// で C の効果が隠れないようにするため）。
    #[test]
    fn test_fb_increases_with_c() {
        let f = 235.0;
        let i = 100.0;
        let h = 500.0;
        let af = 4000.0;
        let lb = 5200.0;
        let fb_c1 = steel_fb_h(f, LoadTerm::Long, lb, i, h, af, 1.0);
        let fb_c23 = steel_fb_h(f, LoadTerm::Long, lb, i, h, af, 2.3);
        let cap = f / 1.5;
        assert!(
            fb_c1 < cap - 1e-6,
            "fb1 が cap でクランプされていない前提: fb_c1={} cap={}",
            fb_c1,
            cap
        );
        assert!(
            fb_c23 > fb_c1,
            "C が大きいほど fb は増えるはず: fb(C=1.0)={} fb(C=2.3)={}",
            fb_c1,
            fb_c23
        );
    }

    /// βf=βw=αw=0 のとき、通常の H形強軸断面係数 Z=Iy/(H/2) に一致する
    /// （`Iy` は [`sepika_core::section_shape::SectionShape::SteelH`] と同一の
    /// `(B・H³ − (B−tw)・(H−2tf)³)/12` 式）。
    #[test]
    fn test_z_with_loss_zero_matches_normal_z() {
        let (h, b, tw, tf): (f64, f64, f64, f64) = (500.0, 200.0, 9.0, 14.0);
        let hw = h - 2.0 * tf;
        let iy = (b * h.powi(3) - (b - tw) * hw.powi(3)) / 12.0;
        let z_expected = iy / (h / 2.0);

        let z_mid = steel_h_z_with_loss(h, b, tw, tf, 0.0, 0.0, 0.0, false);
        let z_end = steel_h_z_with_loss(h, b, tw, tf, 0.0, 0.0, 0.0, true);
        assert!(
            (z_mid - z_expected).abs() < 1e-6,
            "z_mid={} expected={}",
            z_mid,
            z_expected
        );
        assert!(
            (z_end - z_expected).abs() < 1e-6,
            "z_end={} expected={}",
            z_end,
            z_expected
        );
    }

    /// βf=100（フランジ全損）ならフランジ寄与 If' が消え、ウェブ寄与のみが
    /// 残る（Z' = Iw'/(H/2)）。
    #[test]
    fn test_z_with_loss_beta_f_100_removes_flange_contribution() {
        let (h, b, tw, tf): (f64, f64, f64, f64) = (500.0, 200.0, 9.0, 14.0);
        let hw = h - 2.0 * tf;
        let i_w = tw * hw.powi(3) / 12.0;
        let z_expected = i_w / (h / 2.0);

        let z = steel_h_z_with_loss(h, b, tw, tf, 100.0, 0.0, 0.0, false);
        assert!(
            (z - z_expected).abs() < 1e-6,
            "z={} expected={}",
            z,
            z_expected
        );
    }

    /// スカラップ αw の 3 乗効果: is_end=true で αw を大きくすると
    /// Iw''=tw・(hw・(1−αw/100))³/12 は (1−αw/100)³ に比例して小さくなる。
    #[test]
    fn test_z_with_loss_scallop_cubic_effect() {
        let (h, b, tw, tf): (f64, f64, f64, f64) = (500.0, 200.0, 9.0, 14.0);
        let z_no_scallop = steel_h_z_with_loss(h, b, tw, tf, 0.0, 0.0, 0.0, true);
        let z_scallop = steel_h_z_with_loss(h, b, tw, tf, 0.0, 0.0, 50.0, true);

        assert!(
            z_scallop < z_no_scallop,
            "z_scallop={} z_no_scallop={}",
            z_scallop,
            z_no_scallop
        );
        let z_mid_with_alpha = steel_h_z_with_loss(h, b, tw, tf, 0.0, 0.0, 50.0, false);
        let z_mid_no_alpha = steel_h_z_with_loss(h, b, tw, tf, 0.0, 0.0, 0.0, false);
        assert!((z_mid_with_alpha - z_mid_no_alpha).abs() < 1e-9);
    }

    /// resolve_lb: 直接入力があれば位置に応じて始端/中央/終端を返す。
    #[test]
    fn test_resolve_lb_direct_input_priority() {
        let direct = Some((1000.0, 2000.0, 3000.0));
        assert_eq!(resolve_lb(0.0, 9000.0, direct, Some(2)), 1000.0);
        assert_eq!(resolve_lb(0.24, 9000.0, direct, Some(2)), 1000.0);
        assert_eq!(resolve_lb(0.25, 9000.0, direct, Some(2)), 2000.0);
        assert_eq!(resolve_lb(0.5, 9000.0, direct, Some(2)), 2000.0);
        assert_eq!(resolve_lb(0.74, 9000.0, direct, Some(2)), 2000.0);
        assert_eq!(resolve_lb(0.75, 9000.0, direct, Some(2)), 3000.0);
        assert_eq!(resolve_lb(1.0, 9000.0, direct, Some(2)), 3000.0);
    }

    /// resolve_lb: 直接入力がなく等間隔横補剛の本数があれば L/(n+1)。
    #[test]
    fn test_resolve_lb_brace_count_when_no_direct() {
        assert_eq!(resolve_lb(0.5, 9000.0, None, Some(2)), 3000.0);
        assert_eq!(resolve_lb(0.0, 9000.0, None, Some(2)), 3000.0);
    }

    /// resolve_lb: どちらもなければ部材長そのまま。
    #[test]
    fn test_resolve_lb_falls_back_to_length() {
        assert_eq!(resolve_lb(0.5, 9000.0, None, None), 9000.0);
    }
}
