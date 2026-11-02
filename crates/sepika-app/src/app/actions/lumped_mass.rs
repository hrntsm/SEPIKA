//! 質点系解析。

use super::*;
use crate::app::vibration::{lumped_vibration_dim_from_stick, lumped_vibration_dir_from_seismic};
use sepika_solver::dynamic::lumped_mass::LumpedMassResult;

impl App {
    fn lumped_static_and_pushover(
        &self,
    ) -> (
        Option<sepika_solver::statics::linear::StaticOnce>,
        Option<sepika_solver::statics::linear::StaticOnce>,
        Option<sepika_solver::nonlinear::pushover::PushoverResult>,
        Option<sepika_solver::nonlinear::pushover::PushoverResult>,
    ) {
        let res_x = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.seismic(SeismicDir::X))
            .cloned();
        let res_y = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.seismic(SeismicDir::Y))
            .cloned();
        let po_x = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.pushover_x.clone());
        let po_y = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.pushover_y.clone());
        (res_x, res_y, po_x, po_y)
    }

    /// 質点系固有値（時刻歴なし）をバックグラウンドで実行する。
    pub fn start_lumped_mass_eigen_job(&mut self) {
        self.start_lumped_mass_job(None, None, "質点系固有値");
    }

    /// 質点系時刻歴をバックグラウンドで実行する（固有値も同時に算定する）。
    pub fn start_lumped_mass_th_job(&mut self, accel: Vec<f64>) {
        let input = self.lumped_time_history_input(accel, "入力波".into());
        self.start_lumped_mass_th_input_job(input);
    }

    pub(crate) fn lumped_time_history_input(
        &self,
        accel: Vec<f64>,
        wave_name: String,
    ) -> TimeHistoryInput<Vec<f64>> {
        let cfg = self.core.analysis_cfg;
        let raw_identity = crate::app::vibration::vibration_identity(&(cfg.lumped_th_dt, &accel));
        let dependency_identity = self.lumped_prerequisite_identity();
        let analysis_identity = crate::app::vibration::vibration_identity(&(
            &accel,
            crate::app::vibration::vibration_settings(cfg, true),
            &dependency_identity,
        ));
        let identity = self.time_history_identity(&analysis_identity);
        let target = self
            .core
            .model
            .lumped_vibration_cases
            .iter()
            .find(|c| c.input_identity.as_ref() == Some(&identity))
            .map(|c| c.id.0);
        TimeHistoryInput {
            data: accel,
            source: TimeHistorySource {
                raw_identity,
                cfg,
                wave_name,
                identity,
                analysis_identity,
                dependency_identity: Some(dependency_identity),
                target,
            },
        }
    }

    pub(crate) fn start_lumped_mass_th_input_job(&mut self, input: TimeHistoryInput<Vec<f64>>) {
        if !self.time_history_source_is_unchanged(&input.source) {
            return;
        }
        self.start_lumped_mass_job(Some(input.data), Some(input.source), "質点系時刻歴");
    }

    fn start_lumped_mass_job(
        &mut self,
        accel: Option<Vec<f64>>,
        mut source: Option<TimeHistorySource>,
        label: &'static str,
    ) {
        if !self.begin_analysis_job() {
            return;
        }
        if let Err(error) = self.require_lumped_prerequisites() {
            self.report_error(error);
            return;
        }
        if source.as_ref().is_some_and(|source| {
            source.dependency_identity.as_ref() != Some(&self.lumped_prerequisite_identity())
        }) {
            self.report_error("波形生成後に質点系が使う静的・増分結果の生成入力が変更されたため起動できません。波形を生成・読込し直してください。");
            return;
        }
        if let Some(source) = source.as_mut() {
            self.bind_time_history_source(source, true);
            self.accept_time_history_reference(source, true);
        }
        let build_identity = source.is_none().then(|| self.lumped_model_input_identity());
        let model = self.core.model.clone();
        let cfg = self.core.analysis_cfg;
        let (res_x, res_y, po_x, po_y) = self.lumped_static_and_pushover();
        self.spawn_analysis_job(label, move || {
            let computed = Self::run_compute(|| {
                match sepika_job::compute::compute_lumped_mass(
                    model,
                    cfg,
                    res_x,
                    res_y,
                    po_x,
                    po_y,
                    accel.as_deref(),
                ) {
                    Ok(result) => Ok(Ok(result)),
                    Err(sepika_job::JobError::LumpedIdentification(diagnostic)) => {
                        Ok(Err(diagnostic))
                    }
                    Err(error) => Err(error.to_string()),
                }
            });
            match computed {
                Ok(Err(diagnostic)) => JobResult::LumpedMassUnidentified(diagnostic),
                other => JobResult::LumpedMass(
                    Box::new(other.map(|r| r.expect("未同定は別結果として処理済み"))),
                    source.map(Box::new),
                    build_identity,
                ),
            }
        });
    }

    fn lumped_prerequisite_keys(&self) -> Vec<ResultInputKey> {
        use sepika_solver::dynamic::lumped_mass::StickDim;
        let cfg = self.core.analysis_cfg;
        let dirs: &[SeismicDir] = if cfg.lumped_dim == StickDim::Spatial {
            &[SeismicDir::X, SeismicDir::Y]
        } else {
            std::slice::from_ref(&cfg.lumped_dir)
        };
        let mut keys = Vec::new();
        for &dir in dirs {
            keys.push(ResultInputKey::Static(StaticCaseKey::Seismic(dir)));
            if cfg.lumped_nonlinear {
                keys.push(ResultInputKey::Pushover(dir));
            }
        }
        keys
    }

    fn lumped_prerequisite_identity(&self) -> String {
        let inputs: Vec<_> = self
            .lumped_prerequisite_keys()
            .into_iter()
            .map(|key| {
                let input = self.core.scoped.results.as_ref().and_then(|bundle| {
                    bundle
                        .input_records
                        .iter()
                        .find(|(saved, _)| *saved == key)
                        .map(|(_, input)| input.as_slice())
                });
                (key, input)
            })
            .collect();
        crate::app::vibration::vibration_identity(&inputs)
    }

    pub(crate) fn lumped_model_input_identity(&self) -> String {
        let c = self.core.analysis_cfg;
        crate::app::vibration::vibration_identity(&(
            self.core.model.calculation_model(),
            c.lumped_mass_type,
            c.lumped_dim,
            c.lumped_stiffness,
            c.lumped_dir,
            c.lumped_nonlinear,
            c.lumped_secant_ratio,
            c.lumped_target_drift_angle,
            c.lumped_n_modes,
            self.lumped_prerequisite_identity(),
        ))
    }

    pub(crate) fn lumped_window_is_current(&self) -> bool {
        self.core.scoped.view_lumped_vibration_case.is_some()
            || self
                .core
                .scoped
                .results
                .as_ref()
                .and_then(|b| b.lumped_input_identity.as_ref())
                .is_some_and(|identity| *identity == self.lumped_model_input_identity())
    }

    pub(crate) fn invalidate_lumped_mass_view(&mut self) {
        if let Some(bundle) = self.core.scoped.results.as_mut() {
            bundle.lumped = None;
            bundle.lumped_input_identity = None;
        }
        self.core.scoped.stick_response = None;
        self.core.scoped.view_lumped_vibration_case = None;
    }

    fn require_lumped_prerequisites(&self) -> Result<(), String> {
        for key in self.lumped_prerequisite_keys() {
            let (exists, label) = match key {
                ResultInputKey::Static(StaticCaseKey::Seismic(dir)) => (
                    self.core
                        .scoped
                        .results
                        .as_ref()
                        .and_then(|r| r.seismic(dir))
                        .is_some(),
                    format!("地震静的 {dir:?}"),
                ),
                ResultInputKey::Pushover(dir) => (
                    self.core
                        .scoped
                        .results
                        .as_ref()
                        .and_then(|r| r.pushover_for_dir(dir))
                        .is_some(),
                    format!("増分解析 {dir:?}"),
                ),
                _ => unreachable!(),
            };
            if !exists {
                return Err(format!(
                    "{label} の結果がありません。{label} を再実行してください。"
                ));
            }
            self.require_result_input(key)?;
        }
        Ok(())
    }

    pub(crate) fn apply_lumped_identification_failure(
        &mut self,
        failure: sepika_solver::dynamic::lumped_mass::LumpedMassIdentificationFailure,
    ) {
        let reason = failure.reason.clone();
        let mut bundle = self.take_results_for_analysis();
        bundle.lumped = None;
        bundle.lumped_input_identity = None;
        bundle.lumped_identification_failure = Some(failure);
        self.core.scoped.stick_response = None;
        self.core.scoped.view_lumped_vibration_case = None;
        self.core.scoped.results = Some(bundle);
        self.core.scoped.staleness.mark_non_calc_edited();
        self.report_error(reason);
    }

    #[cfg(test)]
    pub(crate) fn apply_lumped_mass_result(
        &mut self,
        res: Result<LumpedMassResult, String>,
        source: Option<TimeHistorySource>,
    ) {
        let identity = source.is_none().then(|| self.lumped_model_input_identity());
        self.apply_lumped_mass_result_with_identity(res, source, identity);
    }

    pub(crate) fn apply_lumped_mass_result_with_identity(
        &mut self,
        res: Result<LumpedMassResult, String>,
        source: Option<TimeHistorySource>,
        build_identity: Option<String>,
    ) {
        if source.is_none()
            && build_identity
                .as_ref()
                .is_none_or(|identity| *identity != self.lumped_model_input_identity())
        {
            self.report_error("質点系の開始後に生成条件が変わったため結果を破棄しました。質点系を再実行してください。");
            return;
        }
        match res {
            Ok(result) => {
                if result.response.is_some() && source.is_none() {
                    self.report_error(
                        "質点系時刻歴の入力識別情報がありません。再実行してください。",
                    );
                    return;
                }
                if source.as_ref().is_some_and(|s| {
                    s.target.is_some_and(|id| {
                        !self
                            .core
                            .model
                            .lumped_vibration_cases
                            .iter()
                            .any(|c| c.id.0 == id && c.input_identity.as_ref() == Some(&s.identity))
                    })
                }) {
                    self.report_error("開始時の質点系振動ケースが存在しないか入力識別が変更されたため、解析結果を破棄しました。");
                    return;
                }
                let mut bundle = self.take_results_for_analysis();
                bundle.lumped_identification_failure = None;
                bundle.lumped_input_identity = build_identity;
                if let Some(source) = source {
                    let input = self.vibration_result_input(&source);
                    let dir = lumped_vibration_dir_from_seismic(source.cfg.lumped_dir);
                    let dim = lumped_vibration_dim_from_stick(source.cfg.lumped_dim);
                    let case_id = self.core.model.upsert_lumped_vibration_case(
                        source.wave_name,
                        dir,
                        source.cfg.lumped_nonlinear,
                        dim,
                        Some(source.identity),
                    );
                    let key = ResultInputKey::LumpedTimeHistory(case_id);
                    bundle
                        .reference_vibration_settings
                        .retain(|(saved, _)| *saved != key);
                    bundle
                        .reference_vibration_settings
                        .push((key.clone(), source.cfg));
                    bundle.record_scoped_input(key, input, self.result_source_reference());
                    bundle.upsert_lumped_result(case_id, result.clone());
                    self.core.scoped.results = Some(bundle);
                    self.set_lumped_mass_view(Some(case_id), &result);
                    self.core.scoped.staleness.mark_non_calc_edited();
                    self.core.scoped.staleness.mark_fresh();
                } else {
                    self.core.scoped.results = Some(bundle);
                    self.set_lumped_mass_view(None, &result);
                    self.core.scoped.staleness.mark_non_calc_edited();
                    self.core.scoped.staleness.last_run = Some(SystemTime::now());
                }
                self.core.scoped.last_error = None;
            }
            Err(e) => self.report_error(e),
        }
    }

    /// 質点系の固有モードを 3D ビューアの「質点モード」で表示する。
    ///
    /// 結果タブの「質点系」は表とグラフなので、そこに切り替えるとモード形が
    /// 見えない。立体の固有値と同じく、3D ビューア側の表示モードを切り替える。
    #[cfg(any(test, feature = "gui"))]
    #[cfg_attr(not(feature = "gui"), allow(unused_variables))]
    pub(crate) fn select_lumped_eigen_mode(&mut self, mode_idx: usize) {
        self.ui.scoped.nav.focus_result = None;
        self.ui.view.active_tab = Tab::Results;
        #[cfg(feature = "gui")]
        {
            self.open_result_workspace(ResultsView::Spatial);
            self.active_viewport_mut().view_mode = crate::viewer::ViewMode::LumpedMode;
            self.active_viewport_mut().view_mode_idx = mode_idx;
        }
    }

    /// 質点系サンプル波で時刻歴を開始する。
    pub fn start_lumped_mass_sample_th_job(&mut self) {
        let wave = match sepika_job::sample_lumped_ground_motion(&self.core.analysis_cfg) {
            Ok(wave) => wave,
            Err(e) => {
                self.report_error(e.to_string());
                return;
            }
        };
        match sepika_job::lumped_accel_from_wave(&wave, self.core.analysis_cfg.lumped_dir) {
            Ok(accel) => {
                let input = self.lumped_time_history_input(accel, "サンプル".into());
                #[cfg(feature = "gui")]
                self.request_analysis(PendingAnalysis::LumpedMassTimeHistory(Box::new(input)));
                #[cfg(not(feature = "gui"))]
                self.start_lumped_mass_th_input_job(input);
            }
            Err(e) => self.report_error(e),
        }
    }

    /// 波形ライブラリの選択波形で質点系時刻歴を開始する。
    #[cfg(feature = "gui")]
    pub fn start_lumped_mass_library_th_job(&mut self) {
        if self.core.scoped.lumped_wave_library_selection.is_none() {
            return;
        }
        let Some(dir) = sepika_io::wave_library::wave_library_dir() else {
            self.report_error("波形ライブラリの保存先を特定できませんでした。".to_string());
            return;
        };
        self.start_lumped_mass_library_th_job_in(&dir);
    }

    #[cfg(feature = "gui")]
    pub(crate) fn start_lumped_mass_library_th_job_in(&mut self, dir: &std::path::Path) {
        let Some(name) = self.core.scoped.lumped_wave_library_selection.clone() else {
            return;
        };
        let Some(content) = self.read_wave_library_file(dir, &name) else {
            return;
        };
        let mut cfg = self.core.analysis_cfg;
        cfg.th_dt = cfg.lumped_th_dt;
        cfg.th_dir = match cfg.lumped_dir {
            SeismicDir::X => ThDir::X,
            SeismicDir::Y => ThDir::Y,
        };
        let Some(wave) = self.ground_motion_or_report(&cfg, &content) else {
            return;
        };
        self.core.scoped.lumped_wave_library_selected_sha256 =
            sepika_io::wave_library::wave_sha256(dir, &name).ok();
        match sepika_job::lumped_accel_from_wave(&wave, self.core.analysis_cfg.lumped_dir) {
            Ok(accel) => {
                let input = self.lumped_time_history_input(accel, name);
                self.request_analysis(PendingAnalysis::LumpedMassTimeHistory(Box::new(input)));
            }
            Err(e) => self.report_error(e),
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn set_lumped_wave_library_selection(&mut self, name: Option<String>) {
        if name != self.core.scoped.lumped_wave_library_selection {
            self.core.scoped.lumped_wave_library_selection = name;
            self.core.scoped.lumped_wave_library_selected_sha256 = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::{dof::Dof6Mask, ids::NodeId, model::FrameSectionUse};
    use sepika_solver::dynamic::lumped_mass::{LumpedStiffnessSource, StickDim};

    fn start_eigen(app: &mut App) {
        #[cfg(feature = "gui")]
        app.request_analysis(PendingAnalysis::LumpedMassEigen);
        #[cfg(not(feature = "gui"))]
        app.start_lumped_mass_eigen_job();
    }

    fn start_static(app: &mut App, dir: SeismicDir) {
        #[cfg(feature = "gui")]
        app.request_analysis(PendingAnalysis::StaticTarget(StaticTarget::Case(
            app.seismic_case_id(dir).unwrap(),
        )));
        #[cfg(not(feature = "gui"))]
        app.start_seismic_job(dir);
    }

    #[test]
    fn unidentified_diagnostic_survives_public_save_open_without_old_success_model() {
        use sepika_core::model::{LumpedSkeletonInterval, LumpedVibrationDir};
        use sepika_solver::dynamic::lumped_mass::{
            LumpedMassIdentificationFailure, LumpedMassType,
        };
        for failed_dir in [SeismicDir::X, SeismicDir::Y] {
            let mut app = App::default();
            app.load_model(crate::sample::portal_frame());
            app.core.analysis_cfg.lumped_dir = if failed_dir == SeismicDir::X {
                SeismicDir::Y
            } else {
                SeismicDir::X
            };
            let interval = LumpedSkeletonInterval {
                story: sepika_core::ids::StoryId(0),
                dir: LumpedVibrationDir::Y,
                interval_mm: [12.0, 25.0],
            };
            app.core.model.lumped_skeleton_intervals = vec![interval];
            app.core.scoped.results = Some(ResultsBundle {
                lumped: Some(crate::app::tests::dummy_lumped(0.5, true)),
                ..Default::default()
            });
            app.core.scoped.stick_response = Some(Default::default());
            let failure = LumpedMassIdentificationFailure {
                reason: format!("{failed_dir:?}方向全層: 同定上限へ未到達"),
                model_type: LumpedMassType::EquivalentBendingShear,
                dim: StickDim::Spatial,
                dir: failed_dir,
                target_drift_angle: 0.005,
                secant_ratio: 0.75,
                manual_intervals: vec![interval],
                source_targets: vec![(
                    SeismicDir::Y,
                    Some(sepika_solver::nonlinear::pushover::PushoverTarget {
                        max_disp: Some(42.0),
                        max_drift_angle: Some(1.0 / 150.0),
                    }),
                )],
            };
            app.apply_lumped_identification_failure(failure.clone());
            assert!(app.core.scoped.results.as_ref().unwrap().lumped.is_none());
            assert!(app.core.scoped.stick_response.is_none());
            let path = crate::app::tests::test_tmp().join("issue1087_unidentified.ovika");
            app.save_project_to(path.clone());
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            let mut restored = App::default();
            restored.open_project_from(path.clone());
            assert!(
                restored.core.scoped.last_error.is_none(),
                "{:?}",
                restored.core.scoped.last_error
            );
            let bundle = restored
                .core
                .scoped
                .results
                .as_ref()
                .expect("診断を保存する");
            assert!(bundle.lumped.is_none());
            assert!(restored.core.scoped.stick_response.is_none());
            let diagnostic = bundle.lumped_identification_failure.as_ref().unwrap();
            assert_eq!(diagnostic.reason, failure.reason);
            assert_eq!(diagnostic.dir, failed_dir);
            assert_eq!(diagnostic.target_drift_angle, 0.005);
            assert_eq!(diagnostic.secant_ratio, 0.75);
            assert_eq!(diagnostic.manual_intervals, vec![interval]);
            assert_eq!(diagnostic.source_targets[0].1.unwrap().max_disp, Some(42.0));
            assert_eq!(
                restored.core.model.lumped_skeleton_intervals,
                vec![interval]
            );
            #[cfg(feature = "gui")]
            {
                restored.core.model.lumped_skeleton_intervals[0].interval_mm = [99.0, 100.0];
                let context = egui::Context::default();
                crate::app::install_japanese_fonts(&context);
                let output =
                    context.run_ui(Default::default(), |ui| restored.lumped_mass_panel(ui));
                let visible = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) => Some(t.galley.text()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                assert!(
                    visible.contains("元の下端階ID 0 / Y: [12.000, 25.000] mm"),
                    "{visible}"
                );
                assert!(!visible.contains("99.000"));
                assert!(
                    visible.contains(&format!("Spatial / {failed_dir:?}")),
                    "{visible}"
                );
            }
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn identified_bending_metadata_and_both_ductilities_survive_public_project_roundtrip() {
        use sepika_core::ids::StoryId;
        use sepika_core::model::Story;
        use sepika_solver::dynamic::lumped_mass::*;
        let fit = identify_story_trilinear(
            &[(1.0, 100.0), (3.0, 140.0), (6.0, 155.0), (10.0, 175.0)],
            StoryFitOptions {
                initial_stiffness: 100.0,
                secant_ratio: 0.75,
                area_limit: 6.0,
                identification_limit: 10.0,
                manual_interval: Some([4.0, 10.0]),
            },
        )
        .unwrap();
        let mut model = LumpedMassModel::from_stories(
            LumpedMassType::EquivalentBendingShear,
            vec![StoryStick {
                story: StoryId(0),
                mass: 1.0,
                height: 3000.0,
                skeleton: fit.skeleton,
            }],
        );
        model.bending = vec![StoryBending {
            ei_x: 0.0,
            ei_y: 1e12,
            inertia: [0.0, 1e6, 0.0],
        }];
        model.identifications = vec![StoryIdentification {
            story: StoryId(0),
            dir: SeismicDir::X,
            fit,
            source_target: sepika_solver::nonlinear::pushover::PushoverTarget {
                max_disp: Some(20.0),
                max_drift_angle: None,
            },
            bending_yield_drift: fit.skeleton.q2 * 0.009,
        }];
        let modal = lumped_mass_eigen(&model, 2).unwrap();
        let acceleration = vec![0.0, 10.0, -10.0, 0.0];
        let response = bending_shear_time_history(&model, &acceleration, 0.01, 0.02).unwrap();
        let expected_main = response.story_ductility.clone();
        let expected_total = response.story_total_ductility.clone().unwrap();
        let mut app = App::default();
        let building = sepika_core::model::Model {
            stories: vec![
                Story::new(StoryId(0), "1F".into(), Some(0.0), Some(0.0)),
                Story::new(StoryId(1), "2F".into(), Some(3000.0), Some(0.0)),
            ],
            ..Default::default()
        };
        app.load_model(building);
        app.core.analysis_cfg.lumped_mass_type = LumpedMassType::EquivalentBendingShear;
        app.core.analysis_cfg.lumped_nonlinear = true;
        app.core.analysis_cfg.lumped_target_drift_angle = 0.002;
        let mut source = app
            .lumped_time_history_input(acceleration, "独立骨格".into())
            .source;
        app.bind_time_history_source(&mut source, true);
        app.accept_time_history_reference(&source, true);
        app.apply_lumped_mass_result(
            Ok(LumpedMassResult {
                model,
                modal,
                response: Some(response),
            }),
            Some(source),
        );
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let path = crate::app::tests::test_tmp().join("issue1087_identified.ovika");
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut restored = App::default();
        restored.open_project_from(path.clone());
        assert!(
            restored.core.scoped.last_error.is_none(),
            "{:?}",
            restored.core.scoped.last_error
        );
        let result = restored
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped
            .as_ref()
            .unwrap();
        assert_eq!(result.model.identifications[0].fit, fit);
        assert_eq!(
            result.model.identifications[0].bending_yield_drift,
            fit.skeleton.q2 * 0.009
        );
        assert_eq!(
            result.model.identifications[0].source_target.max_disp,
            Some(20.0)
        );
        let response = result.response.as_ref().unwrap();
        assert_eq!(response.story_ductility, expected_main);
        assert_eq!(
            response.story_total_ductility.as_ref().unwrap(),
            &expected_total
        );
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "gui")]
    #[test]
    fn manual_intervals_invalidate_only_lumped_window_on_edit_undo_redo_display_and_save() {
        use sepika_core::model::{LumpedSkeletonInterval, LumpedVibrationDir};
        let mut app = portal_app();
        app.core.analysis_cfg.lumped_nonlinear = true;
        app.core.analysis_cfg.lumped_dim = StickDim::Planar;
        app.core.analysis_cfg.lumped_dir = SeismicDir::X;
        app.core.analysis_cfg.push_dir = SeismicDir::X;
        app.core.analysis_cfg.push_use_max_disp = true;
        app.core.analysis_cfg.push_use_drift_angle = false;
        app.core.analysis_cfg.push_max_disp = 100.0;
        app.run_pushover();
        crate::app::tests::install_independent_lumped_backbone(&mut app, 100.0);
        let predecessors = bincode::serialize(&(
            app.core.scoped.results.as_ref().unwrap().statics.clone(),
            app.core.scoped.results.as_ref().unwrap().pushover_x.clone(),
        ))
        .unwrap();
        start_eigen(&mut app);
        crate::app::tests::wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let old_result = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped
            .clone()
            .unwrap();
        let old_identity = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped_input_identity
            .clone()
            .unwrap();
        assert!(old_result.model.identifications[0]
            .fit
            .options
            .manual_interval
            .is_none());
        let interval = LumpedSkeletonInterval {
            story: sepika_core::ids::StoryId(0),
            dir: LumpedVibrationDir::X,
            interval_mm: [8.0, 100.0],
        };
        assert!(
            app.apply_model_edit(Box::new(sepika_edit::SetLumpedSkeletonIntervals {
                intervals: vec![interval]
            }))
        );
        assert!(app.core.scoped.results.as_ref().unwrap().lumped.is_none());
        assert!(app.core.scoped.stick_response.is_none());
        assert!(app.require_lumped_prerequisites().is_ok());
        app.core.scoped.results.as_mut().unwrap().lumped = Some(old_result.clone());
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .lumped_input_identity = Some(old_identity.clone());
        assert!(!app.lumped_window_is_current());
        let context = egui::Context::default();
        crate::app::install_japanese_fonts(&context);
        let output = context.run_ui(Default::default(), |ui| app.lumped_mass_panel(ui));
        fn text(shape: &egui::Shape) -> String {
            match shape {
                egui::Shape::Text(t) => t.galley.text().into(),
                egui::Shape::Vec(v) => v.iter().map(text).collect::<Vec<_>>().join(" "),
                _ => String::new(),
            }
        }
        let visible = output
            .shapes
            .iter()
            .map(|s| text(&s.shape))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(visible.contains("生成条件が変わっています"), "{visible}");
        assert!(!visible.contains("第2折点"));
        let path = crate::app::tests::test_tmp().join("issue1087_manual_stale.ovika");
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut restored = App::default();
        restored.open_project_from(path.clone());
        assert!(restored
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped
            .is_none());
        std::fs::remove_file(path).unwrap();
        app.apply_lumped_mass_result_with_identity(Ok(old_result), None, Some(old_identity));
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("破棄"));
        app.undo_action();
        assert!(app.core.model.lumped_skeleton_intervals.is_empty());
        assert!(app.core.scoped.results.as_ref().unwrap().lumped.is_none());
        start_eigen(&mut app);
        crate::app::tests::wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.redo_action();
        assert_eq!(app.core.model.lumped_skeleton_intervals, vec![interval]);
        assert!(app.core.scoped.results.as_ref().unwrap().lumped.is_none());
        start_eigen(&mut app);
        crate::app::tests::wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert_eq!(
            app.core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .lumped
                .as_ref()
                .unwrap()
                .model
                .identifications[0]
                .fit
                .options
                .manual_interval,
            Some([8.0, 100.0])
        );
        app.undo_action();
        assert!(app.core.scoped.results.as_ref().unwrap().lumped.is_none());
        assert_eq!(
            bincode::serialize(&(
                app.core.scoped.results.as_ref().unwrap().statics.clone(),
                app.core.scoped.results.as_ref().unwrap().pushover_x.clone()
            ))
            .unwrap(),
            predecessors
        );
    }

    fn cantilever_app() -> App {
        let mut model = crate::sample::portal_frame();
        model.nodes.truncate(2);
        model.nodes[1].coord = [0.0, 0.0, 3000.0];
        model.stories = crate::app::tests::explicit_story_inputs(&[0.0, 3000.0]);
        model.nodes[1].restraint = Dof6Mask::FREE;
        model.elements.truncate(1);
        model.elements[0].nodes = [NodeId(0), NodeId(1)].into_iter().collect();
        model.sections = vec![sepika_core::section_shape::SectionShape::SteelFlatBar {
            width: 300.0,
            thick: 300.0,
        }
        .to_section(sepika_core::ids::SectionId(0), "片持柱".into())];
        model.sections[0].frame_use = Some(FrameSectionUse::Column);
        model.sections[0].material = Some(model.materials[0].id);
        model.materials[0].name = "独立検証用鋼材".into();
        model.materials[0].young = 200_000.0;
        model.materials[0].poisson = 0.3;
        model.materials[0].shear = None;
        model.load_cases.clear();
        let mut app = App::default();
        app.load_model(model);
        app.core.analysis_cfg.threads = 1;
        app.core.analysis_cfg.ai_mode = AiMode::Approx;
        app.core.analysis_cfg.lumped_dim = StickDim::Planar;
        app.core.analysis_cfg.lumped_stiffness = LumpedStiffnessSource::StoryQd;
        app.core.analysis_cfg.lumped_dir = SeismicDir::X;
        app.core.analysis_cfg.lumped_nonlinear = false;
        app.generate_stories_action();
        app.core.model.stories[1].weight_override = Some(100_000.0);
        app.generate_stories_action();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app
    }

    #[test]
    fn lumped_story_qd_gui_rejects_static_before_young_modulus_edit() {
        let mut app = cantilever_app();
        start_static(&mut app, SeismicDir::X);
        crate::app::tests::wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mass_before = app.core.model.layers()[0]
            .dynamic_mass
            .unwrap()
            .mass_equiv_weight_n
            / sepika_core::units::GRAVITY_MM_S2;
        let sec = &app.core.model.sections[0];
        assert_eq!(sec.iy, 675_000_000.0);
        assert_eq!(sec.iz, 675_000_000.0);
        assert_eq!(sec.as_y, 75_000.0);
        assert_eq!(sec.as_z, 75_000.0);
        assert!((mass_before - 1.05975).abs() < 1e-12);
        assert_eq!(app.core.model.elements[0].rigid_zone.length_i, 0.0);
        assert_eq!(app.core.model.elements[0].rigid_zone.length_j, 0.0);
        assert_eq!(app.core.model.elements[0].rigid_zone.panel_offset_i, 0.0);
        assert_eq!(app.core.model.elements[0].rigid_zone.panel_offset_j, 0.0);
        assert!((app.core.model.materials[0].shear_modulus() - 76_923.076_923_076_92).abs() < 1e-9);
        let reference = sepika_solver::nonlinear::pushover::story_reference_node(
            &app.core.model,
            &app.core.model.stories[1],
        )
        .unwrap();
        assert_eq!(
            app.core.model.nodes[reference.index()].coord,
            app.core.model.nodes[1].coord
        );
        assert!(app
            .core
            .model
            .diaphragms_of(app.core.model.stories[1].id)
            .any(|dia| dia.master == reference && dia.slaves.contains(&NodeId(1))));
        println!("fixture: L=3000 mm E=200000 N/mm2 nu=0.3 Iy={} Iz={} AsY={} AsZ={} mass={} N*s2/mm rigid_zone={:?}", sec.iy, sec.iz, sec.as_y, sec.as_z, mass_before, app.core.model.elements[0].rigid_zone);
        start_eigen(&mut app);
        crate::app::tests::wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let initial = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped
            .as_ref()
            .unwrap();
        assert!((initial.model.stories[0].skeleton.k1 - 14_883.905_536_812_86).abs() < 1e-7);
        assert!((initial.modal.period[0] - 0.053_017_974_078_015_15).abs() < 1e-12);
        let period_before = initial.modal.period[0];
        let results_before = bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap();
        app.core.model.materials[0].young *= 0.5;
        start_eigen(&mut app);
        assert_eq!(
            app.core.model.layers()[0]
                .dynamic_mass
                .unwrap()
                .mass_equiv_weight_n
                / sepika_core::units::GRAVITY_MM_S2,
            mass_before
        );
        assert!(
            app.core.scoped.job.is_none(),
            "E変更前のStaticで質点系ジョブを開始した"
        );
        let error = app.core.scoped.last_error.as_deref().unwrap();
        assert!(
            error.contains("地震静的 X") && error.contains("再実行"),
            "{error}"
        );
        assert_eq!(
            bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap(),
            results_before
        );
        start_static(&mut app, SeismicDir::X);
        crate::app::tests::wait_for_job(&mut app);
        start_eigen(&mut app);
        crate::app::tests::wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let recovered = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped
            .as_ref()
            .unwrap();
        assert_eq!(recovered.model.stories[0].mass, mass_before);
        assert!((recovered.model.stories[0].skeleton.k1 - 7_441.952_768_406_43).abs() < 1e-7);
        assert!((recovered.modal.period[0] - 0.074_978_737_990_674_23).abs() < 1e-12);
        assert!((recovered.modal.period[0] / period_before - 2.0_f64.sqrt()).abs() < 1e-12);
    }

    fn portal_app() -> App {
        let mut app = App::default();
        app.load_model(crate::app::tests::with_explicit_pushover_gravity(
            crate::sample::portal_frame(),
        ));
        app.core.analysis_cfg.threads = 1;
        app.core.analysis_cfg.ai_mode = AiMode::Approx;
        app.core.analysis_cfg.push_steps = 10;
        app.generate_stories_action();
        for dir in [SeismicDir::X, SeismicDir::Y] {
            app.run_seismic(dir);
            app.core.analysis_cfg.push_dir = dir;
            app.run_pushover();
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
        }
        app
    }

    fn remove_slot(bundle: &mut ResultsBundle, key: &ResultInputKey) {
        match key {
            ResultInputKey::Static(key) => bundle.statics.retain(|(k, _)| k != key),
            ResultInputKey::Pushover(SeismicDir::X) => bundle.pushover_x = None,
            ResultInputKey::Pushover(SeismicDir::Y) => bundle.pushover_y = None,
            _ => unreachable!(),
        }
    }

    #[test]
    fn lumped_prerequisites_allow_current_attempts_and_reject_missing_or_stale_dependencies() {
        let mut app = portal_app();
        let all_results = app.core.scoped.results.as_ref().unwrap().clone();
        for source in [
            LumpedStiffnessSource::ColumnKi,
            LumpedStiffnessSource::StoryQd,
        ] {
            for dim in [StickDim::Planar, StickDim::Spatial] {
                for nonlinear in [false, true] {
                    for dir in [SeismicDir::X, SeismicDir::Y] {
                        app.core.analysis_cfg.lumped_stiffness = source;
                        app.core.analysis_cfg.lumped_dim = dim;
                        app.core.analysis_cfg.lumped_nonlinear = nonlinear;
                        app.core.analysis_cfg.lumped_dir = dir;
                        let required: Vec<_> = [
                            ResultInputKey::Static(StaticCaseKey::Seismic(SeismicDir::X)),
                            ResultInputKey::Static(StaticCaseKey::Seismic(SeismicDir::Y)),
                            ResultInputKey::Pushover(SeismicDir::X),
                            ResultInputKey::Pushover(SeismicDir::Y),
                        ]
                        .into_iter()
                        .filter(|key| match key {
                            ResultInputKey::Static(StaticCaseKey::Seismic(d)) => {
                                dim == StickDim::Spatial || *d == dir
                            }
                            ResultInputKey::Pushover(d) => {
                                nonlinear && (dim == StickDim::Spatial || *d == dir)
                            }
                            _ => unreachable!(),
                        })
                        .collect();
                        let mut bundle = all_results.clone();
                        for key in &all_results
                            .input_records
                            .iter()
                            .map(|(k, _)| k.clone())
                            .collect::<Vec<_>>()
                        {
                            if matches!(
                                key,
                                ResultInputKey::Static(_) | ResultInputKey::Pushover(_)
                            ) && !required.contains(key)
                            {
                                remove_slot(&mut bundle, key);
                                bundle.input_records.retain(|(k, _)| k != key);
                            }
                        }
                        app.core.scoped.results = Some(bundle.clone());
                        start_eigen(&mut app);
                        assert!(
                            app.core.scoped.job.is_some(),
                            "{source:?}/{dim:?}/{nonlinear}/{dir:?}: {:?}",
                            app.core.scoped.last_error
                        );
                        crate::app::tests::wait_for_job(&mut app);
                        if nonlinear {
                            let diagnostic = app
                                .core
                                .scoped
                                .results
                                .as_ref()
                                .unwrap()
                                .lumped_identification_failure
                                .as_ref()
                                .expect("降伏後枝が不足する元曲線は診断として返す");
                            assert!(
                                diagnostic.reason.contains("等価降伏未同定"),
                                "{diagnostic:?}"
                            );
                            assert!(app.core.scoped.results.as_ref().unwrap().lumped.is_none());
                        } else {
                            assert!(
                                app.core.scoped.last_error.is_none(),
                                "{:?}",
                                app.core.scoped.last_error
                            );
                            assert!(!app
                                .core
                                .scoped
                                .results
                                .as_ref()
                                .unwrap()
                                .lumped
                                .as_ref()
                                .unwrap()
                                .modal
                                .period
                                .is_empty());
                        }
                        for key in &required {
                            for fault in 0..3 {
                                let mut bad = bundle.clone();
                                match fault {
                                    0 => remove_slot(&mut bad, key),
                                    1 => bad.input_records.retain(|(k, _)| k != key),
                                    _ => {
                                        bad.input_records
                                            .iter_mut()
                                            .find(|(k, _)| k == key)
                                            .unwrap()
                                            .1 = vec![0]
                                    }
                                }
                                let before = bincode::serialize(&bad).unwrap();
                                app.core.scoped.results = Some(bad);
                                start_eigen(&mut app);
                                assert!(
                                    app.core.scoped.job.is_none(),
                                    "{source:?}/{dim:?}/{nonlinear}/{dir:?}/{key:?}/{fault}"
                                );
                                let error = app.core.scoped.last_error.as_deref().unwrap();
                                let label = match key {
                                    ResultInputKey::Static(StaticCaseKey::Seismic(d)) => {
                                        format!("地震静的 {d:?}")
                                    }
                                    ResultInputKey::Pushover(d) => format!("増分解析 {d:?}"),
                                    _ => unreachable!(),
                                };
                                assert!(
                                    error.contains(&label) && error.contains("再実行"),
                                    "{error}"
                                );
                                assert_eq!(
                                    bincode::serialize(app.core.scoped.results.as_ref().unwrap())
                                        .unwrap(),
                                    before
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn lumped_edits_require_reanalysis_but_display_and_lumped_settings_do_not() {
        let mut app = cantilever_app();
        app.run_seismic(SeismicDir::X);
        let model = app.core.model.clone();
        let cfg = app.core.analysis_cfg;
        let bundle = app.core.scoped.results.as_ref().unwrap().clone();
        for edit in 0..6 {
            app.core.model = model.clone();
            app.core.analysis_cfg = cfg;
            app.core.scoped.results = Some(bundle.clone());
            match edit {
                0 => app.core.model.materials[0].young *= 0.9,
                1 => app.core.model.nodes[1].coord[0] += 100.0,
                2 => app.core.model.materials[0].density *= 0.9,
                3 => app.core.model.stories[1].weight_override = Some(90_000.0),
                4 => {
                    app.core.model.load_cases[0]
                        .nodal
                        .push(sepika_core::model::NodalLoad::manual(
                            NodeId(1),
                            [100.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                        ))
                }
                _ => app.core.analysis_cfg.c0 = 0.3,
            }
            start_eigen(&mut app);
            assert!(app.core.scoped.job.is_none(), "edit={edit}");
            assert!(app
                .core
                .scoped
                .last_error
                .as_deref()
                .unwrap()
                .contains("再実行"));
            assert!(
                app.core.scoped.results.as_ref().unwrap().input_records == bundle.input_records,
                "edit={edit}: {:?}",
                app.core.scoped.last_error
            );
        }
        app.core.model = model;
        app.core.analysis_cfg = cfg;
        app.core.scoped.results = Some(bundle);
        app.ui.view.active_tab = Tab::Results;
        app.core.analysis_cfg.lumped_n_modes = 1;
        app.core.analysis_cfg.lumped_th_dt = 0.02;
        start_eigen(&mut app);
        crate::app::tests::wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
    }

    #[test]
    fn lumped_job_edit_failure_and_cancellation_keep_previous_results() {
        let mut app = cantilever_app();
        app.run_seismic(SeismicDir::X);
        start_eigen(&mut app);
        crate::app::tests::wait_for_job(&mut app);
        let before = bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap();
        start_eigen(&mut app);
        app.core.model.materials[0].young *= 0.5;
        crate::app::tests::wait_for_job(&mut app);
        assert!(app
            .core
            .scoped
            .last_error
            .as_deref()
            .unwrap()
            .contains("旧入力の解析結果を破棄"));
        assert_eq!(
            bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap(),
            before
        );
        app.core.model.materials[0].young *= 2.0;
        app.start_lumped_mass_th_job(vec![]);
        crate::app::tests::wait_for_job(&mut app);
        assert!(app.core.scoped.last_error.is_some());
        assert_eq!(
            bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap(),
            before
        );
        start_eigen(&mut app);
        assert!(app.core.scoped.job.take().is_some());
        assert!(!app.poll_job());
        assert_eq!(
            bincode::serialize(app.core.scoped.results.as_ref().unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn lumped_nonlinear_rejects_changed_pushover_settings() {
        let mut app = portal_app();
        app.core.analysis_cfg.lumped_dim = StickDim::Planar;
        app.core.analysis_cfg.lumped_nonlinear = true;
        app.core.analysis_cfg.lumped_dir = SeismicDir::X;
        let before = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .input_records
            .clone();
        app.core.analysis_cfg.push_steps += 1;
        start_eigen(&mut app);
        assert!(app.core.scoped.job.is_none());
        let error = app.core.scoped.last_error.as_deref().unwrap();
        assert!(
            error.contains("増分解析 X") && error.contains("再実行"),
            "{error}"
        );
        assert_eq!(
            app.core.scoped.results.as_ref().unwrap().input_records,
            before
        );
    }
    #[test]
    fn both_nonlinear_model_types_require_current_elastic_static_and_pushover() {
        use sepika_solver::dynamic::lumped_mass::LumpedMassType;
        let mut app = portal_app();
        app.core.analysis_cfg.lumped_dim = StickDim::Planar;
        app.core.analysis_cfg.lumped_nonlinear = true;
        app.core.analysis_cfg.lumped_dir = SeismicDir::X;
        let key = ResultInputKey::Static(StaticCaseKey::Seismic(SeismicDir::X));
        remove_slot(app.core.scoped.results.as_mut().unwrap(), &key);
        assert!(app
            .require_lumped_prerequisites()
            .unwrap_err()
            .contains("地震静的 X"));
        app.core.analysis_cfg.lumped_mass_type = LumpedMassType::EquivalentBendingShear;
        let error = app.require_lumped_prerequisites().unwrap_err();
        assert!(error.contains("地震静的 X"), "{error}");
        app.run_seismic(SeismicDir::X);
        assert!(app.require_lumped_prerequisites().is_ok());
        app.core.model.materials[0].young *= 0.5;
        let error = app.require_lumped_prerequisites().unwrap_err();
        assert!(error.contains("再実行"), "{error}");
    }

    #[test]
    fn bending_planar_gui_job_retains_ei_and_rotations() {
        use sepika_solver::dynamic::lumped_mass::LumpedMassType;
        let mut app = portal_app();
        app.core.analysis_cfg.lumped_mass_type = LumpedMassType::EquivalentBendingShear;
        app.core.analysis_cfg.lumped_dim = StickDim::Planar;
        app.core.analysis_cfg.lumped_dir = SeismicDir::X;
        app.core.analysis_cfg.lumped_nonlinear = false;
        start_eigen(&mut app);
        crate::app::tests::wait_for_job(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let result = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .lumped
            .as_ref()
            .unwrap();
        assert_eq!(
            result.model.model_type,
            LumpedMassType::EquivalentBendingShear
        );
        assert!(result.model.bending[0].ei_y > 0.0);
        assert!(!result.modal.bending_rotations.is_empty());
        assert_eq!(result.modal.period.len(), 2);
    }
}
