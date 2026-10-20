#[derive(Debug, Clone, Copy)]
pub(crate) struct LoadFractionState {
    /// 収束が確定した載荷率。
    applied: f64,
    /// 失敗時に半減する前の基準増分（`1/n_grav`）。成功のたびにこれへ戻す。
    base_increment: f64,
    /// 次回試行する増分（半減リトライ中は `base_increment` より小さい）。
    increment: f64,
    /// 現在の載荷率に対する連続失敗回数。
    attempts: usize,
    /// 連続失敗の許容回数（これを超えたら `record_failure` が `false` を返す）。
    max_attempts: usize,
}

impl LoadFractionState {
    pub(crate) fn new(n_grav: usize) -> Self {
        let base_increment = 1.0 / n_grav.max(1) as f64;
        Self {
            applied: 0.0,
            base_increment,
            increment: base_increment,
            attempts: 0,
            max_attempts: 5,
        }
    }

    /// 次に試行すべき載荷率。100% に達していれば `None`。
    pub(crate) fn next_target(&self) -> Option<f64> {
        if self.applied >= 1.0 {
            None
        } else {
            Some((self.applied + self.increment).min(1.0))
        }
    }

    /// 直前の [`Self::next_target`] の載荷率が収束した。載荷率を確定し、
    /// 次回の増分を基準増分へ戻す（半減リトライ後も残増分を基準ペースで継続する）。
    pub(crate) fn record_success(&mut self, mu_target: f64) {
        self.applied = mu_target;
        self.increment = self.base_increment;
        self.attempts = 0;
    }

    /// 直前の載荷率が収束しなかった。増分を半減して再試行する。
    /// 連続失敗が `max_attempts` に達したら `false`（呼び出し側は Err とする）。
    pub(crate) fn record_failure(&mut self) -> bool {
        self.attempts += 1;
        if self.attempts >= self.max_attempts {
            return false;
        }
        self.increment *= 0.5;
        true
    }
}

#[cfg(test)]
mod load_fraction_tests {
    use super::LoadFractionState;

    #[test]
    fn nearly_full_load_still_requires_the_remaining_load() {
        let mut state = LoadFractionState::new(5);
        state.record_success(1.0 - 5.0e-10);
        assert_eq!(state.next_target(), Some(1.0));
        state.record_success(1.0);
        assert_eq!(state.next_target(), None);
    }

    /// 全ステップ成功する場合、基準増分（1/5=0.2）刻みで 5 回で 100% へ到達する。
    #[test]
    fn all_success_reaches_full_load_in_five_steps() {
        let mut state = LoadFractionState::new(5);
        let mut targets = Vec::new();
        while let Some(mu) = state.next_target() {
            targets.push(mu);
            state.record_success(mu);
        }
        assert_eq!(targets.len(), 5);
        assert!((targets.last().copied().unwrap() - 1.0).abs() < 1e-12);
        assert!((state.applied - 1.0).abs() < 1e-12);
    }

    /// 途中で失敗→増分半減で成功するパスでも、最終的に載荷率は 100% に到達すること。
    #[test]
    fn half_step_success_still_reaches_full_load() {
        let mut state = LoadFractionState::new(5);
        let mut n_targets = 0usize;
        while let Some(mu) = state.next_target() {
            n_targets += 1;
            assert!(n_targets < 1000, "無限ループの疑い");
            // 最初の載荷率（1回目の目標=0.2）だけ 1 回失敗させ、以降は毎回成功させる。
            if n_targets == 1 {
                assert!(state.record_failure());
                continue;
            }
            state.record_success(mu);
        }
        assert!((state.applied - 1.0).abs() < 1e-9);
        // 100% に到達するまで、半減で増えた分の目標を含め複数回試行しているはず。
        assert!(n_targets > 5);
    }

    /// 最大試行回数を超えて失敗し続けると `record_failure` が false を返す
    /// （呼び出し側はこれを Err に変換する）。
    #[test]
    fn repeated_failure_exceeds_max_attempts() {
        let mut state = LoadFractionState::new(5);
        let mut ok = true;
        for _ in 0..10 {
            if state.next_target().is_none() {
                break;
            }
            ok = state.record_failure();
            if !ok {
                break;
            }
        }
        assert!(!ok);
    }
}
