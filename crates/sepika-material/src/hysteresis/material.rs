//! 集中ばね用の履歴状態機械 [`HysteresisMaterial`]。

use crate::state_serde::impl_material_serde;
use crate::uniaxial::UniaxialMaterial;

use super::rule::HysteresisRule;

/// 履歴則の内部状態。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct HystState {
    theta: f64,
    m: f64,
    kt: f64,
    /// 直前の移動方向 (+1/-1/0)
    dir: f64,
    /// 最大経験変形とその時の力（正側・負側）
    max_pos: (f64, f64),
    max_neg: (f64, f64),
    /// 現在の分岐
    branch: Branch,
    /// 反転点
    reversal: (f64, f64),
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
enum Branch {
    #[default]
    Skeleton,
    /// 降伏後の除荷枝。
    Unloading { ku: f64 },
    /// 再載荷枝。
    Reloading {
        origin: (f64, f64),
        target: (f64, f64),
    },
    /// 内側ループ枝（武田の規則）。
    InnerLoop {
        origin: (f64, f64),
        target: (f64, f64),
        outer_target: (f64, f64),
    },
    /// 標準型（Masing 則）の除荷・再載荷枝。
    Masing { reversal: (f64, f64) },
    /// 最大点指向型のピーク指向枝。
    PeakOriented {
        origin: (f64, f64),
        target: (f64, f64),
    },
}

/// 履歴則パラメータ + 状態を持つ `UniaxialMaterial`。
/// `theta` は M-θ では回転角`[rad]`、Q-δ では変位`[mm]`。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HysteresisMaterial {
    pub rule: HysteresisRule,
    negative_rule: Option<HysteresisRule>,
    committed: HystState,
    trial: HystState,
}

impl HysteresisMaterial {
    pub fn new(rule: HysteresisRule) -> Self {
        let (m, kt) = rule.skeleton(0.0);
        let initial = HystState {
            m,
            kt,
            ..Default::default()
        };
        Self {
            rule,
            negative_rule: None,
            committed: initial.clone(),
            trial: initial,
        }
    }

    /// 負側の包絡線を指定する。履歴則の種類と初期剛性は正側と一致させること。
    pub fn with_negative_rule(mut self, rule: HysteresisRule) -> Self {
        assert_eq!(
            std::mem::discriminant(&self.rule),
            std::mem::discriminant(&rule)
        );
        let kp = self.rule.skeleton(0.0).1;
        let kn = rule.skeleton(0.0).1;
        assert!((kp - kn).abs() <= 1e-10 * kp.abs());
        self.negative_rule = Some(rule);
        self
    }

    fn rule_for(&self, direction: f64) -> &HysteresisRule {
        if direction < 0.0 {
            self.negative_rule.as_ref().unwrap_or(&self.rule)
        } else {
            &self.rule
        }
    }

    /// 反対側の目標点。経験がなければ降伏点。
    fn opposite_target(&self, dir: f64) -> (f64, f64) {
        let ty = self.rule_for(dir).yield_deformation();
        let my = self.rule_for(dir).yield_strength();
        if dir > 0.0 {
            if self.committed.max_pos.0.abs() > 1e-15 {
                (self.committed.max_pos.0, self.committed.max_pos.1)
            } else {
                (ty, my)
            }
        } else {
            if self.committed.max_neg.0.abs() > 1e-15 {
                (self.committed.max_neg.0, self.committed.max_neg.1)
            } else {
                (-ty, -my)
            }
        }
    }

    /// 降伏したか。
    fn has_yielded(&self) -> bool {
        let ty = self.rule.yield_deformation();
        self.committed.max_pos.0.abs() >= ty
            || self.committed.max_neg.0.abs() >= self.rule_for(-1.0).yield_deformation()
    }

    /// 最大経験変形（絶対値）。
    fn max_deformation(&self) -> f64 {
        self.committed
            .max_pos
            .0
            .abs()
            .max(self.committed.max_neg.0.abs())
    }

    /// 与えた θ での trial 状態を計算する。
    fn evaluate(&self, theta: f64) -> HystState {
        let c = &self.committed;
        let delta = theta - c.theta;
        if delta == 0.0 {
            return c.clone();
        }
        let dir_new = delta.signum();
        let mut s = c.clone();

        if self.rule.is_retrograde() {
            let (m, kt) = self.rule_for(theta).skeleton(theta);
            s.theta = theta;
            s.m = m;
            s.kt = kt;
            s.dir = dir_new;
            s.branch = Branch::Skeleton;
            if theta > s.max_pos.0 {
                s.max_pos = (theta, m);
            }
            if theta < s.max_neg.0 {
                s.max_neg = (theta, m);
            }
            return s;
        }

        let reversed = c.dir != 0.0 && dir_new != c.dir;
        if reversed {
            s.reversal = (c.theta, c.m);
            let ty = self.rule_for(c.theta).yield_deformation();
            let yielded = c.theta.abs() >= ty || self.has_yielded();
            if self.rule.is_standard() {
                s.branch = Branch::Masing {
                    reversal: (c.theta, c.m),
                };
            } else if self.rule.is_max_point_oriented() {
                s.branch = if yielded {
                    Branch::PeakOriented {
                        origin: (c.theta, c.m),
                        target: self.opposite_target(dir_new),
                    }
                } else {
                    Branch::Skeleton
                };
            } else if c.m.abs() < 1e-12 {
                let target = self.opposite_target(dir_new);
                s.branch = Branch::Reloading {
                    origin: (c.theta, c.m),
                    target,
                };
            } else if matches!(c.branch, Branch::Reloading { .. })
                || matches!(c.branch, Branch::InnerLoop { .. })
            {
                let prev_origin = match c.branch {
                    Branch::Reloading { origin, .. } => origin,
                    Branch::InnerLoop { origin, .. } => origin,
                    _ => (0.0, 0.0),
                };
                s.branch = Branch::InnerLoop {
                    origin: (c.theta, c.m),
                    target: prev_origin,
                    outer_target: self.opposite_target(dir_new),
                };
            } else if yielded {
                let ku = self.unloading_slope(c.theta, c.m);
                s.branch = Branch::Unloading { ku };
            } else {
                s.branch = Branch::Skeleton;
            }
            s.dir = dir_new;
        } else {
            s.dir = dir_new;
        }

        let (m, kt, branch_out) = self.eval_on_branch(theta, &s);
        s.theta = theta;
        s.m = m;
        s.kt = kt;
        s.branch = branch_out;

        if matches!(s.branch, Branch::Skeleton) {
            let ty = self.rule_for(theta).yield_deformation();
            if theta > c.max_pos.0 && theta > ty {
                s.max_pos = (theta, m);
            }
            if theta < c.max_neg.0 && theta < -ty {
                s.max_neg = (theta, m);
            }
        }

        s
    }

    /// 反転点からの除荷剛性。
    fn unloading_slope(&self, tr: f64, mr: f64) -> f64 {
        let ty = self.rule_for(tr).yield_deformation();
        let my = self.rule_for(tr).yield_strength();
        if tr.abs() < 1e-15 {
            return my / ty;
        }
        if self.rule_for(tr).is_takeda() {
            let ku = self
                .rule_for(tr)
                .unloading_stiffness(self.max_deformation())
                .unwrap_or(mr / tr);
            let k1 = self
                .rule_for(tr)
                .crack_point()
                .map(|(mc, tc)| mc / tc)
                .unwrap_or(my / ty);
            ku.min(k1)
        } else {
            (mr / tr).abs()
        }
    }

    /// 現在の branch 上で θ を評価する。
    fn eval_on_branch(&self, theta: f64, s: &HystState) -> (f64, f64, Branch) {
        match s.branch {
            Branch::Skeleton => {
                let (m, k) = self.rule_for(theta).skeleton(theta);
                (m, k, Branch::Skeleton)
            }
            Branch::Unloading { ku } => {
                let (tr, mr) = s.reversal;
                let m = mr + ku * (theta - tr);
                let crossed_zero = (mr > 0.0 && m <= 0.0) || (mr < 0.0 && m >= 0.0);
                if crossed_zero {
                    let theta_zero = tr - mr / ku;
                    let origin = (theta_zero, 0.0);
                    let target = self.opposite_target(s.dir);
                    let (m2, k2) = reload_line(origin, target, theta);
                    (m2, k2, Branch::Reloading { origin, target })
                } else {
                    (m, ku, Branch::Unloading { ku })
                }
            }
            Branch::Reloading { origin, target } => {
                let reached = if target.0 >= origin.0 {
                    theta >= target.0
                } else {
                    theta <= target.0
                };
                if reached {
                    let (m, k) = self.rule_for(theta).skeleton(theta);
                    (m, k, Branch::Skeleton)
                } else {
                    let (m, k) = reload_line(origin, target, theta);
                    (m, k, Branch::Reloading { origin, target })
                }
            }
            Branch::InnerLoop {
                origin,
                target,
                outer_target,
            } => {
                let reached_target = if target.0 >= origin.0 {
                    theta >= target.0
                } else {
                    theta <= target.0
                };
                if reached_target {
                    if s.dir * (theta - outer_target.0) >= 0.0 {
                        let (m, k) = self.rule_for(theta).skeleton(theta);
                        return (m, k, Branch::Skeleton);
                    }
                    let (m, k) = reload_line(target, outer_target, theta);
                    (
                        m,
                        k,
                        Branch::Reloading {
                            origin: target,
                            target: outer_target,
                        },
                    )
                } else {
                    let (m, k) = reload_line(origin, target, theta);
                    (
                        m,
                        k,
                        Branch::InnerLoop {
                            origin,
                            target,
                            outer_target,
                        },
                    )
                }
            }
            Branch::Masing { reversal } => {
                let (tr, qr) = reversal;
                let rule = self.rule_for(s.dir);
                let scale = (self.rule.yield_strength() + self.rule_for(-1.0).yield_strength())
                    / rule.yield_strength();
                let arg = (tr - theta).abs() / scale;
                let (g_mag, g_tan) = rule.skeleton(arg);
                let q = qr - (tr - theta).signum() * scale * g_mag;
                let opposite_peak =
                    tr.abs() * rule.yield_deformation() / self.rule_for(tr).yield_deformation();
                let rejoined = s.dir * theta >= opposite_peak;
                if rejoined {
                    let (m, k) = self.rule_for(theta).skeleton(theta);
                    (m, k, Branch::Skeleton)
                } else {
                    (q, g_tan.max(1e-9), Branch::Masing { reversal })
                }
            }
            Branch::PeakOriented { origin, target } => {
                let reached = if target.0 >= origin.0 {
                    theta >= target.0
                } else {
                    theta <= target.0
                };
                if reached {
                    let (m, k) = self.rule_for(theta).skeleton(theta);
                    (m, k, Branch::Skeleton)
                } else {
                    let dt = target.0 - origin.0;
                    if dt.abs() < 1e-15 {
                        (origin.1, 1e-9, Branch::PeakOriented { origin, target })
                    } else {
                        let k = (target.1 - origin.1) / dt;
                        let m = origin.1 + k * (theta - origin.0);
                        (m, k.max(1e-9), Branch::PeakOriented { origin, target })
                    }
                }
            }
        }
    }
}

/// 再載荷直線。
fn reload_line(origin: (f64, f64), target: (f64, f64), theta: f64) -> (f64, f64) {
    let dt = target.0 - origin.0;
    if dt.abs() < 1e-15 {
        return (origin.1, 0.0);
    }
    let k = (target.1 - origin.1) / dt;
    (origin.1 + k * (theta - origin.0), k)
}

impl UniaxialMaterial for HysteresisMaterial {
    fn trial(&mut self, theta: f64) -> (f64, f64) {
        let s = self.evaluate(theta);
        self.trial = s;
        (self.trial.m, self.trial.kt)
    }
    fn probe(&self, theta: f64) -> (f64, f64) {
        let s = self.evaluate(theta);
        (s.m, s.kt)
    }
    fn commit(&mut self) {
        self.committed = self.trial.clone();
    }
    fn revert(&mut self) {
        self.trial = self.committed.clone();
    }
    impl_material_serde!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hysteresis::rule::{max_point, retrograde, standard, takeda};
    use approx::assert_relative_eq;

    fn zero_increment_rule() -> HysteresisRule {
        HysteresisRule::Takeda {
            crack: (10.0, 0.01),
            yield_point: (20.0, 0.02),
            ultimate: (30.0, 0.04),
            alpha: 0.0,
        }
    }

    #[test]
    fn zero_increment_preserves_initial_tangent_and_finite_tiny_motion() {
        for zero in [0.0, -0.0] {
            let mut m = HysteresisMaterial::new(zero_increment_rule());
            assert_eq!(m.probe(zero), (0.0, 1000.0));
            assert_eq!(m.trial(zero), (0.0, 1000.0));
            m.commit();
            m.trial(0.03);
            m.revert();
            assert_eq!(m.trial, m.committed);
            assert_eq!(m.probe(zero), (0.0, 1000.0));
        }
        for theta in [1e-20, -1e-20] {
            let mut m = HysteresisMaterial::new(zero_increment_rule());
            assert_eq!(m.trial(theta), (1000.0 * theta, 1000.0));
            assert_eq!(m.trial.theta, theta);
            assert_eq!(m.trial.dir, theta.signum());
        }
    }

    #[test]
    fn zero_increment_signed_elastic_and_post_yield_response_is_independent() {
        for (theta, force, tangent) in [
            (-0.005, -5.0, 1000.0),
            (0.005, 5.0, 1000.0),
            (-0.03, -25.0, 500.0),
            (0.03, 25.0, 500.0),
        ] {
            let mut material = HysteresisMaterial::new(zero_increment_rule());
            assert_eq!(material.trial(theta), (force, tangent));
            material.commit();
            let before = material.serialize_state();
            assert_eq!(material.probe(theta), (force, tangent));
            assert_eq!(material.trial(theta), (force, tangent));
            material.commit();
            assert_eq!(before, material.serialize_state());
        }
    }

    #[test]
    fn zero_increment_negative_peak_has_independent_response_and_entire_history() {
        let mut direct = HysteresisMaterial::new(zero_increment_rule());
        assert_relative_eq!(direct.trial(-0.03).0, -25.0, epsilon = 1e-12);
        assert_eq!(direct.trial.kt, 500.0);
        direct.commit();
        assert_eq!(direct.committed.dir, -1.0);
        assert_eq!(direct.committed.branch, Branch::Skeleton);
        assert_eq!(direct.committed.max_neg, (-0.03, -25.0));
        assert_eq!(
            direct.committed,
            HystState {
                theta: -0.03,
                m: -25.0,
                kt: 500.0,
                dir: -1.0,
                max_neg: (-0.03, -25.0),
                ..Default::default()
            }
        );
        let mut reversal = direct.clone();
        assert_relative_eq!(reversal.trial(-0.029).0, -24.0, epsilon = 1e-12);
        assert_eq!(reversal.trial.kt, 1000.0);
        assert_eq!(reversal.trial.dir, 1.0);
        assert_eq!(reversal.trial.branch, Branch::Unloading { ku: 1000.0 });
        assert_eq!(reversal.trial.reversal, (-0.03, -25.0));
        assert_eq!(reversal.trial.max_neg, (-0.03, -25.0));
        assert_eq!(reversal.trial.max_pos, (0.0, 0.0));
        let before_failed_restore = direct.serialize_state();
        assert!(direct.deserialize_state(&[0xff, 0x00, 0x01]).is_err());
        assert_eq!(before_failed_restore, direct.serialize_state());
        let mut repeated = direct.clone();
        assert_eq!(repeated.trial(-0.03), (-25.0, 500.0));
        repeated.commit();
        assert_eq!(direct.committed, repeated.committed);
        assert_eq!(direct.trial(-0.04), (-30.0, 500.0));
        assert_eq!(repeated.trial(-0.04), (-30.0, 500.0));
        for theta in [-0.04, -0.039, 0.0, 0.03, 0.02, -0.01, -0.05] {
            assert_eq!(direct.trial(theta), repeated.trial(theta));
            direct.commit();
            repeated.commit();
            assert_eq!(direct.committed, repeated.committed);
        }
    }

    #[test]
    fn zero_increment_probe_keeps_outstanding_trial_and_trial_replaces_it() {
        let mut base = HysteresisMaterial::new(zero_increment_rule());
        for theta in [-0.005, -0.03, -0.029, 0.025, 0.015, -0.02] {
            base.trial(theta);
            base.commit();
            let mut probe = base.clone();
            probe.trial(theta + 0.001);
            let outstanding = probe.trial.clone();
            let committed = probe.committed.clone();
            let before = probe.serialize_state();
            assert_eq!(probe.probe(theta), (committed.m, committed.kt));
            assert_eq!(before, probe.serialize_state());
            probe.commit();
            assert_eq!(probe.committed, outstanding);
            let mut trial = base.clone();
            trial.trial(theta + 0.001);
            assert_eq!(trial.trial(theta), (committed.m, committed.kt));
            trial.commit();
            assert_eq!(trial.committed, committed);
            trial.trial(theta - 0.001);
            trial.revert();
            assert_eq!(trial.trial, committed);
            assert_eq!(trial.trial(theta + 0.001), base.trial(theta + 0.001));
            base.revert();
        }
    }

    #[test]
    fn zero_increment_asymmetric_cycles_preserve_history_and_branches() {
        let negative = HysteresisRule::Takeda {
            crack: (10.0, 0.01),
            yield_point: (30.0, 0.03),
            ultimate: (40.0, 0.05),
            alpha: 0.0,
        };
        let mut direct =
            HysteresisMaterial::new(zero_increment_rule()).with_negative_rule(negative);
        let mut repeated = direct.clone();
        for theta in [
            -0.005, -0.04, -0.039, 0.0, 0.03, 0.029, -0.04, -0.02, 0.01, 0.04,
        ] {
            assert_eq!(direct.trial(theta), repeated.trial(theta));
            direct.commit();
            repeated.commit();
            assert_eq!(direct.committed, repeated.committed);
            let before = repeated.serialize_state();
            assert_eq!(
                repeated.probe(theta),
                (direct.committed.m, direct.committed.kt)
            );
            assert_eq!(before, repeated.serialize_state());
            repeated.trial(theta);
            repeated.commit();
            assert_eq!(direct.committed, repeated.committed);
        }
    }

    #[test]
    fn asymmetric_envelopes_preserve_initial_stiffness_and_cyclic_limits() {
        for kind in 0..5 {
            let rule = |q: f64| {
                let crack = (q / 3.0, q / 3000.0);
                let yield_point = (q, q / 1000.0);
                let ultimate = (q, q * 10.0);
                match kind {
                    0 => HysteresisRule::MaxPointOriented {
                        crack,
                        yield_point,
                        ultimate,
                    },
                    1 => HysteresisRule::Retrograde {
                        crack,
                        yield_point,
                        ultimate,
                    },
                    2 => HysteresisRule::Standard {
                        crack,
                        yield_point,
                        ultimate,
                    },
                    3 => HysteresisRule::OriginOriented {
                        yield_point,
                        ultimate,
                    },
                    _ => HysteresisRule::Takeda {
                        crack,
                        yield_point,
                        ultimate,
                        alpha: 0.4,
                    },
                }
            };
            let mut m = HysteresisMaterial::new(rule(100.0)).with_negative_rule(rule(200.0));
            assert_relative_eq!(m.probe(0.001).0, 1.0, epsilon = 1e-10);
            assert_relative_eq!(m.probe(-0.001).0, -1.0, epsilon = 1e-10);
            let mut previous = 0.0;
            for target in [0.5, -0.8, 0.7, -0.6, 0.03, -0.02, 0.9] {
                let start = previous;
                for step in 1..=100 {
                    let u = start + (target - start) * step as f64 / 100.0;
                    let (q, k) = m.trial(u);
                    assert!(
                        (-200.0 - 1e-7..=100.0 + 1e-7).contains(&q),
                        "履歴則{kind}, u={u}, q={q}"
                    );
                    assert!(q.is_finite() && k.is_finite());
                    let (_, tangent) = m.probe(u);
                    let forward = (m.probe(u + 1e-8).0 - q) / 1e-8;
                    let backward = (q - m.probe(u - 1e-8).0) / 1e-8;
                    // 折点では中央差分が両側接線の平均になるため、片側接線と照合する。
                    assert!(
                        (tangent - forward).abs().min((tangent - backward).abs()) < 1e-3,
                        "履歴則{kind}, u={u}, 接線{tangent}, 片側差分({forward}, {backward})"
                    );
                    m.commit();
                    previous = u;
                }
            }
            assert_relative_eq!(m.probe(1.0).0, 100.0, epsilon = 1e-7);
            let before = m.probe(-1.0);
            m.trial(0.0);
            m.revert();
            assert_eq!(before, m.probe(-1.0));
            let snapshot = m.serialize_state();
            let mut restored = HysteresisMaterial::new(rule(100.0));
            restored.deserialize_state(&snapshot).unwrap();
            assert_eq!(m.probe(-1.0), restored.probe(-1.0));
        }
    }

    #[test]
    fn test_hysteresis_monotonic_follows_skeleton() {
        let mut mat = HysteresisMaterial::new(takeda());
        for &theta in &[0.001, 0.002, 0.005, 0.01, 0.03] {
            let (m, _) = mat.trial(theta);
            let (ms, _) = takeda().skeleton(theta);
            assert_relative_eq!(m, ms, epsilon = 1e-6);
            mat.commit();
        }
    }

    #[test]
    fn test_takeda_unloading_stiffness_degraded() {
        // 降伏後(θ=0.03)で反転 → 除荷剛性 Ku = Ky*(θm/θy)^(-α) < K1
        let mut mat = HysteresisMaterial::new(takeda());
        mat.trial(0.03);
        mat.commit();
        let (m_r, _) = mat.trial(0.029);
        let (m2, _) = mat.trial(0.028);
        let ku = (m_r - m2) / (0.029 - 0.028);
        // 武田モデル: Kd+ = K0·|δmax/δy2|^(−ν), K0 = Mc/θc（初期勾配）, δy2 = θy, ν = α。
        let k0 = 40.0 / 0.002;
        let expected_ku: f64 = k0 * (0.03_f64 / 0.01).powf(-0.4);
        let k1 = 40.0 / 0.002;
        assert!(
            ku < k1,
            "unloading stiffness ({}) must be below elastic K1 ({})",
            ku,
            k1
        );
        assert_relative_eq!(ku, expected_ku.min(k1), epsilon = expected_ku * 0.05);
    }

    #[test]
    fn test_takeda_cyclic_returns_near_peak() {
        let mut mat = HysteresisMaterial::new(takeda());
        // +0.03 → 0 → -0.03 → 0 → +0.03
        for &theta in &[0.03, 0.0, -0.03, 0.0, 0.03] {
            mat.trial(theta);
            mat.commit();
        }
        let (m, _) = mat.trial(0.03);
        // 再載荷で正側ピークに戻る（降伏後耐力はスケルトン上 ≈ 120 に近い、少なくとも 100 超）
        assert!(
            m > 100.0,
            "cyclic reload should return near positive peak, got M={}",
            m
        );
    }

    #[test]
    fn test_origin_oriented_unloads_to_origin() {
        let rule = HysteresisRule::OriginOriented {
            yield_point: (100.0, 0.01),
            ultimate: (120.0, 0.05),
        };
        let mut mat = HysteresisMaterial::new(rule);
        mat.trial(0.03);
        mat.commit();
        mat.trial(0.0);
        mat.commit();
        let (m, _) = mat.trial(0.0);
        assert_relative_eq!(m, 0.0, epsilon = 1e-6);
    }

    #[test]
    fn test_probe_matches_trial_without_mutating_state() {
        // probe は trial と数学的に同一の結果を返し、状態を書き換えない
        // （反転・内側ループを経た複雑な分岐状態で確認）。
        let mut mat = HysteresisMaterial::new(takeda());
        mat.trial(0.03);
        mat.commit();
        mat.trial(0.01);
        mat.commit();

        let probe_theta = -0.005; // 内側ループへ入る反転
        let before = mat.probe(probe_theta);
        assert_eq!(before, mat.probe(probe_theta));

        let mut clone_for_trial = mat.clone();
        let via_trial = clone_for_trial.trial(probe_theta);
        assert_eq!(before, via_trial, "probe は trial と完全一致すること");

        let after_probe = mat.trial(probe_theta);
        assert_eq!(after_probe, via_trial);
    }

    #[test]
    fn test_takeda_inner_loop_polygon() {
        // 外側ループ途中で反転 → 内側ループ → 再反転でポリゴン則。
        let mut mat = HysteresisMaterial::new(takeda());
        // +0.03（降伏後ピーク）→ 0.01（除荷途中・M=0 未満）→ -0.005（内側反転）
        mat.trial(0.03);
        mat.commit();
        mat.trial(0.01);
        mat.commit();
        // 内側ループに入る反転
        let (m_inner, _) = mat.trial(-0.005);
        mat.commit();
        // 再反転で target（直前の反転点 θ=0.01 側）へ向かう直線
        let (m_back, _) = mat.trial(0.008);
        // 内側ループの戻りは、ピーク直前の反転点付近の M に近い値を取る
        assert!(
            m_back > m_inner,
            "inner loop should return toward previous reversal point: inner={}, back={}",
            m_inner,
            m_back
        );
    }

    #[test]
    fn test_retrograde_traces_skeleton_both_ways() {
        // 逆行型: 除荷・再載荷ともスケルトンを可逆に辿る（履歴ループなし）。
        let mut mat = HysteresisMaterial::new(retrograde());
        mat.trial(0.03);
        mat.commit();
        // 除荷: 力はスケルトン値に一致（除荷枝を描かない）。
        let (m, _) = mat.trial(0.02);
        let (ms, _) = retrograde().skeleton(0.02);
        assert_relative_eq!(m, ms, epsilon = 1e-6);
        mat.commit();
        // 原点まで戻れば力は 0（エネルギー吸収なし）。
        let (m0, _) = mat.trial(0.0);
        assert_relative_eq!(m0, 0.0, epsilon = 1e-6);
    }

    #[test]
    fn test_standard_masing_unload_starts_at_initial_stiffness() {
        // 標準型: 除荷開始剛性は初期剛性 K1 = Mc/θc = 20000。
        let mut mat = HysteresisMaterial::new(standard());
        mat.trial(0.03); // スケルトン上 (110, 0.03)
        mat.commit();
        let peak = 110.0_f64;
        let (m1, k1) = mat.trial(0.0299); // 反転 → Masing 枝
        let k1_expected = 40.0 / 0.002;
        let slope = (peak - m1) / (0.03 - 0.0299);
        assert_relative_eq!(slope, k1_expected, epsilon = k1_expected * 0.05);
        assert_relative_eq!(k1, k1_expected, epsilon = k1_expected * 0.1);
    }

    #[test]
    fn test_standard_masing_reaches_opposite_skeleton() {
        // 標準型: 反射点（反対側 |θ|≥|反転点|）でスケルトンへ復帰する。
        let mut mat = HysteresisMaterial::new(standard());
        mat.trial(0.03);
        mat.commit();
        for &t in &[0.02, 0.0, -0.02, -0.03] {
            mat.trial(t);
            mat.commit();
        }
        let (m, _) = mat.trial(-0.03);
        let (ms, _) = standard().skeleton(-0.03);
        assert_relative_eq!(m, ms, epsilon = 2.0);
        // 途中（θ=0）は履歴枝上にあり、原点指向型のように 0 にはならない。
        let mut mat2 = HysteresisMaterial::new(standard());
        mat2.trial(0.03);
        mat2.commit();
        let (m_zero, _) = mat2.trial(0.0);
        assert!(
            m_zero < -1.0,
            "Masing loop should carry negative force at θ=0, got {}",
            m_zero
        );
    }

    #[test]
    fn test_max_point_oriented_targets_opposite_peak() {
        // 最大点指向型: 戻り点から反対側の最大経験点を直線で指向する。
        let mut mat = HysteresisMaterial::new(max_point());
        mat.trial(0.03); // +ピーク (110, 0.03)
        mat.commit();
        for &t in &[0.0, -0.01, -0.025] {
            mat.trial(t);
            mat.commit();
        }
        // -0.025 から再載荷（反転）→ +ピークを直線で指向。
        mat.trial(-0.02);
        mat.commit();
        let (m_mid, _) = mat.trial(0.0);
        let (m_end, _) = mat.trial(0.03);
        assert!(m_end > 100.0, "should reach positive peak, got {}", m_end);
        assert!(
            m_mid > -110.0 && m_mid < m_end,
            "peak-oriented interpolation mid={}",
            m_mid
        );
    }
}
