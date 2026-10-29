use sepika_core::ids::{LumpedVibrationCaseId, VibrationCaseId};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) enum SpatialResultSource {
    Case(VibrationCaseId, String),
    Legacy(Arc<sepika_solver::dynamic::timehistory::ResponseResult>),
}

#[derive(Clone)]
pub(crate) enum LumpedResultSource {
    Case(LumpedVibrationCaseId, String),
    Legacy(Arc<sepika_solver::dynamic::lumped_mass::LumpedMassResult>),
}

/// 一つの図の視点・表示条件・再生位置を保持する。モデルと画面選択は共有する。
#[derive(Clone)]
pub struct ViewportState {
    pub view_mode: crate::viewer::ViewMode,
    pub force_components: crate::viewer::ForceComponents,
    pub diagram_values: bool,
    pub cmq_component: crate::viewer::CmqComponent,
    pub cmq_axes: crate::viewer::CmqAxes,
    pub check_ratio_filter: crate::viewer::CheckRatioFilter,
    pub check_ratio_markers: bool,
    pub check_ratio_label_all: bool,
    pub diagram_contour: bool,
    pub contour_colormap: crate::theme::ColorMap,
    pub overlay_deform: bool,
    pub show_floor_secondary: bool,
    pub camera: crate::viewer::CameraState,
    pub show_sections: bool,
    pub deform_scale_factor: f32,
    pub(crate) fixed_deform_scale: Option<f64>,
    pub show_diaphragm_master: bool,
    pub show_supports: bool,
    pub show_space_grid: bool,
    pub modeling_analysis: crate::viewer::ModelingAnalysis,
    pub show_beam_interpolation: bool,
    pub th_speed: f32,
    pub lumped_show_frame: bool,
    pub pushover_dir: Option<sepika_solver::statics::analysis::SeismicDir>,
    pub hinge_step: Option<usize>,
    pub view_mode_idx: usize,
    pub frame_target: Option<sepika_core::frame::FrameTarget>,
    pub th_frame: usize,
    pub th_playing: bool,
    pub th_play_time: f64,
    pub th_scale_cache: Option<crate::viewer::TimeHistoryScaleCache>,
    pub(crate) spatial_source: Option<SpatialResultSource>,
    pub(crate) lumped_source: Option<LumpedResultSource>,
    pub(crate) static_unavailable: bool,
    pub(crate) static_key: Option<crate::app::StaticKey>,
    pub(crate) static_case_locked: bool,
    pub(crate) load_case: Option<sepika_core::ids::LoadCaseId>,
    pub(crate) pane_index: usize,
    pub(crate) allow_creation: bool,
    pub(crate) fit_scale: Option<f32>,
}
impl Default for ViewportState {
    fn default() -> Self {
        Self {
            view_mode: crate::viewer::ViewMode::default(),
            force_components: crate::viewer::ForceComponents::default(),
            diagram_values: false,
            cmq_component: crate::viewer::CmqComponent::default(),
            cmq_axes: crate::viewer::CmqAxes::default(),
            check_ratio_filter: crate::viewer::CheckRatioFilter::default(),
            check_ratio_markers: true,
            check_ratio_label_all: false,
            diagram_contour: false,
            contour_colormap: crate::theme::ColorMap::default(),
            overlay_deform: false,
            show_floor_secondary: true,
            camera: crate::viewer::CameraState::default(),
            show_sections: false,
            deform_scale_factor: 1.0,
            fixed_deform_scale: None,
            show_diaphragm_master: false,
            show_supports: true,
            show_space_grid: true,
            modeling_analysis: crate::viewer::ModelingAnalysis::default(),
            show_beam_interpolation: true,
            th_speed: 1.0,
            lumped_show_frame: true,
            pushover_dir: None,
            hinge_step: None,
            view_mode_idx: 0,
            frame_target: None,
            th_frame: 0,
            th_playing: false,
            th_play_time: 0.0,
            th_scale_cache: None,
            spatial_source: None,
            lumped_source: None,
            static_unavailable: false,
            static_key: None,
            static_case_locked: false,
            load_case: None,
            pane_index: 0,
            allow_creation: true,
            fit_scale: None,
        }
    }
}

impl ViewportState {
    pub(crate) fn prepare_coordinate_edit(&mut self, app: &crate::app::App) {
        if !self.allow_creation {
            return;
        }
        let scoped = &app.ui.scoped;
        if scoped.beam_draw_mode
            || scoped.wall_draw_mode
            || scoped.slab_draw_mode
            || scoped.beam_place_mode
            || scoped.post_place_mode
            || app.load_pick_active()
        {
            if matches!(
                self.view_mode,
                super::ViewMode::Deformed
                    | super::ViewMode::Mode
                    | super::ViewMode::TimeHistory
                    | super::ViewMode::LumpedMode
                    | super::ViewMode::LumpedTimeHistory
            ) {
                self.view_mode = super::ViewMode::Shape;
            }
            self.overlay_deform = false;
        }
    }
}

impl ViewportState {
    pub(crate) fn reset_model_state(&mut self) {
        self.frame_target = None;
        self.spatial_source = None;
        self.lumped_source = None;
        self.pushover_dir = None;
        self.hinge_step = None;
        self.view_mode_idx = 0;
        self.th_frame = 0;
        self.th_playing = false;
        self.th_play_time = 0.0;
        self.th_scale_cache = None;
        self.static_key = None;
        self.static_case_locked = false;
        self.static_unavailable = false;
        self.load_case = None;
        self.fit_scale = None;
        self.fixed_deform_scale = None;
    }

    pub(crate) fn current_static<'a>(
        &self,
        app: &'a crate::app::App,
    ) -> Option<&'a sepika_solver::statics::linear::StaticOnce> {
        if self.static_unavailable {
            return None;
        }
        let Some(key) = self.static_key else {
            return app.current_static();
        };
        let bundle = app.core.scoped.results.as_ref()?;
        match key {
            crate::app::StaticKey::Case(key) => bundle
                .statics
                .iter()
                .find(|(entry, _)| *entry == key)
                .map(|(_, result)| result),
            crate::app::StaticKey::Combo(index) => {
                bundle.combos.get(index).map(|(_, result)| result)
            }
        }
    }

    pub(crate) fn member_forces<'a>(
        &self,
        app: &'a crate::app::App,
    ) -> &'a [(
        sepika_core::ids::ElemId,
        sepika_element::frame::beam::MemberForces,
    )] {
        if let Some(result) = self.current_static(app) {
            return &result.member_forces;
        }
        if self.static_unavailable || self.static_key.is_some() {
            return &[];
        }
        app.core
            .scoped
            .results
            .as_ref()
            .map_or(&[], |bundle| &bundle.member_forces)
    }

    pub(crate) fn cmq_load_case<'a>(
        &self,
        app: &'a crate::app::App,
    ) -> Option<&'a sepika_core::model::LoadCase> {
        if let Some(id) = self.load_case {
            app.core.model.load_cases.iter().find(|case| case.id == id)
        } else {
            app.cmq_display_load_case()
        }
    }
}

impl ViewportState {
    pub(crate) fn capture_result_sources(&mut self, app: &crate::app::App) {
        if self.pushover_dir.is_none() && app.displayed_pushover().is_some() {
            self.pushover_dir = Some(app.core.scoped.pushover_view_dir);
        }
        if self.spatial_source.is_none() {
            if let Some(id) = app.core.scoped.view_vibration_case {
                self.select_spatial_case(app, id);
            }
            if self.spatial_source.is_none() {
                self.spatial_source = app
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|bundle| bundle.time_history.clone())
                    .map(|result| SpatialResultSource::Legacy(Arc::new(result)));
            }
        }
        if self.lumped_source.is_none() {
            if let Some(id) = app.core.scoped.view_lumped_vibration_case {
                self.select_lumped_case(app, id);
            }
            if self.lumped_source.is_none() {
                self.lumped_source = app
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|bundle| bundle.lumped.clone())
                    .map(|result| LumpedResultSource::Legacy(Arc::new(result)));
            }
        }
    }

    pub(crate) fn select_spatial_case(&mut self, app: &crate::app::App, id: VibrationCaseId) {
        if let Some(case) = app
            .core
            .model
            .vibration_cases
            .iter()
            .find(|case| case.id == id)
        {
            self.spatial_source = Some(SpatialResultSource::Case(
                id,
                crate::app::vibration::vibration_identity(case),
            ));
            self.th_scale_cache = None;
        }
    }

    pub(crate) fn select_lumped_case(&mut self, app: &crate::app::App, id: LumpedVibrationCaseId) {
        if let Some(case) = app
            .core
            .model
            .lumped_vibration_cases
            .iter()
            .find(|case| case.id == id)
        {
            self.lumped_source = Some(LumpedResultSource::Case(
                id,
                crate::app::vibration::vibration_identity(case),
            ));
        }
    }

    pub(crate) fn spatial_result<'a>(
        &'a self,
        app: &'a crate::app::App,
    ) -> Option<&'a sepika_solver::dynamic::timehistory::ResponseResult> {
        match self.spatial_source.as_ref() {
            Some(SpatialResultSource::Case(id, identity)) => {
                let case = app
                    .core
                    .model
                    .vibration_cases
                    .iter()
                    .find(|case| case.id == *id)?;
                if crate::app::vibration::vibration_identity(case) != *identity {
                    return None;
                }
                app.core.scoped.results.as_ref()?.time_history_for(*id)
            }
            Some(SpatialResultSource::Legacy(result)) => Some(result),
            None => app.core.scoped.results.as_ref()?.time_history.as_ref(),
        }
    }

    pub(crate) fn lumped_result<'a>(
        &'a self,
        app: &'a crate::app::App,
    ) -> Option<&'a sepika_solver::dynamic::lumped_mass::LumpedMassResult> {
        match self.lumped_source.as_ref() {
            Some(LumpedResultSource::Case(id, identity)) => {
                let case = app
                    .core
                    .model
                    .lumped_vibration_cases
                    .iter()
                    .find(|case| case.id == *id)?;
                if crate::app::vibration::vibration_identity(case) != *identity {
                    return None;
                }
                app.core.scoped.results.as_ref()?.lumped_result_for(*id)
            }
            Some(LumpedResultSource::Legacy(result)) => Some(result),
            None => app.core.scoped.results.as_ref()?.lumped.as_ref(),
        }
    }
}

impl ViewportState {
    pub(crate) fn pushover_result<'a>(
        &self,
        app: &'a crate::app::App,
    ) -> Option<&'a sepika_solver::nonlinear::pushover::PushoverResult> {
        app.pushover_for(
            self.pushover_dir
                .unwrap_or(app.core.scoped.pushover_view_dir),
        )
    }
}

impl ViewportState {
    pub(crate) fn spatial_case_id(&self, app: &crate::app::App) -> Option<VibrationCaseId> {
        self.spatial_result(app)?;
        match self.spatial_source.as_ref() {
            Some(SpatialResultSource::Case(id, _)) => Some(*id),
            Some(SpatialResultSource::Legacy(_)) => None,
            None => app.core.scoped.view_vibration_case,
        }
    }
}
