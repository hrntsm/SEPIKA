//! 固定初期剛性による等価トリリニアの同定。

use super::StoryTrilinear;

/// 等価骨格の評価条件。変位は mm、剛性は N/mm。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoryFitOptions {
    pub initial_stiffness: f64,
    pub secant_ratio: f64,
    pub area_limit: f64,
    pub identification_limit: f64,
    /// 降伏後直線の手動区間 [開始変位, 終了変位]。未指定なら自動選定。
    pub manual_interval: Option<[f64; 2]>,
}

/// 採用した降伏後直線と共通評価範囲の規格化誤差。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoryFit {
    pub skeleton: StoryTrilinear,
    pub options: StoryFitOptions,
    pub interval: [f64; 2],
    pub normalized_rmse: f64,
}

/// 等価降伏を同定できなかった理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StoryFitError {
    InvalidInput,
    InitialStiffnessMismatch,
    IdentificationLimitNotReached,
    AreaLimitNotReached,
    StrengthDegradation,
    FirstBreakpointNotIdentified,
    DegenerateArea,
    InsufficientPostYieldInterval,
    NoEqualAreaSolution,
    InvalidSkeleton,
    NoAdmissibleCandidates {
        insufficient_intervals: usize,
        no_solution: usize,
        non_unique: usize,
        invalid_shape: usize,
    },
    PoorApproximation,
    UnstableYield,
}

impl std::fmt::Display for StoryFitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdmissibleCandidates{insufficient_intervals,no_solution,non_unique,invalid_shape}=>write!(f,"全候補が不成立（降伏後区間不足 {insufficient_intervals}件、等面積解不存在 {no_solution}件、等面積解非一意 {non_unique}件、折点・剛性順序不成立 {invalid_shape}件）"),
            _=>f.write_str(match self {
                Self::InvalidInput=>"骨格の入力・初期剛性・評価区間が不正",
                Self::InitialStiffnessMismatch=>"元骨格が固定初期直線を上回ります。初期状態・水平力分布・代表位置を確認してください",
                Self::IdentificationLimitNotReached=>"元増分解析の同定上限へ未到達",
                Self::AreaLimitNotReached=>"目標層間変形角の面積評価上限へ未到達",
                Self::StrengthDegradation=>"耐力低下があり剛性低下型骨格の適用範囲外",
                Self::FirstBreakpointNotIdentified=>"第1折点の割線交点が未同定",
                Self::DegenerateArea=>"面積条件が退化し等価降伏が非一意",
                Self::InsufficientPostYieldInterval=>"降伏後直線の同定区間が最小変形幅に不足、または第1折点より後の区間でない",
                Self::NoEqualAreaSolution=>"指定区間の降伏後直線に成立する等面積解が存在しない",
                Self::InvalidSkeleton=>"独立した折点または剛性順序の成立条件を満たさない",
                Self::PoorApproximation=>"最良候補でも骨格全体の近似誤差が過大",
                Self::UnstableYield=>"近傍精細化または同程度の競合候補間で等価降伏変位が不安定",
                Self::NoAdmissibleCandidates{..}=>unreachable!(),
            }),
        }
    }
}

impl std::error::Error for StoryFitError {}

const SEARCH_DIVISIONS: usize = 128;
const REFINEMENT_DIVISIONS: usize = 16;
const MIN_INTERVAL: f64 = 0.1;
const MAX_RMSE: f64 = 0.05;
const COMPETING_ERROR: f64 = 0.001;
const YIELD_STABILITY: f64 = 0.05;
const NUMERICAL_TOLERANCE: f64 = 1.0e-10;

fn ordinate(points: &[(f64, f64)], d: f64) -> f64 {
    let i = points.partition_point(|p| p.0 < d).max(1);
    let (x, y) = points[i - 1];
    let (xx, yy) = points[i.min(points.len() - 1)];
    if xx == x {
        y
    } else {
        y + (yy - y) * ((d - x) / (xx - x))
    }
}

fn clipped(points: &[(f64, f64)], a: f64, b: f64) -> Vec<(f64, f64)> {
    let mut out = vec![(a, ordinate(points, a))];
    out.extend(points.iter().copied().filter(|p| p.0 > a && p.0 < b));
    out.push((b, ordinate(points, b)));
    out
}

fn area(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|p| 0.5 * (p[0].1 + p[1].1) * (p[1].0 - p[0].0))
        .sum()
}

fn least_squares(points: &[(f64, f64)], a: f64, b: f64) -> (f64, f64) {
    let center = 0.5 * (a + b);
    let center_q = ordinate(points, center);
    let mut integral_q = 0.0;
    let mut integral_xq = 0.0;
    for p in clipped(points, a, b).windows(2) {
        let [(x, q), (y, r)] = [p[0], p[1]];
        let h = y - x;
        integral_q += h * (q + r) / 2.0;
        integral_xq += h
            * ((x - center) * (2.0 * (q - center_q) + r - center_q)
                + (y - center) * (q - center_q + 2.0 * (r - center_q)))
            / 6.0;
    }
    let slope = integral_xq / ((b - a).powi(3) / 12.0);
    (slope, integral_q / (b - a) - slope * center)
}

#[derive(Clone, Copy)]
struct Candidate {
    a: f64,
    b: f64,
    dy: f64,
    qy: f64,
    m: f64,
    intercept: f64,
    error: f64,
}

fn candidate(
    points: &[(f64, f64)],
    k: f64,
    dc: f64,
    target: f64,
    a: f64,
    b: f64,
) -> Result<Candidate, StoryFitError> {
    if b - a < MIN_INTERVAL - NUMERICAL_TOLERANCE || a <= dc {
        return Err(StoryFitError::InsufficientPostYieldInterval);
    }
    let (m, intercept) = least_squares(points, a, b);
    if m < 0.0 || m >= k {
        return Err(StoryFitError::InvalidSkeleton);
    }
    let qc = k * dc;
    let tri_area = |dy: f64| {
        let qy = m * dy + intercept;
        let middle_end = target.min(dy);
        let first = 0.5 * dc * qc;
        if dy == dc {
            first + 0.5 * (m * dc + intercept + m * target + intercept) * (target - dc)
        } else {
            let k2 = (qy - qc) / (dy - dc);
            first
                + qc * (middle_end - dc)
                + 0.5 * k2 * (middle_end - dc).powi(2)
                + if target > dy {
                    0.5 * (qy + m * target + intercept) * (target - dy)
                } else {
                    0.0
                }
        }
    };
    let actual = area(&clipped(points, 0.0, target));
    let (mut low, mut high) = (dc, a);
    let (fl, fh) = (tri_area(low) - actual, tri_area(high) - actual);
    if (fl - fh).abs() < NUMERICAL_TOLERANCE {
        return Err(if fl.abs() < NUMERICAL_TOLERANCE {
            StoryFitError::DegenerateArea
        } else {
            StoryFitError::NoEqualAreaSolution
        });
    }
    if fl * fh > 0.0 {
        return Err(StoryFitError::NoEqualAreaSolution);
    }
    for _ in 0..64 {
        let mid = 0.5 * (low + high);
        if (tri_area(mid) - actual) * fl > 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }
    let dy = 0.5 * (low + high);
    if dy - dc <= NUMERICAL_TOLERANCE {
        return Err(StoryFitError::InvalidSkeleton);
    }
    let qy = m * dy + intercept;
    let k2 = (qy - qc) / (dy - dc);
    if k2 >= k || k2 <= m + NUMERICAL_TOLERANCE || qy <= qc || dy > a {
        return Err(StoryFitError::InvalidSkeleton);
    }
    let tri = [(0.0, 0.0), (dc, qc), (dy, qy), (1.0, m + intercept)];
    let mut breaks: Vec<_> = points.iter().map(|p| p.0).chain([dc, dy]).collect();
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let squared_error: f64 = breaks
        .windows(2)
        .map(|w| {
            let r0 = ordinate(&tri, w[0]) - ordinate(points, w[0]);
            let r1 = ordinate(&tri, w[1]) - ordinate(points, w[1]);
            (w[1] - w[0]) * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0
        })
        .sum();
    Ok(Candidate {
        a,
        b,
        dy,
        qy,
        m,
        intercept,
        error: squared_error.sqrt(),
    })
}

fn retain_candidate(
    result: Result<Candidate, StoryFitError>,
    candidates: &mut Vec<Candidate>,
    failures: &mut [usize; 4],
) {
    match result {
        Ok(candidate) => candidates.push(candidate),
        Err(error) => {
            failures[match error {
                StoryFitError::InsufficientPostYieldInterval => 0,
                StoryFitError::NoEqualAreaSolution => 1,
                StoryFitError::DegenerateArea => 2,
                _ => 3,
            }] += 1
        }
    }
}

/// 単調な Q–d 折れ線を固定初期剛性で近似する。範囲不足・非同定はエラー。
/// 原点を省略できる。記録点追加で重みを変更せず、同定上限外の点を評価しない。
pub fn identify_story_trilinear(
    curve: &[(f64, f64)],
    options: StoryFitOptions,
) -> Result<StoryFit, StoryFitError> {
    identify_with_resolution(curve, options, SEARCH_DIVISIONS, REFINEMENT_DIVISIONS)
}

fn identify_with_resolution(
    curve: &[(f64, f64)],
    options: StoryFitOptions,
    search_divisions: usize,
    refinement_divisions: usize,
) -> Result<StoryFit, StoryFitError> {
    use StoryFitError as E;
    let StoryFitOptions {
        initial_stiffness: k0,
        secant_ratio: alpha,
        area_limit: da,
        identification_limit: df,
        manual_interval,
    } = options;
    if [k0, da, df].iter().any(|x| !x.is_finite() || *x <= 0.0) || !(0.0..1.0).contains(&alpha) {
        return Err(E::InvalidInput);
    }
    if da > df {
        return Err(E::AreaLimitNotReached);
    }
    let mut raw = vec![(0.0, 0.0)];
    for &(d, q) in curve {
        if !d.is_finite() || !q.is_finite() || d < 0.0 || q < 0.0 {
            return Err(E::InvalidInput);
        }
        if (d, q) == *raw.last().unwrap() {
            continue;
        }
        if d <= raw.last().unwrap().0 {
            return Err(E::InvalidInput);
        }
        raw.push((d, q));
        if d >= df {
            break;
        }
    }
    if raw.last().unwrap().0 < df {
        return Err(E::IdentificationLimitNotReached);
    }
    let raw = clipped(&raw, 0.0, df);
    if raw.windows(2).any(|p| p[1].1 < p[0].1) {
        return Err(E::StrengthDegradation);
    }
    let qref = raw.iter().map(|p| p.1).fold(0.0, f64::max);
    if qref <= 0.0 {
        return Err(E::InvalidInput);
    }
    let points: Vec<_> = raw.iter().map(|&(d, q)| (d / df, q / qref)).collect();
    let k = k0 * df / qref;
    if points.iter().any(|&(d, q)| q - k * d > NUMERICAL_TOLERANCE) {
        return Err(E::InitialStiffnessMismatch);
    }
    if !k.is_finite() || k <= 0.0 {
        return Err(E::InvalidInput);
    }
    let target = da / df;
    let mut dc = None;
    for p in points.windows(2) {
        let [(x, q), (y, r)] = [p[0], p[1]];
        let left = q - alpha * k * x;
        let right = r - alpha * k * y;
        if x > 0.0 && left >= 0.0 && right <= 0.0 {
            let star = if left == right {
                x
            } else {
                x + (y - x) * left / (left - right)
            };
            dc = Some(alpha * star);
            break;
        }
    }
    let dc = dc.ok_or(E::FirstBreakpointNotIdentified)?;
    let initial_area_difference =
        (area(&clipped(&points, 0.0, target)) - 0.5 * k * target * target).abs();
    if initial_area_difference < NUMERICAL_TOLERANCE {
        return Err(E::DegenerateArea);
    }
    if target <= dc {
        return Err(E::NoEqualAreaSolution);
    }
    let mut candidates = Vec::new();
    let mut failures = [0_usize; 4];
    let mut coarse_best_yield = None;
    if let Some([a, b]) = manual_interval {
        if !a.is_finite() || !b.is_finite() || a < 0.0 || b > df || b <= a {
            return Err(E::InvalidInput);
        }
        candidates.push(candidate(&points, k, dc, target, a / df, b / df)?);
    } else {
        for i in 1..search_divisions {
            let a = dc + (1.0 - MIN_INTERVAL - dc) * i as f64 / search_divisions as f64;
            retain_candidate(
                candidate(&points, k, dc, target, a, 1.0),
                &mut candidates,
                &mut failures,
            );
        }
        coarse_best_yield = candidates
            .iter()
            .min_by(|a, b| a.error.total_cmp(&b.error))
            .map(|c| c.dy);
        if refinement_divisions > 0 {
            if let Some(best) = candidates
                .iter()
                .min_by(|a, b| a.error.total_cmp(&b.error))
                .copied()
            {
                let width = (1.0 - MIN_INTERVAL - dc) / search_divisions as f64;
                for i in 0..=2 * refinement_divisions {
                    let a = best.a + width * (i as f64 / refinement_divisions as f64 - 1.0);
                    retain_candidate(
                        candidate(&points, k, dc, target, a, 1.0),
                        &mut candidates,
                        &mut failures,
                    );
                }
            }
        }
    }
    let best = candidates
        .iter()
        .min_by(|a, b| a.error.total_cmp(&b.error))
        .copied()
        .ok_or(E::NoAdmissibleCandidates {
            insufficient_intervals: failures[0],
            no_solution: failures[1],
            non_unique: failures[2],
            invalid_shape: failures[3],
        })?;
    if best.error > MAX_RMSE {
        return Err(E::PoorApproximation);
    }
    let competitors: Vec<_> = candidates
        .iter()
        .filter(|c| c.error <= best.error + COMPETING_ERROR)
        .collect();
    if coarse_best_yield.is_some_and(|dy| (dy - best.dy).abs() > YIELD_STABILITY)
        || competitors
            .iter()
            .any(|c| (c.dy - best.dy).abs() > YIELD_STABILITY)
    {
        return Err(E::UnstableYield);
    }
    let chosen = competitors
        .iter()
        .copied()
        .filter(|c| {
            (c.error - best.error).abs() <= NUMERICAL_TOLERANCE
                && (c.dy - best.dy).abs() <= NUMERICAL_TOLERANCE
        })
        .min_by(|a, b| a.a.total_cmp(&b.a))
        .copied()
        .unwrap_or(best);
    Ok(StoryFit {
        skeleton: StoryTrilinear {
            k1: k0,
            d1: dc * df,
            q1: k * dc * qref,
            d2: chosen.dy * df,
            q2: chosen.qy * qref,
            d3: df,
            q3: (chosen.m + chosen.intercept) * qref,
        },
        options,
        interval: [chosen.a * df, chosen.b * df],
        normalized_rmse: chosen.error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(area_limit: f64) -> StoryFitOptions {
        StoryFitOptions {
            initial_stiffness: 100.0,
            secant_ratio: 0.75,
            area_limit,
            identification_limit: 10.0,
            manual_interval: Some([4.0, 10.0]),
        }
    }

    const CURVE: &[(f64, f64)] = &[(1.0, 100.0), (3.0, 140.0), (6.0, 155.0), (10.0, 175.0)];

    #[test]
    fn fixed_stiffness_projected_intersection_and_independent_area() {
        let fit = identify_story_trilinear(CURVE, options(6.0)).unwrap();
        let s = fit.skeleton;
        assert_eq!(s.k1, 100.0);
        assert!((s.d1 - 12.0 / 11.0).abs() < 1e-12);
        assert!((s.q1 - 1200.0 / 11.0).abs() < 1e-10);
        assert!((s.d2 - 173.0 / 47.0).abs() < 1e-10);
        assert!((s.k3() - 5.0).abs() < 1e-10);
        let integral = 0.5 * s.q1 * s.d1
            + 0.5 * (s.q1 + s.q2) * (s.d2 - s.d1)
            + 0.5 * (s.q2 + 155.0) * (6.0 - s.d2);
        assert!((integral - 732.5).abs() < 1e-9);
        assert!(s.k1 > s.k2() && s.k2() > s.k3());
    }

    #[test]
    fn yield_beyond_area_limit_remains_identifiable() {
        let mut o = options(3.0);
        o.manual_interval = Some([5.0, 10.0]);
        let fit = identify_story_trilinear(CURVE, o).unwrap();
        let s = fit.skeleton;
        assert!(s.d2 > 3.0 && s.d2 <= 5.0);
        let integral =
            0.5 * s.d1 * s.q1 + s.q1 * (3.0 - s.d1) + 0.5 * s.k2() * (3.0 - s.d1).powi(2);
        assert!((integral - 290.0).abs() < 1e-9);
    }

    #[test]
    fn elastic_and_degenerate_curves_do_not_create_yield_points() {
        assert_eq!(
            identify_story_trilinear(CURVE, options(1.05)),
            Err(StoryFitError::NoEqualAreaSolution)
        );
        assert_eq!(
            identify_story_trilinear(&[(1.0, 100.0), (10.0, 1000.0)], options(6.0)),
            Err(StoryFitError::FirstBreakpointNotIdentified)
        );
        assert_eq!(
            identify_story_trilinear(CURVE, options(0.5)),
            Err(StoryFitError::DegenerateArea)
        );
        assert_eq!(
            identify_story_trilinear(&CURVE[..3], options(6.0)),
            Err(StoryFitError::IdentificationLimitNotReached)
        );
    }

    #[test]
    fn recording_density_and_points_beyond_identification_limit_do_not_change_fit() {
        let mut dense = vec![
            (0.5, 50.0),
            (1.0, 100.0),
            (2.0, 120.0),
            (3.0, 140.0),
            (4.0, 145.0),
            (6.0, 155.0),
            (8.0, 165.0),
            (10.0, 175.0),
        ];
        let mut o = options(6.0);
        o.manual_interval = None;
        let original = identify_story_trilinear(CURVE, o).unwrap();
        let refined = identify_story_trilinear(&dense, o).unwrap();
        assert!((original.skeleton.d2 - refined.skeleton.d2).abs() < 1e-10);
        assert!((original.interval[0] - refined.interval[0]).abs() < 1e-10);
        dense.push((12.0, 10.0));
        assert_eq!(refined, identify_story_trilinear(&dense, o).unwrap());
    }

    #[test]
    fn stiffness_mismatch_and_even_small_strength_degradation_are_rejected() {
        let mut o = options(6.0);
        o.initial_stiffness = 99.0;
        assert_eq!(
            identify_story_trilinear(CURVE, o),
            Err(StoryFitError::InitialStiffnessMismatch)
        );
        let falling = [
            (1.0, 100.0),
            (3.0, 140.0),
            (6.0, 140.0),
            (10.0, 140.0 - 1e-10),
        ];
        assert_eq!(
            identify_story_trilinear(&falling, options(6.0)),
            Err(StoryFitError::StrengthDegradation)
        );
        let flat = [(0.0, 0.0), (0.1, 1.0), (0.3, 1.4), (0.6, 1.4), (1.0, 1.4)];
        assert_eq!(least_squares(&flat, 0.6, 1.0).0, 0.0);
    }

    #[test]
    fn units_and_load_magnitude_do_not_change_normalized_selection() {
        let mut o = options(6.0);
        o.manual_interval = None;
        let reference = identify_story_trilinear(CURVE, o).unwrap();
        for (length, force) in [(1e-3, 1e6), (1e3, 1e-6)] {
            let points: Vec<_> = CURVE
                .iter()
                .map(|&(d, q)| (d * length, q * force))
                .collect();
            let fit = identify_story_trilinear(
                &points,
                StoryFitOptions {
                    initial_stiffness: 100.0 * force / length,
                    area_limit: 6.0 * length,
                    identification_limit: 10.0 * length,
                    ..o
                },
            )
            .unwrap();
            assert!((fit.skeleton.d2 / length - reference.skeleton.d2).abs() < 1e-9);
            assert!((fit.interval[0] / length - reference.interval[0]).abs() < 1e-9);
        }
    }

    #[test]
    fn search_resolution_and_nearby_refinement_preserve_identifiable_yield() {
        let mut o = options(6.0);
        o.manual_interval = None;
        let baseline = identify_story_trilinear(CURVE, o).unwrap();
        for (coarse, fine) in [(64, 0), (64, 8), (128, 0), (128, 32), (256, 16)] {
            let result = identify_with_resolution(CURVE, o, coarse, fine).unwrap();
            assert!((result.skeleton.d2 - baseline.skeleton.d2).abs() < 1e-9);
            assert!((result.normalized_rmse - baseline.normalized_rmse).abs() < 1e-9);
        }
    }

    #[test]
    fn insufficient_interval_no_root_and_nonunique_area_remain_distinct() {
        assert_eq!(
            identify_story_trilinear(CURVE, options(1.05)),
            Err(StoryFitError::NoEqualAreaSolution)
        );
        assert_eq!(
            identify_story_trilinear(CURVE, options(0.5)),
            Err(StoryFitError::DegenerateArea)
        );
        let mut short = options(6.0);
        short.manual_interval = Some([9.5, 10.0]);
        assert_eq!(
            identify_story_trilinear(CURVE, short),
            Err(StoryFitError::InsufficientPostYieldInterval)
        );
        assert_eq!(
            identify_story_trilinear(CURVE, options(1.5)),
            Err(StoryFitError::NoEqualAreaSolution)
        );
        assert_eq!(
            identify_story_trilinear(&[(1.0, 100.0), (10.0, 100.0)], options(6.0)),
            Err(StoryFitError::DegenerateArea)
        );
        assert_eq!(
            identify_story_trilinear(&[(1.0, 90.0), (10.0, 90.0)], options(6.0)),
            Err(StoryFitError::NoEqualAreaSolution)
        );
        let mut automatic = options(6.0);
        automatic.manual_interval = None;
        assert!(
            matches!(identify_story_trilinear(&[(1.0,100.0),(10.0,100.0)],automatic),Err(StoryFitError::NoAdmissibleCandidates{non_unique,..}) if non_unique>0)
        );
    }

    #[test]
    fn poor_approximation_and_competing_unstable_yields_are_distinct() {
        let mut o = options(6.0);
        o.manual_interval = None;
        let poor = [
            100.0, 116.0, 120.0, 174.0, 228.0, 255.0, 262.0, 284.0, 285.0, 312.0,
        ];
        let unstable = [
            100.0, 123.0, 153.0, 205.0, 206.0, 241.0, 248.0, 297.0, 305.0, 328.0,
        ];
        for (loads, error) in [
            (poor, StoryFitError::PoorApproximation),
            (unstable, StoryFitError::UnstableYield),
        ] {
            let points: Vec<_> = loads
                .into_iter()
                .enumerate()
                .map(|(i, q)| ((i + 1) as f64, q))
                .collect();
            assert_eq!(identify_story_trilinear(&points, o), Err(error));
        }
    }
}
