//! 鉄筋コンクリート造梁・柱の**終局せん断強度（塑性理論式）**および
//! **付着割裂による終局せん断耐力**（靭性保証型耐震設計指針）。
//!
//! 終局検定の
//! 「終局強度型設計指針」を選択した場合の終局せん断強度 `Qsu`（塑性理論式）と
//! 付着割裂によって決定するせん断耐力 `Qbu` を算定する。
//!
//! 既存の [`sepika_core::rc_capacity::rc_qsu_simple`]（荒川mean式）は部材ランク
//! 自動判定・プッシュオーバーのせん断降伏判定用の略算式であり、本モジュールの
//! 塑性理論式（トラス機構＋アーチ機構）とは定式が異なる別物である。

/// コンクリート強度Fc `[N/mm²]` から塑性理論式の有効係数ν0を返す。下限は0。
pub fn plastic_nu0(fc: f64) -> f64 {
    (0.7 - fc / 200.0).max(0.0)
}

/// Fc `[N/mm²]`・ヒンジ回転角Rp `[rad]` から塑性理論式の有効係数νを返す。
/// Rpが0以下ならν0を用い、結果の下限は0。
pub fn plastic_nu(fc: f64, rp: f64) -> f64 {
    plastic_nu_from_nu0(plastic_nu0(fc), rp)
}

/// ν0とヒンジ回転角Rp `[rad]` から塑性理論式の有効係数νを返す。
/// Rpが0以下ならν0を用い、結果の下限は0。
pub fn plastic_nu_from_nu0(nu0: f64, rp: f64) -> f64 {
    let factor = if rp <= 0.0 {
        1.0
    } else if rp <= 0.05 {
        (1.0 - 15.0 * rp).max(0.25)
    } else {
        0.25
    };
    (factor * nu0).max(0.0)
}

/// ヒンジ回転角Rp `[rad]` から塑性理論式のトラス角度係数cotφを返す。
/// Rpが0以下なら2、結果の下限は1。
pub fn plastic_cot_phi(rp: f64) -> f64 {
    if rp <= 0.0 {
        2.0
    } else if rp <= 0.02 {
        (2.0 - 50.0 * rp).max(1.0)
    } else {
        1.0
    }
}

/// 内法長さL・部材せいD `[mm]` からアーチ機構の係数k1を返す。
/// Dが0以下、またはLが負なら0。
pub fn plastic_k1(l_clear: f64, d_full: f64) -> f64 {
    if d_full <= 0.0 || l_clear < 0.0 {
        return 0.0;
    }
    let ld = l_clear / d_full;
    ((ld * ld + 1.0).sqrt() - ld) / 2.0
}

/// 補強筋比pw、σwy・Fc `[N/mm²]`、有効係数νからトラス寄与率k2を返す。
/// pw・σwyの負値は0、結果の上限は1。νFcが0以下なら1。
pub fn plastic_k2(pw: f64, sigma_wy: f64, nu: f64, fc: f64) -> f64 {
    let denom = nu * fc;
    if denom <= 0.0 {
        return 1.0;
    }
    (2.0 * pw.max(0.0) * sigma_wy.max(0.0) / denom).min(1.0)
}

/// 塑性理論式による終局せん断強度 `Qsu` の算定入力（靭性保証型耐震設計指針）。
#[derive(Clone, Copy, Debug)]
pub struct RcPlasticShearInput {
    /// 部材幅 b `[mm]`。
    pub b: f64,
    /// 部材せい D `[mm]`。
    pub d_full: f64,
    /// 主筋の重心間距離 jt `[mm]`（トラス機構。通常 7/8·d 相当）。
    pub jt: f64,
    /// せん断補強筋比 pw（小数、= aw·組数/(b·ピッチ)）。
    pub pw: f64,
    /// せん断補強筋の降伏強度算定用強度 σwy [N/mm²]。
    pub sigma_wy: f64,
    /// 内法長さ L `[mm]`（アーチ機構の k1 に用いる）。
    pub l_clear: f64,
    /// コンクリートの設計基準強度 Fc [N/mm²]。
    pub fc: f64,
    /// 終局限界状態でのヒンジ領域の回転角 Rp `[rad]`（ν・cotφ に用いる）。
    /// 塑性化前の終局強度を評価する場合は 0 を与える（cotφ=2.0, ν=ν0）。
    pub rp: f64,
    /// 軽量コンクリートを使用する場合 true（せん断終局耐力を 0.9 倍に低減）。
    pub lightweight: bool,
}

/// 塑性理論式の終局せん断強度Qsu `[N]` を返す。
/// 軽量コンクリートは0.9倍。b・D・jt・Fcのいずれかが0以下なら0。
pub fn rc_shear_qsu_plastic(inp: &RcPlasticShearInput) -> f64 {
    if inp.b <= 0.0 || inp.d_full <= 0.0 || inp.jt <= 0.0 || inp.fc <= 0.0 {
        return 0.0;
    }
    let nu0 = plastic_nu0(inp.fc);
    let nu = plastic_nu_from_nu0(nu0, inp.rp);
    let cot_phi = plastic_cot_phi(inp.rp);
    let k1 = plastic_k1(inp.l_clear, inp.d_full);

    let pw_sigma = (inp.pw.max(0.0) * inp.sigma_wy.max(0.0)).min(nu * inp.fc / 2.0);
    let k2 = plastic_k2(inp.pw, inp.sigma_wy, nu, inp.fc);

    let truss = inp.b * inp.jt * pw_sigma * cot_phi;
    let arch = k1 * (1.0 - k2) * inp.b * inp.d_full * nu * inp.fc;
    let qsu = (truss + arch).max(0.0);

    if inp.lightweight {
        0.9 * qsu
    } else {
        qsu
    }
}

/// 付着割裂による終局せん断耐力 `Qbu` の算定入力（靭性保証型耐震設計指針）。
#[derive(Clone, Copy, Debug)]
pub struct RcBondSplitInput {
    /// 部材幅 b `[mm]`。
    pub b: f64,
    /// 部材せい D `[mm]`。
    pub d_full: f64,
    /// 主筋の重心間距離 jt `[mm]`。
    pub jt: f64,
    /// 1 段目主筋の付着信頼強度 τbu [N/mm²]（[`bond_reliable_strength_deformed`]）。
    pub tau_bu: f64,
    /// 引張鉄筋の周長和 Σφ `[mm]`（2 段筋・寄筋を含める）。
    pub sum_phi: f64,
    /// 内法長さ L `[mm]`（アーチ機構の k1 に用いる）。
    pub l_clear: f64,
    /// コンクリートの設計基準強度 Fc [N/mm²]。
    pub fc: f64,
    /// 終局限界状態でのヒンジ領域の回転角 Rp `[rad]`（ν に用いる）。
    pub rp: f64,
    /// 軽量コンクリートを使用する場合 true（0.9 倍低減）。
    pub lightweight: bool,
}

/// 付着割裂による終局せん断耐力Qbu `[N]` を返す。
/// 軽量コンクリートは0.9倍。b・D・jt・Fcのいずれかが0以下なら0。
pub fn rc_shear_qbu_bond(inp: &RcBondSplitInput) -> f64 {
    if inp.b <= 0.0 || inp.d_full <= 0.0 || inp.jt <= 0.0 || inp.fc <= 0.0 {
        return 0.0;
    }
    let nu = plastic_nu(inp.fc, inp.rp);
    let k1 = plastic_k1(inp.l_clear, inp.d_full);
    let denom = inp.b * nu * inp.fc;
    let k3 = if denom > 0.0 {
        (2.0 * inp.tau_bu.max(0.0) * inp.sum_phi.max(0.0) / denom).min(1.0)
    } else {
        1.0
    };
    let bond = inp.jt * inp.tau_bu.max(0.0) * inp.sum_phi.max(0.0);
    let arch = k1 * (1.0 - k3) * inp.b * inp.d_full * nu * inp.fc;
    let qbu = (bond + arch).max(0.0);
    if inp.lightweight {
        0.9 * qbu
    } else {
        qbu
    }
}

/// 異径鉄筋（普通の異形鉄筋）の 1 段目主筋の付着信頼強度 τbu [N/mm²] の
/// 算定入力。
#[derive(Clone, Copy, Debug)]
pub struct BondStrengthInput {
    /// コンクリートの設計基準強度 Fc [N/mm²]。
    pub fc: f64,
    /// 部材幅 b `[mm]`。
    pub b: f64,
    /// 1 段目主筋径（呼び名）db1 `[mm]`。
    pub db1: f64,
    /// 外側一列の引張鉄筋の本数 N。
    pub n_bars: u32,
    /// 主筋の側面かぶり Cs `[mm]`。
    pub cover_side: f64,
    /// 主筋の底面（上面）かぶり Cb `[mm]`。
    pub cover_bottom: f64,
    /// せん断補強筋 1 組の断面積 aw `[mm²]`（kst 用）。
    pub hoop_area: f64,
    /// せん断補強筋ピッチ x `[mm]`（kst 用）。
    pub hoop_pitch: f64,
    /// せん断補強筋比 pw（小数、kst 用）。
    pub pw: f64,
    /// 梁の上端主筋の場合 true（付着強度低減係数 αt = 0.75 − Fc/400）。
    pub top_bar: bool,
}

/// 付着割裂線長さ比biを返す。寸法はmm。
/// db1が0以下、または主筋本数Nが0なら0。結果の下限は0。
pub fn bond_split_ratio(b: f64, db1: f64, n_bars: u32, cover_side: f64, cover_bottom: f64) -> f64 {
    if db1 <= 0.0 || n_bars == 0 {
        return 0.0;
    }
    let n = n_bars as f64;
    let c_min = cover_side.min(cover_bottom);
    let bvi = 3.0_f64.sqrt() * (2.0 * c_min / db1 + 1.0);
    let bci = 2.0_f64.sqrt() * ((cover_side + cover_bottom) / db1 - 1.0);
    let bsi = b / (n * db1) - 1.0;
    bvi.min(bci).min(bsi).max(0.0)
}

/// 異形鉄筋の1段目主筋の付着信頼強度τbu `[N/mm²]` を返す。
/// Fc・b・db1のいずれかが0以下、またはNが0なら0。横補強筋ピッチが0以下ならその寄与は0。
pub fn bond_reliable_strength_deformed(inp: &BondStrengthInput) -> f64 {
    if inp.fc <= 0.0 || inp.b <= 0.0 || inp.db1 <= 0.0 || inp.n_bars == 0 {
        return 0.0;
    }
    let b1 = bond_split_ratio(inp.b, inp.db1, inp.n_bars, inp.cover_side, inp.cover_bottom);
    let alpha_t = if inp.top_bar {
        (0.75 - inp.fc / 400.0).max(0.0)
    } else {
        1.0
    };
    let kst = if inp.hoop_pitch > 0.0 {
        140.0 * inp.hoop_area.max(0.0) / (inp.db1 * inp.hoop_pitch)
    } else {
        0.0
    };
    (alpha_t * ((0.085 * b1 + 0.10) * inp.fc.sqrt() + kst)).max(0.0)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plastic_nu0_and_nu() {
        // ν0 = 0.7 − 24/200 = 0.58。
        assert!((plastic_nu0(24.0) - (0.7 - 24.0 / 200.0)).abs() < 1e-12);
        assert!((plastic_nu(24.0, 0.0) - plastic_nu0(24.0)).abs() < 1e-12);
        assert!((plastic_nu(24.0, 0.02) - 0.7 * plastic_nu0(24.0)).abs() < 1e-12);
        assert!((plastic_nu(24.0, 0.05) - 0.25 * plastic_nu0(24.0)).abs() < 1e-12);
        assert!((plastic_nu(24.0, 0.1) - 0.25 * plastic_nu0(24.0)).abs() < 1e-12);
    }

    #[test]
    fn test_plastic_cot_phi() {
        assert!((plastic_cot_phi(0.0) - 2.0).abs() < 1e-12);
        assert!((plastic_cot_phi(0.01) - (2.0 - 50.0 * 0.01)).abs() < 1e-12);
        assert!((plastic_cot_phi(0.02) - 1.0).abs() < 1e-12);
        assert!((plastic_cot_phi(0.05) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_plastic_k1_matches_handcalc() {
        // L/D=2.0 → k1 = (√5 − 2)/2。
        let k1 = plastic_k1(1200.0, 600.0);
        let hand = ((2.0_f64 * 2.0 + 1.0).sqrt() - 2.0) / 2.0;
        assert!((k1 - hand).abs() < 1e-12, "k1={k1} vs {hand}");
        assert_eq!(plastic_k1(1200.0, 0.0), 0.0);
    }

    #[test]
    fn test_plastic_k2_capped_at_one() {
        let nu = plastic_nu0(24.0);
        assert!((plastic_k2(0.1, 1000.0, nu, 24.0) - 1.0).abs() < 1e-12);
        let k2 = plastic_k2(0.003, 295.0, nu, 24.0);
        assert!((k2 - (2.0 * 0.003 * 295.0 / (nu * 24.0))).abs() < 1e-12);
        assert!(k2 < 1.0);
    }

    fn sample_qsu() -> RcPlasticShearInput {
        RcPlasticShearInput {
            b: 400.0,
            d_full: 600.0,
            jt: 7.0 * 530.0 / 8.0,
            pw: 0.003,
            sigma_wy: 295.0,
            l_clear: 3000.0,
            fc: 24.0,
            rp: 0.0,
            lightweight: false,
        }
    }

    #[test]
    fn test_rc_shear_qsu_plastic_matches_handcalc() {
        let inp = sample_qsu();
        let qsu = rc_shear_qsu_plastic(&inp);
        // 手計算（Rp=0: cotφ=2.0, ν=ν0）。
        let nu = 0.7 - 24.0 / 200.0;
        let cot = 2.0_f64;
        let ld: f64 = 3000.0 / 600.0;
        let k1 = ((ld * ld + 1.0).sqrt() - ld) / 2.0;
        let pw_sigma = (0.003_f64 * 295.0).min(nu * 24.0 / 2.0);
        let k2 = (2.0 * 0.003 * 295.0 / (nu * 24.0)).min(1.0);
        let truss = 400.0 * (7.0 * 530.0 / 8.0) * pw_sigma * cot;
        let arch = k1 * (1.0 - k2) * 400.0 * 600.0 * nu * 24.0;
        let hand = truss + arch;
        assert!((qsu - hand).abs() < 1e-3, "Qsu={qsu} vs {hand}");
        assert!(qsu > 0.0);
    }

    #[test]
    fn test_rc_shear_qsu_plastic_rp_reduces() {
        let mut inp = sample_qsu();
        let q0 = rc_shear_qsu_plastic(&inp);
        inp.rp = 0.03;
        let q_rp = rc_shear_qsu_plastic(&inp);
        assert!(q_rp < q0, "Qsu(Rp=0.03)={q_rp} should be < Qsu(0)={q0}");
    }

    #[test]
    fn test_rc_shear_qsu_plastic_lightweight_09() {
        let mut inp = sample_qsu();
        let q_std = rc_shear_qsu_plastic(&inp);
        inp.lightweight = true;
        let q_lw = rc_shear_qsu_plastic(&inp);
        assert!(
            (q_lw - 0.9 * q_std).abs() < 1e-6,
            "lw={q_lw} vs {}",
            0.9 * q_std
        );
    }

    #[test]
    fn test_rc_shear_qsu_plastic_invalid_zero() {
        let mut bad = sample_qsu();
        bad.fc = 0.0;
        assert_eq!(rc_shear_qsu_plastic(&bad), 0.0);
        bad = sample_qsu();
        bad.b = 0.0;
        assert_eq!(rc_shear_qsu_plastic(&bad), 0.0);
        bad = sample_qsu();
        bad.jt = 0.0;
        assert_eq!(rc_shear_qsu_plastic(&bad), 0.0);
    }

    #[test]
    fn test_rc_shear_qsu_plastic_pw_sigma_capped() {
        // pw·σwy ≤ ν·Fc/2 の制約: 過大な補強で頭打ちになり、k2=1（アーチ項 0）。
        let mut inp = sample_qsu();
        inp.pw = 0.05;
        inp.sigma_wy = 1275.0;
        let qsu = rc_shear_qsu_plastic(&inp);
        // トラス項のみ（アーチ項 0）: Qsu = b·jt·(ν·Fc/2)·cotφ。
        let nu = 0.7 - 24.0 / 200.0;
        let truss = 400.0 * (7.0 * 530.0 / 8.0) * (nu * 24.0 / 2.0) * 2.0;
        assert!(
            (qsu - truss).abs() < 1e-3,
            "Qsu={qsu} vs truss-only {truss}"
        );
    }

    #[test]
    fn test_bond_split_ratio_matches_handcalc() {
        // b=400, db1=25, N=4, Cs=Cb=40。
        let b1 = bond_split_ratio(400.0, 25.0, 4, 40.0, 40.0);
        let bvi = 3.0_f64.sqrt() * (2.0 * 40.0 / 25.0 + 1.0);
        let bci = 2.0_f64.sqrt() * ((40.0 + 40.0) / 25.0 - 1.0);
        let bsi = 400.0 / (4.0 * 25.0) - 1.0;
        let hand = bvi.min(bci).min(bsi);
        assert!((b1 - hand).abs() < 1e-9, "b1={b1} vs {hand}");
        assert_eq!(bond_split_ratio(400.0, 0.0, 4, 40.0, 40.0), 0.0);
    }

    fn sample_bond() -> BondStrengthInput {
        BondStrengthInput {
            fc: 24.0,
            b: 400.0,
            db1: 25.0,
            n_bars: 4,
            cover_side: 40.0,
            cover_bottom: 40.0,
            hoop_area: 2.0 * std::f64::consts::PI / 4.0 * 10.0 * 10.0,
            hoop_pitch: 100.0,
            pw: 0.003,
            top_bar: false,
        }
    }

    #[test]
    fn test_bond_reliable_strength_matches_handcalc() {
        let inp = sample_bond();
        let tau = bond_reliable_strength_deformed(&inp);
        let b1 = bond_split_ratio(400.0, 25.0, 4, 40.0, 40.0);
        let kst = 140.0 * (2.0 * std::f64::consts::PI / 4.0 * 100.0) / (25.0 * 100.0);
        let hand = 1.0 * ((0.085 * b1 + 0.10) * 24.0_f64.sqrt() + kst);
        assert!((tau - hand).abs() < 1e-6, "τbu={tau} vs {hand}");
        assert!(tau > 0.0);
    }

    #[test]
    fn test_bond_reliable_strength_top_bar_reduced() {
        let mut inp = sample_bond();
        let tau_other = bond_reliable_strength_deformed(&inp);
        inp.top_bar = true;
        let tau_top = bond_reliable_strength_deformed(&inp);
        assert!(tau_top < tau_other, "top={tau_top} other={tau_other}");
    }

    #[test]
    fn test_rc_shear_qbu_bond_matches_handcalc() {
        let tau_bu = bond_reliable_strength_deformed(&sample_bond());
        let sum_phi = 4.0 * std::f64::consts::PI * 25.0;
        let inp = RcBondSplitInput {
            b: 400.0,
            d_full: 600.0,
            jt: 7.0 * 530.0 / 8.0,
            tau_bu,
            sum_phi,
            l_clear: 3000.0,
            fc: 24.0,
            rp: 0.0,
            lightweight: false,
        };
        let qbu = rc_shear_qbu_bond(&inp);
        let nu = 0.7 - 24.0 / 200.0;
        let ld: f64 = 3000.0 / 600.0;
        let k1 = ((ld * ld + 1.0).sqrt() - ld) / 2.0;
        let k3 = (2.0 * tau_bu * sum_phi / (400.0 * nu * 24.0)).min(1.0);
        let bond = (7.0 * 530.0 / 8.0) * tau_bu * sum_phi;
        let arch = k1 * (1.0 - k3) * 400.0 * 600.0 * nu * 24.0;
        let hand = bond + arch;
        assert!((qbu - hand).abs() < 1e-3, "Qbu={qbu} vs {hand}");
        assert!(qbu > 0.0);
    }

    #[test]
    fn test_rc_shear_qbu_bond_invalid_zero() {
        let inp = RcBondSplitInput {
            b: 0.0,
            d_full: 600.0,
            jt: 400.0,
            tau_bu: 3.0,
            sum_phi: 300.0,
            l_clear: 3000.0,
            fc: 24.0,
            rp: 0.0,
            lightweight: false,
        };
        assert_eq!(rc_shear_qbu_bond(&inp), 0.0);
    }
}
