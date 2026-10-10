//! 増分解析（プッシュオーバー）。
//!
//! `actions` からの構造分割。アルゴリズム変更は行わない。

use super::*;

impl App {
    /// 方向別スロットの増分解析結果を返す。
    pub fn pushover_for(
        &self,
        dir: SeismicDir,
    ) -> Option<&sepika_solver::nonlinear::pushover::PushoverResult> {
        let results = self.core.scoped.results.as_ref()?;
        results.pushover_for_dir(dir)
    }

    /// 結果タブ・設計タブで表示中の増分解析結果を返す。
    pub fn displayed_pushover(
        &self,
    ) -> Option<&sepika_solver::nonlinear::pushover::PushoverResult> {
        let view_dir = self.core.scoped.pushover_view_dir;
        let results = self.core.scoped.results.as_ref()?;
        results
            .pushover_for_dir(view_dir)
            .or(results.pushover.as_ref())
    }

    /// 結果タブの増分解析表示方向を切り替え、`pushover` 窓口も同期する。
    #[cfg(any(test, feature = "gui"))]
    pub(crate) fn set_pushover_view_dir(&mut self, dir: SeismicDir) {
        if self.core.scoped.pushover_view_dir == dir {
            return;
        }
        self.core.scoped.pushover_view_dir = dir;
        if let Some(bundle) = self.core.scoped.results.as_mut() {
            if let Some(po) = bundle.pushover_for_dir(dir).cloned() {
                bundle.pushover = Some(po);
            }
        }
        #[cfg(feature = "gui")]
        {
            self.ui.scoped.hinge_view_cache = None;
        }
    }

    /// 保存直前に `pushover` 窓口を表示中方向へ同期する。
    pub(crate) fn sync_pushover_for_save(&mut self) {
        if let Some(bundle) = self.core.scoped.results.as_mut() {
            bundle.pushover = bundle
                .pushover_for_dir(self.core.scoped.pushover_view_dir)
                .cloned();
        }
    }

    /// `compute_pushover` の結果を適用する（bundle 格納・最終実行時刻更新・エラー設定）。
    pub(super) fn apply_pushover_result(
        &mut self,
        res: Result<sepika_solver::nonlinear::pushover::PushoverResult, String>,
    ) {
        let input_key = ResultInputKey::Pushover(self.core.analysis_cfg.push_dir);
        let input = self.result_input(&input_key);
        match res {
            Ok(mut result) => {
                result.identify_wall_input(input.clone());
                if result.termination.is_premature() {
                    self.append_analysis_notice(format!(
                        "⚠ 増分解析は目標到達前に打ち切られました（{}）。最大ベースシアはその時点までの最大値です。",
                        result.termination.describe()
                    ));
                }
                let dir = self.core.analysis_cfg.push_dir;
                let mut bundle = self.core.scoped.results.take().unwrap_or_default();
                bundle.record_input(input_key, input);
                match dir {
                    SeismicDir::X => bundle.pushover_x = Some(result.clone()),
                    SeismicDir::Y => bundle.pushover_y = Some(result.clone()),
                }
                bundle.pushover = Some(result);
                self.core.scoped.results = Some(bundle);
                self.core.scoped.pushover_view_dir = dir;
                self.core.scoped.staleness.mark_fresh();
                self.core.scoped.last_error = None;
            }
            Err(e) => self.report_error(e),
        }
    }

    /// 増分解析を実行する。鋼板耐震壁の座屈未考慮は注意として通知し、解析を継続する。
    pub fn run_pushover(&mut self) {
        if !self.begin_analysis() {
            return;
        }
        self.notice_steel_seismic_walls();
        let res =
            sepika_job::compute::compute_pushover(self.core.model.clone(), self.core.analysis_cfg)
                .map_err(|e| e.to_string());
        self.apply_pushover_result(res);
    }

    /// 増分解析（プッシュオーバー）をバックグラウンドスレッドで実行する。
    /// UI スレッドをブロックしないよう重い解析を逃がす。
    /// 既にジョブが実行中の場合は何もしない（last_error に案内文を設定）。
    pub fn start_pushover_job(&mut self) {
        if !self.begin_analysis_job() {
            return;
        }
        self.notice_steel_seismic_walls();
        let model = self.core.model.clone();
        let cfg = self.core.analysis_cfg;
        self.spawn_analysis_job("増分解析", move || {
            JobResult::Pushover(Box::new(Self::run_compute(|| {
                sepika_job::compute::compute_pushover(model, cfg).map_err(|e| e.to_string())
            })))
        });
        #[cfg(feature = "gui")]
        if let Some(job) = self.core.scoped.job.as_mut() {
            job.jump_on_success = Some((Tab::Results, ResultsView::Pushover));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::steel_wall_notice_tests::{steel_wall_model, wall_notices};
    #[test]
    fn 増分解析の打切り注意は鋼板壁注意を消さない() {
        let mut app = App::default();
        app.load_model(steel_wall_model());
        app.generate_stories_action();
        app.core.analysis_cfg.push_steps = 2;
        app.run_pushover();
        let mut result = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .pushover
            .clone()
            .unwrap();
        result.termination =
            sepika_solver::nonlinear::pushover::PushoverTermination::NonConvergence {
                phase: "荷重制御".into(),
                load_factor: 0.5,
            };
        app.apply_pushover_result(Ok(result));
        let notice = app.core.scoped.last_notice.as_ref().unwrap();
        assert!(notice.contains("せん断座屈"));
        assert!(notice.contains("目標到達前に打ち切られました"));
        assert_eq!(wall_notices(&app).len(), 1);
    }
}
