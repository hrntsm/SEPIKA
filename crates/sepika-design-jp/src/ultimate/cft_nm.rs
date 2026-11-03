//! コンクリート充填鋼管（CFT）柱の**N-M 相互作用（曲げを伴う終局耐力）**
//! （CFT 指針に基づく）。
//!
//! [`super::cft`] が軸方向終局耐力（Ncu/Ntu）を扱うのに対し、本モジュールは軸方向力と
//! 曲げモーメントを同時に受ける柱の終局曲げ耐力 `Mu(N)` を算定する。
//! - **短柱**（[`cft_short_column_mu`]）: 中立軸位置をパラメータとする耐力曲線を軸力 N に
//!   整合させる。角形は鋼管の全断面で応力ブロックを積分する。
//! - **中柱・長柱**（[`cft_long_medium_column_mu`]）: 座屈による曲げ低減 R=(1−cNcu/Nk) を
//!   考慮し、コンクリート放物線 cMu＋鋼管の曲げ耐力（低減後）を重ね合わせる。

use sepika_core::section_shape::RoundedRectangle;
use std::f64::consts::PI;

/// CFT 短柱の N-M 相互作用の算定入力。
/// 角形は同厚鋼管の外内領域を用いる。寸法・材料は上位入口で検証する。
#[derive(Clone, Copy, Debug)]
pub struct CftBendingInput {
    /// 円形断面なら true（角型なら false）。
    pub circular: bool,
    /// 角形鋼管の外角半径 [mm]。円形では使用しない。
    pub corner_r: f64,
    /// 鋼管のせい D `[mm]`（円形は外径）。
    pub d_steel: f64,
    /// 鋼管の幅 B `[mm]`（円形は外径と同値）。
    pub b_steel: f64,
    /// コンクリートのせい cD `[mm]`（= D − 2t）。
    pub c_d: f64,
    /// コンクリートの幅 cB `[mm]`（円形は cD と同値）。
    pub c_b: f64,
    /// 鋼管の板厚 t `[mm]`。
    pub t: f64,
    /// コンクリートの設計基準強度 Fc [N/mm²]。
    pub fc: f64,
    /// 鋼管の降伏強さ Fy [N/mm²]。
    pub fy: f64,
}

fn angular_regions(inp: &CftBendingInput) -> Result<(RoundedRectangle, RoundedRectangle), String> {
    Ok((
        RoundedRectangle::new(inp.d_steel, inp.b_steel, inp.corner_r)?,
        RoundedRectangle::new(inp.c_d, inp.c_b, (inp.corner_r - inp.t).max(0.0))?,
    ))
}

fn angular_material_forces(inp: &CftBendingInput, z: f64) -> (f64, f64, f64, f64) {
    let (outer, core) = angular_regions(inp).expect("検証済みの角形CFT幾何");
    let (ao, qo, _) = outer.above(z);
    let (ac, qc, _) = core.above(z);
    let sa = outer.properties().area - core.properties().area;
    (
        ac * inp.fc,
        qc * inp.fc,
        (2.0 * (ao - ac) - sa) * inp.fy,
        2.0 * (qo - qc) * inp.fy,
    )
}

fn angular_nu_mu(inp: &CftBendingInput, xn: f64) -> (f64, f64) {
    let (cn, cm, sn, sm) = angular_material_forces(inp, inp.c_d / 2.0 - xn);
    (cn + sn, cm + sm)
}

/// 円形短柱: パラメータ角 `θ`（[0, π]、`θ = cos⁻¹(1 − 2xn/cD)`）における (Nu, Mu)。
///
/// ```text
/// cσcB = Fc + 0.78·(2t/(D−2t))·Fy,  r1 = cD/2,  r2 = (D−t)/2
/// cNu = r1²·(θ − sinθcosθ)·cσcB,     cMu = (2/3)·r1³·sin³θ·cσcB
/// sNu = 2·r2·t·(β1·θ − β2·(θ−π))·Fy,  sMu = 2·r2²·t·(β1 − β2)·sinθ·Fy
/// β1 = 0.89,  β2 = −1.08
/// ```
fn circular_nu_mu(inp: &CftBendingInput, theta: f64) -> (f64, f64) {
    let r1 = inp.c_d / 2.0;
    let r2 = (inp.d_steel - inp.t) / 2.0;
    let denom = inp.d_steel - 2.0 * inp.t;
    let c_sigma = if denom > 0.0 {
        inp.fc + 0.78 * (2.0 * inp.t / denom) * inp.fy
    } else {
        inp.fc
    };
    let (b1, b2) = (0.89_f64, -1.08_f64);
    let c_nu = r1 * r1 * (theta - theta.sin() * theta.cos()) * c_sigma;
    let c_mu = (2.0 / 3.0) * r1.powi(3) * theta.sin().powi(3) * c_sigma;
    let s_nu = 2.0 * r2 * inp.t * (b1 * theta - b2 * (theta - PI)) * inp.fy;
    let s_mu = 2.0 * r2 * r2 * inp.t * (b1 - b2) * theta.sin() * inp.fy;
    (c_nu + s_nu, c_mu + s_mu)
}

/// 短柱の Mu `[N·mm]`。軸力 `[N]` は圧縮正。角形は全材料領域の応力ブロックを用いる。
/// 円形のコア外は `ncu1`・`ntu` の端点との直線補間。不正な非正寸法・強度は 0。
pub fn cft_short_column_mu(inp: &CftBendingInput, n_design: f64, ncu1: f64, ntu: f64) -> f64 {
    if inp.d_steel <= 0.0 || inp.c_d <= 0.0 || inp.t <= 0.0 || inp.fc <= 0.0 || inp.fy <= 0.0 {
        return 0.0;
    }
    if !inp.circular {
        let Ok((outer, core)) = angular_regions(inp) else {
            return 0.0;
        };
        let sa = outer.properties().area - core.properties().area;
        let nc = core.properties().area * inp.fc + sa * inp.fy;
        if n_design <= -sa * inp.fy || n_design >= nc {
            return 0.0;
        }
        let mut lo = -inp.t;
        let mut hi = inp.c_d + inp.t;
        for _ in 0..80 {
            let mid = (lo + hi) / 2.0;
            if angular_nu_mu(inp, mid).0 < n_design {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        return angular_nu_mu(inp, (lo + hi) / 2.0).1.max(0.0);
    }
    let p_max = PI;
    let (n_lo, m_lo) = circular_nu_mu(inp, 0.0);
    let (n_hi, m_hi) = circular_nu_mu(inp, p_max);

    if n_design >= n_hi {
        if ncu1 > n_hi {
            (m_hi * (ncu1 - n_design) / (ncu1 - n_hi)).max(0.0)
        } else {
            0.0
        }
    } else if n_design <= n_lo {
        let n_tension = -ntu;
        if n_lo > n_tension {
            (m_lo * (n_design - n_tension) / (n_lo - n_tension)).max(0.0)
        } else {
            0.0
        }
    } else {
        let mut lo = 0.0;
        let mut hi = p_max;
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if circular_nu_mu(inp, mid).0 < n_design {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (_, mu) = circular_nu_mu(inp, 0.5 * (lo + hi));
        mu.max(0.0)
    }
}

/// 座屈補助軸力 `Nk = π²·(cE'·cI/5 + sE·sI)/lk²`（CFT 指針、長柱）。
/// `cE' = (3.32·√Fc + 6.90)×10³`。`lk ≤ 0` は `f64::INFINITY`（座屈しない）。
pub fn cft_nk(c_inertia: f64, s_inertia: f64, s_young: f64, fc: f64, lk: f64) -> f64 {
    if lk <= 0.0 {
        return f64::INFINITY;
    }
    let c_young = (3.32 * fc.max(0.0).sqrt() + 6.90) * 1.0e3;
    PI * PI * (c_young * c_inertia.max(0.0) / 5.0 + s_young.max(0.0) * s_inertia.max(0.0))
        / (lk * lk)
}

/// CFT **中柱・長柱**の N-M 相互作用の算定入力（CFT 指針、中柱・長柱）。
#[derive(Clone, Copy, Debug)]
pub struct CftLongMediumInput {
    /// 短柱 N-M と同じ断面諸元。
    pub bending: CftBendingInput,
    /// 長柱なら true（座屈長さが断面せいの 12 倍超）、中柱なら false。
    pub is_long: bool,
    /// 充填コンクリートの座屈軸耐力 cNcr `[N]`（[`super::cft::cft_concrete_buckling_axial`]）。
    pub c_ncr: f64,
    /// 充填コンクリートの規準化細長比 cλ1（[`super::cft::cft_concrete_slenderness`]）。
    pub c_lambda1: f64,
    /// 座屈補助軸力 Nk `[N]`（[`cft_nk`]）。
    pub nk: f64,
    /// 同一検討軸・座屈長さの鋼管座屈軸耐力 sNcr `[N]`。
    pub s_ncr: f64,
    /// 同一検討軸・座屈長さの鋼管単体オイラー荷重 sNE `[N]`。
    pub s_ne: f64,
    /// 当該分類の軸圧縮終局耐力 `[N]`（中柱・長方形長柱の線形補間に用いる）。
    pub ncu_axial: f64,
    /// 軸引張終局耐力の大きさ Ntu `[N]`。
    pub ntu: f64,
}

/// 中柱・長柱の終局曲げ耐力 `[N·mm]`。軸力 `[N]` は圧縮正。
/// 正方形長柱の Case2 は鋼管の座屈・オイラー荷重を別々に用いる。
/// Case2 の無効荷重・非有限値・純圧縮限界超過は理由付きエラー。
pub fn cft_long_medium_column_mu(inp: &CftLongMediumInput, n_design: f64) -> Result<f64, String> {
    let b = &inp.bending;
    if !n_design.is_finite()
        || ![b.d_steel, b.b_steel, b.c_d, b.c_b, b.t, b.fc, b.fy]
            .into_iter()
            .all(|v| v.is_finite() && v > 0.0)
        || 2.0 * b.t >= b.d_steel
        || 2.0 * b.t >= b.b_steel
        || !inp.c_ncr.is_finite()
        || inp.c_ncr < 0.0
    {
        return Err("CFT 終局 N–M の断面寸法・材料強度が不正です".into());
    }
    let r_factor = if inp.nk.is_finite() && inp.nk > 0.0 {
        (1.0 - inp.c_ncr / inp.nk).max(0.0)
    } else {
        1.0
    };
    let s_mu0 = if b.circular {
        let r2 = (b.d_steel - b.t) / 2.0;
        4.0 * r2 * r2 * b.t * b.fy
    } else {
        {
            let (outer, core) = angular_regions(b)?;
            (outer.properties().plastic_modulus_strong - core.properties().plastic_modulus_strong)
                * b.fy
        }
    };
    if inp.is_long && !b.circular && b.d_steel == b.b_steel && n_design > inp.c_ncr {
        return cft_square_long_case2_mu(inp.c_ncr, inp.s_ncr, inp.s_ne, inp.nk, s_mu0, n_design);
    }
    let cb = 0.923 - 0.0045 * b.fc;
    let cmmax0 = if b.circular {
        b.fc * b.c_d.powi(3) / 12.0
    } else {
        b.fc * angular_regions(b)?.1.properties().plastic_modulus_strong / 2.0
    };
    let denom_cb = cb + inp.c_lambda1 * inp.c_lambda1;
    let cmmax = if denom_cb > 0.0 {
        cb / denom_cb * cmmax0
    } else {
        0.0
    };
    let c_ncr09 = 0.9 * inp.c_ncr;
    let cmu = |cn: f64| -> f64 {
        if c_ncr09 <= 0.0 {
            return 0.0;
        }
        (4.0 * cn / c_ncr09 * (1.0 - cn / c_ncr09) * cmmax).max(0.0)
    };

    let mu = if n_design < 0.0 {
        let mu0 = s_mu0 * r_factor;
        let n_tension = -inp.ntu;
        if 0.0 > n_tension {
            (mu0 * (n_design - n_tension) / (0.0 - n_tension)).max(0.0)
        } else {
            0.0
        }
    } else if n_design <= inp.c_ncr {
        (cmu(n_design) + s_mu0 * r_factor).max(0.0)
    } else if inp.is_long && b.circular {
        let r2 = (b.d_steel - b.t) / 2.0;
        let denom = 4.0 * r2 * b.t * b.fy;
        if denom <= 0.0 {
            return Ok(0.0);
        }
        let theta = (PI / 2.0 + (n_design - inp.c_ncr) / denom).min(PI);
        (4.0 * r2 * r2 * b.t * theta.sin() * b.fy * r_factor).max(0.0)
    } else {
        if inp.ncu_axial > inp.c_ncr {
            (s_mu0 * (1.0 - (n_design - inp.c_ncr) / (inp.ncu_axial - inp.c_ncr)) * r_factor)
                .max(0.0)
        } else {
            return Err(
                "CFT 中長柱の軸圧縮耐力がコンクリート座屈耐力以下のため高軸力曲げを算定できません"
                    .into(),
            );
        }
    };
    if !mu.is_finite() {
        return Err("CFT 中長柱の曲げ耐力が有限値として算定できません".into());
    }
    Ok(mu)
}

fn cft_square_long_case2_mu(
    c_ncr: f64,
    s_ncr: f64,
    s_ne: f64,
    nk: f64,
    s_mu0: f64,
    n: f64,
) -> Result<f64, String> {
    if ![c_ncr, s_ncr, s_ne, nk, s_mu0, n]
        .into_iter()
        .all(|v| v.is_finite() && v > 0.0)
        || s_ncr > s_ne
        || c_ncr >= nk
    {
        return Err(
            "正方形 CFT 長柱 Case2 の座屈荷重・曲げ耐力が無効です（sNcr≤sNE、cNcr<Nk が必要）"
                .into(),
        );
    }
    let delta_n = n - c_ncr;
    if delta_n <= 0.0 {
        return Err("正方形 CFT 長柱 Case2 は N>cNcr が必要です".into());
    }
    if n > c_ncr + s_ncr {
        return Err("正方形 CFT 長柱の軸圧縮終局耐力を超えています（軸NG）".into());
    }
    if n == c_ncr + s_ncr {
        return Ok(0.0);
    }
    Ok((1.0 - delta_n / s_ncr) * (1.0 - delta_n / s_ne) * (1.0 - c_ncr / nk) * s_mu0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn independent_compression(h: f64, b: f64, r: f64, z: f64) -> (f64, f64) {
        let start = (z / (h / 2.0)).clamp(-1.0, 1.0).asin();
        let steps = 65_536;
        let dt = (PI / 2.0 - start) / steps as f64;
        let mut a = 0.0;
        let mut q = 0.0;
        for k in 0..steps {
            let theta = start + (k as f64 + 0.5) * dt;
            let y = h / 2.0 * theta.sin();
            let edge = h / 2.0 - y.abs();
            let w = if edge >= r {
                b
            } else {
                b - 2.0 * r + 2.0 * (2.0 * r * edge - edge * edge).max(0.0).sqrt()
            };
            let da = w * h / 2.0 * theta.cos() * dt;
            a += da;
            q += da * y;
        }
        (a, q)
    }

    #[test]
    fn angular_material_blocks_match_independent_integration_and_axial_equilibrium() {
        for (d, b) in [(400.0, 400.0), (600.0, 400.0), (400.0, 600.0)] {
            for r in [0.0_f64, 8.0, 16.0, 40.0] {
                let mut inp = box_input();
                inp.d_steel = d;
                inp.b_steel = b;
                inp.t = 16.0;
                inp.c_d = d - 32.0;
                inp.c_b = b - 32.0;
                inp.corner_r = r;
                let ri = (r - 16.0).max(0.0);
                let sa = d * b - (4.0 - PI) * r * r - (inp.c_d * inp.c_b - (4.0 - PI) * ri * ri);
                for z in [
                    -d / 2.0,
                    -d / 2.0 + 1.0,
                    -d / 2.0 + r,
                    -inp.c_d / 2.0,
                    -inp.c_d / 2.0 + ri,
                    0.0,
                    inp.c_d / 2.0 - ri,
                    inp.c_d / 2.0,
                    d / 2.0 - r,
                    d / 2.0 - 1.0,
                    d / 2.0,
                ] {
                    let (ao, qo) = independent_compression(d, b, r, z);
                    let (ac, qc) = independent_compression(inp.c_d, inp.c_b, ri, z);
                    let expected = [
                        ac * inp.fc,
                        qc * inp.fc,
                        (2.0 * (ao - ac) - sa) * inp.fy,
                        2.0 * (qo - qc) * inp.fy,
                    ];
                    let (cn, cm, sn, sm) = angular_material_forces(&inp, z);
                    for (j, (v, e)) in [cn, cm, sn, sm].into_iter().zip(expected).enumerate() {
                        let scale = if j % 2 == 0 {
                            inp.fy * d * b
                        } else {
                            inp.fy * d * b * d
                        };
                        assert!(
                            (v - e).abs() < 2e-9 * scale,
                            "D={d},B={b},R={r},z={z}: {v} != {e}"
                        );
                    }
                    let mu = cft_short_column_mu(&inp, cn + sn, 0.0, 0.0);
                    assert!((mu - cm - sm).abs() < 1e-12 * inp.fy * d * b * d);
                }
            }
        }
    }

    #[test]
    fn rounded_short_matches_printed_equations_in_their_neutral_axis_range() {
        let mut inp = box_input();
        inp.t = 16.0;
        inp.c_d = 368.0;
        inp.c_b = 368.0;
        inp.corner_r = 40.0;
        let (cn, cm, sn, sm) = angular_material_forces(&inp, 84.0);
        for (v, e) in [
            (cn, 1096583.3605270158),
            (cm, 146611097.68432254),
            (sn, -1747200.0),
            (sm, 1020860058.9516826),
        ] {
            assert!((v - e).abs() < 1e-5, "{v} != {e}");
        }
    }

    #[test]
    fn square_long_case2_independent_30_knm_fixture_and_limits() {
        let mu = |n, ne, nk| cft_square_long_case2_mu(1e6, 2e6, ne, nk, 100e6, n).unwrap();
        assert!((mu(2e6, 4e6, 5e6) - 30e6).abs() < 1e-6);
        let fixture = CftLongMediumInput {
            bending: CftBendingInput {
                circular: false,
                corner_r: 0.0,
                d_steel: 2.0,
                b_steel: 2.0,
                c_d: 1.0,
                c_b: 1.0,
                t: 0.5,
                fc: 30.0,
                fy: 400e6 / 7.0,
            },
            is_long: true,
            c_ncr: 1e6,
            c_lambda1: 1.0,
            nk: 5e6,
            s_ncr: 2e6,
            s_ne: 4e6,
            ncu_axial: 3e6,
            ntu: 2e6,
        };
        assert!((cft_long_medium_column_mu(&fixture, 2e6).unwrap() - 30e6).abs() < 1e-6);
        assert_eq!(mu(3e6, 4e6, 5e6), 0.0);
        assert!((mu(2e6, 1e20, 5e6) - 40e6).abs() < 1e-5);
        assert!(mu(2e6, 4e6, 1e6 + 1e-3) < 0.04);
        assert!(
            cft_square_long_case2_mu(1e6, 2e6, 4e6, 5e6, 100e6, 3e6 + 1.0)
                .unwrap_err()
                .contains("軸NG")
        );
        assert!(cft_square_long_case2_mu(1e6, 2e6, 4e6, 5e6, 100e6, 10e6).is_err());
        for (sn, ne, nk, n) in [
            (2e6, 4e6, 1e6, 2e6),
            (2e6, 1e6, 5e6, 2e6),
            (0.0, 4e6, 5e6, 2e6),
            (2e6, f64::INFINITY, 5e6, 2e6),
            (2e6, 4e6, 5e6, f64::NAN),
        ] {
            assert!(cft_square_long_case2_mu(1e6, sn, ne, nk, 100e6, n).is_err());
        }
    }

    #[test]
    fn unrelated_cases_keep_their_existing_formulas_without_steel_case2_inputs() {
        for (circular, is_long, rectangular, n) in [
            (false, false, false, 4e6),
            (true, true, false, 4e6),
            (false, true, true, 4e6),
            (false, true, false, 0.0),
            (false, true, false, -1e6),
        ] {
            let (mut inp, _, _) = long_input(circular, is_long);
            if rectangular {
                inp.bending.b_steel = 300.0;
                inp.bending.c_b = 276.0;
            }
            let before = cft_long_medium_column_mu(&inp, n).unwrap();
            inp.s_ne = f64::NAN;
            inp.s_ncr = f64::NAN;
            assert_eq!(cft_long_medium_column_mu(&inp, n).unwrap(), before);
        }
    }

    #[test]
    fn square_long_case2_uses_steel_loads_independently_of_concrete_slenderness() {
        let (mut inp, _, _) = long_input(false, true);
        inp.c_ncr = 1e6;
        inp.s_ncr = 2e6;
        inp.s_ne = 4e6;
        inp.nk = 5e6;
        let mu = cft_long_medium_column_mu(&inp, 2e6).unwrap();
        inp.c_lambda1 = 100.0;
        assert_eq!(cft_long_medium_column_mu(&inp, 2e6).unwrap(), mu);
        inp.ncu_axial = 100e6;
        assert_eq!(cft_long_medium_column_mu(&inp, 2e6).unwrap(), mu);
        inp.s_ne = 2e6;
        assert!((cft_long_medium_column_mu(&inp, 2e6).unwrap() / mu - 2.0 / 3.0).abs() < 1e-12);
        inp.s_ne = 4e6;
        let at = cft_long_medium_column_mu(&inp, 1e6).unwrap();
        assert!((cft_long_medium_column_mu(&inp, 1e6 - 1e-3).unwrap() - at).abs() < 1.0);
        assert!((cft_long_medium_column_mu(&inp, 1e6 + 1e-3).unwrap() - at).abs() < 1.0);
        assert_eq!(cft_long_medium_column_mu(&inp, -inp.ntu).unwrap(), 0.0);
        assert!(cft_long_medium_column_mu(&inp, -0.5 * inp.ntu).unwrap() > 0.0);
        assert!(cft_long_medium_column_mu(&inp, f64::NAN).is_err());
    }

    /// 角形 CFT-□400×400×12, Fc=30, Fy=325。cD=cB=376。
    fn box_input() -> CftBendingInput {
        CftBendingInput {
            circular: false,
            corner_r: 0.0,
            d_steel: 400.0,
            b_steel: 400.0,
            c_d: 376.0,
            c_b: 376.0,
            t: 12.0,
            fc: 30.0,
            fy: 325.0,
        }
    }

    /// 円形 CFT-φ400×12, Fc=30, Fy=325。cD=376。
    fn pipe_input() -> CftBendingInput {
        CftBendingInput {
            circular: true,
            corner_r: 0.0,
            d_steel: 400.0,
            b_steel: 400.0,
            c_d: 376.0,
            c_b: 376.0,
            t: 12.0,
            fc: 30.0,
            fy: 325.0,
        }
    }

    #[test]
    fn test_angular_nu_mu_endpoints_handcalc() {
        let inp = box_input();
        // xn=cD/2（中立軸中央）: sNu=0（軸力は主にコンクリート）。
        let (nu, mu) = angular_nu_mu(&inp, inp.c_d / 2.0);
        let xn = inp.c_d / 2.0;
        let c_nu = xn * inp.c_b * inp.fc;
        let s_nu = 2.0 * inp.t * (2.0 * xn - inp.c_d) * inp.fy; // =0
        assert!((nu - (c_nu + s_nu)).abs() < 1e-3);
        // Mu は正（フランジ＋ウェブ＋コンクリート）。
        assert!(mu > 0.0);
    }

    #[test]
    fn test_angular_web_term_uses_fy() {
        // ウェブ項 2t·xn·(cD−xn)·Fy が sMu に含まれることを確認（Fc ではなく Fy）。
        let inp = box_input();
        let xn = inp.c_d / 3.0;
        let (_, mu) = angular_nu_mu(&inp, xn);
        let c_mu = 0.5 * xn * inp.c_b * (inp.c_d - xn) * inp.fc;
        let s_mu_flange = inp.b_steel * inp.t * (inp.d_steel - inp.t) * inp.fy;
        let s_mu_web = 2.0 * inp.t * xn * (inp.c_d - xn) * inp.fy;
        assert!((mu - (c_mu + s_mu_flange + s_mu_web)).abs() < 1e-3);
    }

    #[test]
    fn test_circular_nu_mu_symmetry() {
        // θ=π/2（中立軸が中心）: cNu = r1²·(π/2)·cσcB、sNu = 2r2t·(β1·π/2 − β2·(−π/2))·Fy。
        let inp = pipe_input();
        let (nu, mu) = circular_nu_mu(&inp, PI / 2.0);
        assert!(nu.is_finite() && mu > 0.0);
        // θ=0（圧縮なし）で cNu=cMu=0。
        let (n0, m0) = circular_nu_mu(&inp, 0.0);
        assert!(m0.abs() < 1e-6 || m0 >= 0.0);
        assert!(n0 < nu, "θ=0 の軸力は θ=π/2 より小さい");
    }

    #[test]
    fn test_cft_short_column_mu_curve_and_interp() {
        let inp = box_input();
        let ncu1 = inp.c_d * inp.c_b * inp.fc + (400.0 * 400.0 - inp.c_d * inp.c_b) * inp.fy;
        let ntu = (400.0 * 400.0 - inp.c_d * inp.c_b) * inp.fy;

        // 中央付近の軸力で Mu 正。
        let n_mid = 0.3 * ncu1;
        let mu_mid = cft_short_column_mu(&inp, n_mid, ncu1, ntu);
        assert!(mu_mid > 0.0);

        // 中心圧縮（N=Ncu1）で Mu→0。
        let mu_at_ncu1 = cft_short_column_mu(&inp, ncu1, ncu1, ntu);
        assert!(mu_at_ncu1.abs() < 1e-3, "Mu(Ncu1)={mu_at_ncu1}");

        // 中心引張（N=−Ntu）で Mu→0。
        let mu_at_ntu = cft_short_column_mu(&inp, -ntu, ncu1, ntu);
        assert!(mu_at_ntu.abs() < 1e-3, "Mu(−Ntu)={mu_at_ntu}");

        // 高圧縮域は中央より Mu 小（相関曲線の山型）。
        let mu_high = cft_short_column_mu(&inp, 0.85 * ncu1, ncu1, ntu);
        assert!(mu_high < mu_mid, "mu_high={mu_high} mu_mid={mu_mid}");
    }

    #[test]
    fn test_cft_short_column_mu_circular_positive() {
        let inp = pipe_input();
        let c_area = PI * inp.c_d * inp.c_d / 4.0;
        let s_area = PI * (400.0 * 400.0 - inp.c_d * inp.c_d) / 4.0;
        let ncu1 = c_area * inp.fc + (1.0 + 0.27) * s_area * inp.fy;
        let ntu = s_area * inp.fy;
        let mu = cft_short_column_mu(&inp, 0.3 * ncu1, ncu1, ntu);
        assert!(mu > 0.0);
    }

    #[test]
    fn test_cft_short_column_mu_invalid_zero() {
        let mut bad = box_input();
        bad.fc = 0.0;
        assert_eq!(cft_short_column_mu(&bad, 1000.0, 1.0e6, 1.0e6), 0.0);
    }

    #[test]
    fn test_cft_nk_handcalc() {
        // Nk = π²(cE'·cI/5 + sE·sI)/lk²、cE'=(3.32√30+6.90)×10³。
        let (c_i, s_i, s_e, fc, lk) = (1.0e9, 5.0e8, 205000.0, 30.0, 5000.0);
        let nk = cft_nk(c_i, s_i, s_e, fc, lk);
        let c_e = (3.32 * 30.0_f64.sqrt() + 6.90) * 1.0e3;
        let hand = PI * PI * (c_e * c_i / 5.0 + s_e * s_i) / (lk * lk);
        assert!((nk - hand).abs() / hand < 1e-9, "Nk={nk} vs {hand}");
        // lk=0 は無限大（座屈しない）。
        assert!(cft_nk(c_i, s_i, s_e, fc, 0.0).is_infinite());
    }

    fn long_input(circular: bool, is_long: bool) -> (CftLongMediumInput, f64, f64) {
        let bending = if circular { pipe_input() } else { box_input() };
        // 代表値（cNcr は座屈で短柱 cNc より小さい）。
        let c_area = if circular {
            PI * bending.c_d * bending.c_d / 4.0
        } else {
            bending.c_d * bending.c_b
        };
        let s_area = if circular {
            PI * (400.0 * 400.0 - bending.c_d * bending.c_d) / 4.0
        } else {
            400.0 * 400.0 - bending.c_d * bending.c_b
        };
        let c_ncr = 0.6 * c_area * bending.fc; // 座屈低減の代表
        let ncu_axial = c_ncr + 0.8 * s_area * bending.fy;
        let ntu = s_area * bending.fy;
        let nk = 3.0 * c_ncr; // Nk > cNcr（R>0）
        (
            CftLongMediumInput {
                bending,
                is_long,
                c_ncr,
                c_lambda1: 1.0,
                nk,
                s_ncr: ncu_axial - c_ncr,
                s_ne: 2.0 * ncu_axial,
                ncu_axial,
                ntu,
            },
            c_ncr,
            ntu,
        )
    }

    #[test]
    fn test_cft_long_medium_mu_continuity_at_cncr() {
        // Case1/Case2 の境界 N=cNcr で連続（いずれも sMu0·R）。
        for (circular, is_long) in [(true, true), (false, true), (false, false), (true, false)] {
            let (inp, c_ncr, _) = long_input(circular, is_long);
            let lo = cft_long_medium_column_mu(&inp, c_ncr - 1.0).unwrap();
            let hi = cft_long_medium_column_mu(&inp, c_ncr + 1.0).unwrap();
            assert!(
                (lo - hi).abs() / hi.max(1.0) < 1e-2,
                "境界不連続 circular={circular} is_long={is_long}: lo={lo} hi={hi}"
            );
        }
    }

    #[test]
    fn test_cft_long_medium_mu_endpoints_and_reduction() {
        let (inp, _, ntu) = long_input(true, true);
        // 中心引張 N=−Ntu で Mu→0。
        assert!(cft_long_medium_column_mu(&inp, -ntu).unwrap().abs() < 1e-3);
        // 圧縮側で Mu 正。
        assert!(cft_long_medium_column_mu(&inp, 0.3 * inp.ncu_axial).unwrap() > 0.0);
        // Nk を小さくする（座屈が厳しい）と R が下がり Mu が減少する。
        let mut stiff = inp;
        stiff.nk = 10.0 * inp.c_ncr; // R 大
        let mut slender = inp;
        slender.nk = 1.2 * inp.c_ncr; // R 小
        let n = 0.2 * inp.c_ncr;
        assert!(
            cft_long_medium_column_mu(&slender, n).unwrap()
                < cft_long_medium_column_mu(&stiff, n).unwrap(),
            "細長い方が Mu 小のはず"
        );
    }

    #[test]
    fn test_cft_medium_less_than_short() {
        // 中柱の N-M 曲げ耐力は座屈低減により短柱より小さい（同一断面・同一 N）。
        let inp = box_input();
        let c_area = inp.c_d * inp.c_b;
        let s_area = 400.0 * 400.0 - c_area;
        let ncu1 = c_area * inp.fc + s_area * inp.fy;
        let ntu = s_area * inp.fy;
        let short = cft_short_column_mu(&inp, 0.3 * ncu1, ncu1, ntu);

        let (med_inp, _, _) = long_input(false, false);
        let medium = cft_long_medium_column_mu(&med_inp, 0.3 * med_inp.ncu_axial).unwrap();
        assert!(
            medium > 0.0 && medium < short,
            "medium={medium} short={short}"
        );
    }
}
