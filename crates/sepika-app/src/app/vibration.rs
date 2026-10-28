//! 振動荷重ケースと結果スロットの upsert・表示切替。

use super::*;
use sepika_core::ids::{LumpedVibrationCaseId, VibrationCaseId};
use sepika_core::model::{LumpedVibrationDim, LumpedVibrationDir, VibrationThDir};
use sepika_solver::dynamic::lumped_mass::StickDim;

pub(crate) fn vibration_th_dir_from_th(dir: ThDir) -> VibrationThDir {
    match dir {
        ThDir::X => VibrationThDir::X,
        ThDir::Y => VibrationThDir::Y,
        ThDir::Xy => VibrationThDir::Xy,
    }
}

pub(crate) fn lumped_vibration_dir_from_seismic(dir: SeismicDir) -> LumpedVibrationDir {
    match dir {
        SeismicDir::X => LumpedVibrationDir::X,
        SeismicDir::Y => LumpedVibrationDir::Y,
    }
}

pub(crate) fn lumped_vibration_dim_from_stick(dim: StickDim) -> LumpedVibrationDim {
    match dim {
        StickDim::Planar => LumpedVibrationDim::Planar,
        StickDim::Spatial => LumpedVibrationDim::Spatial,
    }
}

pub(crate) fn vibration_identity(value: &impl serde::Serialize) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(bincode::serialize(value).expect("時刻歴入力の直列化"))
    )
}

pub(crate) fn vibration_settings(cfg: AnalysisSettings, lumped: bool) -> AnalysisSettings {
    let mut relevant = AnalysisSettings::default();
    if lumped {
        relevant.lumped_mass_type = cfg.lumped_mass_type;
        relevant.lumped_dim = cfg.lumped_dim;
        relevant.lumped_stiffness = cfg.lumped_stiffness;
        relevant.lumped_dir = cfg.lumped_dir;
        relevant.lumped_nonlinear = cfg.lumped_nonlinear;
        relevant.lumped_secant_ratio = cfg.lumped_secant_ratio;
        relevant.lumped_n_modes = cfg.lumped_n_modes;
        relevant.lumped_th_dt = cfg.lumped_th_dt;
        relevant.lumped_th_damping = cfg.lumped_th_damping;
    } else {
        relevant.th_dir = cfg.th_dir;
        relevant.th_damping = cfg.th_damping;
        relevant.th_damping_model = cfg.th_damping_model;
        relevant.th_h2 = cfg.th_h2;
        relevant.th_nonlinear = cfg.th_nonlinear;
        relevant.th_apply_long_term = cfg.th_apply_long_term;
        relevant.th_max_iter = cfg.th_max_iter;
        relevant.th_tol = cfg.th_tol;
        relevant.th_record_every = cfg.th_record_every;
        relevant.phase_diff_enabled = cfg.phase_diff_enabled;
        relevant.phase_diff_vs = cfg.phase_diff_vs;
        relevant.phase_diff_length_m = cfg.phase_diff_length_m;
        relevant.phase_diff_incidence_deg = cfg.phase_diff_incidence_deg;
        relevant.phase_diff_dir_y = cfg.phase_diff_dir_y;
    }
    relevant
}

impl App {
    pub(crate) fn time_history_identity(&self, analysis_identity: &str) -> String {
        vibration_identity(&(analysis_identity, self.core.model.calculation_model()))
    }

    pub(crate) fn bind_time_history_source(&self, source: &mut TimeHistorySource, lumped: bool) {
        let identity = self.time_history_identity(&source.analysis_identity);
        if source.identity != identity || source.target.is_none() {
            source.target = if lumped {
                self.core
                    .model
                    .lumped_vibration_cases
                    .iter()
                    .find(|c| c.input_identity.as_ref() == Some(&identity))
                    .map(|c| c.id.0)
            } else {
                self.core
                    .model
                    .vibration_cases
                    .iter()
                    .find(|c| c.input_identity.as_ref() == Some(&identity))
                    .map(|c| c.id.0)
            };
        }
        source.identity = identity;
    }

    /// 立体時刻歴の表示窓口を切り替える（`time_history_data` も更新）。
    pub(crate) fn set_spatial_time_history_view(
        &mut self,
        case_id: VibrationCaseId,
        res: &sepika_solver::dynamic::timehistory::ResponseResult,
    ) {
        self.core.scoped.view_vibration_case = Some(case_id);
        if let Some(bundle) = &mut self.core.scoped.results {
            bundle.time_history = Some(res.clone());
        }
        self.fill_time_history_data(res);
    }

    fn fill_time_history_data(
        &mut self,
        res: &sepika_solver::dynamic::timehistory::ResponseResult,
    ) {
        #[cfg(feature = "gui")]
        {
            self.ui.scoped.time_history_data = crate::time_history_view::TimeHistoryData {
                time: res.time.clone(),
                node_disp: res.history.node_disp.clone(),
                story_shear: res.history.base_shear.clone(),
                story_drift_angle: res.history.top_drift_angle.clone(),
                node: res.history.node,
            };
        }
        #[cfg(not(feature = "gui"))]
        let _ = res;
    }

    fn fill_time_history_from_window(&mut self) {
        let Some(res) = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|b| b.time_history.clone())
        else {
            #[cfg(feature = "gui")]
            {
                self.ui.scoped.time_history_data =
                    crate::time_history_view::TimeHistoryData::default();
            }
            return;
        };
        self.fill_time_history_data(&res);
    }

    /// 保存済みの表示ケース ID から、時刻歴グラフと質点系窓口を復元する。
    pub(crate) fn hydrate_saved_vibration_views(
        &mut self,
        view_vibration_case: Option<VibrationCaseId>,
        view_lumped_vibration_case: Option<LumpedVibrationCaseId>,
    ) {
        match view_vibration_case {
            Some(id) => {
                if let Some(res) = self
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|b| b.time_history_for(id))
                    .cloned()
                {
                    self.set_spatial_time_history_view(id, &res);
                } else {
                    self.core.scoped.view_vibration_case = None;
                    self.fill_time_history_from_window();
                }
            }
            None => self.fill_time_history_from_window(),
        }

        match view_lumped_vibration_case {
            Some(id) => {
                if let Some(res) = self
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|b| b.lumped_result_for(id))
                    .cloned()
                {
                    self.set_lumped_mass_view(Some(id), &res);
                } else if let Some(res) = self
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|b| b.lumped.clone())
                {
                    self.set_lumped_mass_view(None, &res);
                } else {
                    self.core.scoped.view_lumped_vibration_case = None;
                    self.core.scoped.stick_response = None;
                }
            }
            None => {
                if let Some(res) = self
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|b| b.lumped.clone())
                {
                    self.set_lumped_mass_view(None, &res);
                }
            }
        }
    }

    /// 結果スロットが無い振動ケースをモデルから除く。
    ///
    /// 解析結果を同梱せずに保存した `.ovika` を開いたとき、ナビに空ケースが
    /// 残らないようにする。未実行の空ケースは作らない、という規約に合わせる。
    pub(crate) fn prune_orphan_vibration_cases(&mut self) {
        let spatial: std::collections::HashSet<_> = self
            .core
            .scoped
            .results
            .as_ref()
            .map(|b| b.time_histories.iter().map(|(id, _)| *id).collect())
            .unwrap_or_default();
        let lumped: std::collections::HashSet<_> = self
            .core
            .scoped
            .results
            .as_ref()
            .map(|b| b.lumped_results.iter().map(|(id, _)| *id).collect())
            .unwrap_or_default();
        self.core
            .model
            .vibration_cases
            .retain(|c| spatial.contains(&c.id));
        self.core
            .model
            .lumped_vibration_cases
            .retain(|c| lumped.contains(&c.id));
    }

    /// 質点系結果の表示窓口を切り替える。
    pub(crate) fn set_lumped_mass_view(
        &mut self,
        case_id: Option<LumpedVibrationCaseId>,
        result: &sepika_solver::dynamic::lumped_mass::LumpedMassResult,
    ) {
        self.core.scoped.view_lumped_vibration_case = case_id;
        self.core.scoped.stick_response = result.response.clone();
        if let Some(bundle) = &mut self.core.scoped.results {
            bundle.lumped = Some(result.clone());
        }
    }

    /// 立体振動ケースを選択（ナビ「振動荷重ケース」用）。
    #[cfg(feature = "gui")]
    pub(crate) fn focus_spatial_vibration_case(&mut self, id: VibrationCaseId) {
        self.ui.scoped.nav.focus_vibration_case = Some(id);
        self.ui.scoped.nav.focus_lumped_vibration_case = None;
        self.ui.scoped.nav.focus_load_case = None;
        if let Some(res) = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|b| b.time_history_for(id))
            .cloned()
        {
            self.open_result_workspace(ResultsView::TimeHistory);
            self.set_spatial_time_history_view(id, &res);
            self.bind_spatial_case_to_active_view(id);
            #[cfg(feature = "gui")]
            {
                self.ui.view.results_view = ResultsView::TimeHistory;
                self.active_viewport_mut().view_mode = crate::viewer::ViewMode::TimeHistory;
            }
        }
    }

    /// 質点系振動ケースを選択（ナビ「振動荷重ケース」用）。
    #[cfg(feature = "gui")]
    pub(crate) fn focus_lumped_vibration_case(&mut self, id: LumpedVibrationCaseId) {
        self.ui.scoped.nav.focus_lumped_vibration_case = Some(id);
        self.ui.scoped.nav.focus_vibration_case = None;
        self.ui.scoped.nav.focus_load_case = None;
        if let Some(res) = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|b| b.lumped_result_for(id))
            .cloned()
        {
            self.open_result_workspace(ResultsView::LumpedMass);
            self.set_lumped_mass_view(Some(id), &res);
            self.bind_lumped_case_to_active_view(id);
            self.active_viewport_mut().view_mode = crate::viewer::ViewMode::LumpedTimeHistory;
            #[cfg(feature = "gui")]
            {
                self.ui.view.results_view = ResultsView::LumpedMass;
            }
        }
    }
}

/// 除外保存のために一時的に取り出した時刻歴の詳細記録。
pub(crate) struct TakenThRecordings {
    window: Option<sepika_solver::dynamic::timehistory::ThRecording>,
    slots: Vec<(usize, sepika_solver::dynamic::timehistory::ThRecording)>,
}

impl ResultsBundle {
    /// 立体時刻歴結果をケース ID で upsert する。
    pub fn upsert_time_history(
        &mut self,
        id: VibrationCaseId,
        result: sepika_solver::dynamic::timehistory::ResponseResult,
    ) {
        if let Some(pos) = self.time_histories.iter().position(|(i, _)| *i == id) {
            self.time_histories[pos].1 = result;
        } else {
            self.time_histories.push((id, result));
        }
    }

    pub fn time_history_for(
        &self,
        id: VibrationCaseId,
    ) -> Option<&sepika_solver::dynamic::timehistory::ResponseResult> {
        self.time_histories
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, r)| r)
    }

    /// 質点系結果をケース ID で upsert する。
    pub fn upsert_lumped_result(
        &mut self,
        id: LumpedVibrationCaseId,
        result: sepika_solver::dynamic::lumped_mass::LumpedMassResult,
    ) {
        if let Some(pos) = self.lumped_results.iter().position(|(i, _)| *i == id) {
            self.lumped_results[pos].1 = result;
        } else {
            self.lumped_results.push((id, result));
        }
    }

    pub fn lumped_result_for(
        &self,
        id: LumpedVibrationCaseId,
    ) -> Option<&sepika_solver::dynamic::lumped_mass::LumpedMassResult> {
        self.lumped_results
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, r)| r)
    }

    /// 表示窓口またはケース別スロットに時刻歴の詳細記録があるか。
    pub(crate) fn has_th_recording(&self) -> bool {
        self.time_history
            .as_ref()
            .is_some_and(|th| th.recording.is_some())
            || self
                .time_histories
                .iter()
                .any(|(_, th)| th.recording.is_some())
    }

    /// 詳細記録を一時的に外す（除外保存用。直後に [`Self::restore_th_recordings`]）。
    pub(crate) fn take_th_recordings(&mut self) -> TakenThRecordings {
        let window = self
            .time_history
            .as_mut()
            .and_then(|th| th.recording.take());
        let slots = self
            .time_histories
            .iter_mut()
            .enumerate()
            .filter_map(|(i, (_, th))| th.recording.take().map(|r| (i, r)))
            .collect();
        TakenThRecordings { window, slots }
    }

    pub(crate) fn restore_th_recordings(&mut self, taken: TakenThRecordings) {
        if let Some(rec) = taken.window {
            if let Some(th) = self.time_history.as_mut() {
                th.recording = Some(rec);
            }
        }
        for (i, rec) in taken.slots {
            if let Some((_, th)) = self.time_histories.get_mut(i) {
                th.recording = Some(rec);
            }
        }
    }
}
