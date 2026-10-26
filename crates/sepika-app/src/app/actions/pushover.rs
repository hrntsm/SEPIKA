//! 増分解析（プッシュオーバー）。
//!
//! `actions` からの構造分割。アルゴリズム変更は行わない。

use super::*;

impl App {
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn set_rc_wall_shear_method(
        &mut self,
        method: sepika_core::model::RcWallShearMethod,
    ) {
        if method != self.core.model.stress_cfg.rc_wall_shear_method {
            self.core.scoped.undo.run(
                &mut self.core.model,
                Box::new(sepika_edit::SetRcWallShearMethod { method }),
            );
            self.core.scoped.staleness.mark_edited();
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn adopted_wall_capacity(
        &self,
        elem: &sepika_core::model::ElementData,
        model: &sepika_core::model::Model,
        analysis: crate::viewer::ModelingAnalysis,
    ) -> Result<[f64; 2], String> {
        if !sepika_element::wall::misc_wall::is_rc_wall(elem, model) {
            let qu = sepika_element::wall::wall_element::WallElement::steel_shear_capacity_of(
                elem, model,
            );
            return if qu.is_finite() && qu > 0.0 {
                Ok([qu; 2])
            } else {
                Err("鋼板壁耐力が未解決です".into())
            };
        }
        let capacities = match analysis {
            crate::viewer::ModelingAnalysis::Incremental => {
                let dir = self.core.scoped.pushover_view_dir;
                self.require_result_input(ResultInputKey::Pushover(dir))?;
                &self
                    .pushover_for(dir)
                    .ok_or("採用増分runがありません")?
                    .wall_capacities
            }
            crate::viewer::ModelingAnalysis::TimeHistory => {
                let case = self
                    .core
                    .scoped
                    .view_vibration_case
                    .ok_or("表示する非線形時刻歴ケースが未選択です")?;
                self.require_result_input(ResultInputKey::TimeHistory(case))?;
                let result = self
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|r| {
                        r.time_histories
                            .iter()
                            .find(|(id, _)| *id == case)
                            .map(|(_, r)| r)
                    })
                    .filter(|r| r.nonlinear)
                    .ok_or("採用非線形時刻歴runがありません")?;
                &result.wall_capacities
            }
            crate::viewer::ModelingAnalysis::Static => {
                return Err("線形静解析に初期Quはありません".into())
            }
        };
        let qu = capacities
            .iter()
            .find(|(id, _)| *id == elem.id)
            .map(|(_, qu)| *qu)
            .ok_or("採用runの初期Quが未解決です")?;
        if qu.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err("採用runの初期Quが不正です".into());
        }
        Ok(qu)
    }

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
        results.pushover_for_dir(view_dir)
    }

    /// 結果タブの増分解析表示方向を切り替える。
    #[cfg(any(test, feature = "gui"))]
    pub(crate) fn set_pushover_view_dir(&mut self, dir: SeismicDir) {
        if self.core.scoped.pushover_view_dir == dir {
            return;
        }
        self.core.scoped.pushover_view_dir = dir;
        #[cfg(feature = "gui")]
        {
            self.ui.scoped.hinge_view_cache = None;
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
                let mut bundle = self.take_results_for_analysis();
                bundle.record_input(input_key, input);
                match dir {
                    SeismicDir::X => bundle.pushover_x = Some(result),
                    SeismicDir::Y => bundle.pushover_y = Some(result),
                }
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
            .pushover_x
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

    #[test]
    fn 接線剛性による打切り理由は機構分類と独立に通知と保存結果へ反映する() {
        use sepika_solver::nonlinear::pushover::{MechanismType, PushoverTermination};

        let mut app = App::default();
        app.load_model(steel_wall_model());
        app.generate_stories_action();
        app.core.analysis_cfg.push_steps = 2;
        app.run_pushover();
        let mut result = app.displayed_pushover().unwrap().clone();
        result.mechanism = MechanismType::Partial;
        result.termination = PushoverTermination::TangentSingular {
            phase: "荷重制御".into(),
            load_factor: 0.5,
        };
        let qu = result.qu;
        let curve_len = result.capacity_curve.len();
        app.apply_pushover_result(Ok(result));

        let notice = app.core.scoped.last_notice.as_ref().unwrap();
        assert!(notice.contains("目標到達前に打ち切られました"));
        assert!(notice.contains("接線剛性の数値的な特異性・精度限界"));
        assert!(notice.contains("最大ベースシアはその時点までの最大値"));
        assert!(!notice.contains("機構形成・耐力喪失"));
        let bytes = rmp_serde::to_vec_named(app.core.scoped.results.as_ref().unwrap()).unwrap();
        let restored: ResultsBundle = rmp_serde::from_slice(&bytes).unwrap();
        let result = restored.pushover_x.as_ref().unwrap();
        assert!(matches!(result.mechanism, MechanismType::Partial));
        assert!(matches!(
            result.termination,
            PushoverTermination::TangentSingular { .. }
        ));
        assert!(result.termination.is_premature());
        assert_eq!(result.qu, qu);
        assert_eq!(result.capacity_curve.len(), curve_len);
    }
    #[test]
    fn 保存した表示方向は同じ最大ベースシアでも復元し方向欠落時は保持方向と設定だけで選ぶ() {
        let mut app = App::default();
        app.load_model(crate::app::tests::with_explicit_pushover_gravity(
            crate::sample::portal_frame(),
        ));
        app.core.analysis_cfg.push_steps = 2;
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let x = app.displayed_pushover().unwrap().clone();
        app.core.analysis_cfg.push_dir = SeismicDir::Y;
        app.apply_pushover_result(Ok(x));
        let both = app.core.scoped.results.as_ref().unwrap().clone();
        assert_eq!(
            both.pushover_x.as_ref().unwrap().qu,
            both.pushover_y.as_ref().unwrap().qu
        );

        for (index, (hold_x, hold_y, saved_dir, configured, expected)) in [
            (
                true,
                true,
                Some(SeismicDir::X),
                SeismicDir::Y,
                SeismicDir::X,
            ),
            (
                true,
                true,
                Some(SeismicDir::Y),
                SeismicDir::X,
                SeismicDir::Y,
            ),
            (true, false, None, SeismicDir::Y, SeismicDir::X),
            (false, true, None, SeismicDir::X, SeismicDir::Y),
            (true, true, None, SeismicDir::X, SeismicDir::X),
            (true, true, None, SeismicDir::Y, SeismicDir::Y),
            (
                true,
                false,
                Some(SeismicDir::Y),
                SeismicDir::X,
                SeismicDir::Y,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let mut bundle = both.clone();
            if !hold_x {
                bundle.pushover_x = None;
            }
            if !hold_y {
                bundle.pushover_y = None;
            }
            let saved = SavedResults {
                bundle,
                last_static: None,
                pushover_view_dir: saved_dir,
                view_vibration_case: None,
                view_lumped_vibration_case: None,
                last_run: None,
            };
            let mut value = serde_json::to_value(saved).unwrap();
            if saved_dir.is_none() {
                value.as_object_mut().unwrap().remove("pushover_view_dir");
            }
            let results = rmp_serde::to_vec_named(&value).unwrap();
            app.core.analysis_cfg.push_dir = configured;
            let settings = rmp_serde::to_vec_named(&SavedAnalysisSettings {
                cfg: app.core.analysis_cfg,
                wave_name: None,
                wave_sha256: None,
                lumped_wave_name: None,
                lumped_wave_sha256: None,
            })
            .unwrap();
            let file = std::env::temp_dir()
                .join(format!("sepika865-{}-{index}.ovika", std::process::id()));
            sepika_io::ovika::save_ovika(
                &file,
                &app.core.model,
                sepika_io::ovika::OvikaExtras {
                    results: Some(&results),
                    analysis_settings: Some(&settings),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut restored = App::default();
            restored.open_project_from(file.clone());
            assert!(
                restored.core.scoped.last_error.is_none(),
                "{:?}",
                restored.core.scoped.last_error
            );
            assert_eq!(restored.core.scoped.pushover_view_dir, expected);
            assert_eq!(
                restored.displayed_pushover().is_some(),
                match expected {
                    SeismicDir::X => hold_x,
                    SeismicDir::Y => hold_y,
                }
            );
            std::fs::remove_file(file).unwrap();
        }

        app.set_pushover_view_dir(SeismicDir::X);
        let file =
            std::env::temp_dir().join(format!("sepika865-save-{}.ovika", std::process::id()));
        app.save_project_to(file.clone());
        assert!(app.core.scoped.last_error.is_none());
        let contents = sepika_io::ovika::load_ovika(&file).unwrap();
        let value: serde_json::Value = rmp_serde::from_slice(&contents.results.unwrap()).unwrap();
        assert!(value["bundle"].get("pushover").is_none());
        let mut restored = App::default();
        restored.open_project_from(file.clone());
        assert_eq!(restored.core.scoped.pushover_view_dir, SeismicDir::X);
        assert!(restored.displayed_pushover().is_some());
        std::fs::remove_file(file).unwrap();
    }
}
