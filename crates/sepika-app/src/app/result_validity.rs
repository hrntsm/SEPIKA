use super::*;
use sepika_core::result_contract::{ANALYSIS_CONTRACT_VERSION, DESIGN_CONTRACT_VERSION};
use sepika_design_jp::CheckOutcome;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResultInputKey {
    Static(StaticCaseKey),
    Combo(String),
    Pushover(SeismicDir),
    Modal,
    Design,
    TimeHistory(VibrationCaseId),
    LumpedTimeHistory(sepika_core::ids::LumpedVibrationCaseId),
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VibrationResultInput {
    model: sepika_core::model::Model,
    analysis_identity: String,
    identity: String,
}

pub(super) fn decode_recorded_input(
    key: &ResultInputKey,
    bytes: &[u8],
    vibration_cfg: Option<AnalysisSettings>,
) -> Result<references::RecordedCalculationInput, String> {
    use references::{RecordedCalculationInput, RecordedSettings};
    let unavailable =
        |_| "保存された計算条件は取得不能です。現在の設定で補完しません。".to_string();
    match key {
        ResultInputKey::Design => {
            type DesignInput = (
                sepika_core::model::Model,
                bool,
                bool,
                sepika_design_jp::BondMethod,
                sepika_design_jp::QdMethod,
                Option<(ResultInputKey, Option<Vec<u8>>)>,
            );
            let (model, short_term, rc_damage_control, bond_method, qd_method, source): DesignInput = bincode::deserialize(bytes).map_err(unavailable)?;
            let source = source.map(|(key, input)| {
                let input = input
                    .and_then(|bytes| decode_recorded_input(&key, &bytes, None).ok())
                    .map(Box::new);
                (key, input)
            });
            Ok(RecordedCalculationInput {
                model,
                settings: RecordedSettings::Design {
                    short_term,
                    rc_damage_control,
                    bond_method,
                    qd_method,
                    source,
                },
            })
        }
        ResultInputKey::TimeHistory(_) | ResultInputKey::LumpedTimeHistory(_) => {
            let input: VibrationResultInput = bincode::deserialize(bytes).map_err(unavailable)?;
            Ok(RecordedCalculationInput {
                model: input.model,
                settings: RecordedSettings::Vibration {
                    analysis_identity: input.analysis_identity,
                    identity: input.identity,
                    cfg: vibration_cfg.map(Box::new),
                },
            })
        }
        _ => {
            let (model, cfg, period_s): (sepika_core::model::Model, AnalysisSettings, Option<f64>) =
                bincode::deserialize(bytes).map_err(unavailable)?;
            Ok(RecordedCalculationInput {
                model,
                settings: RecordedSettings::Analysis {
                    kind: key.clone(),
                    cfg: Box::new(cfg),
                    period_s,
                },
            })
        }
    }
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
    pub(super) fn has_analysis_results(&self) -> bool {
        !self.statics.is_empty()
            || !self.combos.is_empty()
            || self.modal.is_some()
            || !self.member_forces.is_empty()
            || !self.panel_moments.is_empty()
            || self.pushover_x.is_some()
            || self.pushover_y.is_some()
            || !self.holding_evaluations.is_empty()
            || self.time_history.is_some()
            || !self.time_histories.is_empty()
            || self.lumped.is_some()
            || !self.lumped_results.is_empty()
    }

    pub(super) fn has_design_results(&self) -> bool {
        !self.member_checks.is_empty()
            || !self.joint_checks.is_empty()
            || !self.wall_checks.is_empty()
            || !self.beam_checks.is_empty()
            || !self.slab_checks.is_empty()
    }

    pub(super) fn clear_design_results(&mut self) {
        self.member_checks.clear();
        self.joint_checks.clear();
        self.wall_checks.clear();
        self.beam_checks.clear();
        self.slab_checks.clear();
        self.design_source = None;
        self.design_contract_version = None;
        self.input_records
            .retain(|(key, _)| *key != ResultInputKey::Design);
    }

    pub(super) fn record_scoped_input(
        &mut self,
        key: ResultInputKey,
        input: Vec<u8>,
        source: references::InputReference,
    ) {
        self.reference_sources.retain(|(saved, _)| *saved != key);
        self.reference_sources.push((key.clone(), source));
        self.record_input(key, input);
    }

    pub(super) fn record_input(&mut self, key: ResultInputKey, input: Vec<u8>) {
        self.reference_runs.retain(|(k, _)| *k != key);
        self.reference_runs
            .push((key.clone(), uuid::Uuid::new_v4().to_string()));
        self.input_records.retain(|(k, _)| *k != key);
        self.input_records.push((key, input));
    }
}

fn contract_reason(category: &str, saved: Option<u32>, current: u32) -> Option<String> {
    if saved == Some(current) {
        return None;
    }
    let reason = match saved {
        None => "版不明",
        Some(version) if version < current => "旧契約",
        Some(_) => "現アプリ未対応の未来版",
    };
    let saved = saved.map_or_else(|| "欠落".into(), |version| version.to_string());
    Some(format!("{category}結果は利用不可です（{reason}: 保存版={saved}, 現行版={current}）。{category}を再実行してください。"))
}

impl App {
    pub(crate) fn validate_restored_results(&mut self) {
        if self.core.scoped.results.is_none() {
            self.core.scoped.last_static = None;
            self.ui.scoped.nav.focus_result = None;
            self.core.scoped.staleness.results_stale = true;
            self.core.scoped.staleness.design_stale = true;
            return;
        }
        self.clear_result_contract_notice();
        self.core.scoped.staleness.results_stale = false;
        self.core.scoped.staleness.design_stale = false;
        self.reject_result_contracts();
        self.reject_unsupported_panel_results(true);
        if let Some(key) = self.core.scoped.last_static {
            if let Err(reason) = self.require_selected_static_input() {
                let diagnostics = self
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .joint_checks
                    .iter()
                    .filter(|check| {
                        check.label.starts_with("パネルゾーン(S)")
                            && matches!(
                                check.outcome,
                                sepika_design_jp::CheckOutcome::Skipped { .. }
                            )
                    })
                    .cloned()
                    .collect();
                match key {
                    StaticKey::Case(key) => self.invalidate_static_key(key),
                    StaticKey::Combo(index) => {
                        let name = self
                            .core
                            .scoped
                            .results
                            .as_ref()
                            .and_then(|bundle| bundle.combos.get(index))
                            .map(|(name, _)| name.clone());
                        if let Some(name) = name {
                            self.remove_excluded_tip_results(&(Vec::new(), vec![name]));
                        }
                    }
                }
                if let Some(bundle) = self.core.scoped.results.as_mut() {
                    bundle.member_forces.clear();
                    bundle.panel_moments.clear();
                    bundle.clear_design_results();
                    bundle.joint_checks = diagnostics;
                }
                self.core.scoped.last_static = None;
                self.ui.scoped.nav.focus_result = None;
                self.core.scoped.staleness.results_stale =
                    !self.core.scoped.results.as_ref().is_some_and(|bundle| {
                        bundle.time_history.is_some()
                            || !bundle.time_histories.is_empty()
                            || bundle.lumped.is_some()
                            || !bundle.lumped_results.is_empty()
                    });
                self.core.scoped.staleness.design_stale = true;
                self.core.scoped.result_contract_notice = Some(reason.clone());
                self.report_notice(reason);
            }
        }
        if self
            .core
            .scoped
            .results
            .as_ref()
            .is_some_and(|b| b.has_design_results())
        {
            self.core.scoped.staleness.design_stale = self.require_design_input().is_err();
        }
    }

    pub(super) fn reject_result_contracts(&mut self) -> bool {
        let Some(bundle) = self.core.scoped.results.as_ref() else {
            return false;
        };
        let analysis = bundle
            .has_analysis_results()
            .then(|| {
                contract_reason(
                    "解析",
                    bundle.analysis_contract_version,
                    ANALYSIS_CONTRACT_VERSION,
                )
            })
            .flatten();
        let design = bundle
            .has_design_results()
            .then(|| {
                contract_reason(
                    "検定",
                    bundle.design_contract_version,
                    DESIGN_CONTRACT_VERSION,
                )
            })
            .flatten();
        if analysis.is_none() && design.is_none() {
            return false;
        }
        let panel_rejected = self.reject_unsupported_panel_results(true);
        if analysis.is_some() {
            let rejected = if panel_rejected {
                self.core
                    .scoped
                    .results
                    .take()
                    .unwrap_or_default()
                    .joint_checks
            } else {
                self.core
                    .scoped
                    .results
                    .as_ref()
                    .unwrap()
                    .joint_checks
                    .iter()
                    .filter(|c| {
                        c.label.starts_with("パネルゾーン(S)")
                            && matches!(c.outcome, CheckOutcome::Skipped { .. })
                    })
                    .cloned()
                    .collect()
            };
            self.core.scoped.results = Some(ResultsBundle {
                joint_checks: rejected,
                ..Default::default()
            });
            self.core.scoped.view_vibration_case = None;
            self.core.scoped.view_lumped_vibration_case = None;
            self.core.scoped.stick_response = None;
            self.hydrate_saved_vibration_views(None, None);
            self.core.scoped.last_static = None;
            self.ui.scoped.nav.focus_result = None;
            self.core.scoped.staleness.results_stale = true;
            self.core.scoped.staleness.last_run = None;
            self.core.scoped.holding_capacity_source = None;
            self.core.scoped.ds_beta_u_by_story.clear();
            self.core.scoped.ds_wall_details.clear();
            self.core.scoped.ds_beta_u_unavailable = false;
            self.core.scoped.ds_rank_fallback_stories.clear();
        } else if let Some(bundle) = self.core.scoped.results.as_mut() {
            let rejected = bundle
                .joint_checks
                .iter()
                .filter(|c| {
                    c.label.starts_with("パネルゾーン(S)")
                        && matches!(c.outcome, CheckOutcome::Skipped { .. })
                })
                .cloned()
                .collect();
            bundle.clear_design_results();
            bundle.joint_checks = rejected;
        }
        self.core.scoped.staleness.design_stale = true;
        let reason = analysis
            .into_iter()
            .chain(design)
            .collect::<Vec<_>>()
            .join(" / ");
        self.core.scoped.result_contract_notice = Some(reason.clone());
        self.report_notice(reason);
        true
    }

    pub(super) fn clear_result_contract_notice(&mut self) {
        if self.core.scoped.last_notice == self.core.scoped.result_contract_notice {
            self.core.scoped.last_notice = None;
        }
        self.core.scoped.result_contract_notice = None;
    }

    pub(super) fn take_results_for_analysis(&mut self) -> ResultsBundle {
        self.reject_result_contracts();
        let mut bundle = self.core.scoped.results.take().unwrap_or_default();
        bundle.analysis_contract_version = Some(ANALYSIS_CONTRACT_VERSION);
        self.clear_result_contract_notice();
        bundle
    }

    pub(super) fn reject_unsupported_panel_results(&mut self, allow_recorded_skips: bool) -> bool {
        let Some(saved) = self.core.scoped.results.as_ref() else {
            return false;
        };
        let has_analysis_results = saved.has_analysis_results();
        let has_panel_success = saved.joint_checks.iter().any(|check| {
            check.label.starts_with("パネルゾーン(S)")
                && matches!(check.outcome, CheckOutcome::Checked(_))
        });
        // 空の結果容器や床板だけの直接設計には、流用を拒否すべき架構解析応力がない。
        if !has_analysis_results && !has_panel_success {
            return false;
        }
        let model = &self.core.model;
        let mut rejected = Vec::new();
        for node in &model.nodes {
            for plane in
                sepika_core::panel_zone::panel_joint_planes(model, node.id, &model.elements)
            {
                let members = model.elements.iter().filter(|e| {
                    e.nodes.contains(&node.id)
                        && (sepika_core::panel_zone::member_orientation(model, e)
                            == Some(sepika_core::panel_zone::MemberOrientation::Column)
                            || plane.beams.iter().any(|b| b.element == e.id))
                });
                if let Err(reason) =
                    sepika_core::panel_zone::resolve_panel_joint(model, node.id, members)
                {
                    let reason = reason.to_string();
                    if allow_recorded_skips && saved.joint_checks.iter().any(|check| {
                        check.node == node.id && check.label.starts_with("パネルゾーン(S)")
                            && matches!(&check.outcome, CheckOutcome::Skipped { reason: recorded } if recorded == &reason)
                            && !saved.joint_checks.iter().any(|other| other.node == node.id
                                && (other.label == check.label || other.label == "パネルゾーン(S)")
                                && matches!(other.outcome, CheckOutcome::Checked(_)))
                    }) { continue; }
                    rejected.push(JointCheck {
                        node: node.id,
                        label: format!("パネルゾーン(S) {}", plane.label()),
                        outcome: CheckOutcome::Skipped { reason },
                    });
                }
            }
        }
        if rejected.is_empty() {
            return false;
        }
        let reasons = rejected
            .iter()
            .filter_map(|check| match &check.outcome {
                CheckOutcome::Skipped { reason } => Some(reason.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" / ");
        // 全体解析の応力は接合部の剛性と連成するため、旧成功成果の他面だけを再認証できない。
        self.core.scoped.results = Some(ResultsBundle {
            joint_checks: rejected,
            ..Default::default()
        });
        self.core.scoped.last_static = None;
        self.core.scoped.staleness.results_stale = true;
        self.core.scoped.staleness.design_stale = true;
        self.core.scoped.staleness.last_run = None;
        self.report_notice(format!("既存の解析・検定結果は利用不可です。標準パネルの適用条件を再確認して解析を再実行してください: {reasons}"));
        true
    }
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
        self.core.scoped.results.as_ref().is_some_and(|bundle| {
            bundle.analysis_contract_version == Some(ANALYSIS_CONTRACT_VERSION)
        }) && input.point.input_generation
            == self.result_input_with_settings(
                &ResultInputKey::Pushover(input.point.direction),
                input.conditions,
            )
    }

    fn calculation_model(&self) -> sepika_core::model::Model {
        self.core.model.calculation_model()
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
        if matches!(
            key,
            ResultInputKey::TimeHistory(_) | ResultInputKey::LumpedTimeHistory(_)
        ) {
            let input = self
                .core
                .scoped
                .results
                .as_ref()
                .and_then(|bundle| bundle.input_records.iter().find(|(saved, _)| saved == key))
                .and_then(|(_, input)| bincode::deserialize::<VibrationResultInput>(input).ok());
            input
                .map(|mut input| {
                    input.model = model;
                    bincode::serialize(&input).expect("時刻歴入力の直列化")
                })
                .unwrap_or_default()
        } else {
            bincode::serialize(&(model, relevant, self.result_generation_period(key, cfg)))
                .expect("計算入力の直列化")
        }
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
            ResultInputKey::Modal
            | ResultInputKey::Design
            | ResultInputKey::TimeHistory(_)
            | ResultInputKey::LumpedTimeHistory(_) => false,
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

    /// 検定が実際に採用した応力源。表示選択は生成証明として扱わない。
    pub(super) fn selected_design_source(&self) -> Option<ResultInputKey> {
        match self.core.scoped.last_static {
            Some(StaticKey::Case(key)) => Some(ResultInputKey::Static(key)),
            Some(StaticKey::Combo(index)) => self
                .core
                .scoped
                .results
                .as_ref()
                .and_then(|r| r.combos.get(index))
                .map(|(name, _)| ResultInputKey::Combo(name.clone())),
            None => None,
        }
    }

    fn design_input(&self, source: Option<ResultInputKey>) -> Vec<u8> {
        let term = match &source {
            Some(ResultInputKey::Static(key)) => {
                let id = match key {
                    StaticCaseKey::User(id) => Some(*id),
                    StaticCaseKey::Seismic(dir) => self.seismic_case_id(*dir),
                };
                id.and_then(|id| self.core.model.load_cases.iter().find(|case| case.id == id))
                    .and_then(|case| sepika_core::load_combo::case_design_state(case.kind).ok())
            }
            Some(ResultInputKey::Combo(name)) => self
                .core
                .model
                .combinations
                .iter()
                .find(|combo| combo.name == *name)
                .and_then(|combo| {
                    sepika_core::load_combo::combination_design_state(
                        combo,
                        &self.core.model.load_cases,
                    )
                    .ok()
                }),
            _ => None,
        }
        .is_some_and(|state| state.duration == sepika_core::load_combo::LoadDuration::Short);
        let source = source.map(|key| {
            let input = self
                .core
                .scoped
                .results
                .as_ref()
                .and_then(|r| r.input_records.iter().find(|(k, _)| *k == key))
                .map(|(_, input)| input.clone());
            (key, input)
        });
        bincode::serialize(&(
            self.calculation_model(),
            term,
            self.core.analysis_cfg.rc_damage_control,
            self.core.analysis_cfg.bond_method,
            self.core.analysis_cfg.qd_method,
            source,
        ))
        .expect("検定入力の直列化")
    }

    pub(super) fn record_design_input(&mut self) {
        let source = self.selected_design_source();
        let input = self.design_input(source.clone());
        let reference_source = self.input_reference();
        if let Some(bundle) = self.core.scoped.results.as_mut() {
            bundle.design_source = source;
            bundle.record_scoped_input(ResultInputKey::Design, input, reference_source);
        }
    }

    #[cfg(test)]
    pub(crate) fn certify_design_fixture(&mut self) {
        self.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .design_contract_version = Some(DESIGN_CONTRACT_VERSION);
        self.record_design_input();
    }

    pub(crate) fn require_design_input(&self) -> Result<(), String> {
        let bundle = self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("検定結果がありません")?;
        if let Some(reason) = contract_reason(
            "検定",
            bundle.design_contract_version,
            DESIGN_CONTRACT_VERSION,
        ) {
            return Err(reason);
        }
        if let Some(key) = &bundle.design_source {
            self.require_result_input(key.clone())?;
        }
        let input = bundle
            .input_records
            .iter()
            .find(|(key, _)| *key == ResultInputKey::Design);
        if input.is_some_and(|(_, input)| *input == self.design_input(bundle.design_source.clone()))
        {
            Ok(())
        } else {
            Err("検定結果は利用不可です。生成入力が欠落または現在のモデル・実効設計条件と不一致です。検定を再実行してください。".into())
        }
    }

    #[cfg(any(feature = "gui", test))]
    pub(crate) fn current_design_results(&self) -> Option<&ResultsBundle> {
        self.require_design_input().ok()?;
        self.core.scoped.results.as_ref()
    }

    pub(crate) fn vibration_result_input(&self, source: &TimeHistorySource) -> Vec<u8> {
        bincode::serialize(&VibrationResultInput {
            model: self.calculation_model(),
            analysis_identity: source.analysis_identity.clone(),
            identity: source.identity.clone(),
        })
        .expect("時刻歴生成入力の直列化")
    }

    fn vibration_result_input_is_valid(&self, key: &ResultInputKey, current_model: bool) -> bool {
        let identity = match key {
            ResultInputKey::TimeHistory(id) => self
                .core
                .model
                .vibration_cases
                .iter()
                .find(|c| c.id == *id)
                .and_then(|c| c.input_identity.as_ref()),
            ResultInputKey::LumpedTimeHistory(id) => self
                .core
                .model
                .lumped_vibration_cases
                .iter()
                .find(|c| c.id == *id)
                .and_then(|c| c.input_identity.as_ref()),
            _ => return false,
        };
        let Some(identity) = identity else {
            return false;
        };
        let Some(bundle) = self.core.scoped.results.as_ref() else {
            return false;
        };
        if bundle.analysis_contract_version != Some(ANALYSIS_CONTRACT_VERSION) {
            return false;
        }
        let Some((_, input_bytes)) = bundle.input_records.iter().find(|(saved, _)| saved == key)
        else {
            return false;
        };
        let Ok(input) = bincode::deserialize::<VibrationResultInput>(input_bytes) else {
            return false;
        };
        bincode::serialize(&input).unwrap() == *input_bytes
            && input.identity == *identity
            && input.identity
                == crate::app::vibration::vibration_identity(&(
                    &input.analysis_identity,
                    &input.model,
                ))
            && (!current_model
                || bincode::serialize(&input.model).unwrap()
                    == bincode::serialize(&self.calculation_model()).unwrap())
    }

    pub(super) fn filter_vibration_results(&self, bundle: &mut ResultsBundle) -> bool {
        let before = (bundle.time_histories.len(), bundle.lumped_results.len());
        bundle.time_histories.retain(|(id, _)| {
            self.vibration_result_input_is_valid(&ResultInputKey::TimeHistory(*id), false)
        });
        bundle.lumped_results.retain(|(id, _)| {
            self.vibration_result_input_is_valid(&ResultInputKey::LumpedTimeHistory(*id), false)
        });
        let spatial_ids: Vec<_> = bundle.time_histories.iter().map(|(id, _)| *id).collect();
        let lumped_ids: Vec<_> = bundle.lumped_results.iter().map(|(id, _)| *id).collect();
        bundle.input_records.retain(|(key, _)| match key {
            ResultInputKey::TimeHistory(id) => spatial_ids.contains(id),
            ResultInputKey::LumpedTimeHistory(id) => lumped_ids.contains(id),
            _ => true,
        });
        if self
            .core
            .scoped
            .view_vibration_case
            .is_none_or(|id| !spatial_ids.contains(&id))
        {
            bundle.time_history = None;
        }
        if bundle.lumped.as_ref().is_some_and(|r| r.response.is_some())
            && self
                .core
                .scoped
                .view_lumped_vibration_case
                .is_none_or(|id| !lumped_ids.contains(&id))
        {
            bundle.lumped = None;
        }
        before != (bundle.time_histories.len(), bundle.lumped_results.len())
    }

    pub(super) fn job_input(&self) -> Vec<u8> {
        let period = matches!(self.core.analysis_cfg.ai_mode, AiMode::SemiPrecise)
            .then(|| self.design_seismic_period().ok())
            .flatten();
        bincode::serialize(&(self.calculation_model(), self.core.analysis_cfg, period))
            .expect("ジョブ入力の直列化")
    }

    pub(super) fn require_selected_static_input(&self) -> Result<(), String> {
        let key = match self.core.scoped.last_static {
            Some(StaticKey::Case(key)) => Some(ResultInputKey::Static(key)),
            Some(StaticKey::Combo(index)) => self
                .core
                .scoped
                .results
                .as_ref()
                .and_then(|r| r.combos.get(index))
                .map(|(name, _)| ResultInputKey::Combo(name.clone())),
            None => None,
        }
        .ok_or_else(|| "参照解析結果の選択がありません。解析を再実行してください。".to_string())?;
        self.require_result_input(key)
    }

    pub(crate) fn require_result_input(&self, key: ResultInputKey) -> Result<(), String> {
        if key == ResultInputKey::Design {
            return self.require_design_input();
        }
        if let Some(bundle) = self
            .core
            .scoped
            .results
            .as_ref()
            .filter(|b| b.has_analysis_results())
        {
            if let Some(reason) = contract_reason(
                "解析",
                bundle.analysis_contract_version,
                ANALYSIS_CONTRACT_VERSION,
            ) {
                return Err(reason);
            }
        }
        if matches!(
            key,
            ResultInputKey::TimeHistory(_) | ResultInputKey::LumpedTimeHistory(_)
        ) {
            return if self.vibration_result_input_is_valid(&key, true) {
                Ok(())
            } else {
                Err("時刻歴結果の生成入力が欠落または入力識別・現在のモデルと不一致です。再実行してください。".into())
            };
        }
        let label = match &key {
            ResultInputKey::Static(StaticCaseKey::Seismic(dir)) => format!("地震静的 {dir:?}"),
            ResultInputKey::Static(StaticCaseKey::User(id)) => format!("静的荷重ケース {id:?}"),
            ResultInputKey::Combo(name) => format!("静的組合せ「{name}」"),
            ResultInputKey::Pushover(dir) => format!("増分解析 {dir:?}"),
            ResultInputKey::Modal => "固有値解析".into(),
            ResultInputKey::Design => "検定".into(),
            ResultInputKey::TimeHistory(id) => format!("時刻歴解析 {}", id.0),
            ResultInputKey::LumpedTimeHistory(id) => format!("質点系時刻歴解析 {}", id.0),
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
            Some(_) => Err(format!("解析結果の利用は無効です。{label}の生成入力が現在のモデル・計算設定と一致しません。{label}を再実行してください。")),
            None => Err(format!("解析結果の利用は無効です。{label}の入力識別情報がありません。{label}を再実行してください。")),
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
        app.load_model(crate::app::tests::with_explicit_pushover_gravity(
            crate::sample::portal_frame(),
        ));
        app.core.analysis_cfg.threads = 1;
        app.core.analysis_cfg.push_steps = 3;
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
            |a| a.core.model.stress_cfg.misc_wall_n = Some(1.0),
            |a| {
                a.core.model.stress_cfg.misc_wall_method =
                    sepika_core::model::MiscWallStiffnessMethod::RepresentativeColumn
            },
            |a| {
                a.core.model.stress_cfg.misc_wall_representatives.push(
                    sepika_core::model::MiscWallRepresentativeColumn {
                        story: sepika_core::ids::StoryId(1),
                        column: sepika_core::ids::ElemId(0),
                    },
                )
            },
            |a| a.core.analysis_cfg.push_steps += 1,
            |a| {
                a.core.model.stress_cfg.consider_rigid_zones =
                    !a.core.model.stress_cfg.consider_rigid_zones
            },
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
        let initial = app
            .compute_unconfirmed_holding_capacity()
            .unwrap()
            .0
            .stories[0]
            .qun;
        #[cfg(feature = "gui")]
        {
            app.ui.view.hinge_step = Some(1);
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
            app.compute_unconfirmed_holding_capacity()
                .unwrap()
                .0
                .stories[0]
                .qun,
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
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
        app.core.model.materials[0].fy = Some(345.0);
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.core.analysis_cfg.push_dir = SeismicDir::X;
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
        app.run_eigen(1);
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
    }

    #[test]
    fn 精算周期は固有値を再解析しても標準地震結果として採用しない() {
        let mut app = ready();
        app.run_eigen(1);
        app.core.analysis_cfg.ai_mode = AiMode::SemiPrecise;
        app.run_eigen(1);
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
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
        let index = app.core.model.ultimate_gravity_combination.unwrap();
        app.core.model.combinations[index].name = "長期".into();
        app.run_static_all();
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        let bytes = rmp_serde::to_vec(app.core.scoped.results.as_ref().unwrap()).unwrap();
        app.core.scoped.results = Some(rmp_serde::from_slice(&bytes).unwrap());
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
        app.core.model.stories[1].weight_override = Some(100_000.0);
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        let baseline = app
            .compute_unconfirmed_holding_capacity()
            .unwrap()
            .0
            .stories[0]
            .qud;
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
        let with_modal = app
            .compute_unconfirmed_holding_capacity()
            .unwrap()
            .0
            .stories[0]
            .qud;
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
        basement.elevation = Some(1000.0);
        basement.node_ids = vec![basement_node];
        basement.weight_override = Some(100_000.0);
        basement.level_kind = sepika_core::model::StoryLevelKind::Basement { depth_mm: 0.0 };
        app.core.model.stories[1].id = StoryId(2);
        app.core.model.stories[1].weight_override = Some(100_000.0);
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
        node.coord[2] = 1000.0;
        node.story = Some(StoryId(1));
        node.restraint = Dof6Mask(0b111100);
        app.core.model.nodes.push(node);
        let mut spring = app.core.model.elements[0].clone();
        spring.id = spring_id;
        spring.kind = sepika_core::model::ElementKind::NodalSpring;
        spring.nodes = [NodeId(0), basement_node].into_iter().collect();
        spring.section = None;
        spring.spring = Some([1000.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
        app.core.model.elements.push(spring.clone());
        let mut node = app.core.model.nodes[basement_node.index()].clone();
        node.id = NodeId(app.core.model.nodes.len() as u32);
        node.coord[1] += 6000.0;
        spring.id = ElemId(app.core.model.elements.len() as u32);
        spring.nodes = [NodeId(0), node.id].into_iter().collect();
        app.core.model.nodes.push(node);
        let other = spring.nodes[1];
        app.core.model.elements.push(spring);
        app.core
            .model
            .constraints
            .push(sepika_core::model::Constraint::RigidDiaphragm {
                story: StoryId(1),
                master: basement_node,
                slaves: vec![other],
                weight: Some(100_000.0),
                ci_override: None,
            });
        let mut middle = app.core.model.nodes[basement_node.index()].clone();
        middle.id = NodeId(app.core.model.nodes.len() as u32);
        middle.coord[0] = 6000.0;
        let middle_id = middle.id;
        app.core.model.nodes.push(middle);
        app.core.model.stories[1].node_ids.push(middle_id);
        if let Some(sepika_core::model::Constraint::RigidDiaphragm { slaves, .. }) =
            app.core.model.constraints.last_mut()
        {
            slaves.push(middle_id);
        }
        app.core.model.elements[1].nodes[0] = middle_id;
        app.core.model.elements[0].nodes[0] = basement_node;
        let mut base = app.core.model.nodes[0].clone();
        base.id = NodeId(app.core.model.nodes.len() as u32);
        base.coord[1] += 6000.0;
        let other_base = base.id;
        app.core.model.nodes.push(base);
        for (bottom, top) in [
            (NodeId(0), basement_node),
            (other_base, other),
            (NodeId(1), middle_id),
        ] {
            let mut column = app.core.model.elements[0].clone();
            column.id = ElemId(app.core.model.elements.len() as u32);
            column.nodes = [bottom, top].into_iter().collect();
            app.core.model.elements.push(column);
        }
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
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
        let result = app.compute_unconfirmed_holding_capacity().unwrap().0;
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
        app.load_model(crate::app::tests::with_explicit_pushover_gravity(
            crate::sample::portal_frame(),
        ));
        app.core.analysis_cfg.threads = 1;
        app.core.analysis_cfg.push_steps = 3;
        app.core.analysis_cfg.push_use_max_disp = true;
        app.core.analysis_cfg.push_max_disp = 1.0;
        app.core.analysis_cfg.push_use_drift_angle = false;
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
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
                "上側支持抵抗=2 N".into(),
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
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
        app.core.design_rank_auto = true;
        assert!(app
            .compute_holding_capacity()
            .err()
            .unwrap()
            .contains("耐力入力"));
    }
    #[cfg(feature = "gui")]
    #[test]
    fn gui未採用のピーク表示と手動切替はds条件保留と両立する() {
        let mut app = ready();
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let po = bundle.pushover_x.as_mut().unwrap();
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
            app.compute_unconfirmed_holding_capacity()
                .unwrap()
                .0
                .stories[0]
                .qu,
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
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("Ds判定保留"))));
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "120.0")));
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
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
    }
    #[cfg(feature = "gui")]
    #[test]
    fn guiは目的別候補点を維持しても条件未確認のds値を表示しない() {
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
        let (result, _) = app.compute_unconfirmed_holding_capacity().unwrap();
        assert_eq!(app.core.scoped.ds_beta_u_by_story, vec![0.77]);
        assert_eq!(result.stories[0].ds, 0.40);
        let source = app.core.scoped.holding_capacity_source.as_ref().unwrap();
        assert_eq!(source.ds_point.step, ds_step);
        assert_eq!(source.capacity_point.step, capacity_step);
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
        assert!(!labels.contains(&"0.40"));
        assert!(labels.iter().any(|text| text.contains("Ds判定保留")));
        assert!(labels.contains(&"Ds判定"));
        assert!(labels.contains(&"保有耐力比較"));
        assert!(!labels
            .iter()
            .any(|text| text.contains("採用方向: X、Ds部材応答の採用ステップ")));
        assert!(app.core.scoped.holding_capacity_source.is_none());
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
        let po = bundle.pushover_x.as_mut().unwrap();
        po.qu = 150_000.0;
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
        let (result, _) = app.compute_unconfirmed_holding_capacity().unwrap();
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
            loaded
                .compute_unconfirmed_holding_capacity()
                .unwrap()
                .0
                .stories[0]
                .qu,
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
            origin: Default::default(),
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
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        app.adopt_holding_evaluation(EvaluationPurpose::Ds).unwrap();
        app.core.analysis_cfg.c0 = 0.3;
        app.core.analysis_cfg.push_max_disp = 2.0;
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        super::super::tests::select_holding_points(&mut app);
        app.adopt_holding_evaluation(EvaluationPurpose::HoldingCapacity)
            .unwrap();
        assert!(app.compute_unconfirmed_holding_capacity().is_ok());
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
