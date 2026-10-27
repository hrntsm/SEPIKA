//! 時刻歴応答解析。

use super::*;
use crate::app::vibration::vibration_th_dir_from_th;

impl App {
    pub(crate) fn time_history_input(
        &self,
        wave: sepika_solver::dynamic::timehistory::GroundMotion,
        wave_name: String,
    ) -> sepika_job::JobResult<TimeHistoryInput<sepika_solver::dynamic::timehistory::GroundMotion>>
    {
        let cfg = self.core.analysis_cfg;
        let raw_identity = crate::app::vibration::vibration_identity(&(
            wave.dt,
            &wave.accel_x,
            &wave.accel_y,
            &wave.accel_theta,
        ));
        let effective = sepika_job::compute::effective_time_history_wave(
            &cfg,
            sepika_solver::dynamic::timehistory::GroundMotion {
                dt: wave.dt,
                accel_x: wave.accel_x.clone(),
                accel_y: wave.accel_y.clone(),
                accel_theta: wave.accel_theta.clone(),
            },
        )?;
        let analysis_identity = crate::app::vibration::vibration_identity(&(
            effective.dt,
            &effective.accel_x,
            &effective.accel_y,
            &effective.accel_theta,
            crate::app::vibration::vibration_settings(cfg, false),
        ));
        let identity = self.time_history_identity(&analysis_identity);
        let target = self
            .core
            .model
            .vibration_cases
            .iter()
            .find(|c| c.input_identity.as_ref() == Some(&identity))
            .map(|c| c.id.0);
        Ok(TimeHistoryInput {
            data: wave,
            source: TimeHistorySource {
                raw_identity,
                cfg,
                wave_name,
                identity,
                analysis_identity,
                dependency_identity: None,
                target,
            },
        })
    }

    pub(crate) fn apply_time_history_result(
        &mut self,
        res: Result<sepika_solver::dynamic::timehistory::ResponseResult, String>,
        source: TimeHistorySource,
    ) {
        match res {
            Ok(res) => {
                if source.target.is_some_and(|id| {
                    !self.core.model.vibration_cases.iter().any(|c| {
                        c.id.0 == id && c.input_identity.as_ref() == Some(&source.identity)
                    })
                }) {
                    self.report_error("開始時の振動ケースが存在しないか入力識別が変更されたため、解析結果を破棄しました。");
                    return;
                }
                let input = self.vibration_result_input(&source);
                let dir = vibration_th_dir_from_th(source.cfg.th_dir);
                let case_id = self.core.model.upsert_vibration_case(
                    source.wave_name,
                    dir,
                    source.cfg.th_nonlinear,
                    Some(source.identity),
                );
                let mut bundle = self.take_results_for_analysis();
                let key = ResultInputKey::TimeHistory(case_id);
                bundle
                    .reference_vibration_settings
                    .retain(|(saved, _)| *saved != key);
                bundle.reference_vibration_settings.push((key, source.cfg));
                bundle.record_scoped_input(
                    ResultInputKey::TimeHistory(case_id),
                    input,
                    self.result_source_reference(),
                );
                bundle.upsert_time_history(case_id, res.clone());
                self.core.scoped.results = Some(bundle);
                self.set_spatial_time_history_view(case_id, &res);
                self.core.scoped.staleness.mark_non_calc_edited();
                self.core.scoped.staleness.mark_fresh();
                self.core.scoped.last_error = None;
            }
            Err(e) => self.report_error(e),
        }
    }

    /// 入力波による時刻歴応答解析を実行する。表示名は「入力波」。
    pub fn run_time_history(&mut self, wave: sepika_solver::dynamic::timehistory::GroundMotion) {
        let input = match self.time_history_input(wave, "入力波".into()) {
            Ok(input) => input,
            Err(error) => {
                self.report_error(error.to_string());
                return;
            }
        };
        self.run_time_history_input(input);
    }

    fn run_time_history_input(
        &mut self,
        mut input: TimeHistoryInput<sepika_solver::dynamic::timehistory::GroundMotion>,
    ) {
        if !self.begin_analysis() {
            return;
        }
        self.bind_time_history_source(&mut input.source, false);
        self.accept_time_history_reference(&input.source, false);
        if input.source.cfg.th_nonlinear {
            self.notice_steel_seismic_walls();
        }
        let res = sepika_job::compute::compute_time_history(
            self.core.model.clone(),
            input.source.cfg,
            input.data,
        )
        .map_err(|e| e.to_string());
        self.apply_time_history_result(res, input.source);
    }

    /// 入力波による時刻歴をバックグラウンドで実行する。実行中なら起動しない。
    pub fn start_time_history_job(
        &mut self,
        wave: sepika_solver::dynamic::timehistory::GroundMotion,
    ) {
        let input = match self.time_history_input(wave, "入力波".into()) {
            Ok(input) => input,
            Err(error) => {
                self.report_error(error.to_string());
                return;
            }
        };
        self.start_time_history_input_job(input);
    }

    pub(crate) fn start_time_history_input_job(
        &mut self,
        input: TimeHistoryInput<sepika_solver::dynamic::timehistory::GroundMotion>,
    ) {
        if !self.time_history_source_is_unchanged(&input.source) {
            return;
        }
        if !self.begin_analysis_job() {
            return;
        }
        let TimeHistoryInput {
            data: wave,
            mut source,
        } = input;
        self.bind_time_history_source(&mut source, false);
        self.accept_time_history_reference(&source, false);
        if source.cfg.th_nonlinear {
            self.notice_steel_seismic_walls();
        }
        let model = self.core.model.clone();
        let cfg = source.cfg;
        let label = if cfg.th_nonlinear {
            "時刻歴応答(非線形)"
        } else {
            "時刻歴応答(線形)"
        };
        self.spawn_analysis_job(label, move || {
            JobResult::TimeHistory(
                Box::new(Self::run_compute(|| {
                    sepika_job::compute::compute_time_history(model, cfg, wave)
                        .map_err(|e| e.to_string())
                })),
                Box::new(source),
            )
        });
        #[cfg(feature = "gui")]
        if let Some(job) = self.core.scoped.job.as_mut() {
            job.jump_on_success = Some((Tab::Results, ResultsView::TimeHistory));
        }
    }

    pub(crate) fn time_history_source_is_unchanged(&mut self, source: &TimeHistorySource) -> bool {
        if bincode::serialize(&source.cfg).unwrap()
            != bincode::serialize(&self.core.analysis_cfg).unwrap()
        {
            self.report_error("波形生成後に計算設定が変更されたため起動できません。波形を生成・読込し直してください。");
            return false;
        }
        true
    }

    pub(crate) fn sample_wave(
        cfg: &AnalysisSettings,
    ) -> sepika_job::JobResult<sepika_solver::dynamic::timehistory::GroundMotion> {
        sepika_job::sample_ground_motion(cfg)
    }

    /// 正弦減衰のサンプル地震波を生成して時刻歴解析を実行する（同期）。
    pub fn run_time_history_sample(&mut self) {
        self.apply_parallelism_setting();
        let wave = match Self::sample_wave(&self.core.analysis_cfg) {
            Ok(wave) => wave,
            Err(e) => {
                self.report_error(e.to_string());
                return;
            }
        };
        let input = match self.time_history_input(wave, "サンプル".into()) {
            Ok(input) => input,
            Err(error) => {
                self.report_error(error.to_string());
                return;
            }
        };
        self.run_time_history_input(input);
    }
}
