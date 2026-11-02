//! 区間線形荷重の位置応答。位置は全長で正規化する。

use sepika_core::model::{MemberLoadKind, PostLateralSupport};

/// 正規化位置 t の昇べき係数。演算は N-mm 系。
#[derive(Clone, Debug, Default)]
pub struct Polynomial(pub Vec<f64>);
impl Polynomial {
    pub fn constant(v: f64) -> Self {
        Self(vec![v])
    }
    pub fn value(&self, t: f64) -> f64 {
        self.0.iter().rev().fold(0.0, |v, c| v * t + c)
    }
    pub fn add(&self, rhs: &Self) -> Self {
        let mut c = vec![0.0; self.0.len().max(rhs.0.len())];
        for (i, v) in self.0.iter().enumerate() {
            c[i] += v;
        }
        for (i, v) in rhs.0.iter().enumerate() {
            c[i] += v;
        }
        Self(c)
    }
    pub fn scale(&self, s: f64) -> Self {
        Self(self.0.iter().map(|v| v * s).collect())
    }
    pub fn multiply(&self, rhs: &Self) -> Self {
        let mut c = vec![0.0; self.0.len() + rhs.0.len() - 1];
        for (i, a) in self.0.iter().enumerate() {
            for (j, b) in rhs.0.iter().enumerate() {
                c[i + j] += a * b;
            }
        }
        Self(c)
    }
    pub fn derivative(&self) -> Self {
        Self(
            self.0
                .iter()
                .enumerate()
                .skip(1)
                .map(|(i, v)| v * i as f64)
                .collect(),
        )
    }
    pub fn integral(&self) -> Self {
        let mut c = vec![0.0];
        c.extend(self.0.iter().enumerate().map(|(i, v)| v / (i + 1) as f64));
        Self(c)
    }
    /// 閉区間内の実根。導関数の根で単調区間を分割して二分する。
    pub fn roots(&self, a: f64, b: f64) -> Vec<f64> {
        let scale = self.0.iter().map(|v| v.abs()).fold(0.0, f64::max);
        if scale == 0.0 {
            return Vec::new();
        }
        let mut c = self.scale(1.0 / scale);
        while c.0.last().is_some_and(|v| v.abs() < 1e-14) {
            c.0.pop();
        }
        if c.0.len() <= 1 {
            return Vec::new();
        }
        let mut cuts = vec![a];
        cuts.extend(c.derivative().roots(a, b));
        cuts.push(b);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        let mut roots = Vec::new();
        for &x in &cuts {
            if c.value(x).abs() < 1e-12 {
                roots.push(x);
            }
        }
        for w in cuts.windows(2) {
            let (mut lo, mut hi) = (w[0], w[1]);
            let mut fl = c.value(lo);
            if fl * c.value(hi) >= 0.0 {
                continue;
            }
            for _ in 0..60 {
                let m = (lo + hi) / 2.0;
                let fm = c.value(m);
                if fl * fm <= 0.0 {
                    hi = m;
                } else {
                    lo = m;
                    fl = fm;
                }
            }
            roots.push((lo + hi) / 2.0);
        }
        roots.sort_by(f64::total_cmp);
        roots.dedup_by(|x, y| (*x - *y).abs() < 1e-10);
        roots
    }
}

fn power_shift(a: f64, n: usize, s: f64) -> Polynomial {
    let mut p = Polynomial::constant(s);
    for _ in 0..n {
        p = p.multiply(&Polynomial(vec![-a, 1.0]));
    }
    p
}

/// 荷重端点で分割した区間の応答。t は始端0・終端1。
#[derive(Clone, Debug)]
pub struct ResponseInterval {
    pub range: [f64; 2],
    pub moment: Polynomial,
    pub shear: Polynomial,
    pub displacement: Polynomial,
    pub cumulative_load: Polynomial,
}

/// 単純支持・片持ちの閉形式積分応答。反力は端番号順 `[N]`、固定端モーメント [N mm]。
#[derive(Clone, Debug)]
pub struct BeamResponse {
    pub intervals: Vec<ResponseInterval>,
    pub reactions: [f64; 2],
    pub support_moments: [f64; 2],
}

/// 荷重位置は `[0,L]`、分布荷重は a < b、EI は正であること。
pub fn beam_response(
    loads: &[MemberLoadKind],
    l: f64,
    ei: f64,
    support: PostLateralSupport,
) -> Result<BeamResponse, String> {
    if !l.is_finite() || l <= 0.0 || !ei.is_finite() || ei <= 0.0 {
        return Err("長さ・曲げ剛性は有限かつ正が必要です".into());
    }
    let mut knots = vec![0.0, 1.0];
    let mut terms: Vec<(f64, usize, f64)> = Vec::new();
    let (mut total, mut first) = (0.0, 0.0);
    for load in loads {
        match *load {
            MemberLoadKind::Point { a, p } => {
                if !a.is_finite() || !p.is_finite() || !(0.0..=l).contains(&a) {
                    return Err("集中荷重の位置・値が不正です".into());
                }
                knots.push(a / l);
                terms.push((a / l, 1, -p * l));
                total += p;
                first += p * a;
            }
            MemberLoadKind::Distributed { a, b, w1, w2 } => {
                if ![a, b, w1, w2].iter().all(|v| v.is_finite()) || a < 0.0 || b > l || b <= a {
                    return Err("分布荷重の区間・値が不正です".into());
                }
                let k = (w2 - w1) / (b - a);
                knots.extend([a / l, b / l]);
                terms.extend([
                    (a / l, 2, -w1 * l * l / 2.0),
                    (a / l, 3, -k * l * l * l / 6.0),
                    (b / l, 2, w2 * l * l / 2.0),
                    (b / l, 3, k * l * l * l / 6.0),
                ]);
                let (r0, r1) = super::simple_reactions(load, l);
                total += r0 + r1;
                first += r1 * l;
            }
        }
    }
    let (reactions, m0) = match support {
        PostLateralSupport::Pinned => ([total - first / l, first / l], 0.0),
        PostLateralSupport::FixedStart => ([total, 0.0], -first),
        PostLateralSupport::FixedEnd => ([0.0, total], 0.0),
    };
    knots.sort_by(f64::total_cmp);
    knots.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let moment_at = |t: f64| {
        let mut p = Polynomial(vec![m0, reactions[0] * l]);
        for &(a, n, s) in &terms {
            if t >= a {
                p = p.add(&power_shift(a, n, s));
            }
        }
        p
    };
    let mut integrated = Vec::new();
    let (mut slope, mut displacement) = (0.0, 0.0);
    for w in knots.windows(2) {
        let m = moment_at((w[0] + w[1]) / 2.0);
        let mut v = m.scale(l * l / ei).integral().integral();
        let c1 = slope - v.derivative().value(w[0]);
        let c0 = displacement - v.value(w[0]) - c1 * w[0];
        v = v.add(&Polynomial(vec![c0, c1]));
        slope = v.derivative().value(w[1]);
        displacement = v.value(w[1]);
        integrated.push((w, m, v));
    }
    let correction = match support {
        PostLateralSupport::Pinned => Polynomial(vec![0.0, -displacement]),
        PostLateralSupport::FixedStart => Polynomial::constant(0.0),
        PostLateralSupport::FixedEnd => Polynomial(vec![slope - displacement, -slope]),
    };
    let intervals = integrated
        .into_iter()
        .map(|(w, m, v)| {
            let shear = m.derivative().scale(1.0 / l);
            let cumulative_load = Polynomial::constant(reactions[0]).add(&shear.scale(-1.0));
            ResponseInterval {
                range: [w[0], w[1]],
                moment: m,
                shear,
                displacement: v.add(&correction),
                cumulative_load,
            }
        })
        .collect();
    Ok(BeamResponse {
        intervals,
        reactions,
        support_moments: [m0, moment_at(1.0).value(1.0)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_three_metre_two_kn_per_m() {
        let r = beam_response(
            &[MemberLoadKind::Distributed {
                a: 0.0,
                b: 3000.0,
                w1: 2.0,
                w2: 2.0,
            }],
            3000.0,
            205000.0 * 8e6,
            PostLateralSupport::Pinned,
        )
        .unwrap();
        assert_eq!(r.reactions, [3000.0, 3000.0]);
        let p = &r.intervals[0];
        assert!((p.moment.value(0.5) - 2.25e6).abs() < 1e-6);
        assert!((p.displacement.value(0.5).abs() - 1.286204268292683).abs() < 1e-10);
        assert_eq!(p.moment.derivative().roots(0.0, 1.0), vec![0.5]);
    }
    #[test]
    fn cantilever_tip_force_and_end_reversal() {
        let loads = [MemberLoadKind::Point {
            a: 3000.0,
            p: 2000.0,
        }];
        let r = beam_response(
            &loads,
            3000.0,
            205000.0 * 8e6,
            PostLateralSupport::FixedStart,
        )
        .unwrap();
        assert_eq!(r.reactions, [2000.0, 0.0]);
        assert_eq!(r.support_moments[0], -6e6);
        assert!((r.intervals[0].displacement.value(1.0).abs() - 10.975609756097562).abs() < 1e-10);
        let r = beam_response(
            &[MemberLoadKind::Point { a: 0.0, p: 2000.0 }],
            3000.0,
            205000.0 * 8e6,
            PostLateralSupport::FixedEnd,
        )
        .unwrap();
        assert!((r.intervals[0].displacement.value(0.0).abs() - 10.975609756097562).abs() < 1e-10);
    }
}
