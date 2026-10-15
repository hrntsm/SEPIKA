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
        }
        let model = self.core.model.clone();
        let cfg = self.core.analysis_cfg;
        let (res_x, res_y, po_x, po_y) = self.lumped_static_and_pushover();
        self.spawn_analysis_job(label, move || {
            JobResult::LumpedMass(
                Box::new(Self::run_compute(|| {
                    sepika_job::compute::compute_lumped_mass(
                        model,
                        cfg,
                        res_x,
                        res_y,
                        po_x,
                        po_y,
                        accel.as_deref(),
                    )
                    .map_err(|e| e.to_string())
                })),
                source.map(Box::new),
            )
        });
        #[cfg(feature = "gui")]
        if let Some(job) = self.core.scoped.job.as_mut() {
            job.jump_on_success = Some((Tab::Results, ResultsView::LumpedMass));
        }
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
            if cfg.lumped_dim == StickDim::Spatial || !cfg.lumped_nonlinear {
                keys.push(ResultInputKey::Static(StaticCaseKey::Seismic(dir)));
            }
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

    pub(crate) fn apply_lumped_mass_result(
        &mut self,
        res: Result<LumpedMassResult, String>,
        source: Option<TimeHistorySource>,
    ) {
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
                    bundle.record_input(key, input);
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
            self.ui.view.results_view = ResultsView::Spatial;
            self.ui.view.view_mode = crate::viewer::ViewMode::LumpedMode;
            self.ui.scoped.view_mode_idx = mode_idx;
        }
    }

    /// 質点系サンプル波で時刻歴を開始する。
    pub fn start_lumped_mass_sample_th_job(&mut self) {
        let wave = sepika_job::sample_lumped_ground_motion(&self.core.analysis_cfg);
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

    fn cantilever_app() -> App {
        let mut model = crate::sample::portal_frame();
        model.nodes.truncate(2);
        model.nodes[1].coord = [0.0, 0.0, 3000.0];
        model.nodes[1].restraint = Dof6Mask::FREE;
        model.elements.truncate(1);
        model.elements[0].nodes = [NodeId(0), NodeId(1)].into_iter().collect();
        model.sections = vec![sepika_section::shape::SectionShape::SteelFlatBar {
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
        app.load_model(crate::sample::portal_frame());
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
    fn lumped_actual_dependencies_accept_only_required_current_results() {
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
                                dim == StickDim::Spatial || (!nonlinear && *d == dir)
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
                        assert!(
                            app.core.scoped.last_error.is_none(),
                            "{source:?}/{dim:?}/{nonlinear}/{dir:?}: {:?}",
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
}
