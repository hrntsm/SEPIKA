//! RC 矩形断面の簡易終局耐力算定（部材ランク判定・プッシュオーバーせん断降伏判定用）。

/// RC 矩形断面の簡易終局耐力算定用の入力一式。
#[derive(Clone, Copy)]
pub struct RcCapacityInput {
    /// 断面幅 b \[mm\]
    pub b: f64,
    /// 全せい D \[mm\]（有効せいではない）
    pub d: f64,
    /// 引張側主筋の総断面積 at \[mm²\]（片側）
    pub at: f64,
    /// 有効せい d_e \[mm\]（= D − dt。dt は [`crate::rc_rebar_geom::tension_dt`] と同規約）
    pub d_eff: f64,
    /// 主筋降伏強度 σy \[N/mm²\]
    pub sigma_y: f64,
    /// コンクリート強度 Fc \[N/mm²\]
    pub fc: f64,
    /// せん断補強筋比 pw（小数、0.2% は 0.002。= aw・組数/(b・ピッチ)）
    pub pw: f64,
    /// せん断補強筋降伏強度 σwy \[N/mm²\]
    pub sigma_wy: f64,
    /// 内法スパン h0 \[mm\]（反曲点中央を仮定し Qmu = 2Mu/h0）
    pub clear_span: f64,
    /// 軸方向圧縮応力度 σ0 \[N/mm²\]（既定 0）。`rc_qsu_simple` の軸力項
    /// `0.1・σ0・b・j` に用いる。荒川式の適用範囲である 0〜0.4Fc に
    /// `rc_qsu_simple` 内でクランプされる（要・原典照合）。
    pub sigma_0: f64,
}

/// αy 用の断面・引張方向。T形では有効幅 B [mm] と協力スラブ筋面積 [mm²] を明示する。
#[derive(Clone, Copy, Debug)]
pub enum RcAlphaSection {
    Rectangular,
    TBottomTension {
        effective_width_mm: f64,
    },
    TTopTension {
        effective_width_mm: f64,
        slab_tension_area_mm2: f64,
    },
}

/// 幾何・配筋から αy 用の小数とせん断用の百分率を別々に生成した鉄筋比。
#[derive(Clone, Copy, Debug)]
pub struct RcRebarRatios {
    pub pt_alpha_ratio: f64,
    pub pt_shear_percent: f64,
}

/// b・D・d [mm]、片側主筋 at [mm²]から方向別鉄筋比を生成する。
/// 非有限・非正値、d>D、不正なT形入力は入力エラー。I0/IT補正は適用しない。
pub fn rc_rebar_ratios(
    b_mm: f64,
    full_depth_mm: f64,
    effective_depth_mm: f64,
    tension_area_mm2: f64,
    section: RcAlphaSection,
) -> Result<RcRebarRatios, crate::error::CoreError> {
    use crate::error::CoreError;
    for (name, value) in [
        ("b", b_mm),
        ("D", full_depth_mm),
        ("d", effective_depth_mm),
        ("at", tension_area_mm2),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(CoreError::InvalidInput(format!(
                "{name} は正の有限値が必要です"
            )));
        }
    }
    if effective_depth_mm > full_depth_mm {
        return Err(CoreError::InvalidInput(
            "有効せい d は全せい D 以下が必要です".into(),
        ));
    }
    let (alpha_width_mm, alpha_tension_area_mm2) = match section {
        RcAlphaSection::Rectangular => (b_mm, tension_area_mm2),
        RcAlphaSection::TBottomTension { effective_width_mm } => {
            (effective_width_mm, tension_area_mm2)
        }
        RcAlphaSection::TTopTension {
            effective_width_mm,
            slab_tension_area_mm2,
        } => {
            if !slab_tension_area_mm2.is_finite() || slab_tension_area_mm2 < 0.0 {
                return Err(CoreError::InvalidInput(
                    "スラブ引張筋面積は非負の有限値が必要です".into(),
                ));
            }
            if !effective_width_mm.is_finite() || effective_width_mm < b_mm {
                return Err(CoreError::InvalidInput(
                    "T形有効幅 B は梁幅 b 以上の有限値が必要です".into(),
                ));
            }
            (b_mm, tension_area_mm2 + slab_tension_area_mm2)
        }
    };
    if !alpha_width_mm.is_finite() || alpha_width_mm < b_mm {
        return Err(CoreError::InvalidInput(
            "T形有効幅 B は梁幅 b 以上の有限値が必要です".into(),
        ));
    }
    let ratios = RcRebarRatios {
        pt_alpha_ratio: alpha_tension_area_mm2 / (alpha_width_mm * full_depth_mm),
        pt_shear_percent: 100.0 * tension_area_mm2 / (b_mm * effective_depth_mm),
    };
    if !ratios.pt_alpha_ratio.is_finite()
        || !ratios.pt_shear_percent.is_finite()
        || ratios.pt_alpha_ratio <= 0.0
        || ratios.pt_shear_percent <= 0.0
    {
        return Err(CoreError::InvalidInput(
            "鉄筋比を有限の正値として算定できません".into(),
        ));
    }
    Ok(ratios)
}

/// 曲げ終局モーメント Mu = 0.9・at・σy・d（引張鉄筋降伏型の略算式）。
/// 不正入力（at, d_eff, σy のいずれかが 0 以下）は 0.0 を返す。
pub fn rc_mu_simple(inp: &RcCapacityInput) -> f64 {
    if inp.at <= 0.0 || inp.d_eff <= 0.0 || inp.sigma_y <= 0.0 {
        return 0.0;
    }
    0.9 * inp.at * inp.sigma_y * inp.d_eff
}

/// 曲げ終局時せん断力 Qmu = 2・Mu / h0（両端曲げ降伏・反曲点中央を仮定）。
///
/// `clear_span`（h0）が 0 以下の場合は 0.0 を返す。
pub fn rc_qmu_simple(inp: &RcCapacityInput) -> f64 {
    if inp.clear_span <= 0.0 {
        return 0.0;
    }
    2.0 * rc_mu_simple(inp) / inp.clear_span
}

/// RC 柱の曲げ終局モーメント Mu \[N·mm\]（軸力を考慮した略算式。要・原典照合）。
///
/// ```text
/// Nmax = b・D・Fc + ag・σy
/// Nmin = −ag・σy
/// N > 0.4・b・D・Fc:
///   Mu = {0.8・at・σy・D + 0.12・b・D²・Fc}・(Nmax − N)/(Nmax − 0.4・b・D・Fc)
/// 0 ≤ N ≤ 0.4・b・D・Fc:
///   Mu = 0.8・at・σy・D + 0.5・N・D・(1 − N/(b・D・Fc))
/// Nmin ≤ N < 0:
///   Mu = 0.8・at・σy・D + 0.4・N・D
/// ```
///
/// - `ag`: 全主筋断面積 \[mm²\]、`n_axial`: 設計軸力 \[N\]（**圧縮を正**）。
/// - `N` は適用範囲 \[Nmin, Nmax\] にクランプし、結果が負となる場合は 0 を返す
///   （N=Nmax（全断面圧縮）・N=Nmin（全主筋引張降伏）で曲げ余力なし）。
/// - `inp.b`, `inp.d`(=D), `inp.at`, `inp.sigma_y`, `inp.fc` を用いる。
///   不正入力（b, d, at, σy, Fc のいずれかが 0 以下）は 0.0 を返す。
///
/// RC 規準の柱設計用せん断力 QD1 = ΣcMy/h′
/// における柱の終局曲げ（cMy）の算定に用いる。
pub fn rc_column_mu_simple(inp: &RcCapacityInput, ag: f64, n_axial: f64) -> f64 {
    if inp.b <= 0.0 || inp.d <= 0.0 || inp.at <= 0.0 || inp.sigma_y <= 0.0 || inp.fc <= 0.0 {
        return 0.0;
    }
    let (b, d, at, sy, fc) = (inp.b, inp.d, inp.at, inp.sigma_y, inp.fc);
    let ag = ag.max(at);
    let n_max = b * d * fc + ag * sy;
    let n_min = -ag * sy;
    let n = n_axial.clamp(n_min, n_max);
    let n_bal = 0.4 * b * d * fc;

    let mu = if n > n_bal {
        let m_bal = 0.8 * at * sy * d + 0.12 * b * d * d * fc;
        m_bal * (n_max - n) / (n_max - n_bal)
    } else if n >= 0.0 {
        0.8 * at * sy * d + 0.5 * n * d * (1.0 - n / (b * d * fc))
    } else {
        0.8 * at * sy * d + 0.4 * n * d
    };
    mu.max(0.0)
}

/// せん断終局耐力 Qsu \[N\]（荒川mean式系の略算式、要・原典照合）。
///
/// ```text
/// Qsu = { 0.068・pt^0.23・(Fc+18) / (M/(Q・d_e)+0.12) + 0.85・√(pw・σwy) + 0.1・σ0 }・b・j
/// ```
/// - `pt = 100・at/(b・d_e)` \[%\]（引張鉄筋比）
/// - `j = 7・d_e/8`
/// - せん断スパン比 `M/(Q・d_e) = h0/(2・d_e)` は反曲点中央（等曲げ勾配）の仮定から
///   導く略算のため、式の適用範囲である 1.0〜3.0 にクランプする。
/// - `pw` は式の適用範囲の上限 0.012 でクランプする（下限は 0）。
/// - 軸力項 `0.1・σ0`（σ0: 軸方向圧縮応力度）は荒川式の適用範囲である
///   0〜0.4Fc にクランプする（負の σ0（引張）は 0 とみなし、Qsu を低減しない
///   安全側の扱いとする）。
///
/// 全係数は要・原典照合。
/// 不正入力（b, d_eff, at, Fc, clear_span のいずれかが 0 以下）は 0.0 を返す。
pub fn rc_qsu_simple(inp: &RcCapacityInput) -> f64 {
    if inp.b <= 0.0 || inp.d_eff <= 0.0 || inp.at <= 0.0 || inp.fc <= 0.0 || inp.clear_span <= 0.0 {
        return 0.0;
    }
    let pt_shear_percent = 100.0 * inp.at / (inp.b * inp.d_eff);
    let j = 7.0 * inp.d_eff / 8.0;
    let shear_span_ratio = (inp.clear_span / (2.0 * inp.d_eff)).clamp(1.0, 3.0);
    let pw_ratio = inp.pw.clamp(0.0, 0.012);
    let concrete_term =
        0.068 * pt_shear_percent.powf(0.23) * (inp.fc + 18.0) / (shear_span_ratio + 0.12);
    let hoop_term = 0.85 * (pw_ratio * inp.sigma_wy).max(0.0).sqrt();
    let sigma_0 = inp.sigma_0.clamp(0.0, 0.4 * inp.fc);
    let axial_term = 0.1 * sigma_0;
    (concrete_term + hoop_term + axial_term) * inp.b * j
}

/// 幾何・配筋の入力診断後にせん断終局耐力 [N] を算定する。
/// b・D・d・at の不正値はゼロへ置換せず入力エラー。pt は幾何から内部生成する。
pub fn rc_qsu_simple_checked(inp: &RcCapacityInput) -> Result<f64, crate::error::CoreError> {
    rc_rebar_ratios(inp.b, inp.d, inp.d_eff, inp.at, RcAlphaSection::Rectangular)?;
    Ok(rc_qsu_simple(inp))
}

/// 採用RC梁式のαyを数値評価する。pt=at/(bD)は小数、a/D・d/D・nは無次元。
/// 入力域へのクランプ・結果の非正値補正は行わない。設計適用・ばね接続可否は別途診断する。
pub fn rc_alpha_y_sugano(pt_alpha_ratio: f64, a_over_d: f64, d_over_full: f64, n: f64) -> f64 {
    let base = if a_over_d <= 2.0 {
        0.043 + 1.64 * n * pt_alpha_ratio + 0.043 * a_over_d
    } else {
        -0.0336 - 0.1935 * n * pt_alpha_ratio + 0.1270 * a_over_d
    };
    base * d_over_full * d_over_full
}

/// 正の有限比率・d/D≤1を診断してαyを評価する。有限な非正結果もそのまま返す。
pub fn rc_alpha_y_sugano_checked(
    pt_alpha_ratio: f64,
    a_over_d: f64,
    d_over_full: f64,
    n: f64,
) -> Result<f64, crate::error::CoreError> {
    for (name, value) in [
        ("pt_alpha_ratio", pt_alpha_ratio),
        ("a/D", a_over_d),
        ("d/D", d_over_full),
        ("n", n),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(crate::error::CoreError::InvalidInput(format!(
                "{name} は正の有限値が必要です"
            )));
        }
    }
    if d_over_full > 1.0 {
        return Err(crate::error::CoreError::InvalidInput(
            "有効せい d は全せい D 以下が必要です".into(),
        ));
    }
    let alpha = rc_alpha_y_sugano(pt_alpha_ratio, a_over_d, d_over_full, n);
    if !alpha.is_finite() {
        return Err(crate::error::CoreError::InvalidInput(
            "αyを有限値として算定できません".into(),
        ));
    }
    Ok(alpha)
}

/// ひび割れ強度の係数 κ。
///
/// 曲げひび割れ `Mc = κ·√Fc·Ze`、引張ひび割れ `Nct = κ·√Fc·Ac` の双方に用いる。
pub const RC_CRACK_COEF: f64 = 0.56;

/// RC 断面の曲げひび割れモーメント Mc \[N·mm\]。
///
/// `Mc = κ·√Fc·Ze`（κ=[`RC_CRACK_COEF`]、Fc \[N/mm²\]、Ze=引張側断面係数 \[mm³\]）。
/// 不正入力（Fc・Ze のいずれかが 0 以下）は 0.0 を返す。
///
/// Mc を降伏モーメント My との
/// 関係でクランプするかどうかは用途ごとに異なるため、**呼び出し側**で行う。
pub fn rc_crack_moment(fc: f64, ze: f64) -> f64 {
    if fc <= 0.0 || ze <= 0.0 {
        return 0.0;
    }
    RC_CRACK_COEF * fc.sqrt() * ze
}

/// RC 矩形柱の用途別配筋から [`RcCapacityInput`] を組み立てる。
///
/// - `rebar`: X 方向の引張主筋・かぶり・帯筋
/// - σy は主筋材質 → 材料 `fy` → 345（SD345 相当）の順。**材料強度割増は掛けない**
///   （保有水平耐力など割増が要る呼び出し側が後掛けする）
/// - σwy はせん断補強筋材質 → SD295 相当既定。割増対象外
/// - σ0 は 0（プレースホルダ）。軸力反映は呼び出し側
/// - `fc` 未設定なら `None`
#[allow(clippy::too_many_arguments)]
pub fn rc_capacity_input_from_rect(
    b: f64,
    d: f64,
    rebar: &crate::section_shape::RcRectColumnRebar,
    mat: &crate::model::Material,
    rebar_mat: Option<&crate::model::Material>,
    shear_mat: Option<&crate::model::Material>,
    clear_span: f64,
) -> Option<RcCapacityInput> {
    let fc = mat.fc?;
    let at = rebar.x_direction_area_mm2() / 2.0;
    let side = rebar.edge_steel(crate::rc_rebar_geom::RectEdge::Top, b, d);
    let d_eff = side.effective_depth_mm;
    let pw = if rebar.hoop.pitch > 0.0 && b > 0.0 {
        rebar.aw_x_mm2() / (b * rebar.hoop.pitch)
    } else {
        0.0
    };
    Some(RcCapacityInput {
        b,
        d,
        at,
        d_eff,
        sigma_y: crate::material_grade::rebar_yield_strength(rebar_mat)
            .or(mat.fy)
            .unwrap_or(345.0),
        fc,
        pw,
        sigma_wy: crate::material_grade::shear_rebar_yield_strength(shear_mat)
            .unwrap_or(crate::material_grade::SHEAR_REBAR_DEFAULT_FY),
        clear_span,
        sigma_0: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::MaterialId;
    use crate::model::{Material, MaterialCategory};
    use crate::section_shape::{RcRectColumnRebar, RectColumnHoop};

    #[test]
    fn rebar_ratios_use_distinct_denominators_and_tension_directions() {
        let rect =
            rc_rebar_ratios(300.0, 600.0, 540.0, 1800.0, RcAlphaSection::Rectangular).unwrap();
        assert!((rect.pt_alpha_ratio - 0.010).abs() < 1e-15);
        assert!((rect.pt_shear_percent - 1.111_111_111_111_111).abs() < 1e-14);
        assert!((rect.pt_shear_percent - 1.0).abs() > 0.1);
        let bottom = rc_rebar_ratios(
            300.0,
            600.0,
            540.0,
            1800.0,
            RcAlphaSection::TBottomTension {
                effective_width_mm: 1500.0,
            },
        )
        .unwrap();
        assert!((bottom.pt_alpha_ratio - 0.002).abs() < 1e-15);
        let top = rc_rebar_ratios(
            300.0,
            600.0,
            540.0,
            1800.0,
            RcAlphaSection::TTopTension {
                effective_width_mm: 1500.0,
                slab_tension_area_mm2: 600.0,
            },
        )
        .unwrap();
        assert!((top.pt_alpha_ratio - 0.013_333_333_333_333_3).abs() < 1e-15);
        assert_eq!(bottom.pt_shear_percent, rect.pt_shear_percent);
        assert_eq!(top.pt_shear_percent, rect.pt_shear_percent);
        assert!(
            (rc_alpha_y_sugano(rect.pt_alpha_ratio, 3.0, 0.9, 10.0) - 0.265_720_5).abs() < 1e-12
        );
        assert!(
            (rc_alpha_y_sugano(rect.pt_alpha_ratio * 100.0, 3.0, 0.9, 10.0) - 0.265_720_5).abs()
                > 1.0
        );
        let input = RcCapacityInput {
            b: 300.0,
            d: 600.0,
            d_eff: 540.0,
            at: 1800.0,
            ..sample_input()
        };
        assert!((rc_qsu_simple(&input) - 235_681.416_383_415_87).abs() < 1e-8);
        assert!((input.pw * 100.0 - 0.2).abs() < 1e-15);
        let percent_pw = RcCapacityInput { pw: 0.2, ..input };
        assert!((rc_qsu_simple(&percent_pw) - rc_qsu_simple(&input)).abs() > 100_000.0);
    }

    #[test]
    fn rebar_ratios_diagnose_invalid_geometry_and_t_section_inputs() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            for i in 0..4 {
                let mut values = [300.0, 600.0, 540.0, 1800.0];
                values[i] = bad;
                assert!(rc_rebar_ratios(
                    values[0],
                    values[1],
                    values[2],
                    values[3],
                    RcAlphaSection::Rectangular
                )
                .is_err());
            }
            assert!(rc_rebar_ratios(
                300.0,
                600.0,
                540.0,
                1800.0,
                RcAlphaSection::TBottomTension {
                    effective_width_mm: bad
                }
            )
            .is_err());
        }
        assert!(rc_rebar_ratios(300.0, 600.0, 601.0, 1800.0, RcAlphaSection::Rectangular).is_err());
        assert!(rc_rebar_ratios(
            300.0,
            600.0,
            540.0,
            1800.0,
            RcAlphaSection::TBottomTension {
                effective_width_mm: 299.0
            }
        )
        .is_err());
        for bad in [f64::NAN, f64::INFINITY, -1.0] {
            assert!(rc_rebar_ratios(
                300.0,
                600.0,
                540.0,
                1800.0,
                RcAlphaSection::TTopTension {
                    effective_width_mm: 1500.0,
                    slab_tension_area_mm2: bad
                }
            )
            .is_err());
        }
    }

    #[test]
    fn checked_shear_rejects_invalid_geometry_without_zero_capacity() {
        let base = RcCapacityInput {
            b: 300.0,
            d: 600.0,
            d_eff: 540.0,
            at: 1800.0,
            ..sample_input()
        };
        assert!((rc_qsu_simple_checked(&base).unwrap() - 235_681.416_383_415_87).abs() < 1e-8);
        for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            for input in [
                RcCapacityInput { b: bad, ..base },
                RcCapacityInput { d: bad, ..base },
                RcCapacityInput { d_eff: bad, ..base },
                RcCapacityInput { at: bad, ..base },
            ] {
                assert!(rc_qsu_simple_checked(&input).is_err());
            }
        }
        assert!(rc_qsu_simple_checked(&RcCapacityInput {
            d_eff: 601.0,
            ..base
        })
        .is_err());
    }

    /// 代表断面: b=400, D=600, at=1935(D25×3程度), d_eff=530, σy=345, Fc=24,
    /// pw=0.002, σwy=295, h0=3000。
    fn sample_input() -> RcCapacityInput {
        RcCapacityInput {
            b: 400.0,
            d: 600.0,
            at: 1935.0,
            d_eff: 530.0,
            sigma_y: 345.0,
            fc: 24.0,
            pw: 0.002,
            sigma_wy: 295.0,
            clear_span: 3000.0,
            sigma_0: 0.0,
        }
    }

    /// `rc_capacity_input_from_rect` が用途別配筋と `Material` を `RcCapacityInput` へ
    /// 配線する処理を、独立に計算した代表値で確認する。main_x = 8-D22 の総断面積の
    /// 半分が引張側 `at`、かぶり・帯筋径・主筋径から決まる有効せいが `d_eff`。
    #[test]
    fn rc_capacity_input_from_rect_matches_handcalc_without_strength_factor() {
        let rebar = RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![4],
            y: vec![2],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 150.0,
                legs_x: 2,
                legs_y: 2,
            },
        };
        let mat = Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "FC24".into(),
            category: MaterialCategory::Concrete,
            young: 23000.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
        };
        let input = rc_capacity_input_from_rect(400.0, 600.0, &rebar, &mat, None, None, 3000.0)
            .expect("fc set");
        let at_expected = 8.0 * std::f64::consts::PI * (22.0_f64 / 2.0).powi(2) / 2.0;
        let d_eff_expected = 600.0 - (40.0 + 10.0 + 22.0 / 2.0);
        assert!((input.at - at_expected).abs() < 1e-9);
        assert!((input.d_eff - d_eff_expected).abs() < 1e-9);
        assert_eq!(input.sigma_y, 345.0);
        assert_eq!(input.sigma_wy, 295.0);
    }

    #[test]
    fn test_rc_mu_simple_matches_handcalc() {
        let inp = sample_input();
        // 手計算: Mu = 0.9·at·σy·d = 0.9*1935*345*530（技術基準解説書 P.623）
        let mu_handcalc = 0.9 * 1935.0 * 345.0 * 530.0;
        let mu = rc_mu_simple(&inp);
        assert!(
            (mu - mu_handcalc).abs() < 1e-6,
            "Mu={} vs handcalc={}",
            mu,
            mu_handcalc
        );
    }

    /// 曲げひび割れモーメント Mc = κ·√Fc·Ze（κ=0.56）と不正入力の 0 返し。
    #[test]
    fn test_rc_crack_moment_matches_handcalc() {
        let ze = 300.0 * 600.0_f64.powi(2) / 6.0;
        let mc = rc_crack_moment(24.0, ze);
        assert!(
            (mc - RC_CRACK_COEF * 24.0_f64.sqrt() * ze).abs() < 1e-3,
            "Mc={mc}"
        );
        assert_eq!(rc_crack_moment(0.0, ze), 0.0);
        assert_eq!(rc_crack_moment(24.0, 0.0), 0.0);
        assert_eq!(rc_crack_moment(24.0, -1.0), 0.0);
    }

    #[test]
    fn test_rc_column_mu_simple_branches() {
        let inp = sample_input();
        let (b, d, at, sy, fc) = (400.0_f64, 600.0, 1935.0, 345.0, 24.0);
        let ag = 2.0 * at; // 対称配筋の全主筋
        let n_bal = 0.4 * b * d * fc; // 2,304,000 N
        let n_max = b * d * fc + ag * sy;

        // N=0: Mu = 0.8・at・σy・D。
        let mu0 = rc_column_mu_simple(&inp, ag, 0.0);
        assert!((mu0 - 0.8 * at * sy * d).abs() < 1e-6);

        // 中間圧縮軸力（N=0.2bDFc）: 軸力項で Mu が増える。
        let n1 = 0.2 * b * d * fc;
        let mu1 = rc_column_mu_simple(&inp, ag, n1);
        let expect1 = 0.8 * at * sy * d + 0.5 * n1 * d * (1.0 - n1 / (b * d * fc));
        assert!((mu1 - expect1).abs() < 1e-6);
        assert!(mu1 > mu0);

        // 高圧縮域（N>0.4bDFc）: Nmax で 0 に線形低減。
        let mu_at_nmax = rc_column_mu_simple(&inp, ag, n_max);
        assert!(mu_at_nmax.abs() < 1e-6);
        let n2 = 0.7 * n_max + 0.3 * n_bal;
        let mu2 = rc_column_mu_simple(&inp, ag, n2);
        let m_bal = 0.8 * at * sy * d + 0.12 * b * d * d * fc;
        let expect2 = m_bal * (n_max - n2) / (n_max - n_bal);
        assert!((mu2 - expect2).abs() < 1e-6);

        // 引張軸力: Mu = 0.8atσyD + 0.4ND（N<0）で減少、Nmin 以下で 0。
        let n3 = -0.5 * ag * sy;
        let mu3 = rc_column_mu_simple(&inp, ag, n3);
        assert!((mu3 - (0.8 * at * sy * d + 0.4 * n3 * d)).abs() < 1e-6);
        assert!(mu3 < mu0);
        // 境界の連続性: N=0.4bDFc で両分岐が一致する。
        let lo = rc_column_mu_simple(&inp, ag, n_bal - 1e-6);
        let hi = rc_column_mu_simple(&inp, ag, n_bal + 1e-6);
        assert!(
            (lo - hi).abs() / lo < 1e-6,
            "branch continuity: {lo} vs {hi}"
        );
    }

    #[test]
    fn test_rc_qmu_simple_matches_handcalc() {
        let inp = sample_input();
        let mu_handcalc = 0.9 * 1935.0 * 345.0 * 530.0;
        let qmu_handcalc = 2.0 * mu_handcalc / 3000.0;
        let qmu = rc_qmu_simple(&inp);
        assert!(
            (qmu - qmu_handcalc).abs() < 1e-6,
            "Qmu={} vs handcalc={}",
            qmu,
            qmu_handcalc
        );
    }

    #[test]
    fn adopted_alpha_branches_preserve_boundary_and_extrapolation() {
        for (r, expected) in [
            (1.0, 0.2025),
            (2.0, 0.23733),
            (5.0, 0.4714605),
            (0.5, 0.185085),
            (8.0, 0.7800705),
        ] {
            assert!((rc_alpha_y_sugano(0.01, r, 0.9, 10.0) - expected).abs() < 1e-12);
        }
        let epsilon = 1e-8;
        assert!((rc_alpha_y_sugano(0.01, 2.0 - epsilon, 0.9, 10.0) - 0.23733).abs() < 1e-9);
        assert!((rc_alpha_y_sugano(0.01, 2.0 + epsilon, 0.9, 10.0) - 0.1628505).abs() < 2e-9);
        assert!(rc_alpha_y_sugano_checked(0.5, 3.0, 0.9, 10.0).unwrap() < 0.0);
        assert!(rc_alpha_y_sugano_checked(0.01, 20.0, 0.9, 10.0).unwrap() > 1.0);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(rc_alpha_y_sugano_checked(bad, 3.0, 0.9, 10.0).is_err());
            assert!(rc_alpha_y_sugano_checked(0.01, bad, 0.9, 10.0).is_err());
            assert!(rc_alpha_y_sugano_checked(0.01, 3.0, bad, 10.0).is_err());
            assert!(rc_alpha_y_sugano_checked(0.01, 3.0, 0.9, bad).is_err());
        }
        assert!(rc_alpha_y_sugano_checked(0.01, 3.0, 1.01, 10.0).is_err());
    }

    #[test]
    fn test_rc_qsu_simple_matches_handcalc() {
        let inp = sample_input();
        // 手計算(クランプ域内): pt=100*1935/(400*530)=0.912736%,
        // shear_span_ratio=3000/(2*530)=2.830189(1.0-3.0の範囲内なのでクランプなし),
        // pw=0.002(0.012以下なのでクランプなし)。
        let pt: f64 = 100.0 * 1935.0 / (400.0 * 530.0);
        let j = 7.0 * 530.0 / 8.0;
        let shear_span_ratio: f64 = 3000.0 / (2.0 * 530.0);
        let concrete_term = 0.068 * pt.powf(0.23) * (24.0 + 18.0) / (shear_span_ratio + 0.12);
        let hoop_term = 0.85 * (0.002_f64 * 295.0).sqrt();
        let qsu_handcalc = (concrete_term + hoop_term) * 400.0 * j;

        let qsu = rc_qsu_simple(&inp);
        assert!(
            (qsu - qsu_handcalc).abs() < 1e-6,
            "Qsu={} vs handcalc={}",
            qsu,
            qsu_handcalc
        );
        // 参考: せん断余裕度 Qsu/Qmu ≈ 1.6 程度（曲げ降伏が先行する健全な部材の目安）。
        let qmu = rc_qmu_simple(&inp);
        assert!(qsu / qmu > 1.0, "Qsu/Qmu={}", qsu / qmu);
    }

    #[test]
    fn test_rc_qsu_simple_clamps_shear_span_ratio_low() {
        // h0 を極端に短くすると shear_span_ratio = h0/(2*d_eff) < 1.0 → 1.0 にクランプ。
        let mut inp = sample_input();
        inp.clear_span = 200.0; // 200/(2*530)=0.1887 < 1.0
        let qsu = rc_qsu_simple(&inp);

        let pt: f64 = 100.0 * 1935.0 / (400.0 * 530.0);
        let j = 7.0 * 530.0 / 8.0;
        let concrete_term = 0.068 * pt.powf(0.23) * (24.0 + 18.0) / (1.0 + 0.12); // クランプ後 1.0
        let hoop_term = 0.85 * (0.002_f64 * 295.0).sqrt();
        let qsu_handcalc = (concrete_term + hoop_term) * 400.0 * j;
        assert!(
            (qsu - qsu_handcalc).abs() < 1e-6,
            "Qsu={} vs handcalc(clamped)={}",
            qsu,
            qsu_handcalc
        );
    }

    #[test]
    fn test_rc_qsu_simple_clamps_shear_span_ratio_high() {
        // h0 を極端に長くすると shear_span_ratio = h0/(2*d_eff) > 3.0 → 3.0 にクランプ。
        let mut inp = sample_input();
        inp.clear_span = 6000.0; // 6000/(2*530)=5.660 > 3.0
        let qsu = rc_qsu_simple(&inp);

        let pt: f64 = 100.0 * 1935.0 / (400.0 * 530.0);
        let j = 7.0 * 530.0 / 8.0;
        let concrete_term = 0.068 * pt.powf(0.23) * (24.0 + 18.0) / (3.0 + 0.12); // クランプ後 3.0
        let hoop_term = 0.85 * (0.002_f64 * 295.0).sqrt();
        let qsu_handcalc = (concrete_term + hoop_term) * 400.0 * j;
        assert!(
            (qsu - qsu_handcalc).abs() < 1e-6,
            "Qsu={} vs handcalc(clamped)={}",
            qsu,
            qsu_handcalc
        );
    }

    #[test]
    fn test_rc_qsu_simple_clamps_pw_upper_bound() {
        // pw が適用範囲の上限 0.012 を超える場合は 0.012 にクランプされる。
        let mut inp_over = sample_input();
        inp_over.pw = 0.05;
        let mut inp_clamped = sample_input();
        inp_clamped.pw = 0.012;

        let qsu_over = rc_qsu_simple(&inp_over);
        let qsu_clamped = rc_qsu_simple(&inp_clamped);
        assert!(
            (qsu_over - qsu_clamped).abs() < 1e-9,
            "qsu_over={} vs qsu_clamped={}",
            qsu_over,
            qsu_clamped
        );
        // クランプなしでは pw=0.05 の方が pw=0.002 より Qsu が大きくなるはず。
        assert!(qsu_clamped > rc_qsu_simple(&sample_input()));
    }

    #[test]
    fn test_rc_mu_simple_invalid_inputs_are_zero() {
        let base = sample_input();

        let mut at_zero = sample_input();
        at_zero.at = 0.0;
        assert_eq!(rc_mu_simple(&at_zero), 0.0);

        let mut d_eff_zero = sample_input();
        d_eff_zero.d_eff = 0.0;
        assert_eq!(rc_mu_simple(&d_eff_zero), 0.0);

        let mut sigma_y_zero = sample_input();
        sigma_y_zero.sigma_y = 0.0;
        assert_eq!(rc_mu_simple(&sigma_y_zero), 0.0);

        // 妥当な入力は正の値になることの確認（比較対象）。
        assert!(rc_mu_simple(&base) > 0.0);
    }

    #[test]
    fn test_rc_qmu_simple_zero_clear_span_is_zero() {
        let mut inp = sample_input();
        inp.clear_span = 0.0;
        assert_eq!(rc_qmu_simple(&inp), 0.0);

        let mut inp_neg = sample_input();
        inp_neg.clear_span = -100.0;
        assert_eq!(rc_qmu_simple(&inp_neg), 0.0);
    }

    #[test]
    fn test_rc_qsu_simple_invalid_inputs_are_zero() {
        let mut b_zero = sample_input();
        b_zero.b = 0.0;
        assert_eq!(rc_qsu_simple(&b_zero), 0.0);

        let mut d_eff_zero = sample_input();
        d_eff_zero.d_eff = 0.0;
        assert_eq!(rc_qsu_simple(&d_eff_zero), 0.0);

        let mut at_zero = sample_input();
        at_zero.at = 0.0;
        assert_eq!(rc_qsu_simple(&at_zero), 0.0);

        let mut fc_zero = sample_input();
        fc_zero.fc = 0.0;
        assert_eq!(rc_qsu_simple(&fc_zero), 0.0);

        let mut span_zero = sample_input();
        span_zero.clear_span = 0.0;
        assert_eq!(rc_qsu_simple(&span_zero), 0.0);
    }

    #[test]
    fn test_rc_qsu_simple_axial_term_adds_01_sigma0_b_j() {
        // 適用範囲内(0〜0.4Fc=9.6)の sigma_0=5.0 のとき、Qsu は
        // sigma_0=0 の場合に対して厳密に 0.1・σ0・b・j 分だけ増える。
        let mut inp = sample_input();
        let qsu_base = rc_qsu_simple(&inp);
        inp.sigma_0 = 5.0;
        let qsu_with_axial = rc_qsu_simple(&inp);
        let j = 7.0 * 530.0 / 8.0;
        let expected_delta = 0.1 * 5.0 * 400.0 * j;
        assert!(
            (qsu_with_axial - qsu_base - expected_delta).abs() < 1e-6,
            "delta={} expected={}",
            qsu_with_axial - qsu_base,
            expected_delta
        );
    }

    #[test]
    fn test_rc_qsu_simple_sigma_0_clamped_to_upper_bound_04fc() {
        // Fc=24.0 → 上限 0.4*24=9.6。これを超える sigma_0=20.0 は 9.6 にクランプされる。
        let mut inp_over = sample_input();
        inp_over.sigma_0 = 20.0;
        let mut inp_clamped = sample_input();
        inp_clamped.sigma_0 = 0.4 * 24.0;
        assert!((rc_qsu_simple(&inp_over) - rc_qsu_simple(&inp_clamped)).abs() < 1e-9);
        // クランプなしでは sigma_0=9.6 の方が sigma_0=0 より Qsu が大きいはず。
        assert!(rc_qsu_simple(&inp_clamped) > rc_qsu_simple(&sample_input()));
    }

    #[test]
    fn test_rc_qsu_simple_sigma_0_negative_is_clamped_to_zero() {
        // 負の sigma_0（引張）は 0 とみなす（Qsu を低減しない安全側）。
        let mut inp_neg = sample_input();
        inp_neg.sigma_0 = -10.0;
        let qsu_neg = rc_qsu_simple(&inp_neg);
        let qsu_zero = rc_qsu_simple(&sample_input());
        assert!((qsu_neg - qsu_zero).abs() < 1e-9);
    }
}
