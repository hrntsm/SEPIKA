//! 鉄筋コンクリート造梁の非線形復元力特性（曲げトリリニア・せん断・軸）。

use sepika_core::rc_capacity::{rc_alpha_y_sugano, rc_mu_simple, RcCapacityInput};

/// RC 梁の曲げひび割れ強度 Mc [N·mm]。`Mc = κ·√Fc·Ze`。
/// 不正入力（Fc・Ze のいずれかが 0 以下）は 0.0。
pub fn rc_beam_crack_moment(fc: f64, ze: f64) -> f64 {
    sepika_core::rc_capacity::rc_crack_moment(fc, ze)
}

/// RC 梁のせん断ひび割れ強度 Qc [N]。
/// 不正入力（Fc・b・j のいずれかが 0 以下）は 0.0。
pub fn rc_beam_shear_crack(fc: f64, m_over_qd: f64, b: f64, j: f64) -> f64 {
    if fc <= 0.0 || b <= 0.0 || j <= 0.0 {
        return 0.0;
    }
    (0.061 * (fc + 49.0) / (m_over_qd.max(0.0) + 1.7)) * b * j
}

/// RC 梁の軸復元力特性（引張ひび割れ・引張降伏・圧縮降伏）[N]。
#[derive(Clone, Copy, Debug)]
pub struct RcAxial {
    pub tension_crack: f64,
    pub tension_yield: f64,
    pub compression_yield: f64,
}

/// RC 梁の軸復元力特性を算定する。`ac`: コンクリート断面積、`at`: 鉄筋断面積、
/// `sigma_y`: 鉄筋降伏、`fc`: コンクリート強度。
pub fn rc_beam_axial(fc: f64, ac: f64, at: f64, sigma_y: f64) -> RcAxial {
    let nct = if fc > 0.0 && ac > 0.0 {
        sepika_core::rc_capacity::RC_CRACK_COEF * fc.sqrt() * ac
    } else {
        0.0
    };
    let nut = (at.max(0.0)) * sigma_y.max(0.0);
    let nuc = nut + fc.max(0.0) * (ac - at).max(0.0);
    RcAxial {
        tension_crack: nct,
        tension_yield: nut,
        compression_yield: nuc,
    }
}

/// RC 梁の曲げトリリニア骨格の算定入力。
#[derive(Clone, Copy, Debug)]
pub struct RcBeamBendingInput {
    /// コンクリート設計基準強度 Fc [N/mm²]。
    pub fc: f64,
    /// 引張側断面係数 Ze [mm³]（鉄筋考慮、Ie/(D−g) 等。Mc 用）。
    pub ze: f64,
    /// 引張鉄筋断面積 at [mm²]（片側）。
    pub at: f64,
    /// αy 用の引張鉄筋比 at/(bD)（小数、1% は 0.01）。
    /// T形は明示した引張方向の仮想断面で生成し、せん断用百分率を渡さない。
    pub pt: f64,
    /// 有効せい d [mm]。
    pub d_eff: f64,
    /// 全せい D [mm]（αy の d/D 用）。
    pub d_full: f64,
    /// 主筋降伏強度 σy [N/mm²]。
    pub sigma_y: f64,
    /// 明示した三角形曲げ分布のせん断スパン a = abs(M/Q) [mm]（αy の a/D 用）。
    pub a_shear_span: f64,
    /// 鉄筋ヤング係数 Es [N/mm²]。
    pub es: f64,
    /// コンクリートヤング係数 Ec [N/mm²]（n=Es/Ec）。
    pub ec: f64,
}

/// RC 梁の曲げトリリニア骨格諸元。
#[derive(Clone, Copy, Debug)]
pub struct RcBeamBending {
    /// 曲げひび割れ強度 Mc [N·mm]。
    pub mc: f64,
    /// 曲げ降伏（＝終局）強度 My [N·mm]（=0.9·at·σy·j）。
    pub my: f64,
    /// 曲げ降伏時剛性低下率 αy（菅野式、無次元）。
    pub alpha_y: f64,
}

/// RC 梁の曲げトリリニア骨格（Mc・My・αy）を算定する。
pub fn rc_beam_bending(inp: &RcBeamBendingInput) -> RcBeamBending {
    let mc = rc_beam_crack_moment(inp.fc, inp.ze);
    let cap = RcCapacityInput {
        b: 1.0,
        d: inp.d_full,
        at: inp.at,
        d_eff: inp.d_eff,
        sigma_y: inp.sigma_y,
        fc: inp.fc.max(1e-9),
        pw: 0.0,
        sigma_wy: 0.0,
        clear_span: 1.0,
        sigma_0: 0.0,
    };
    let my = rc_mu_simple(&cap);
    let n = inp.es / inp.ec;
    let alpha_y = rc_alpha_y_sugano(
        inp.pt,
        inp.a_shear_span / inp.d_full,
        inp.d_eff / inp.d_full,
        n,
    );
    RcBeamBending { mc, my, alpha_y }
}

/// αy 用の小数入力と at・D・d を診断して曲げ骨格諸元を算定する。
/// 不正入力は既定値やゼロへ置換せず入力エラーを返す。
pub fn rc_beam_bending_checked(
    inp: &RcBeamBendingInput,
) -> Result<RcBeamBending, sepika_core::error::CoreError> {
    use sepika_core::error::CoreError;
    for (name, value) in [
        ("at", inp.at),
        ("D", inp.d_full),
        ("d", inp.d_eff),
        ("pt_alpha_ratio", inp.pt),
        ("Es", inp.es),
        ("Ec", inp.ec),
        ("a", inp.a_shear_span),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(CoreError::InvalidInput(format!(
                "{name} は正の有限値が必要です"
            )));
        }
    }
    if inp.d_eff > inp.d_full {
        return Err(CoreError::InvalidInput(
            "有効せい d は全せい D 以下が必要です".into(),
        ));
    }
    for (name, value) in [("Fc", inp.fc), ("Ze", inp.ze), ("σy", inp.sigma_y)] {
        if !value.is_finite() || value <= 0.0 {
            return Err(CoreError::InvalidInput(format!(
                "{name} は正の有限値が必要です"
            )));
        }
    }
    let result = rc_beam_bending(inp);
    sepika_core::rc_capacity::rc_alpha_y_sugano_checked(
        inp.pt,
        inp.a_shear_span / inp.d_full,
        inp.d_eff / inp.d_full,
        inp.es / inp.ec,
    )?;
    if !result.mc.is_finite() || !result.my.is_finite() {
        return Err(CoreError::InvalidInput("骨格耐力が非有限です".into()));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bending_reference_receives_alpha_ratio_without_percent_conversion() {
        use sepika_core::rc_capacity::{rc_rebar_ratios, RcAlphaSection};
        let ratios =
            rc_rebar_ratios(300.0, 600.0, 540.0, 1800.0, RcAlphaSection::Rectangular).unwrap();
        let input = RcBeamBendingInput {
            fc: 24.0,
            ze: 18_000_000.0,
            at: 1800.0,
            pt: ratios.pt_alpha_ratio,
            d_eff: 540.0,
            d_full: 600.0,
            sigma_y: 345.0,
            a_shear_span: 1800.0,
            es: 200_000.0,
            ec: 20_000.0,
        };
        assert!((rc_beam_bending_checked(&input).unwrap().alpha_y - 0.265_720_5).abs() < 1e-12);
        assert!(
            (rc_beam_bending(&RcBeamBendingInput {
                pt: input.pt * 100.0,
                ..input
            })
            .alpha_y
                - 0.265_720_5)
                .abs()
                > 1.0
        );
        for (section, expected) in [
            (
                RcAlphaSection::TBottomTension {
                    effective_width_mm: 1500.0,
                },
                0.278_259_3,
            ),
            (
                RcAlphaSection::TTopTension {
                    effective_width_mm: 1500.0,
                    slab_tension_area_mm2: 600.0,
                },
                0.260_496,
            ),
        ] {
            let pt = rc_rebar_ratios(300.0, 600.0, 540.0, 1800.0, section)
                .unwrap()
                .pt_alpha_ratio;
            assert!(
                (rc_beam_bending_checked(&RcBeamBendingInput { pt, ..input })
                    .unwrap()
                    .alpha_y
                    - expected)
                    .abs()
                    < 1e-12
            );
        }
        for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert!(rc_beam_bending_checked(&RcBeamBendingInput { at: bad, ..input }).is_err());
            assert!(rc_beam_bending_checked(&RcBeamBendingInput {
                d_full: bad,
                ..input
            })
            .is_err());
            assert!(rc_beam_bending_checked(&RcBeamBendingInput {
                d_eff: bad,
                ..input
            })
            .is_err());
        }
        assert!(rc_beam_bending_checked(&RcBeamBendingInput {
            d_eff: 601.0,
            ..input
        })
        .is_err());
    }

    /// 曲げひび割れモーメント Mc = 0.56·√Fc·Ze の配線 smoke。
    #[test]
    fn test_rc_beam_crack_moment_smoke() {
        let ze = 300.0 * 600.0_f64.powi(2) / 6.0;
        let mc = rc_beam_crack_moment(24.0, ze);
        assert!((mc - 0.56 * 24.0_f64.sqrt() * ze).abs() < 1e-6, "Mc={mc}");
        assert!(mc > 0.0);
    }

    #[test]
    fn test_rc_beam_shear_crack_matches_handcalc() {
        let qc = rc_beam_shear_crack(24.0, 1.5, 300.0, 500.0);
        let hand = (0.061 * (24.0 + 49.0) / (1.5 + 1.7)) * 300.0 * 500.0;
        assert!((qc - hand).abs() < 1e-6, "Qc={qc} vs {hand}");
        assert!(qc > 0.0);
    }

    #[test]
    fn test_rc_beam_axial_matches_handcalc() {
        let ax = rc_beam_axial(24.0, 180_000.0, 2000.0, 345.0);
        assert!((ax.tension_crack - 0.56 * 24.0_f64.sqrt() * 180_000.0).abs() < 1e-3);
        assert!((ax.tension_yield - 2000.0 * 345.0).abs() < 1e-6);
        assert!(
            (ax.compression_yield - (2000.0 * 345.0 + 24.0 * (180_000.0 - 2000.0))).abs() < 1e-3
        );
        // 圧縮降伏 > 引張降伏（コンクリート寄与）。
        assert!(ax.compression_yield > ax.tension_yield);
    }

    #[test]
    fn test_rc_beam_bending_trilinear() {
        let inp = RcBeamBendingInput {
            fc: 24.0,
            ze: 300.0 * 600.0_f64.powi(2) / 6.0,
            at: 1935.0,
            pt: 0.008,
            d_eff: 540.0,
            d_full: 600.0,
            sigma_y: 345.0,
            a_shear_span: 1500.0,
            es: 205000.0,
            ec: 21000.0,
        };
        let b = rc_beam_bending(&inp);
        // My = 0.9·at·σy·d の手計算一致（d = 有効せい）。
        assert!((b.my - 0.9 * 1935.0 * 345.0 * 540.0).abs() < 1e-3);
        // ひび割れ < 降伏（健全な折れ点順序）。
        assert!(b.mc > 0.0 && b.mc < b.my, "Mc={} My={}", b.mc, b.my);
        // αy は 0〜1 の妥当値。
        assert!(b.alpha_y > 0.0 && b.alpha_y < 1.0, "αy={}", b.alpha_y);
    }
}
