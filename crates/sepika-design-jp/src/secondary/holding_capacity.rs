use sepika_core::ids::{ElemId, StoryId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MemberRank {
    FA,
    FB,
    FC,
    FD,
}

pub struct StoryCheck {
    pub story: StoryId,
    pub rs: f64,
    pub re: f64,
    pub ds: f64,
    pub fes: f64,
    pub qu: f64,
    pub qud: f64,
    pub qun: f64,
    pub drift_angle: f64,
    pub ok: bool,
}

pub struct HoldingCapacityResult {
    pub stories: Vec<StoryCheck>,
    pub member_ranks: Vec<(ElemId, MemberRank)>,
}

pub fn check_story_drift(story_height: f64, interstory_drift: f64) -> bool {
    let angle = interstory_drift / story_height;
    angle <= 1.0 / 200.0
}

/// 下層から上層の階高・重心位置の層間変位 `[mm]` から剛性率を返す。
/// 同数の有限な正値が必要。空入力・ゼロ変位・演算不能は Err。
pub fn stiffness_ratios(story_heights: &[f64], story_drifts: &[f64]) -> Result<Vec<f64>, String> {
    if story_heights.is_empty() || story_heights.len() != story_drifts.len() {
        return Err("Rs: 階高と層間変位の層数が不一致又は空です".into());
    }
    let ks = story_heights
        .iter()
        .zip(story_drifts)
        .enumerate()
        .map(|(i, (&h, &d))| {
            if !h.is_finite() || h <= 0.0 {
                return Err(format!(
                    "Rs: 下から{}層の階高が正の有限値ではありません",
                    i + 1
                ));
            }
            if !d.is_finite() || d <= 0.0 {
                return Err(format!(
                    "Rs: 下から{}層の層間変位が正の有限値ではありません",
                    i + 1
                ));
            }
            let k = h / d;
            if !k.is_finite() || k <= 0.0 {
                return Err(format!("Rs: 下から{}層の相対剛性を算定できません", i + 1));
            }
            Ok(k)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mean = ks.iter().sum::<f64>() / ks.len() as f64;
    if !mean.is_finite() || mean <= 0.0 {
        return Err("Rs: 全層の相対剛性の平均を算定できません".into());
    }
    ks.into_iter()
        .enumerate()
        .map(|(i, k)| {
            let rs = k / mean;
            if !rs.is_finite() || rs <= 0.0 {
                Err(format!("Rs: 下から{}層の剛性率を算定できません", i + 1))
            } else {
                Ok(rs)
            }
        })
        .collect()
}

/// 偏心距離 e `[mm]` と正の弾力半径 r `[mm]` から偏心率を返す。
/// 非有限値・非正の半径・演算不能は Err。距離の符号は絶対値として扱う。
pub fn eccentricity_ratio(e: f64, r: f64) -> Result<f64, String> {
    if !e.is_finite() || !r.is_finite() || r <= 0.0 {
        return Err("Re: 偏心距離が非有限又は弾力半径が正の有限値ではありません".into());
    }
    let re = (e / r).abs();
    if !re.is_finite() {
        return Err("Re: 偏心距離と弾力半径から偏心率を算定できません".into());
    }
    Ok(re)
}

/// 有限な非負の剛性率 Rs から Fs を返す。不正値は Err。
pub fn fs(rs: f64) -> Result<f64, String> {
    if !rs.is_finite() || rs < 0.0 {
        return Err("Fs: 剛性率が非負の有限値ではありません".into());
    }
    Ok(if rs >= 0.6 { 1.0 } else { 2.0 - rs / 0.6 })
}

/// 有限な非負の偏心率 Re から Fe を返す。不正値は Err。
pub fn fe(re: f64) -> Result<f64, String> {
    if !re.is_finite() || re < 0.0 {
        return Err("Fe: 偏心率が非負の有限値ではありません".into());
    }
    Ok(if re <= 0.15 {
        1.0
    } else if re >= 0.30 {
        1.5
    } else {
        1.0 + (re - 0.15) / 0.30
    })
}

/// 有限な非負の Rs・Re から形状係数 Fes を返す。不正値は Err。
pub fn fes(rs: f64, re: f64) -> Result<f64, String> {
    Ok(fs(rs)? * fe(re)?)
}

/// 架構種別。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameType {
    RcFrame,
    RcWall,
    SteelFrame,
    SteelBrace,
}

/// 告示1792 の Ds 値。
pub fn ds_value(frame: FrameType, rank: MemberRank) -> f64 {
    use FrameType::*;
    use MemberRank::*;
    match (frame, rank) {
        (RcFrame, FA) => 0.30,
        (RcFrame, FB) => 0.35,
        (RcFrame, FC) => 0.40,
        (RcFrame, FD) => 0.45,
        (RcWall, FA) => 0.35,
        (RcWall, FB) => 0.40,
        (RcWall, FC) => 0.45,
        (RcWall, FD) => 0.55,
        (SteelFrame, FA) => 0.25,
        (SteelFrame, FB) => 0.30,
        (SteelFrame, FC) => 0.35,
        (SteelFrame, FD) => 0.40,
        (SteelBrace, FA) => 0.30,
        (SteelBrace, FB) => 0.35,
        (SteelBrace, FC) => 0.40,
        (SteelBrace, FD) => 0.50,
    }
}

/// 二次設計の地震時層せん断 Qud（C0 = 1.0 の Ai 分布層せん断 Qi）。
/// 戻り値は層せん断 Qi（下→上インデックス）。
pub fn qud_by_story(story_weights_bottom_to_top: &[f64], z: f64, rt: f64, t: f64) -> Vec<f64> {
    sepika_load::ai::ai_distribution(story_weights_bottom_to_top, z, rt, 1.0, t).qi
}

use sepika_solver::nonlinear::pushover::PushoverResult;

/// 二次設計（保有水平耐力）の層チェックを統合する。
/// 判定は `ok = (Qu ≥ Qun)`、`Qun = Ds·Fes·Qud`。
#[allow(clippy::too_many_arguments)]
pub fn check_holding_capacity(
    pushover: &PushoverResult,
    qud_by_story: &[f64],
    ds_by_story: &[f64],
    fes_by_story: &[f64],
    rs_by_story: &[f64],
    re_by_story: &[f64],
    story_heights: &[f64],
    member_ranks: Vec<(ElemId, MemberRank)>,
) -> Result<HoldingCapacityResult, String> {
    let last_point = pushover
        .capacity_curve
        .last()
        .ok_or("保有耐力: 性能曲線がありません")?;
    let n = qud_by_story.len();
    let mut qu = vec![0.0_f64; n];
    for point in &pushover.capacity_curve {
        if point.story_shear.len() != n || point.story_shear.iter().any(|q| !q.is_finite()) {
            return Err("保有耐力: 性能曲線の層せん断力が欠損又は不正です".into());
        }
        for (peak, &q) in qu.iter_mut().zip(&point.story_shear) {
            *peak = peak.max(q);
        }
    }
    check_holding_capacity_at_state(
        &qu,
        &last_point.story_drift,
        qud_by_story,
        ds_by_story,
        fes_by_story,
        rs_by_story,
        re_by_story,
        story_heights,
        member_ranks,
    )
}

/// 明示採用した同一状態の層力 `[N]`・層間変位 `[mm]` で判定する。
/// 全層の入力長・有限性・正の階高が必要。欠損や不正値は Err。
#[allow(clippy::too_many_arguments)]
pub fn check_holding_capacity_at_state(
    qu_n: &[f64],
    story_drifts_mm: &[f64],
    qud_n: &[f64],
    ds: &[f64],
    fes: &[f64],
    rs: &[f64],
    re: &[f64],
    heights_mm: &[f64],
    member_ranks: Vec<(ElemId, MemberRank)>,
) -> Result<HoldingCapacityResult, String> {
    let n = qud_n.len();
    if n == 0
        || [qu_n, story_drifts_mm, ds, fes, rs, re, heights_mm]
            .iter()
            .any(|v| v.len() != n || v.iter().any(|x| !x.is_finite()))
        || qud_n.iter().any(|x| !x.is_finite())
        || heights_mm.iter().any(|h| *h <= 0.0)
    {
        return Err("選定状態の層力・変形・階高・設計入力が欠損又は不正です".into());
    }
    for i in 0..n {
        self::fes(rs[i], re[i]).map_err(|reason| format!("下から{}層: {reason}", i + 1))?;
        if fes[i] < 1.0 {
            return Err(format!("下から{}層: Fesが1未満です", i + 1));
        }
        if !(ds[i] * fes[i] * qud_n[i]).is_finite()
            || !(story_drifts_mm[i] / heights_mm[i]).is_finite()
        {
            return Err(format!(
                "下から{}層: 必要耐力又は層間変形角を算定できません",
                i + 1
            ));
        }
    }
    let stories = (0..n)
        .map(|i| {
            story_check(
                StoryId(i as u32),
                qu_n[i].abs(),
                story_drifts_mm[i] / heights_mm[i],
                qud_n[i],
                ds[i],
                fes[i],
                rs[i],
                re[i],
            )
        })
        .collect();
    Ok(HoldingCapacityResult {
        stories,
        member_ranks,
    })
}

#[allow(clippy::too_many_arguments)]
fn story_check(
    story: StoryId,
    qu: f64,
    drift_angle: f64,
    qud: f64,
    ds: f64,
    fes: f64,
    rs: f64,
    re: f64,
) -> StoryCheck {
    let qun = ds * fes * qud;
    StoryCheck {
        story,
        rs,
        re,
        ds,
        fes,
        qu,
        qud,
        qun,
        drift_angle,
        ok: qu >= qun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 明示状態のqu120と変位30を直接判定し欠損非有限を拒否する() {
        let run = |drift: &[f64], heights: &[f64]| {
            check_holding_capacity_at_state(
                &[120_000.0],
                drift,
                &[200_000.0],
                &[0.3],
                &[1.0],
                &[1.0],
                &[0.0],
                heights,
                vec![],
            )
        };
        let r = run(&[30.0], &[3000.0]).unwrap();
        assert_eq!(r.stories[0].qu, 120_000.0);
        assert_eq!(r.stories[0].qun, 60_000.0);
        assert_eq!(r.stories[0].drift_angle, 0.01);
        assert!(r.stories[0].ok);
        for drift in [vec![], vec![f64::NAN], vec![f64::INFINITY]] {
            assert!(run(&drift, &[3000.0]).is_err());
        }
        assert!(run(&[30.0], &[0.0]).is_err());
    }

    #[test]
    fn test_stiffness_ratios_example() {
        let heights = vec![1.0, 1.0, 1.0];
        let drifts = vec![1.0 / 200.0, 1.0 / 150.0, 1.0 / 250.0];
        let rs = stiffness_ratios(&heights, &drifts).unwrap();
        assert_eq!(rs.len(), 3);
        for (r, expected) in rs.iter().zip([1.0, 0.75, 1.25].iter()) {
            assert!((r - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn stiffness_ratios_reject_missing_nonfinite_nonpositive_and_undefined_inputs() {
        for (heights, drifts) in [
            (vec![], vec![]),
            (vec![3.0], vec![]),
            (vec![3.0], vec![0.0]),
            (vec![3.0, 3.0], vec![0.01, 0.0]),
            (vec![0.0], vec![1.0]),
            (vec![-3.0], vec![1.0]),
            (vec![3.0], vec![-1.0]),
            (vec![f64::NAN], vec![1.0]),
            (vec![3.0], vec![f64::INFINITY]),
            (vec![f64::MAX], vec![f64::MIN_POSITIVE]),
            (vec![f64::MAX; 2], vec![1.0; 2]),
        ] {
            assert!(
                stiffness_ratios(&heights, &drifts).is_err(),
                "{heights:?} {drifts:?}"
            );
        }
        assert_eq!(
            stiffness_ratios(&[3.0, 3.0], &[0.01, 0.01]).unwrap(),
            vec![1.0, 1.0]
        );
    }

    #[test]
    fn test_check_story_drift_ok() {
        assert!(check_story_drift(3.0, 0.01));
        assert!(!check_story_drift(3.0, 0.02));
    }

    #[test]
    fn test_eccentricity_ratio_basic() {
        assert!((eccentricity_ratio(1.5, 3.0).unwrap() - 0.5).abs() < 1e-9);
        assert!(eccentricity_ratio(1.5, 0.0).is_err());
        assert_eq!(eccentricity_ratio(0.0, 3.0).unwrap(), 0.0);
    }

    #[test]
    fn test_fs_branch_and_cap() {
        assert!((fs(0.6).unwrap() - 1.0).abs() < 1e-9);
        assert!((fs(1.0).unwrap() - 1.0).abs() < 1e-9);
        assert!((fs(0.3).unwrap() - 1.5).abs() < 1e-9);
        assert!((fs(0.0).unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_fe_branch_and_cap() {
        assert!((fe(0.15).unwrap() - 1.0).abs() < 1e-9);
        assert!((fe(0.0).unwrap() - 1.0).abs() < 1e-9);
        assert!((fe(0.30).unwrap() - 1.5).abs() < 1e-9);
        assert!((fe(0.45).unwrap() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn test_fes_product_and_cap() {
        assert!((fes(0.8, 0.1).unwrap() - 1.0).abs() < 1e-9);
        assert!((fes(0.3, 0.30).unwrap() - 2.25).abs() < 1e-9);
    }

    #[test]
    fn fes718_normal_values_and_invalid_inputs() {
        for (re, expected) in [(0.15, 1.0), (0.225, 1.25), (0.30, 1.5)] {
            assert!((fe(re).unwrap() - expected).abs() < 1e-12);
        }
        for (rs, expected) in [(0.60, 1.0), (0.30, 1.5), (0.15, 1.75)] {
            assert!((fs(rs).unwrap() - expected).abs() < 1e-12);
        }
        assert!((fes(0.15, 0.225).unwrap() - 2.1875).abs() < 1e-12);
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert!(fe(invalid).is_err());
            assert!(fs(invalid).is_err());
            assert!(fes(invalid, 0.0).is_err());
            assert!(fes(1.0, invalid).is_err());
            assert!(eccentricity_ratio(0.0, invalid).is_err());
        }
        assert!(eccentricity_ratio(f64::NAN, 1.0).is_err());
        assert!(eccentricity_ratio(f64::INFINITY, 1.0).is_err());
        assert!(eccentricity_ratio(f64::MAX, f64::MIN_POSITIVE).is_err());
    }

    #[test]
    fn fes718_holding_capacity_rejects_invalid_fes_and_overflow() {
        let run = |f: f64, rs: f64, re: f64, qud: f64| {
            check_holding_capacity_at_state(
                &[100.0],
                &[1.0],
                &[qud],
                &[0.3],
                &[f],
                &[rs],
                &[re],
                &[3000.0],
                vec![],
            )
        };
        for invalid in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert!(run(invalid, 1.0, 0.0, 100.0).is_err());
        }
        assert!(run(1.0, f64::NAN, 0.0, 100.0).is_err());
        assert!(run(1.0, 1.0, f64::NAN, 100.0).is_err());
        assert!(run(f64::MAX, 1.0, 0.0, f64::MAX).is_err());
        let pushover = pushover_with_qu(vec![100.0], vec![1.0]);
        assert!(check_holding_capacity(
            &pushover,
            &[100.0],
            &[0.3],
            &[],
            &[1.0],
            &[0.0],
            &[3000.0],
            vec![]
        )
        .is_err());
        assert!(check_holding_capacity(
            &pushover,
            &[100.0],
            &[0.3],
            &[f64::NAN],
            &[1.0],
            &[0.0],
            &[3000.0],
            vec![]
        )
        .is_err());
    }

    /// 告示1792 の Ds 値: 架構種別 × 部材ランクの全組合せ。
    #[test]
    fn test_ds_value_all_combinations() {
        use FrameType::*;
        use MemberRank::*;
        for (f, r, expected) in [
            (RcFrame, FA, 0.30),
            (RcFrame, FB, 0.35),
            (RcFrame, FC, 0.40),
            (RcFrame, FD, 0.45),
            (RcWall, FA, 0.35),
            (RcWall, FB, 0.40),
            (RcWall, FC, 0.45),
            (RcWall, FD, 0.55),
            (SteelFrame, FA, 0.25),
            (SteelFrame, FB, 0.30),
            (SteelFrame, FC, 0.35),
            (SteelFrame, FD, 0.40),
            (SteelBrace, FA, 0.30),
            (SteelBrace, FB, 0.35),
            (SteelBrace, FC, 0.40),
            (SteelBrace, FD, 0.50),
        ] {
            assert!(
                (ds_value(f, r) - expected).abs() < 1e-9,
                "ds_value({f:?}, {r:?}) should be {expected}, got {}",
                ds_value(f, r)
            );
        }
    }

    /// Qu を持つ capacity_curve 1点だけの PushoverResult を作る。
    fn pushover_with_qu(story_shear: Vec<f64>, story_drift: Vec<f64>) -> PushoverResult {
        use sepika_solver::nonlinear::pushover::{CapacityPoint, MechanismType};
        PushoverResult {
            wall_capacities: Vec::new(),
            rc_wall_shear_method: sepika_core::model::RcWallShearMethod::default(),
            steps: vec![],
            wall_history: None,
            wall_run: None,
            confirmed_history: None,
            ds_evaluation: None,
            capacity_evaluation: None,
            capacity_curve: vec![CapacityPoint {
                step: 0,
                roof_disp: 0.0,
                base_shear: story_shear.first().copied().unwrap_or(0.0),
                story_shear,
                story_drift,
            }],
            hinges: vec![],
            shear_yields: vec![],
            mechanism: MechanismType::Overall,
            qu: 0.0,
            member_response: vec![],
            control: Default::default(),
            member_history: vec![],
            fiber_states: vec![],
            termination: Default::default(),
        }
    }

    /// 保有水平耐力 Qu は性能曲線上の層別ピーク層せん断を採る。
    /// 最終点より前にピークがある曲線で検証する。
    #[test]
    fn test_check_holding_capacity_uses_peak_not_last_point() {
        use sepika_solver::nonlinear::pushover::{CapacityPoint, MechanismType};
        // 1層。層せん断は 100 → 150(ピーク) → 120(劣化) と推移。
        let pushover = PushoverResult {
            wall_capacities: Vec::new(),
            rc_wall_shear_method: sepika_core::model::RcWallShearMethod::default(),
            steps: vec![],
            wall_history: None,
            wall_run: None,
            confirmed_history: None,
            ds_evaluation: None,
            capacity_evaluation: None,
            capacity_curve: vec![
                CapacityPoint {
                    step: 0,
                    roof_disp: 10.0,
                    base_shear: 100.0,
                    story_shear: vec![100.0],
                    story_drift: vec![5.0],
                },
                CapacityPoint {
                    step: 1,
                    roof_disp: 20.0,
                    base_shear: 150.0,
                    story_shear: vec![150.0],
                    story_drift: vec![8.0],
                },
                CapacityPoint {
                    step: 2,
                    roof_disp: 30.0,
                    base_shear: 120.0,
                    story_shear: vec![120.0],
                    story_drift: vec![12.0],
                },
            ],
            hinges: vec![],
            shear_yields: vec![],
            mechanism: MechanismType::Overall,
            qu: 150.0,
            member_response: vec![],
            control: Default::default(),
            member_history: vec![],
            fiber_states: vec![],
            termination: Default::default(),
        };
        let result = check_holding_capacity(
            &pushover,
            &[100.0],
            &[0.3],
            &[1.0],
            &[1.0],
            &[0.0],
            &[3000.0],
            vec![],
        )
        .unwrap();
        // Qu はピーク 150（最終点 120 ではない）。
        assert!(
            (result.stories[0].qu - 150.0).abs() < 1e-9,
            "Qu should be peak 150, got {}",
            result.stories[0].qu
        );
        // 層間変形角は最終点（最大変位 12mm）から。
        assert!((result.stories[0].drift_angle - 12.0 / 3000.0).abs() < 1e-9);
    }

    #[test]
    fn test_check_holding_capacity_basic() {
        // Qu は pushover 性能曲線の層別ピークから取得（単点なので [100,200] がそのまま）。
        let pushover = pushover_with_qu(vec![100.0, 200.0], vec![15.0, 12.0]);
        let qud = vec![80.0, 180.0];
        let ds = vec![0.30, 0.35];
        let fes = vec![1.0, 1.0];
        let rs = vec![1.0, 0.75];
        let re = vec![0.05, 0.10];
        let heights = vec![3000.0, 3000.0];
        let result =
            check_holding_capacity(&pushover, &qud, &ds, &fes, &rs, &re, &heights, vec![]).unwrap();
        assert_eq!(result.stories.len(), 2);
        assert!(result.stories[0].ok); // Qu=100 ≥ Qun=24
        assert!(result.stories[1].ok); // Qu=200 ≥ Qun=63
        assert!((result.stories[0].qu - 100.0).abs() < 1e-9);
        assert!((result.stories[0].qun - 24.0).abs() < 1e-9);
        assert!((result.stories[1].qun - 63.0).abs() < 1e-9);
        // Rs/Re が出力に反映されている。
        assert!((result.stories[1].rs - 0.75).abs() < 1e-9);
        assert!((result.stories[0].re - 0.05).abs() < 1e-9);
        // 層間変形角 = drift/height = 15/3000 = 1/200。
        assert!((result.stories[0].drift_angle - 15.0 / 3000.0).abs() < 1e-9);
    }

    #[test]
    fn test_check_holding_capacity_ng() {
        let pushover = pushover_with_qu(vec![20.0, 50.0], vec![10.0, 10.0]);
        let qud = vec![80.0, 180.0];
        let ds = vec![0.30, 0.35];
        let fes = vec![1.0, 1.0];
        let rs = vec![1.0, 1.0];
        let re = vec![0.0, 0.0];
        let heights = vec![3000.0, 3000.0];
        let result =
            check_holding_capacity(&pushover, &qud, &ds, &fes, &rs, &re, &heights, vec![]).unwrap();
        assert!(!result.stories[0].ok); // Qu=20 < Qun=24
        assert!(!result.stories[1].ok); // Qu=50 < Qun=63
    }

    /// 境界（Qu = Qun）は ok。
    #[test]
    fn test_check_holding_capacity_boundary() {
        // Qun = Ds·Fes·Qud = 0.5·1.0·100 = 50。Qu = 50 ちょうど。
        let pushover = pushover_with_qu(vec![50.0], vec![5.0]);
        let qud = vec![100.0];
        let ds = vec![0.5];
        let fes = vec![1.0];
        let rs = vec![1.0];
        let re = vec![0.0];
        let heights = vec![3000.0];
        let result =
            check_holding_capacity(&pushover, &qud, &ds, &fes, &rs, &re, &heights, vec![]).unwrap();
        assert!((result.stories[0].qun - 50.0).abs() < 1e-9);
        assert!(result.stories[0].ok, "Qu=Qun は ok（≥）であるべき");
    }

    /// 等重量3層で qud_by_story の基部層せん断が C0=1.0 版 = C0=0.2 版の 5 倍であること。
    #[test]
    fn test_qud_by_story_linearity_in_c0() {
        use sepika_load::ai::ai_distribution;
        let weights = &[1.0_f64, 1.0, 1.0];
        let z = 1.0;
        let rt = 1.0;
        let t = 0.24;
        let qud = qud_by_story(weights, z, rt, t);
        let qi_c0_02 = ai_distribution(weights, z, rt, 0.2, t).qi;
        // C0 に線形なので qud[0] = 5 · qi_c0_02[0]。
        assert!(
            (qud[0] - 5.0 * qi_c0_02[0]).abs() < 1e-9,
            "qud[0]={} vs 5×qi0.2[0]={}",
            qud[0],
            5.0 * qi_c0_02[0]
        );
        // 基部層せん断 ≈ C0·Ai0·総重量 = 1.0·1.0·3.0 = 3.0。
        assert!(
            (qud[0] - 3.0).abs() < 1e-9,
            "qud[0]={} (expected ≈3.0)",
            qud[0]
        );
    }

    /// 部材ランクが出力に格納される。
    #[test]
    fn test_check_holding_capacity_member_ranks() {
        let pushover = pushover_with_qu(vec![100.0], vec![5.0]);
        let ranks = vec![(ElemId(0), MemberRank::FA), (ElemId(1), MemberRank::FC)];
        let result = check_holding_capacity(
            &pushover,
            &[80.0],
            &[0.3],
            &[1.0],
            &[1.0],
            &[0.0],
            &[3000.0],
            ranks,
        )
        .unwrap();
        assert_eq!(result.member_ranks.len(), 2);
        assert_eq!(result.member_ranks[1].1, MemberRank::FC);
    }
}
