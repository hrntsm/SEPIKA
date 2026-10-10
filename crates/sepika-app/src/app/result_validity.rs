use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ResultInputKey {
    Static(StaticCaseKey),
    Combo(String),
    Pushover(SeismicDir),
    Modal,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HoldingEvaluationInput {
    pub point: sepika_solver::nonlinear::pushover::story_response::EvaluationPoint,
    pub conditions: AnalysisSettings,
    pub run: sepika_solver::nonlinear::pushover::PushoverResult,
}

#[derive(Clone, Debug)]
pub struct HoldingCapacitySource {
    pub direction: SeismicDir,
    /// 下層から順に、保有耐力比較で明示採用した確定ステップ。
    pub qu_steps: Vec<Option<u32>>,
    /// Ds 判定の部材応答を採用した確定ステップ。
    pub response_step: Option<u32>,
    pub ds_point: sepika_solver::nonlinear::pushover::story_response::EvaluationPoint,
    pub capacity_point: sepika_solver::nonlinear::pushover::story_response::EvaluationPoint,
    pub ds_forces: Vec<sepika_solver::nonlinear::pushover::story_response::StoryForceEvaluation>,
    pub capacity_forces:
        Vec<sepika_solver::nonlinear::pushover::story_response::StoryForceEvaluation>,
    pub termination: sepika_solver::nonlinear::pushover::PushoverTermination,
    pub capacity_termination: sepika_solver::nonlinear::pushover::PushoverTermination,
    pub ds_mechanism: sepika_solver::nonlinear::pushover::MechanismType,
    pub ds_conditions: AnalysisSettings,
    pub capacity_conditions: AnalysisSettings,
}

impl ResultsBundle {
    pub(super) fn record_input(&mut self, key: ResultInputKey, input: Vec<u8>) {
        self.input_records.retain(|(k, _)| *k != key);
        self.input_records.push((key, input));
    }
}

impl App {
    /// 現在runの明示評価点を目的別に保存する。目的別runは次の解析で上書きしない。
    pub fn adopt_holding_evaluation(
        &mut self,
        purpose: sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose,
    ) -> Result<(), String> {
        use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
        let dir = self.core.scoped.pushover_view_dir;
        self.require_result_input(ResultInputKey::Pushover(dir))?;
        let bundle = self
            .core
            .scoped
            .results
            .as_mut()
            .ok_or("増分解析結果がありません")?;
        let po = bundle
            .pushover_for_dir(dir)
            .ok_or("対象方向の増分解析結果がありません")?;
        let point = if purpose == EvaluationPurpose::Ds {
            &po.ds_evaluation
        } else {
            &po.capacity_evaluation
        }
        .as_ref()
        .ok_or("目的別評価点が未指定です")?
        .clone();
        po.confirmed_response(&point, purpose)?;
        let mut run = po.clone();
        if let Some(history) = &mut run.confirmed_history {
            history.retain(|r| r.step == point.step);
        }
        if let Some(history) = &mut run.wall_history {
            history.retain(|r| r.step == point.step);
        }
        if purpose == EvaluationPurpose::Ds {
            run.capacity_evaluation = None;
        } else {
            run.ds_evaluation = None;
        }
        let input = HoldingEvaluationInput {
            point,
            conditions: self.core.analysis_cfg,
            run,
        };
        bundle
            .holding_evaluations
            .retain(|e| e.point.direction != dir || e.point.purpose != purpose);
        bundle.holding_evaluations.push(input);
        Ok(())
    }

    pub(crate) fn holding_evaluation_is_current(&self, input: &HoldingEvaluationInput) -> bool {
        input.point.input_generation
            == self.result_input_with_settings(
                &ResultInputKey::Pushover(input.point.direction),
                input.conditions,
            )
    }

    fn calculation_model(&self) -> sepika_core::model::Model {
        let mut model = self.core.model.clone();
        model.axes.clear();
        model.vibration_cases.clear();
        model.lumped_vibration_cases.clear();
        model
    }

    pub(super) fn result_input(&self, key: &ResultInputKey) -> Vec<u8> {
        self.result_input_with_settings(key, self.core.analysis_cfg)
    }

    pub(super) fn result_input_with_settings(
        &self,
        key: &ResultInputKey,
        cfg: AnalysisSettings,
    ) -> Vec<u8> {
        let mut relevant = AnalysisSettings {
            ai_mode: cfg.ai_mode,
            z: cfg.z,
            soil: cfg.soil,
            c0: cfg.c0,
            mass_method: cfg.mass_method,
            heavy_snow_zone: cfg.heavy_snow_zone,
            snow_delta1: cfg.snow_delta1,
            snow_delta3: cfg.snow_delta3,
            threads: cfg.threads,
            ..Default::default()
        };
        if let ResultInputKey::Pushover(dir) = key {
            relevant.push_dir = *dir;
            relevant.push_steps = cfg.push_steps;
            relevant.push_max_disp = cfg.push_max_disp;
            relevant.push_use_max_disp = cfg.push_use_max_disp;
            relevant.push_use_drift_angle = cfg.push_use_drift_angle;
            relevant.push_drift_denom = cfg.push_drift_denom;
            relevant.push_control = cfg.push_control;
            relevant.push_apply_long_term = cfg.push_apply_long_term;
            relevant.ductility_method = cfg.ductility_method;
        }
        let mut model = self.calculation_model();
        if matches!(key, ResultInputKey::Pushover(_)) {
            let generated = sepika_job::auto_loads::compute_seismic_auto_load_cases(
                &model,
                &cfg,
                self.result_generation_period(key, cfg),
            );
            sepika_job::auto_loads::apply_auto_load_cases(&mut model, &generated.cases);
        }
        if matches!(key, ResultInputKey::Modal) {
            // 固有周期で再生成する EX/EY を含めると、Ai 更新だけで固有値自身が
            // 陳腐化する。固有値の剛性・質量行列はこれらの水平荷重に依存しない。
            model
                .load_cases
                .retain(|case| !is_standard_seismic_case(case));
        }
        bincode::serialize(&(model, relevant, self.result_generation_period(key, cfg)))
            .expect("計算入力の直列化")
    }

    fn result_generation_period(&self, key: &ResultInputKey, cfg: AnalysisSettings) -> Option<f64> {
        if !matches!(cfg.ai_mode, AiMode::SemiPrecise) {
            return None;
        }
        let depends_on_seismic = match key {
            ResultInputKey::Pushover(_) | ResultInputKey::Static(StaticCaseKey::Seismic(_)) => true,
            ResultInputKey::Static(StaticCaseKey::User(id)) => self
                .core
                .model
                .load_cases
                .iter()
                .any(|case| case.id == *id && is_standard_seismic_case(case)),
            ResultInputKey::Combo(name) => self
                .core
                .model
                .combinations
                .iter()
                .find(|combo| combo.name == *name)
                .is_some_and(|combo| {
                    combo.terms.iter().any(|(id, _)| {
                        self.core
                            .model
                            .load_cases
                            .iter()
                            .any(|case| case.id == *id && is_standard_seismic_case(case))
                    })
                }),
            ResultInputKey::Modal => false,
        };
        depends_on_seismic
            .then(|| {
                self.core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|r| r.modal.as_ref())
                    .and_then(|m| m.period.first().copied())
            })
            .flatten()
    }

    pub(super) fn job_input(&self) -> Vec<u8> {
        let period = matches!(self.core.analysis_cfg.ai_mode, AiMode::SemiPrecise)
            .then(|| self.design_seismic_period().ok())
            .flatten();
        bincode::serialize(&(self.calculation_model(), self.core.analysis_cfg, period))
            .expect("ジョブ入力の直列化")
    }

    pub(super) fn require_result_input(&self, key: ResultInputKey) -> Result<(), String> {
        let label = match &key {
            ResultInputKey::Static(StaticCaseKey::Seismic(dir)) => format!("地震静的 {dir:?}"),
            ResultInputKey::Static(StaticCaseKey::User(id)) => format!("静的荷重ケース {id:?}"),
            ResultInputKey::Combo(name) => format!("静的組合せ「{name}」"),
            ResultInputKey::Pushover(dir) => format!("増分解析 {dir:?}"),
            ResultInputKey::Modal => "固有値解析".into(),
        };
        let input = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.input_records.iter().find(|(k, _)| *k == key))
            .map(|(_, input)| input);
        match input {
            Some(input) if *input == self.result_input(&key) => Ok(()),
            Some(_) => Err(format!("保有水平耐力の判定は無効です。{label}の生成入力が現在のモデル・計算設定と一致しません。{label}を再実行してください。")),
            None => Err(format!("保有水平耐力の判定は無効です。{label}の入力識別情報がありません。{label}を再実行してください。")),
        }
    }
}

fn is_standard_seismic_case(case: &sepika_core::model::LoadCase) -> bool {
    use sepika_core::model::{LoadCaseKind, EX_CASE_NAME, EY_CASE_NAME};
    case.kind == LoadCaseKind::Seismic && matches!(case.name.as_str(), EX_CASE_NAME | EY_CASE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::{dof::Dof6Mask, ids::StoryId};

    fn ready() -> App {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.core.analysis_cfg.threads = 1;
        app.core.analysis_cfg.push_steps = 3;
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_holding_capacity().is_ok());
        app
    }

    #[test]
    fn 採用点は層最大値へ暗黙変換せず同じ確定状態を記録する() {
        let mut app = ready();
        let source = app.core.scoped.holding_capacity_source.as_ref().unwrap();
        let step = source.capacity_point.step;
        assert!(source.qu_steps.iter().all(|s| *s == Some(step)));
        assert_eq!(source.response_step, Some(source.ds_point.step));
        assert!(source
            .ds_forces
            .iter()
            .all(|f| f.residual_n.abs() <= f.tolerance_n));
        app.core.model.materials[0].fy = None;
        assert!(app.compute_holding_capacity().is_err());
        assert!(app.core.scoped.holding_capacity_source.is_none());
    }

    #[test]
    fn 材料編集後に静的解析だけ再実行しても旧増分結果は使えない() {
        let mut app = ready();
        app.core.model.materials[0].fy = None;
        app.core.scoped.staleness.mark_edited();
        app.run_seismic(SeismicDir::X);
        assert!(!app.core.scoped.staleness.results_stale);
        let reason = app.compute_holding_capacity().err().expect("拒否される");
        assert!(
            reason.contains("増分解析 X") && reason.contains("一致しません"),
            "{reason}"
        );
        assert!(app.core.scoped.ds_beta_u_by_story.is_empty());
        assert!(app.core.scoped.ds_rank_fallback_stories.is_empty());
        app.core.model.materials[0].fy = Some(235.0);
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 各計算入力の編集後は保有耐力判定が無効になる() {
        let base = ready();
        let edits: &[fn(&mut App)] = &[
            |a| a.core.model.materials[0].fc = Some(99.0),
            |a| a.core.model.materials[0].fy = None,
            |a| a.core.model.sections[0].area *= 1.1,
            |a| a.core.model.sections[0].shape = None,
            |a| {
                a.core.model.wall_attrs.push(sepika_core::model::WallAttr {
                    elem: ElemId(0),
                    opening_area: 1.0,
                    opening_weight: 0.0,
                    slit: Default::default(),
                    openings: Vec::new(),
                    finish_intensity: 0.0,
                })
            },
            |a| a.core.model.load_cases[0].member.clear(),
            |a| {
                a.core
                    .model
                    .combinations
                    .push(sepika_core::model::LoadCombination {
                        name: "追加".into(),
                        terms: vec![(LoadCaseId(0), 1.0)],
                    })
            },
            |a| a.core.model.stories[0].seismic_weight = Some(1.0),
            |a| a.core.analysis_cfg.z = 0.8,
            |a| a.core.analysis_cfg.push_steps += 1,
            |a| {
                a.core.model.stress_cfg.rigid_zone_consider_walls =
                    !a.core.model.stress_cfg.rigid_zone_consider_walls
            },
        ];
        for (index, edit) in edits.iter().enumerate() {
            let mut app = App::default();
            app.core.model = base.core.model.clone();
            app.core.analysis_cfg = base.core.analysis_cfg;
            app.core.scoped.results = base.core.scoped.results.clone();
            edit(&mut app);
            assert!(app.compute_holding_capacity().is_err(), "編集 {index}");
        }
    }

    #[test]
    fn 表示ステップと表示選択と表示専用通り芯の変更は利用を維持する() {
        let mut app = ready();
        let initial = app.compute_holding_capacity().unwrap().0.stories[0].qun;
        #[cfg(feature = "gui")]
        {
            app.ui.scoped.hinge_step = Some(1);
            app.ui.view.camera.yaw += 0.1;
        }
        app.ui.scoped.nav.focus_result = Some(StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
        app.core.model.axes.push(sepika_core::model::AxisGroup {
            name: "表示".into(),
            kind: sepika_core::model::AxisGroupKind::Other,
            axes: Vec::new(),
        });
        app.core.scoped.staleness.mark_non_calc_edited();
        app.core.analysis_cfg.th_record_every = 17;
        app.core.analysis_cfg.seismic_dir = SeismicDir::Y;
        assert_eq!(
            app.compute_holding_capacity().unwrap().0.stories[0].qun,
            initial
        );
    }

    #[test]
    fn 方向不足と他方向の旧結果は代用しない() {
        let mut app = ready();
        app.set_pushover_view_dir(SeismicDir::Y);
        let reason = app.compute_holding_capacity().err().expect("拒否される");
        assert!(reason.contains("増分解析 Y"), "{reason}");
        app.set_pushover_view_dir(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.core.analysis_cfg.push_dir = SeismicDir::Y;
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_holding_capacity().is_ok());
        app.core.model.materials[0].fy = Some(345.0);
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.core.analysis_cfg.push_dir = SeismicDir::X;
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_holding_capacity().is_ok());
        app.set_pushover_view_dir(SeismicDir::Y);
        let reason = app.compute_holding_capacity().err().expect("拒否される");
        assert!(
            reason.contains("増分解析 Y") && reason.contains("一致しません"),
            "{reason}"
        );
    }

    #[test]
    fn 略算_qudは固有値に依存せず偏心精算の静的結果は入力一致を要求する() {
        let mut app = ready();
        app.run_seismic(SeismicDir::Y);
        app.run_eigen(1);
        assert!(app.compute_holding_capacity().is_ok());
        app.core.model.materials[0].fy = Some(345.0);
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app
            .compute_holding_capacity()
            .err()
            .expect("拒否される")
            .contains("地震静的 Y"));
        app.run_seismic(SeismicDir::Y);
        assert!(app.compute_holding_capacity().is_ok());
        app.run_eigen(1);
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 精算周期は固有値を再解析しても標準地震結果として採用しない() {
        let mut app = ready();
        app.run_eigen(1);
        app.core.analysis_cfg.ai_mode = AiMode::SemiPrecise;
        app.run_static_all();
        assert!(app.compute_holding_capacity().is_err());
        let old_period = app.design_seismic_period().unwrap();
        app.core.model.materials[0].young *= 0.5;
        app.run_eigen(1);
        assert!(app.design_seismic_period().unwrap() > old_period);
        app.run_static_all();
        assert!(app.compute_holding_capacity().is_err());
        assert!(app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .seismic(SeismicDir::X)
            .is_none());
        assert!(app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .seismic(SeismicDir::Y)
            .is_none());
    }

    #[test]
    fn 略算周期での固有値更新は地震結果の入力識別を変えない() {
        let mut app = ready();
        app.run_eigen(1);
        let push_input = app.result_input(&ResultInputKey::Pushover(SeismicDir::X));
        let static_input = app.result_input(&ResultInputKey::Static(StaticCaseKey::Seismic(
            SeismicDir::X,
        )));
        app.run_eigen(3);
        assert_eq!(
            app.result_input(&ResultInputKey::Pushover(SeismicDir::X)),
            push_input
        );
        assert_eq!(
            app.result_input(&ResultInputKey::Static(StaticCaseKey::Seismic(
                SeismicDir::X
            ))),
            static_input
        );
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 時刻歴だけ再実行しても旧増分結果を最新にしない() {
        let mut app = ready();
        app.core.model.materials[0].fy = Some(345.0);
        app.run_time_history(sepika_solver::dynamic::timehistory::GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0; 3],
            accel_y: None,
            accel_theta: None,
        });
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert!(!app.core.scoped.staleness.results_stale);
        assert!(app
            .compute_holding_capacity()
            .err()
            .expect("拒否される")
            .contains("増分解析 X"));
    }

    #[test]
    fn 長期軸力の精算に使う組合せも再解析が必要になる() {
        let mut app = ready();
        app.core
            .model
            .combinations
            .push(sepika_core::model::LoadCombination {
                name: "長期".into(),
                terms: vec![(LoadCaseId(0), 1.0)],
            });
        app.run_static_all();
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_holding_capacity().is_ok());
        app.core.model.materials[0].fy = Some(345.0);
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app
            .compute_holding_capacity()
            .err()
            .expect("拒否される")
            .contains("静的組合せ「長期」"));
        app.run_static_all();
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 剛域反映で変化した入力も判定前に検査する() {
        let mut app = ready();
        app.core.model.elements[2].rigid_zone.face_i = None;
        let input = app.result_input(&ResultInputKey::Pushover(SeismicDir::X));
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .record_input(ResultInputKey::Pushover(SeismicDir::X), input);
        assert!(app
            .compute_holding_capacity()
            .err()
            .expect("拒否される")
            .contains("一致しません"));
        assert!(app.core.model.elements[2].rigid_zone.face_i.is_some());
    }

    #[test]
    fn 入力識別欠落の復元結果は判定に使えない() {
        let mut app = ready();
        let mut bundle = app.core.scoped.results.take().unwrap();
        bundle.input_records.clear();
        let bytes = rmp_serde::to_vec(&bundle).unwrap();
        app.core.scoped.results = Some(rmp_serde::from_slice(&bytes).unwrap());
        let reason = app.compute_holding_capacity().err().expect("拒否される");
        assert!(reason.contains("入力識別情報がありません"));
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        let bytes = rmp_serde::to_vec(app.core.scoped.results.as_ref().unwrap()).unwrap();
        app.core.scoped.results = Some(rmp_serde::from_slice(&bytes).unwrap());
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 計算中編集の旧ジョブ結果は破棄される() {
        let mut app = ready();
        let old_qu = app.pushover_for(SeismicDir::X).unwrap().qu;
        app.start_pushover_job();
        assert!(app.core.scoped.job.is_some());
        app.core.model.materials[0].fy = Some(345.0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !app.poll_job() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("旧入力の解析結果を破棄"));
        assert_eq!(app.pushover_for(SeismicDir::X).unwrap().qu, old_qu);
        assert!(app.compute_holding_capacity().is_err());
    }
    #[test]
    fn 略算_qudは固有値の参考周期があっても独立略算値を採用する() {
        let mut app = ready();
        app.core.model.stories[1].seismic_weight = Some(100_000.0);
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        let baseline = app.compute_holding_capacity().unwrap().0.stories[0].qud;
        assert!((baseline - 100_000.0).abs() < 1e-8);
        app.run_eigen(1);
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .modal
            .as_mut()
            .unwrap()
            .period[0] = 20.0;
        let with_modal = app.compute_holding_capacity().unwrap().0.stories[0].qud;
        assert_eq!(baseline, with_modal);
    }
    #[test]
    fn 必要耐力用地震力は未検証精算塔屋と指定_ci副剛床を拒否する() {
        let mut app = ready();
        app.core.analysis_cfg.ai_mode = AiMode::SemiPrecise;
        assert!(app
            .compute_holding_capacity()
            .err()
            .unwrap()
            .contains("未対応"));
        app.core.analysis_cfg.ai_mode = AiMode::Approx;
        app.core.model.stories[1].level_kind =
            sepika_core::model::StoryLevelKind::Penthouse { k: 1.0 };
        assert!(app
            .compute_holding_capacity()
            .err()
            .unwrap()
            .contains("未検証"));
        app.core.model.stories[1].level_kind = Default::default();
        app.core
            .model
            .constraints
            .push(sepika_core::model::Constraint::RigidDiaphragm {
                story: StoryId(1),
                master: NodeId(2),
                slaves: vec![],
                weight: Some(100.0),
                ci_override: Some(0.3),
            });
        let error = app.compute_holding_capacity().err().unwrap();
        assert!(
            error.contains("指定 Ci") && error.contains("未対応"),
            "{error}"
        );
    }

    #[test]
    fn 地下_qudは上部_c0一のせん断力と地下震度重量を加算する() {
        let mut app = ready();
        let basement_node = NodeId(app.core.model.nodes.len() as u32);
        let spring_id = ElemId(app.core.model.elements.len() as u32);
        let mut basement = app.core.model.stories[1].clone();
        basement.id = StoryId(1);
        basement.name = "B1".into();
        basement.elevation = 1000.0;
        basement.node_ids = vec![basement_node];
        basement.seismic_weight = Some(100_000.0);
        basement.level_kind = sepika_core::model::StoryLevelKind::Basement { depth_mm: 0.0 };
        app.core.model.stories[1].id = StoryId(2);
        app.core.model.stories[1].seismic_weight = Some(100_000.0);
        for node in &mut app.core.model.nodes {
            if node.story == Some(StoryId(1)) {
                node.story = Some(StoryId(2));
            }
        }
        for constraint in &mut app.core.model.constraints {
            if let sepika_core::model::Constraint::RigidDiaphragm { story, .. } = constraint {
                if *story == StoryId(1) {
                    *story = StoryId(2);
                }
            }
        }
        app.core.model.stories.insert(1, basement);
        let mut node = app.core.model.nodes[0].clone();
        node.id = basement_node;
        node.story = Some(StoryId(1));
        node.restraint = Dof6Mask(0b111110);
        app.core.model.nodes.push(node);
        let mut spring = app.core.model.elements[0].clone();
        spring.id = spring_id;
        spring.kind = sepika_core::model::ElementKind::NodalSpring;
        spring.nodes = [NodeId(0), basement_node].into_iter().collect();
        spring.section = None;
        spring.spring = Some([1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        app.core.model.elements.push(spring);
        app.run_seismic(SeismicDir::X);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let result = app.compute_holding_capacity().unwrap().0;
        assert!((result.stories[0].qud - 110_000.0).abs() < 1e-8);
        assert!((result.stories[1].qud - 100_000.0).abs() < 1e-8);
    }
}

#[cfg(test)]
mod purpose_tests {
    use super::*;
    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    fn ready() -> App {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.core.analysis_cfg.threads = 1;
        app.core.analysis_cfg.push_steps = 3;
        app.core.analysis_cfg.push_use_max_disp = true;
        app.core.analysis_cfg.push_max_disp = 1.0;
        app.core.analysis_cfg.push_use_drift_angle = false;
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app
    }
    #[test]
    fn 拒否した層力の数値と採用識別を共通入口csvとguiへ保持する() {
        use sepika_solver::nonlinear::pushover::story_response::{CutForce, ForceGroup};
        for (wall, brace, frame, external, reason, beta, residual) in [
            (90.0, 0.0, 60.0, 120.0, "釣合い残差", "0.6", "30"),
            (
                160.0,
                0.0,
                -10.0,
                150.0,
                "範囲外",
                "1.0666666666666667",
                "0",
            ),
            (0.0, 0.0, 0.0, 0.0, "分母", "未定義", "0"),
            (
                90.0,
                -10.0,
                70.0,
                150.0,
                "負の負担寄与",
                "0.5333333333333333",
                "0",
            ),
        ] {
            let mut app = ready();
            super::super::tests::select_holding_points(&mut app);
            let po = app
                .core
                .scoped
                .results
                .as_mut()
                .unwrap()
                .pushover_x
                .as_mut()
                .unwrap();
            let point = po.ds_evaluation.as_ref().unwrap().clone();
            let cut = &mut po
                .confirmed_history
                .as_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r.step == point.step)
                .unwrap()
                .cuts[0];
            cut.forces = [
                (0, ForceGroup::Wall, wall),
                (1, ForceGroup::Brace, brace),
                (2, ForceGroup::Frame, frame),
            ]
            .into_iter()
            .map(|(id, group, force_n)| CutForce {
                elem: sepika_core::ids::ElemId(id),
                group,
                force_n,
            })
            .collect();
            cut.external_n = external;
            cut.reference_n = 100.0;
            cut.support_n = 2.0;
            cut.tolerance_n = 1e-6;
            let expected = [
                reason.to_string(),
                "purpose=Ds".into(),
                format!("run={}", point.run_id),
                "direction=X".into(),
                format!("step={}", point.step),
                format!("Wall={wall} N"),
                format!("Brace={brace} N"),
                format!("Frame={frame} N"),
                format!("上層外力={external} N"),
                "基準外力=100 N".into(),
                "支持ばね内力=2 N".into(),
                format!("残差={residual} N"),
                "許容差=0.000001 N".into(),
                format!("βu={beta} [-]"),
                format!("Qu={} N", wall + brace + frame),
            ];
            let error = app.compute_holding_capacity().err().unwrap();
            let csv = crate::summary::build_report_csv(&app);
            for fragment in &expected {
                assert!(error.contains(fragment), "{fragment}: {error}");
                assert!(csv.contains(fragment), "{fragment}: {csv}");
            }
            #[cfg(feature = "gui")]
            {
                let ctx = egui::Context::default();
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(4000.0, 10000.0),
                        )),
                        ..Default::default()
                    },
                    |ui| crate::design_view::design_table(ui, &mut app),
                );
                let text = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                for fragment in &expected {
                    assert!(text.contains(fragment), "{fragment}: {text}");
                }
            }
        }
    }
    #[test]
    fn 共通入口は目的別点の未指定を法定既定点で埋めない() {
        let mut app = ready();
        assert!(app
            .compute_holding_capacity()
            .err()
            .unwrap()
            .contains("評価点を明示"));
    }
    #[test]
    fn 手動ランクは未使用の自動耐力入力を検査せず自動ランクは不正入力を拒否する() {
        let mut app = ready();
        super::super::tests::select_holding_points(&mut app);
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .pushover_x
            .as_mut()
            .unwrap()
            .ds_evaluation
            .as_mut()
            .unwrap()
            .member_capacities_n = vec![
            (sepika_core::ids::ElemId(0), 0.0),
            (sepika_core::ids::ElemId(0), f64::NAN),
        ];
        app.core.design_rank_auto = false;
        assert!(app.compute_holding_capacity().is_ok());
        app.core.design_rank_auto = true;
        assert!(app
            .compute_holding_capacity()
            .err()
            .unwrap()
            .contains("耐力入力"));
    }
    #[cfg(feature = "gui")]
    #[test]
    fn gui未採用でもピークは解析経過と表示し自動耐力未入力は手動切替を妨げない() {
        let mut app = ready();
        let bundle = app.core.scoped.results.as_mut().unwrap();
        for po in [bundle.pushover.as_mut(), bundle.pushover_x.as_mut()]
            .into_iter()
            .flatten()
        {
            po.qu = 150_000.0;
            for point in &mut po.capacity_curve {
                point.story_shear.fill(120_000.0);
            }
            po.capacity_curve
                .first_mut()
                .unwrap()
                .story_shear
                .fill(150_000.0);
            let step = po.capacity_curve.last().unwrap().step;
            let response = po
                .confirmed_history
                .as_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r.step == step)
                .unwrap();
            for cut in &mut response.cuts {
                let count = cut.forces.len() as f64;
                for force in &mut cut.forces {
                    force.force_n = 120_000.0 / count;
                }
                cut.external_n = 120_000.0;
                cut.reference_n = 120_000.0;
                cut.support_n = 0.0;
            }
        }
        let ctx = egui::Context::default();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.pushover_results_panel(ui)
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"解析経過の最大ベースシア = 150.0 kN"));
        assert!(!labels.iter().any(|text| text.contains("保有水平耐力 Qu")));
        assert!(labels.iter().any(|text| text
            .starts_with("解析経過の層別最大せん断力（各層のピーク）:")
            && text.contains("150.0 kN")));
        assert!(!labels.iter().any(|text| text.contains("層別 Qu:")));
        let csv = crate::summary::build_report_csv(&app);
        assert!(csv.contains("解析経過の最大ベースシア[kN],150.00"));
        assert!(!csv.contains("保有水平耐力Qu[kN]"));
        super::super::tests::select_holding_points(&mut app);
        assert_eq!(
            app.compute_holding_capacity().unwrap().0.stories[0].qu,
            120_000.0
        );
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.pushover_results_panel(ui)
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|text| text
            .starts_with("解析経過の層別最大せん断力（各層のピーク）:")
            && text.contains("150.0 kN")));
        assert!(!labels.iter().any(|text| text.contains("層別 Qu:")));
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2000.0, 10000.0),
                )),
                ..Default::default()
            },
            |ui| crate::design_view::design_table(ui, &mut app),
        );
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "120.0")));
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .pushover_x
            .as_mut()
            .unwrap()
            .ds_evaluation
            .as_mut()
            .unwrap()
            .member_capacities_n
            .clear();
        app.core.design_rank_auto = true;
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::design_view::holding_evaluation_inputs(ui, &mut app);
        });
        let editor_pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.job.text == "部材群の耐力入力 [N]（負担力とは別）" =>
                {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap();
        for pressed in [true, false] {
            let raw = egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(editor_pos),
                    egui::Event::PointerButton {
                        pos: editor_pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run_ui(raw, |ui| {
                crate::design_view::holding_evaluation_inputs(ui, &mut app)
            });
        }
        assert!(app
            .pushover_for(SeismicDir::X)
            .unwrap()
            .ds_evaluation
            .as_ref()
            .unwrap()
            .member_capacities_n
            .iter()
            .any(|(_, q)| *q == 0.0));
        assert!(app
            .compute_holding_capacity()
            .err()
            .unwrap()
            .contains("非正"));
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::design_view::holding_rank_mode_input(ui, &mut app)
        });
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.job.text == "自動判定（鋼=幅厚比・RC矩形=Qsu/Qmu）" =>
                {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap();
        for pressed in [true, false] {
            let raw = egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run_ui(raw, |ui| {
                crate::design_view::holding_rank_mode_input(ui, &mut app)
            });
        }
        assert!(!app.core.design_rank_auto);
        assert!(app.compute_holding_capacity().is_ok());
    }
    #[cfg(feature = "gui")]
    #[test]
    fn guiは壁負担率適用後のdsと目的ごとの部材応答変形stepを表示する() {
        use sepika_design_jp::secondary::holding_capacity::{FrameType, MemberRank};
        use sepika_solver::nonlinear::pushover::story_response::{CutForce, ForceGroup};
        let mut app = ready();
        app.core.design_rank_auto = false;
        app.core.design_frame = FrameType::RcFrame;
        app.core.design_rank = MemberRank::FA;
        let po = app
            .core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .pushover_x
            .as_mut()
            .unwrap();
        let ds_step = po.capacity_curve.first().unwrap().step;
        let capacity_step = po.capacity_curve.last().unwrap().step;
        assert_ne!(ds_step, capacity_step);
        po.ds_evaluation = Some(
            po.evaluation_point(
                EvaluationPurpose::Ds,
                SeismicDir::X,
                ds_step,
                "Ds部材応答を初回確定点から採用".into(),
            )
            .unwrap(),
        );
        po.capacity_evaluation = Some(
            po.evaluation_point(
                EvaluationPurpose::HoldingCapacity,
                SeismicDir::X,
                capacity_step,
                "比較変形を最終確定点から採用".into(),
            )
            .unwrap(),
        );
        let response = po
            .confirmed_history
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r.step == ds_step)
            .unwrap();
        let cut = &mut response.cuts[0];
        cut.forces = vec![
            CutForce {
                elem: sepika_core::ids::ElemId(0),
                group: ForceGroup::Wall,
                force_n: 77_000.0,
            },
            CutForce {
                elem: sepika_core::ids::ElemId(1),
                group: ForceGroup::Frame,
                force_n: 23_000.0,
            },
        ];
        cut.external_n = 100_000.0;
        cut.reference_n = 100_000.0;
        cut.support_n = 0.0;
        let (result, _) = app.compute_holding_capacity().unwrap();
        assert_eq!(app.core.scoped.ds_beta_u_by_story, vec![0.77]);
        assert_eq!(result.stories[0].ds, 0.40);
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2000.0, 10000.0),
                )),
                ..Default::default()
            },
            |ui| crate::design_view::design_table(ui, &mut app),
        );
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"0.40"));
        assert!(!labels.iter().any(|text| text.contains("簡易運用")));
        assert!(labels.contains(&format!("採用方向: X、Ds部材応答の採用ステップ: {ds_step}、保有耐力比較・変形の採用ステップ: {capacity_step}").as_str()));
        assert!(!labels
            .iter()
            .any(|text| text.contains("部材応答・変形の採用ステップ")));
    }
    fn two_purposes() -> App {
        let mut app = ready();
        super::super::tests::select_holding_points(&mut app);
        app.adopt_holding_evaluation(EvaluationPurpose::Ds).unwrap();
        app.core.analysis_cfg.push_max_disp = 2.0;
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        app.adopt_holding_evaluation(EvaluationPurpose::HoldingCapacity)
            .unwrap();
        assert_eq!(
            app.core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .holding_evaluations
                .len(),
            2
        );
        app
    }
    #[test]
    fn 目的別run条件の差を許容し同時点の独立期待値と保存帳票を維持する() {
        let mut app = two_purposes();
        let bundle = app.core.scoped.results.as_mut().unwrap();
        for po in [bundle.pushover.as_mut(), bundle.pushover_x.as_mut()]
            .into_iter()
            .flatten()
        {
            po.qu = 150_000.0;
        }
        let entries = &mut app
            .core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .holding_evaluations;
        for e in entries.iter_mut() {
            let qu = if e.point.purpose == EvaluationPurpose::Ds {
                150_000.0
            } else {
                120_000.0
            };
            let record = e
                .run
                .confirmed_history
                .as_mut()
                .unwrap()
                .first_mut()
                .unwrap();
            for cut in &mut record.cuts {
                let count = cut.forces.len() as f64;
                for f in &mut cut.forces {
                    f.force_n = qu / count;
                }
                cut.external_n = qu;
                cut.reference_n = qu;
                cut.support_n = 0.0;
            }
            for point in &mut e.run.capacity_curve {
                point.story_shear.fill(999_000.0);
            }
        }
        let (result, _) = app.compute_holding_capacity().unwrap();
        assert_eq!(result.stories[0].qu, 120_000.0);
        let source = app.core.scoped.holding_capacity_source.as_ref().unwrap();
        assert_eq!(source.ds_forces[0].qu_n, 150_000.0);
        assert_eq!(source.capacity_forces[0].qu_n, 120_000.0);
        assert_ne!(source.ds_point.run_id, source.capacity_point.run_id);
        assert_ne!(
            source.ds_point.input_generation,
            source.capacity_point.input_generation
        );
        assert_eq!(source.ds_conditions.push_max_disp, 1.0);
        assert_eq!(source.capacity_conditions.push_max_disp, 2.0);
        let csv = crate::summary::build_report_csv(&app);
        for expected in [
            "解析経過の最大ベースシア[kN],150.00",
            "目的別採用run Ds",
            "目的別採用run HoldingCapacity",
            "Qu=150000",
            "Qu=120000",
            "残差=0",
            "許容差=",
            "generation_sha256=",
        ] {
            assert!(csv.contains(expected), "{expected}");
        }
        assert!(!csv.contains("保有水平耐力Qu[kN]"));
        let dir = std::env::temp_dir().join(format!("sepika-issue442-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("purpose-runs.ovika");
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path);
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert_eq!(
            loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .holding_evaluations
                .len(),
            2
        );
        assert_eq!(
            loaded.compute_holding_capacity().unwrap().0.stories[0].qu,
            120_000.0
        );
        assert!(crate::summary::build_report_csv(&loaded).contains("Qu=150000"));
    }
    #[test]
    fn 目的別外力分布条件差は許容し標準名の手入力地震力と他荷重変更を拒否する() {
        use sepika_core::ids::{LoadCaseId, NodeId};
        use sepika_core::model::{LoadCase, LoadCaseKind, NodalLoad};
        let mut app = ready();
        let ex = app
            .core
            .model
            .load_cases
            .iter_mut()
            .find(|c| c.name == "EX")
            .unwrap();
        ex.nodal.push(NodalLoad::manual(
            NodeId(1),
            [100.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ));
        let id = LoadCaseId(app.core.model.load_cases.len() as u32);
        app.core.model.load_cases.push(LoadCase {
            id,
            name: "利用者荷重".into(),
            kind: LoadCaseKind::Dead,
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -200.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        });
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        app.adopt_holding_evaluation(EvaluationPurpose::Ds).unwrap();
        app.core.analysis_cfg.c0 = 0.3;
        app.core.analysis_cfg.push_max_disp = 2.0;
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        app.adopt_holding_evaluation(EvaluationPurpose::HoldingCapacity)
            .unwrap();
        assert!(app.compute_holding_capacity().is_ok());
        let ds = &app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .holding_evaluations[0];
        assert_eq!(ds.conditions.c0, 0.2);
        assert!(app.holding_evaluation_is_current(ds));
        app.core
            .model
            .load_cases
            .iter_mut()
            .find(|c| c.name == "EX")
            .unwrap()
            .nodal
            .iter_mut()
            .find(|n| n.source == sepika_core::model::LoadSource::Manual)
            .unwrap()
            .values[0] += 1.0;
        let ds = &app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .holding_evaluations[0];
        assert!(!app.holding_evaluation_is_current(ds));
        assert!(app.compute_holding_capacity().is_err());
        assert!(crate::summary::build_report_csv(&app).contains("生成入力が現在モデルと不一致"));
        app.core
            .model
            .load_cases
            .iter_mut()
            .find(|c| c.name == "EX")
            .unwrap()
            .nodal
            .iter_mut()
            .find(|n| n.source == sepika_core::model::LoadSource::Manual)
            .unwrap()
            .values[0] -= 1.0;
        app.core
            .model
            .load_cases
            .iter_mut()
            .find(|c| c.name == "利用者荷重")
            .unwrap()
            .nodal[0]
            .values[2] -= 1.0;
        let ds = &app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .holding_evaluations[0];
        assert!(!app.holding_evaluation_is_current(ds));
        assert!(app.compute_holding_capacity().is_err());
    }
    #[test]
    fn 目的別run保持後もモデル材料荷重変更と識別欠落を拒否する() {
        let app = two_purposes();
        for case in 0..4 {
            let mut mutated = two_purposes();
            match case {
                0 => mutated.core.model.materials[0].fy = Some(999.0),
                1 => mutated.core.model.nodes[0].coord[0] += 10.0,
                2 => mutated.core.model.stories[1].weight_override = Some(123.0),
                _ => {
                    mutated
                        .core
                        .scoped
                        .results
                        .as_mut()
                        .unwrap()
                        .holding_evaluations[0]
                        .run
                        .wall_run
                        .as_mut()
                        .unwrap()
                        .input_generation = None
                }
            }
            assert!(mutated.compute_holding_capacity().is_err(), "case {case}");
            assert!(mutated.core.scoped.holding_capacity_source.is_none());
        }
        assert_eq!(
            app.core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .holding_evaluations
                .len(),
            2
        );
    }
}
