use crate::app::*;
use sepika_core::model::{ElementData, ElementKind, Model};
use sepika_core::section_shape::SectionShape;
use sepika_core::structure_kind::{member_structure_kind, StructureKind};
use sepika_design_jp::secondary::ds_group::{
    ds_src_frame, ds_src_wall_frame, ds_src_wall_structure, member_group, rank_index_for_group,
    rc_column_type, src_column_group, GroupType,
};
use sepika_design_jp::secondary::holding_capacity::{
    check_holding_capacity_at_state, FrameType, HoldingCapacityResult, MemberRank,
};
use sepika_design_jp::secondary::src_rank::{src_column_rank, src_wall_type};
use sepika_design_jp::MemberKind;
use sepika_solver::nonlinear::pushover::story_response::{
    ConfirmedStepResponse, EvaluationPurpose, ForceGroup, SrcFrameFaceReview, SrcWallShearReview,
    StoryForceEvaluation,
};
use sepika_solver::nonlinear::pushover::PushoverSign;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SrcFrameLayout {
    MomentFrame,
    WallStructure,
    MomentFrameWithWalls,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SrcFailure {
    Bending,
    Shear,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SrcColumnBase {
    NoUnembeddedBaseYield,
    UnembeddedBaseYield,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SrcCollapse {
    Overall,
    Partial,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SrcOtherCollapseCases {
    NoOtherCase,
    ReviewedMinimum,
}

impl SrcCollapse {
    fn label(self) -> &'static str {
        match self {
            Self::Overall => "全体崩壊形",
            Self::Partial => "部分崩壊形",
            Self::Local => "局部崩壊形",
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SrcLayerConfirmation {
    pub layer: usize,
    pub layout: Option<SrcFrameLayout>,
    pub application_basis: String,
    pub uses_collapse_step_for_ds: Option<bool>,
    pub fd_removal: Option<bool>,
    pub fd_removal_basis: String,
    pub other_cases_min_qu_kn: f64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SrcColumnConfirmation {
    pub failure: Option<SrcFailure>,
    pub failure_reached: Option<bool>,
    pub failure_basis: String,
    pub base: Option<SrcColumnBase>,
    pub n0_kn: f64,
    pub steel_m0_knm: f64,
    pub m0_knm: f64,
    pub horizontal_capacity_kn: f64,
    pub strength_basis: String,
    pub brittle_rc: Option<bool>,
    pub ds_limits: [SrcFrameFaceLimits; 2],
    pub qu_limits: [SrcFrameFaceLimits; 2],
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SrcFrameFaceLimits {
    pub axial_kn: f64,
    pub bending_strong_knm: f64,
    pub bending_weak_knm: f64,
    pub shear_strong_kn: f64,
    pub shear_weak_kn: f64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SrcGirderConfirmation {
    pub ds_limits: [SrcFrameFaceLimits; 2],
    pub qu_limits: [SrcFrameFaceLimits; 2],
    pub strength_basis: String,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SrcWallConfirmation {
    pub failure: Option<SrcFailure>,
    pub failure_basis: String,
    pub horizontal_capacity_kn: f64,
    pub strength_basis: String,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SrcCollapseOrigin {
    pub elem: Option<ElemId>,
    pub end: Option<SrcMemberEnd>,
    pub failure: Option<SrcFailure>,
    pub basis: String,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum SrcMemberEnd {
    I,
    J,
}

impl SrcMemberEnd {
    fn index(self) -> usize {
        usize::from(self == Self::J)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct SrcHoldingConfirmation {
    pub ds_run_id: String,
    pub ds_step: u32,
    pub qu_run_id: String,
    pub qu_step: u32,
    pub reviewer: String,
    pub collapse: Option<SrcCollapse>,
    pub collapse_basis: String,
    pub wall_base_overall_mechanism: bool,
    pub ds_origins: Vec<SrcCollapseOrigin>,
    pub qu_origins: Vec<SrcCollapseOrigin>,
    pub other_cases_basis: String,
    pub other_cases: Option<SrcOtherCollapseCases>,
    pub additional_conditions_basis: String,
    pub no_prior_frame_shear: Option<bool>,
    pub all_physical_members_counted: Option<bool>,
    pub layers: Vec<SrcLayerConfirmation>,
    pub columns: BTreeMap<ElemId, SrcColumnConfirmation>,
    pub walls: BTreeMap<ElemId, SrcWallConfirmation>,
    pub girders: BTreeMap<ElemId, SrcGirderConfirmation>,
    pub other_girders: BTreeMap<ElemId, String>,
}

pub(crate) struct SrcHoldingResult {
    pub result: HoldingCapacityResult,
    pub ranks: Vec<MemberRank>,
    pub source: HoldingCapacitySource,
    pub details: Vec<String>,
}

struct DirectionInputs<'a> {
    ds: &'a result_validity::HoldingEvaluationInput,
    qu: &'a result_validity::HoldingEvaluationInput,
    ds_response: &'a ConfirmedStepResponse,
    qu_response: &'a ConfirmedStepResponse,
}

impl DirectionInputs<'_> {
    fn same_point(&self) -> bool {
        self.ds.point.run_id == self.qu.point.run_id && self.ds.point.step == self.qu.point.step
    }

    fn check_prior_shear(
        &self,
        id: ElemId,
        ds: &FrameUtilization,
        qu: &FrameUtilization,
    ) -> Result<(), String> {
        if self.ds.point.run_id == self.qu.point.run_id {
            check_adoption_order(id, self.ds.point.step, ds, self.qu.point.step, qu)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct SrcHoldingGirders {
    src: BTreeSet<ElemId>,
    other: BTreeSet<ElemId>,
}

pub(crate) const SRC_HOLDING_CASES: [(SeismicDir, PushoverSign); 4] = [
    (SeismicDir::X, PushoverSign::Positive),
    (SeismicDir::X, PushoverSign::Negative),
    (SeismicDir::Y, PushoverSign::Positive),
    (SeismicDir::Y, PushoverSign::Negative),
];

pub(crate) fn case_label(direction: SeismicDir, sign: PushoverSign) -> String {
    let suffix = if sign == PushoverSign::Positive {
        "+"
    } else {
        "−"
    };
    format!("{direction:?}{suffix}")
}

#[derive(PartialEq, Eq)]
struct StoryMembers {
    columns: BTreeSet<ElemId>,
    frame_columns: BTreeSet<ElemId>,
    walls: BTreeSet<ElemId>,
}

impl App {
    pub(crate) fn has_src_holding_members(&self) -> bool {
        let model = &self.core.model;
        model.elements.iter().any(|e| {
            member_structure_kind(model, e) == StructureKind::Src
                || (e.kind == ElementKind::Wall && wall_has_src_boundary_column(e, model))
        }) || model.wall_plates.iter().any(|plate| {
            model.wall_plate_becomes_element(plate)
                && (plate.src_steel_plate.is_some() || plate.src_wall_detailing_confirmed)
        })
    }

    fn saved_src_holding_inputs(
        &self,
        direction: SeismicDir,
        sign: PushoverSign,
    ) -> Result<DirectionInputs<'_>, String> {
        let saved = &self
            .core
            .scoped
            .results
            .as_ref()
            .ok_or("増分解析結果がありません")?
            .holding_evaluations;
        let gravity_terms = sepika_core::load_combo::ultimate_gravity_terms(&self.core.model)?;
        let get = |purpose| {
            let mut matches = saved.iter().filter(|e| {
                e.point.direction == direction
                    && e.point.purpose == purpose
                    && result_validity::holding_run_sign(&e.run) == sign
            });
            let input = matches.next().ok_or_else(|| {
                format!("{direction:?}・{purpose:?} の目的別採用点を保存してください")
            })?;
            if matches.next().is_some() || !self.holding_evaluation_is_current(input) {
                return Err(format!("{direction:?}・{purpose:?} の採用点が重複しているか、現在のモデルと一致しません"));
            }
            if input.conditions.ai_mode != self.core.analysis_cfg.ai_mode
                || input.conditions.soil != self.core.analysis_cfg.soil
                || input.conditions.z != self.core.analysis_cfg.z
            {
                return Err(format!(
                    "{direction:?}・{purpose:?} のAi地震条件が現在の計算条件と一致しません"
                ));
            }
            let response = input.run.confirmed_response(&input.point, purpose)?;
            if !input
                .run
                .wall_run
                .as_ref()
                .is_some_and(|run| run.gravity_applied && run.gravity_terms == gravity_terms)
            {
                return Err(format!(
                    "{direction:?}・{purpose:?} の増分解析に長期荷重が確認できません"
                ));
            }
            Ok((input, response))
        };
        let (ds, ds_response) = get(EvaluationPurpose::Ds)?;
        let (qu, qu_response) = get(EvaluationPurpose::HoldingCapacity)?;
        Ok(DirectionInputs {
            ds,
            qu,
            ds_response,
            qu_response,
        })
    }

    pub(crate) fn prepare_src_holding_confirmation(&mut self) -> Result<(), String> {
        let direction = self.core.scoped.pushover_view_dir;
        let sign = self.core.scoped.src_holding_sign;
        let (model, _, _) = sepika_core::wall_expand::expand_wall_elements(&self.core.model);
        let girders = validate_src_holding_model(&model)?;
        let inputs = self.saved_src_holding_inputs(direction, sign)?;
        let stories = story_members(&model, inputs.ds_response)?;
        let confirmation = SrcHoldingConfirmation {
            ds_run_id: inputs.ds.point.run_id.clone(),
            ds_step: inputs.ds.point.step,
            qu_run_id: inputs.qu.point.run_id.clone(),
            qu_step: inputs.qu.point.step,
            reviewer: String::new(),
            collapse: None,
            collapse_basis: String::new(),
            wall_base_overall_mechanism: false,
            ds_origins: Vec::new(),
            qu_origins: Vec::new(),
            other_cases_basis: String::new(),
            other_cases: None,
            additional_conditions_basis: String::new(),
            no_prior_frame_shear: None,
            all_physical_members_counted: None,
            layers: (0..stories.len())
                .map(|layer| SrcLayerConfirmation {
                    layer,
                    ..Default::default()
                })
                .collect(),
            columns: stories
                .iter()
                .flat_map(|s| &s.columns)
                .map(|&id| (id, SrcColumnConfirmation::default()))
                .collect(),
            walls: stories
                .iter()
                .flat_map(|s| &s.walls)
                .map(|&id| (id, SrcWallConfirmation::default()))
                .collect(),
            girders: girders
                .src
                .into_iter()
                .map(|id| (id, SrcGirderConfirmation::default()))
                .collect(),
            other_girders: girders
                .other
                .into_iter()
                .map(|id| (id, String::new()))
                .collect(),
        };
        let ds = self
            .core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .holding_evaluations
            .iter_mut()
            .find(|e| {
                e.point.direction == direction
                    && e.point.purpose == EvaluationPurpose::Ds
                    && result_validity::holding_run_sign(&e.run) == sign
            })
            .unwrap();
        ds.src_confirmation = Some(confirmation);
        Ok(())
    }

    pub(crate) fn src_holding_results(&self) -> Result<Vec<SrcHoldingResult>, String> {
        if self.core.design_frame != FrameType::SrcFrame {
            return Err("SRC柱またはSRC耐震壁を含むモデルにはSRC用の保有水平耐力判定を選び、全評価階・X/Y両方向の適用を確認してください".into());
        }
        let (model, _, _) = sepika_core::wall_expand::expand_wall_elements(&self.core.model);
        let girders = validate_src_holding_model(&model)?;
        if self.core.model.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                sepika_core::model::Constraint::RigidDiaphragm {
                    ci_override: Some(_),
                    ..
                }
            )
        }) || self.core.analysis_cfg.ai_mode == AiMode::SemiPrecise
        {
            return Err(
                "指定Ciの副剛床または精算周期による必要耐力用地震力 Qud は未対応です".into(),
            );
        }
        let bundle = self.core.scoped.results.as_ref().ok_or("解析未実行です")?;
        for direction in [SeismicDir::X, SeismicDir::Y] {
            self.require_result_input(ResultInputKey::Static(StaticCaseKey::Seismic(direction)))?;
        }
        let gravity = bundle
            .combos
            .iter()
            .find(|(name, _)| {
                sepika_core::load_combo::is_gravity_combination(name, &self.core.model)
            })
            .ok_or("Fes算定用の長期荷重組合せ解析結果がありません")?;
        self.require_result_input(ResultInputKey::Combo(gravity.0.clone()))?;
        let period_s = self.design_seismic_period()?;
        SRC_HOLDING_CASES
            .into_iter()
            .map(|(dir, sign)| {
                self.src_holding_direction(&model, &girders, period_s, dir, sign)
                    .map_err(|e| format!("SRC保有水平耐力 {}: {e}", case_label(dir, sign)))
            })
            .collect()
    }

    fn src_holding_direction(
        &self,
        model: &Model,
        girders: &SrcHoldingGirders,
        period_s: f64,
        direction: SeismicDir,
        sign: PushoverSign,
    ) -> Result<SrcHoldingResult, String> {
        let inputs = self.saved_src_holding_inputs(direction, sign)?;
        let confirmation = inputs
            .ds
            .src_confirmation
            .as_ref()
            .ok_or("設計者による崩壊形・部材破壊形式・適用条件の確認がありません")?;
        if confirmation.ds_run_id != inputs.ds.point.run_id
            || confirmation.ds_step != inputs.ds.point.step
            || confirmation.qu_run_id != inputs.qu.point.run_id
            || confirmation.qu_step != inputs.qu.point.step
        {
            return Err("設計者確認が現在のDs・Qu採用runとstepに一致しません".into());
        }
        if confirmation.collapse.is_none()
            || !nonempty(&confirmation.reviewer)
            || !nonempty(&confirmation.collapse_basis)
            || !nonempty(&confirmation.other_cases_basis)
            || confirmation.other_cases.is_none()
            || !nonempty(&confirmation.additional_conditions_basis)
        {
            return Err("設計者名、崩壊形、同一加力方向で想定される他の崩壊形との比較、追加適用条件の確認資料が必要です".into());
        }
        if confirmation.no_prior_frame_shear != Some(true) {
            return Err(
                "各採用点までにSRC柱・梁のせん断破壊が先行していないことを確認してください".into(),
            );
        }
        let ds_members = story_members(model, inputs.ds_response)?;
        if confirmation
            .other_girders
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
            != girders.other
        {
            return Err("SRC以外の梁の確認対象が現在のモデルと一致しません".into());
        }
        for &id in &girders.other {
            if !nonempty(&confirmation.other_girders[&id])
                || [inputs.ds_response, inputs.qu_response]
                    .iter()
                    .any(|r| !r.members.iter().any(|m| m.elem == id))
            {
                return Err(format!(
                    "梁 ID {}: SRC主体の架構に含む他種梁の崩壊時応力・耐力を確認してください",
                    id.0
                ));
            }
        }
        check_girders(model, &inputs, confirmation, &girders.src)?;
        if story_members(model, inputs.qu_response)? != ds_members {
            return Err("DsとQuに採用した方向・層の物理部材が一致しません".into());
        }
        let ds_forces = inputs
            .ds
            .run
            .evaluate_stories(&inputs.ds.point, EvaluationPurpose::Ds)?;
        let capacity_forces = inputs
            .qu
            .run
            .evaluate_stories(&inputs.qu.point, EvaluationPurpose::HoldingCapacity)?;
        let wall_ids: BTreeSet<_> = model
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Wall)
            .map(|e| e.id)
            .collect();
        for (input, response) in [
            (inputs.ds, inputs.ds_response),
            (inputs.qu, inputs.qu_response),
        ] {
            let recorded_wall_ids: BTreeSet<_> = input
                .run
                .wall_run
                .as_ref()
                .unwrap()
                .src_wall_elements
                .iter()
                .copied()
                .collect();
            let reviewed_wall_ids = response
                .src_walls
                .iter()
                .map(|r| r.elem)
                .collect::<BTreeSet<_>>();
            if recorded_wall_ids != wall_ids
                || reviewed_wall_ids != wall_ids
                || response.src_walls.len() != wall_ids.len()
            {
                return Err("同じ解析runの38条照査対象がモデルのSRC耐震壁と一致しません".into());
            }
            for review in &response.src_walls {
                let elem = model
                    .element(review.elem)
                    .ok_or("38条照査対象のSRC耐震壁を確認できません")?;
                wall_bending_reached(model, elem, review)?;
            }
        }
        let qud = sepika_solver::statics::analysis::seismic_distribution_for_model(
            &self.core.model,
            sepika_solver::statics::analysis::SeismicCfg {
                dir: direction,
                mode: self.core.analysis_cfg.ai_mode,
                z: self.core.analysis_cfg.z,
                soil: self.core.analysis_cfg.soil,
                c0: 1.0,
            },
            period_s,
        )
        .map_err(|e| format!("必要耐力用地震力 Qud を算定できません: {e}"))?
        .qi;
        if qud.iter().any(|&q| !positive(q)) {
            return Err("必要耐力用地震力 Qud が非正または不正です".into());
        }
        check_ai_force_pattern(
            &ds_forces,
            &qud,
            inputs.ds.run.steps[inputs.ds.point.step as usize].load_factor,
            sign,
        )?;
        check_ai_force_pattern(
            &capacity_forces,
            &qud,
            inputs.qu.run.steps[inputs.qu.point.step as usize].load_factor,
            sign,
        )?;
        let ctx = crate::summary::metrics_ctx_from_results(
            &self.core.model,
            self.core.scoped.results.as_ref(),
        );
        let metrics = crate::summary::compute_story_metrics_with(&self.core.model, direction, &ctx);
        let fes = metrics
            .iter()
            .map(|m| m.fes.clone())
            .collect::<Result<Vec<_>, _>>()?;
        let rs = metrics
            .iter()
            .map(|m| m.rs.clone())
            .collect::<Result<Vec<_>, _>>()?;
        let re = metrics
            .iter()
            .map(|m| m.re.clone())
            .collect::<Result<Vec<_>, _>>()?;
        let heights_mm: Vec<_> = metrics.iter().map(|m| m.height).collect();
        let curve = inputs
            .qu
            .run
            .capacity_curve
            .iter()
            .find(|p| p.step == inputs.qu.point.step)
            .ok_or("Qu採用点の変形履歴がありません")?;
        let qu_n: Vec<_> = capacity_forces.iter().map(|f| f.qu_n).collect();
        let (ds, ranks, member_ranks, mut details) =
            classify_stories(model, &inputs, confirmation, &ds_members, &ds_forces)?;
        if confirmation.other_cases == Some(SrcOtherCollapseCases::ReviewedMinimum) {
            for (story, (layer, force)) in
                confirmation.layers.iter().zip(&capacity_forces).enumerate()
            {
                let minimum_n = layer.other_cases_min_qu_kn * 1000.0;
                if !positive(minimum_n) || force.qu_n.abs() > minimum_n {
                    return Err(format!("層 {}: 採用Qu={} Nは、他の有効崩壊形で確認した最小Qu={} Nを超えるか、その最小値を確認できません", story + 1, force.qu_n.abs(), minimum_n));
                }
                details.push(format!(
                    "層 {}: 他の有効崩壊形で確認した最小Qu={minimum_n} N",
                    story + 1
                ));
            }
        }
        let same_point = inputs.same_point();
        for (response, origins, use_qu_limits) in [
            (inputs.ds_response, &confirmation.ds_origins, false),
            (
                inputs.qu_response,
                if same_point {
                    &confirmation.ds_origins
                } else {
                    &confirmation.qu_origins
                },
                !same_point,
            ),
        ] {
            let mut member_ends = BTreeSet::new();
            let mut covered_layers = BTreeSet::new();
            let mut base_support_reached = false;
            for origin in origins {
                let (id, end) =
                    check_collapse_origin(model, response, confirmation, origin, use_qu_limits)?;
                if !member_ends.insert((id, end)) {
                    return Err("同じ部材端を崩壊形の構成部材として重複指定できません".into());
                }
                let layer = collapse_origin_layer(model, &ds_members, id)?;
                covered_layers.insert(layer);
                if layer == 0 {
                    let lowest = &ds_members[layer];
                    if lowest.columns.contains(&id) {
                        let elem = model.element(id).ok_or("崩壊形の構成柱がありません")?;
                        base_support_reached |= lower_column_end(elem, model)? == end.index();
                    } else if lowest.walls.contains(&id)
                        && origin.failure == Some(SrcFailure::Shear)
                    {
                        base_support_reached = true;
                    }
                }
                details.push(format!(
                    "崩壊形 部材 ID {} {:?}端 / run {} / step {}、確認資料 {}",
                    id.0, end, response.run_id, response.step, origin.basis,
                ));
            }
            if origins.is_empty() {
                return Err(
                    "採用点で耐力に到達して崩壊形を構成する部材端を指定してください".into(),
                );
            }
            if confirmation.collapse == Some(SrcCollapse::Overall) {
                check_overall_mechanism_evidence(
                    &ds_members,
                    origins,
                    &covered_layers,
                    confirmation.wall_base_overall_mechanism,
                    base_support_reached,
                )?;
            }
        }
        let result = check_holding_capacity_at_state(
            &qu_n,
            &curve.story_drift,
            &qud,
            &ds,
            &fes,
            &rs,
            &re,
            &heights_mm,
            member_ranks,
        )?;
        let source = HoldingCapacitySource {
            direction,
            qu_steps: vec![Some(inputs.qu.point.step); result.stories.len()],
            response_step: Some(inputs.ds.point.step),
            ds_point: inputs.ds.point.clone(),
            capacity_point: inputs.qu.point.clone(),
            ds_forces,
            capacity_forces,
            termination: inputs.ds.run.termination.clone(),
            capacity_termination: inputs.qu.run.termination.clone(),
            ds_mechanism: inputs.ds.run.mechanism_at(&inputs.ds.point, model)?,
            ds_conditions: inputs.ds.conditions,
            capacity_conditions: inputs.qu.conditions,
            src_both_directions_ok: None,
            src_sign: Some(sign),
        };
        Ok(SrcHoldingResult {
            result,
            ranks,
            source,
            details,
        })
    }
}

fn check_overall_mechanism_evidence(
    stories: &[StoryMembers],
    origins: &[SrcCollapseOrigin],
    covered_layers: &BTreeSet<usize>,
    wall_base: bool,
    base_support_reached: bool,
) -> Result<(), String> {
    if wall_base {
        let base_bending = stories.first().is_some_and(|base| {
            origins.iter().any(|origin| {
                origin.failure == Some(SrcFailure::Bending)
                    && origin.end == Some(SrcMemberEnd::I)
                    && origin.elem.is_some_and(|id| base.walls.contains(&id))
            })
        });
        return if base_bending {
            Ok(())
        } else {
            Err(
                "壁脚曲げによる全体崩壊形には、最下評価層の耐震壁下端の曲げ耐力到達が必要です"
                    .into(),
            )
        };
    }
    let one_wall = stories.len() == 1
        && stories[0].frame_columns.is_empty()
        && stories[0].walls.len() == 1
        && origins.len() == 1
        && origins[0].failure == Some(SrcFailure::Shear)
        && origins[0]
            .elem
            .is_some_and(|id| stories[0].walls.contains(&id));
    if covered_layers.len() != stories.len() || (origins.len() < 2 && !one_wall) {
        return Err("全体崩壊形には各評価層に対応する複数の確定部材端が必要です".into());
    }
    if !base_support_reached {
        return Err(
            "全体崩壊形には最下評価層の柱脚または耐震壁のせん断破壊の記録が必要です".into(),
        );
    }
    Ok(())
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn validate_src_holding_model(model: &Model) -> Result<SrcHoldingGirders, String> {
    let mut has_src_column = false;
    let mut girders = SrcHoldingGirders::default();
    for elem in &model.elements {
        match elem.kind {
            ElementKind::PanelZone | ElementKind::NodalSpring => continue,
            ElementKind::Wall
                if sepika_core::src_wall_capacity::resolve_src_wall(model, elem)?.is_some() => {}
            kind if kind.is_primary_frame_support() => {
                let usage = MemberKind::try_of_element(elem, model)?;
                let structure = member_structure_kind(model, elem);
                match (usage, structure) {
                    (MemberKind::Column, StructureKind::Src) => has_src_column = true,
                    (MemberKind::Girder, StructureKind::Src) => {
                        girders.src.insert(elem.id);
                    }
                    (MemberKind::Girder, StructureKind::Rc | StructureKind::S) => {
                        girders.other.insert(elem.id);
                    }
                    (MemberKind::Column, _) => return Err(format!("柱 ID {}: 物理的にSRC以外の柱を含むSRC規定で、この柱の告示部材種別を確定できません", elem.id.0)),
                    _ => return Err(format!("部材 ID {}: SRC用の規定で全体を判定する適用範囲外の部材があります", elem.id.0)),
                }
            }
            _ => {
                return Err(format!(
                    "部材 ID {}: SRC用の規定で全体を判定する適用範囲外の部材があります",
                    elem.id.0
                ))
            }
        }
    }
    if !has_src_column || (!girders.other.is_empty() && girders.src.is_empty()) {
        return Err(
            "SRC架構の主要柱梁を確認できません。架構全体がSRC用規定の適用範囲外です".into(),
        );
    }
    Ok(girders)
}

fn story_members(
    model: &Model,
    response: &ConfirmedStepResponse,
) -> Result<Vec<StoryMembers>, String> {
    let layers = model.layers();
    if layers.is_empty() || response.cuts.len() != layers.len() {
        return Err("層切断面が現在のモデルの層と一致しません".into());
    }
    let mut assigned = BTreeSet::new();
    let direction_index = usize::from(response.direction == SeismicDir::Y);
    let wall_columns = sepika_core::model::WallSideColumnIndex::build(model);
    let stories = layers
        .iter()
        .zip(&response.cuts)
        .map(|(layer, cut)| {
            let z = (layer.bottom_sl_mm + layer.top_sl_mm) * 0.5;
            if cut.layer != layer.index || cut.elevation_mm != z {
                return Err(format!(
                    "層 {}: 層切断面の位置が現在のモデルと一致しません",
                    layer.index + 1
                ));
            }
            let expected: BTreeSet<_> = model
                .elements
                .iter()
                .filter(|e| e.kind != ElementKind::PanelZone)
                .filter(|e| {
                    let elevations = e
                        .nodes
                        .iter()
                        .filter_map(|id| model.node(*id))
                        .map(|n| n.coord[2]);
                    elevations.clone().any(|v| v > z) && elevations.clone().any(|v| v < z)
                })
                .map(|e| e.id)
                .collect();
            let recorded: BTreeSet<_> = cut.forces.iter().map(|f| f.elem).collect();
            if recorded != expected || recorded.len() != cut.forces.len() {
                return Err(format!(
                    "層 {}: 層切断面の物理部材履歴がモデルと一致しません",
                    layer.index + 1
                ));
            }
            let mut columns = BTreeSet::new();
            let mut frame_columns = BTreeSet::new();
            let mut walls = BTreeSet::new();
            for force in &cut.forces {
                let elem = model
                    .element(force.elem)
                    .ok_or("切断面の部材がありません")?;
                if force.group == ForceGroup::Brace
                    || (elem.kind == ElementKind::NodalSpring
                        && force.force_n.abs() > cut.tolerance_n)
                {
                    return Err(format!(
                        "層 {}・部材 ID {}: SRC耐力負担の対象外です",
                        layer.index + 1,
                        elem.id.0
                    ));
                }
                if elem.kind == ElementKind::Wall {
                    let geometry = sepika_core::model::wall_element_geometry(elem, model)
                        .ok_or("切断面に不正な壁形状があります")?;
                    if geometry.ex_bottom[direction_index].abs() > 1e-6 {
                        if force.group != ForceGroup::Wall || !assigned.insert(elem.id) {
                            return Err(format!(
                                "壁 ID {}: 負担群または層識別が不整合です",
                                elem.id.0
                            ));
                        }
                        walls.insert(elem.id);
                    } else if force.group == ForceGroup::Wall
                        || force.force_n.abs() > cut.tolerance_n
                    {
                        return Err(format!(
                            "壁 ID {}: 直交壁の負担力を確認してください",
                            elem.id.0
                        ));
                    }
                } else if elem.kind.is_primary_frame_support() {
                    if MemberKind::try_of_element(elem, model)? != MemberKind::Column
                        || !assigned.insert(elem.id)
                    {
                        return Err(format!(
                            "部材 ID {}: 層を横切る柱の所属階を確認できません",
                            elem.id.0
                        ));
                    }
                    let is_active_wall_column = wall_columns
                        .replacement_bindings(elem.id)
                        .map_err(|_| {
                            format!(
                                "柱 ID {}: 耐震壁側柱としての負担群を確認できません",
                                elem.id.0
                            )
                        })?
                        .iter()
                        .any(|binding| {
                            model
                                .element(binding.wall)
                                .and_then(|wall| {
                                    sepika_core::model::wall_element_geometry(wall, model)
                                })
                                .is_some_and(|g| g.ex_bottom[direction_index].abs() > 1e-6)
                        });
                    if (force.group == ForceGroup::Wall) != is_active_wall_column {
                        return Err(format!(
                            "柱 ID {}: 確定層力の負担群が解析モデルと一致しません",
                            elem.id.0
                        ));
                    }
                    columns.insert(elem.id);
                    if !is_active_wall_column {
                        frame_columns.insert(elem.id);
                    }
                }
            }
            if columns.is_empty() {
                return Err(format!("層 {}: SRC柱群が存在しません", layer.index + 1));
            }
            Ok(StoryMembers {
                columns,
                frame_columns,
                walls,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let missing_columns: Vec<_> = model
        .elements
        .iter()
        .filter(|elem| {
            elem.kind.is_primary_frame_support()
                && member_structure_kind(model, elem) == StructureKind::Src
                && MemberKind::try_of_element(elem, model) == Ok(MemberKind::Column)
                && !assigned.contains(&elem.id)
        })
        .map(|elem| elem.id.0.to_string())
        .collect();
    if !missing_columns.is_empty() {
        return Err(format!(
            "SRC柱 ID {}: 層切断面を横断せず、判定対象となる所属層を一意に確定できません",
            missing_columns.join("、")
        ));
    }
    Ok(stories)
}

fn check_ai_force_pattern(
    forces: &[StoryForceEvaluation],
    qud_n: &[f64],
    load_factor: f64,
    sign: PushoverSign,
) -> Result<(), String> {
    if forces.is_empty() || forces.len() != qud_n.len() {
        return Err("Ai外力分布の層数が一致しません".into());
    }
    let scale = forces[0].reference_n / qud_n[0];
    let expected_sign = if sign == PushoverSign::Positive {
        1.0
    } else {
        -1.0
    };
    if !positive(load_factor)
        || !positive(scale * expected_sign)
        || forces.iter().zip(qud_n).any(|(f, q)| {
            f.qu_n * expected_sign <= f.tolerance_n
                || !positive(f.reference_n * expected_sign)
                || (f.reference_n - q * scale).abs() * load_factor > f.tolerance_n
                || (f.external_n - f.reference_n * load_factor).abs() > f.tolerance_n
        })
    {
        return Err("採用点の層外力が加力符号または告示のAi分布と一致しません".into());
    }
    Ok(())
}

type ClassifiedStories = (
    Vec<f64>,
    Vec<MemberRank>,
    Vec<(ElemId, MemberRank)>,
    Vec<String>,
);

fn classify_stories(
    model: &Model,
    inputs: &DirectionInputs<'_>,
    confirmation: &SrcHoldingConfirmation,
    story_members: &[StoryMembers],
    ds_forces: &[StoryForceEvaluation],
) -> Result<ClassifiedStories, String> {
    let expected_columns: BTreeSet<_> = story_members
        .iter()
        .flat_map(|s| s.columns.iter().copied())
        .collect();
    let expected_walls: BTreeSet<_> = story_members
        .iter()
        .flat_map(|s| s.walls.iter().copied())
        .collect();
    if confirmation
        .columns
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        != expected_columns
        || confirmation.walls.keys().copied().collect::<BTreeSet<_>>() != expected_walls
        || confirmation.layers.len() != story_members.len()
    {
        return Err("設計者が確認した層・柱・壁が現在の解析の対象部材と一致しません".into());
    }
    if confirmation.all_physical_members_counted != Some(true) {
        return Err("DsとQuを同じ解析モデルから採る条件を確認してください。脆性部材等を除外する場合は別途評価が必要です".into());
    }
    let mut values = Vec::new();
    let mut ranks = Vec::new();
    let mut members = Vec::new();
    let mut details = vec![format!(
        "設計者 {}、{}、崩壊形の確認資料 {}、比較資料 {}、追加条件の確認資料 {}",
        confirmation.reviewer,
        confirmation.collapse.unwrap().label(),
        confirmation.collapse_basis,
        confirmation.other_cases_basis,
        confirmation.additional_conditions_basis,
    )];
    for (i, (story, proof)) in story_members.iter().zip(&confirmation.layers).enumerate() {
        let wall_ids = &story.walls;
        if proof.layer != i || !nonempty(&proof.application_basis) || proof.layout.is_none() {
            return Err(format!(
                "層 {}: SRC規定の適用、架構形式、確認資料が必要です",
                i + 1
            ));
        }
        if proof.uses_collapse_step_for_ds != Some(true) {
            return Err(format!("層 {}: 採用した崩壊ステップでDs用応力を確定できません。非崩壊階の別途Ds応力を要する場合は保留します", i + 1));
        }
        let force = &ds_forces[i];
        let mut columns = Vec::new();
        let mut walls = Vec::new();
        let mut column_ranks = Vec::new();
        for &id in &story.columns {
            let elem = model.element(id).ok_or("SRC柱を解決できません")?;
            let evidence = &confirmation.columns[&id];
            let ds_faces = frame_faces(&inputs.ds_response.src_frame_faces, id)?;
            let qu_faces = frame_faces(&inputs.qu_response.src_frame_faces, id)?;
            let checked = review_frame_faces(elem, &ds_faces, &evidence.ds_limits)?;
            let qu_checked = review_frame_faces(
                elem,
                &qu_faces,
                if inputs.same_point() {
                    &evidence.ds_limits
                } else {
                    &evidence.qu_limits
                },
            )?;
            inputs.check_prior_shear(id, &checked, &qu_checked)?;
            let rank = classify_column(elem, model, &ds_faces, checked, evidence)?;
            if !story.frame_columns.is_empty() {
                let direction_index = usize::from(inputs.ds.point.direction == SeismicDir::Y);
                let position = column_direction_components(elem, model, direction_index)?;
                let maximum_n = horizontal_column_limit(position, &evidence.ds_limits);
                let cut_force_n = inputs.ds_response.cuts[i]
                    .forces
                    .iter()
                    .find(|f| f.elem == id)
                    .ok_or_else(|| format!("柱 ID {}: 層切断面の確定負担力がありません", id.0))?
                    .force_n
                    .abs();
                let transferred_n = ds_faces
                    .iter()
                    .map(|f| (f[0] * position[0] - f[1] * position[1] - f[2] * position[2]).abs())
                    .fold(cut_force_n, f64::max);
                if !positive(evidence.horizontal_capacity_kn * 1000.0)
                    || evidence.horizontal_capacity_kn * 1000.0 > maximum_n
                {
                    return Err(format!(
                        "柱 ID {}: 入力した群判定用水平耐力が確定端の耐力と整合しません",
                        id.0
                    ));
                }
                if evidence.horizontal_capacity_kn * 1000.0 + force.tolerance_n < transferred_n {
                    return Err(format!("柱 ID {}: 入力した群判定用水平耐力が確定端・切断面で伝達している水平力を下回ります", id.0));
                }
                columns.push((rank, evidence.horizontal_capacity_kn * 1000.0));
            }
            column_ranks.push(rank);
            members.push((id, rank));
            details.push(format!(
                "柱 ID {}: {:?}、N={:.3} kN、破壊確認 {}、耐力根拠 {}",
                id.0,
                rank,
                column_compression_n(&ds_faces) / 1000.0,
                evidence.failure_basis,
                evidence.strength_basis
            ));
        }
        let includes_fd = column_ranks.contains(&MemberRank::FD);
        let local_failure = if includes_fd {
            proof
                .fd_removal
                .filter(|_| nonempty(&proof.fd_removal_basis))
                .ok_or_else(|| {
                    format!(
                        "層 {}: FD柱除去による局部崩壊の有無と根拠を確認してください",
                        i + 1
                    )
                })?
        } else {
            if proof.fd_removal == Some(true) {
                return Err(format!(
                    "層 {}: FD柱がない階のFD柱除去判定が不整合です",
                    i + 1
                ));
            }
            false
        };
        let cb_group = if local_failure {
            (!columns.is_empty()).then_some(GroupType::D)
        } else {
            src_column_group(&columns, false)
        };
        if includes_fd {
            details.push(format!(
                "層 {}: FD柱除去による局部崩壊 {}、確認資料 {}",
                i + 1,
                if local_failure { "あり" } else { "なし" },
                proof.fd_removal_basis,
            ));
        }
        for &id in wall_ids {
            let evidence = &confirmation.walls[&id];
            let failure = evidence
                .failure
                .filter(|_| nonempty(&evidence.failure_basis))
                .ok_or_else(|| {
                    format!(
                        "壁 ID {}: 採用崩壊形の破壊形式と根拠を確認してください",
                        id.0
                    )
                })?;
            let capacity_n = evidence.horizontal_capacity_kn * 1000.0;
            if !positive(capacity_n) || !nonempty(&evidence.strength_basis) {
                return Err(format!(
                    "壁 ID {}: 群判定用の水平耐力と根拠を確認してください",
                    id.0
                ));
            }
            let wall = inputs
                .ds
                .run
                .wall_response_at(id, inputs.ds.point.step)
                .map_err(|e| format!("壁 ID {}: {}", id.0, e.description()))?;
            inputs
                .qu
                .run
                .wall_response_at(id, inputs.qu.point.step)
                .map_err(|e| format!("壁 ID {}: {}", id.0, e.description()))?;
            let same_state = inputs
                .ds_response
                .src_walls
                .iter()
                .find(|review| review.elem == id)
                .ok_or_else(|| format!("壁 ID {}: 採用stepの38条照査記録がありません", id.0))?;
            let elem = model
                .element(id)
                .ok_or("38条照査対象のSRC耐震壁を確認できません")?;
            let geometry = sepika_core::model::wall_element_geometry(elem, model)
                .ok_or("耐力壁の負担方向を確認できません")?;
            let direction_index = usize::from(inputs.ds.point.direction == SeismicDir::Y);
            check_wall_failure(
                model,
                elem,
                same_state,
                failure,
                capacity_n,
                geometry.ex_bottom[direction_index].abs(),
                force.tolerance_n,
            )?;
            let rank = src_wall_type(failure == SrcFailure::Shear);
            let wall_label = if failure == SrcFailure::Shear {
                "WC"
            } else {
                "WA"
            };
            details.push(format!("壁要素 ID {}: {wall_label}、Qw={:.3} kN、群判定用耐力={:.3} kN、38条照査 |Q|={:?} N / Qu={:?} N、破壊確認 {}、耐力根拠 {}", id.0, wall.qw_n / 1000.0, evidence.horizontal_capacity_kn, same_state.demand_n, same_state.limit_n, evidence.failure_basis, evidence.strength_basis));
            walls.push((rank_index_for_group(rank), capacity_n));
            members.push((id, rank));
        }
        let wall_total_n: f64 = walls.iter().map(|(_, q)| q).sum();
        if wall_total_n + force.tolerance_n < force.wall_n.abs() {
            return Err(format!(
                "層 {}: 入力した壁水平耐力の和が確定層切断面の壁負担力を下回ります",
                i + 1
            ));
        }
        let wall_group = if local_failure {
            (!walls.is_empty()).then_some(GroupType::D)
        } else {
            wall_total_n
                .is_finite()
                .then(|| member_group(&walls))
                .flatten()
        };
        let ds = match proof.layout {
            Some(SrcFrameLayout::MomentFrame) if wall_ids.is_empty() && force.beta_u == 0.0 => {
                cb_group.map(ds_src_frame)
            }
            Some(SrcFrameLayout::WallStructure)
                if story.frame_columns.is_empty() && !wall_ids.is_empty() && force.beta_u > 0.0 =>
            {
                wall_group.map(ds_src_wall_structure)
            }
            Some(SrcFrameLayout::MomentFrameWithWalls) if !wall_ids.is_empty() => cb_group
                .zip(wall_group)
                .and_then(|(cb, wall)| ds_src_wall_frame(wall, force.beta_u, cb)),
            _ => None,
        }
        .ok_or_else(|| {
            format!(
                "層 {}: SRC架構形式、柱・壁の群またはβuが適用表と一致しません",
                i + 1
            )
        })?;
        let rank = column_ranks
            .into_iter()
            .max_by_key(|rank| rank_index_for_group(*rank))
            .unwrap();
        details.push(format!(
            "層 {}: 柱群 {:?}、壁群 {:?}、βu={:.6}、Ds={ds:.2}、SRC規定の根拠 {}",
            i + 1,
            cb_group,
            wall_group,
            force.beta_u,
            proof.application_basis
        ));
        ranks.push(rank);
        values.push(ds);
    }
    Ok((values, ranks, members, details))
}

fn frame_faces(records: &[SrcFrameFaceReview], id: ElemId) -> Result<[[f64; 6]; 2], String> {
    let mut found = records.iter().filter(|r| r.elem == id);
    let missing = || {
        format!(
            "部材 ID {}: 同一確定stepの両端断面力を確認してください",
            id.0
        )
    };
    let record = found
        .next()
        .filter(|_| found.next().is_none())
        .ok_or_else(missing)?;
    let resolve = |face: &Result<[f64; 6], String>| {
        face.clone()
            .map_err(|why| format!("部材 ID {}: {why}", id.0))
    };
    Ok([resolve(&record.faces[0])?, resolve(&record.faces[1])?])
}

fn check_girders(
    model: &Model,
    inputs: &DirectionInputs<'_>,
    confirmation: &SrcHoldingConfirmation,
    expected: &BTreeSet<ElemId>,
) -> Result<(), String> {
    if confirmation
        .girders
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        != *expected
    {
        return Err("確認したSRC梁の一覧が現在のモデルと一致しません".into());
    }
    for (&id, proof) in &confirmation.girders {
        let elem = model.element(id).ok_or("SRC梁を解決できません")?;
        if !nonempty(&proof.strength_basis) {
            return Err(format!(
                "SRC梁 ID {}: 各採用点の両端耐力と確認根拠が必要です",
                id.0
            ));
        }
        let ds = frame_faces(&inputs.ds_response.src_frame_faces, id)?;
        let qu = frame_faces(&inputs.qu_response.src_frame_faces, id)?;
        let ds_checked = review_frame_faces(elem, &ds, &proof.ds_limits)?;
        let qu_checked = review_frame_faces(
            elem,
            &qu,
            if inputs.same_point() {
                &proof.ds_limits
            } else {
                &proof.qu_limits
            },
        )?;
        inputs.check_prior_shear(id, &ds_checked, &qu_checked)?;
    }
    Ok(())
}

fn check_adoption_order(
    id: ElemId,
    ds_step: u32,
    ds: &FrameUtilization,
    qu_step: u32,
    qu: &FrameUtilization,
) -> Result<(), String> {
    let (earlier_step, earlier) = if ds_step < qu_step {
        (ds_step, ds)
    } else {
        (qu_step, qu)
    };
    if ds_step != qu_step && at_capacity(earlier.shear, 1.0) {
        return Err(format!(
            "部材 ID {}: 同一runのstep {} でせん断耐力に到達したため、後続の採用点を使用できません",
            id.0, earlier_step
        ));
    }
    Ok(())
}

#[derive(Default)]
struct FrameUtilization {
    bending: f64,
    shear: f64,
}

fn review_frame_faces(
    elem: &ElementData,
    faces: &[[f64; 6]; 2],
    limits: &[SrcFrameFaceLimits; 2],
) -> Result<FrameUtilization, String> {
    let i = review_frame_face(elem, 0, &faces[0], &limits[0])?;
    let j = review_frame_face(elem, 1, &faces[1], &limits[1])?;
    Ok(FrameUtilization {
        bending: i.bending.max(j.bending),
        shear: i.shear.max(j.shear),
    })
}

fn review_frame_face(
    elem: &ElementData,
    end: usize,
    face: &[f64; 6],
    limit: &SrcFrameFaceLimits,
) -> Result<FrameUtilization, String> {
    let location = if end == 0 { "i" } else { "j" };
    let capacity = [
        limit.axial_kn * 1000.0,
        limit.shear_strong_kn * 1000.0,
        limit.shear_weak_kn * 1000.0,
        limit.bending_weak_knm * 1.0e6,
        limit.bending_strong_knm * 1.0e6,
    ];
    let demand = [face[0], face[1], face[2], face[4], face[5]].map(f64::abs);
    if capacity.iter().any(|&v| !positive(v)) || demand.iter().any(|v| !v.is_finite()) {
        return Err(format!("部材 ID {} {location}端: 確定軸力・曲げ・せん断と、その組合せに対する方向別耐力が必要です", elem.id.0));
    }
    let [n, qy, qz, my, mz] = std::array::from_fn(|index| demand[index] / capacity[index]);
    if [n, qy, qz, my, mz].into_iter().any(|v| v > 1.0) {
        return Err(format!("部材 ID {} {location}端: 確定軸力・曲げ・せん断が入力した同時作用下の耐力を超えています", elem.id.0));
    }
    Ok(FrameUtilization {
        bending: my.max(mz),
        shear: qy.max(qz),
    })
}

fn column_compression_n(faces: &[[f64; 6]; 2]) -> f64 {
    (-faces[0][0]).max(-faces[1][0]).max(0.0)
}

fn column_direction_components(
    elem: &ElementData,
    model: &Model,
    direction_index: usize,
) -> Result<[f64; 3], String> {
    let ends = frame_axis_ends(elem, model)?;
    let frame = sepika_element::transform::LocalFrame::from_nodes(
        ends[0],
        ends[1],
        elem.local_axis.ref_vector,
    )?;
    Ok(frame.rot.map(|r| r[direction_index]))
}

fn frame_axis_ends(elem: &ElementData, model: &Model) -> Result<[[f64; 3]; 2], String> {
    if let Some(ends) = model.primary_frame_axis_ends(elem)? {
        return Ok(ends);
    }
    let [i, j] = elem.nodes.as_slice() else {
        return Err(format!("部材 ID {} の部材軸を解決できません", elem.id.0));
    };
    let Some((i, j)) = model.node(*i).zip(model.node(*j)) else {
        return Err(format!("部材 ID {} の部材軸を解決できません", elem.id.0));
    };
    Ok([i.coord, j.coord])
}

fn horizontal_column_limit(components: [f64; 3], limits: &[SrcFrameFaceLimits; 2]) -> f64 {
    let component = components.map(f64::abs);
    limits
        .iter()
        .map(|limit| {
            1000.0
                * (component[0] * limit.axial_kn
                    + component[1] * limit.shear_strong_kn
                    + component[2] * limit.shear_weak_kn)
        })
        .fold(0.0, f64::max)
}

fn at_capacity(demand: f64, capacity: f64) -> bool {
    positive(capacity)
        && demand <= capacity
        && capacity - demand <= sepika_solver::common::newton::STATIC_NEWTON.tol * capacity
}

fn wall_bending_reached(
    model: &Model,
    elem: &ElementData,
    review: &SrcWallShearReview,
) -> Result<bool, String> {
    Ok(wall_bending_state(model, elem, review)?.contains(&true))
}

fn wall_bending_state(
    model: &Model,
    elem: &ElementData,
    review: &SrcWallShearReview,
) -> Result<[bool; 2], String> {
    let prefix = |msg| format!("SRC耐震壁 ID {}: {msg}", elem.id.0);
    let section = sepika_core::src_wall_capacity::resolve_src_wall(model, elem)?
        .ok_or_else(|| prefix("38条の曲げ耐力が未確認です".to_string()))?;
    let ends = review
        .ends
        .ok_or_else(|| prefix("38条の軸力・曲げ応力が未確認です".to_string()))?;
    let mut reached = [false; 2];
    for (i, end) in ends.into_iter().enumerate() {
        let capacity_nmm = section.ultimate_moment_nmm(end).map_err(prefix)?;
        let demand_nmm = end.moment_nmm.abs();
        if !positive(capacity_nmm) || demand_nmm > capacity_nmm {
            return Err(prefix(format!("同一確定stepの曲げ応力が38条曲げ耐力を超過または曲げ耐力を確定できません（M={demand_nmm} N·mm、Mu={capacity_nmm} N·mm）")));
        }
        reached[i] = at_capacity(demand_nmm, capacity_nmm);
    }
    Ok(reached)
}

fn check_wall_failure(
    model: &Model,
    elem: &ElementData,
    review: &SrcWallShearReview,
    failure: SrcFailure,
    horizontal_capacity_n: f64,
    projection: f64,
    tolerance_n: f64,
) -> Result<(), String> {
    let reason = |msg| format!("SRC耐震壁 ID {}: {msg}", elem.id.0);
    let (Some(demand_n), Some(shear_capacity_n)) = (review.demand_n, review.limit_n) else {
        return Err(reason("採用stepの38条せん断需要・耐力を確認できません"));
    };
    if !positive(shear_capacity_n) || horizontal_capacity_n > shear_capacity_n * projection {
        return Err(reason(
            "入力した群判定用の壁水平耐力が38条せん断耐力の方向成分を超えています",
        ));
    }
    if horizontal_capacity_n + tolerance_n < demand_n * projection {
        return Err(reason(
            "入力した群判定用水平耐力が確定端の壁負担力を下回ります",
        ));
    }
    let at_shear = review
        .reached_limit()
        .map_err(|e| format!("SRC耐震壁 ID {}: {e}", elem.id.0))?;
    let at_bending = wall_bending_reached(model, elem, review)?;
    if wall_failure_matches(failure, at_shear, at_bending) {
        Ok(())
    } else {
        Err(reason(
            "宣言した破壊形式が採用stepの38条せん断・曲げ耐力への到達状況と整合しません",
        ))
    }
}

fn wall_failure_matches(failure: SrcFailure, shear: bool, bending: bool) -> bool {
    match failure {
        SrcFailure::Shear => shear,
        SrcFailure::Bending => bending && !shear,
        SrcFailure::Other => !shear && !bending,
    }
}

fn check_collapse_origin(
    model: &Model,
    response: &ConfirmedStepResponse,
    confirmation: &SrcHoldingConfirmation,
    origin: &SrcCollapseOrigin,
    use_qu_limits: bool,
) -> Result<(ElemId, SrcMemberEnd), String> {
    let missing = "採用点で耐力に達した崩壊形の構成部材・端・形式・確認資料が必要です";
    let id = origin.elem.ok_or(missing)?;
    let end = origin.end.ok_or(missing)?;
    let index = end.index();
    let mode = origin
        .failure
        .filter(|f| *f != SrcFailure::Other && nonempty(&origin.basis))
        .ok_or(missing)?;
    let elem = model.element(id).ok_or(missing)?;
    let reached = if let Some(proof) = confirmation.columns.get(&id) {
        let faces = frame_faces(&response.src_frame_faces, id)?;
        let limits = if use_qu_limits {
            &proof.qu_limits
        } else {
            &proof.ds_limits
        };
        (use_qu_limits || proof.failure == Some(mode))
            && frame_failure_reached(
                mode,
                review_frame_face(elem, index, &faces[index], &limits[index])?,
            )
    } else if let Some(proof) = confirmation.walls.get(&id) {
        let review = response
            .src_walls
            .iter()
            .find(|review| review.elem == id)
            .ok_or(missing)?;
        let shear_any = review
            .reached_limit()
            .map_err(|e| format!("SRC耐震壁 ID {}: {e}", id.0))?;
        let shear_here = shear_any
            && review
                .ends
                .zip(review.limit_n)
                .is_some_and(|(ends, capacity)| {
                    let tolerance = sepika_solver::common::newton::STATIC_NEWTON.tol * capacity;
                    (ends[index].shear_n.abs() - capacity).abs() <= tolerance
                });
        let bending_here = wall_bending_state(model, elem, review)?[index];
        let matches_mode = use_qu_limits || proof.failure == Some(mode);
        matches_mode
            && match mode {
                SrcFailure::Bending => bending_here && !shear_any,
                SrcFailure::Shear => shear_here,
                SrcFailure::Other => false,
            }
    } else if let Some(proof) = confirmation.girders.get(&id) {
        let faces = frame_faces(&response.src_frame_faces, id)?;
        let limits = if use_qu_limits {
            &proof.qu_limits
        } else {
            &proof.ds_limits
        };
        frame_failure_reached(
            mode,
            review_frame_face(elem, index, &faces[index], &limits[index])?,
        )
    } else {
        return Err(format!(
            "崩壊形の構成部材 ID {} は確認対象に含まれません",
            id.0
        ));
    };
    if !reached {
        return Err(format!("部材 ID {} {end:?}端 step {}: 崩壊形の構成部材として宣言した曲げ・せん断耐力に達していません", id.0, response.step));
    }
    Ok((id, end))
}

fn collapse_origin_layer(
    model: &Model,
    stories: &[StoryMembers],
    id: ElemId,
) -> Result<usize, String> {
    if let Some(index) = stories
        .iter()
        .position(|story| story.columns.contains(&id) || story.walls.contains(&id))
    {
        return Ok(index);
    }
    let missing = || {
        format!(
            "部材 ID {}: 崩壊形の構成部材が属する層を確認できません",
            id.0
        )
    };
    let elem = model.element(id).ok_or_else(missing)?;
    let [i, j] = elem.nodes.as_slice() else {
        return Err(missing());
    };
    let story = model
        .node(*i)
        .and_then(|n| n.story)
        .filter(|s| model.node(*j).is_some_and(|n| n.story == Some(*s)))
        .ok_or_else(missing)?;
    let layers = model.layers();
    layers
        .iter()
        .find(|layer| layer.top == story)
        .or_else(|| layers.first().filter(|layer| layer.bottom == story))
        .map(|layer| layer.index)
        .ok_or_else(missing)
}

fn frame_failure_reached(mode: SrcFailure, utilization: FrameUtilization) -> bool {
    let bending = at_capacity(utilization.bending, 1.0);
    let shear = at_capacity(utilization.shear, 1.0);
    match mode {
        SrcFailure::Bending => bending && !shear,
        SrcFailure::Shear => shear && !bending,
        SrcFailure::Other => false,
    }
}

fn classify_column(
    elem: &ElementData,
    model: &Model,
    faces: &[[f64; 6]; 2],
    utilization: FrameUtilization,
    evidence: &SrcColumnConfirmation,
) -> Result<MemberRank, String> {
    let missing = |reason: &str| format!("柱 ID {}: {reason}", elem.id.0);
    let failure = evidence
        .failure
        .filter(|f| {
            matches!(f, SrcFailure::Bending | SrcFailure::Shear)
                && nonempty(&evidence.failure_basis)
        })
        .ok_or_else(|| missing("崩壊形の曲げ・せん断破壊と確認資料が必要です"))?;
    let (governing, other) = if failure == SrcFailure::Bending {
        (utilization.bending, utilization.shear)
    } else {
        (utilization.shear, utilization.bending)
    };
    let at_governing = at_capacity(governing, 1.0);
    let at_other = at_capacity(other, 1.0);
    if evidence.failure_reached != Some(at_governing) || at_other {
        return Err(missing(
            "宣言した破壊形式・到達状況が確定断面力と同時作用下耐力に整合しません",
        ));
    }
    let base = evidence
        .base
        .ok_or_else(|| missing("非埋込み柱脚の曲げ降伏の有無を確認してください"))?;
    if !nonempty(&evidence.strength_basis) {
        return Err(missing(
            "確定端の同時作用下耐力とその根拠を確認してください",
        ));
    }
    if base == SrcColumnBase::UnembeddedBaseYield {
        check_unembedded_base_bending(elem, model, faces, &evidence.ds_limits)?;
        return rc_rank_for_unembedded_base(elem, model, faces, evidence);
    }
    if [
        evidence.n0_kn * 1000.0,
        evidence.steel_m0_knm,
        evidence.m0_knm,
    ]
    .iter()
    .any(|&q| !positive(q))
        || evidence.steel_m0_knm > evidence.m0_knm
    {
        return Err(missing(
            "告示用の材料強度に基づく N0・sM0・M0 が不足または不正です",
        ));
    }
    let n_ratio = column_compression_n(faces) / (evidence.n0_kn * 1000.0);
    let m_ratio = evidence.steel_m0_knm / evidence.m0_knm;
    if !n_ratio.is_finite() || !m_ratio.is_finite() {
        return Err(missing("告示用耐力比が非有限です"));
    }
    if n_ratio > 1.0 {
        return Err(missing(
            "採用stepの圧縮力が確認した圧縮耐力N0を超えています",
        ));
    }
    Ok(src_column_rank(
        n_ratio,
        m_ratio,
        failure == SrcFailure::Shear,
    ))
}

fn check_unembedded_base_bending(
    elem: &ElementData,
    model: &Model,
    faces: &[[f64; 6]; 2],
    limits: &[SrcFrameFaceLimits; 2],
) -> Result<(), String> {
    let footing = lower_column_end(elem, model)?;
    let checked = review_frame_face(elem, footing, &faces[footing], &limits[footing])?;
    if !at_capacity(checked.bending, 1.0) {
        return Err(format!(
            "柱 ID {}: 非埋込み柱脚とした下側の物理フェースが曲げ耐力に達していません",
            elem.id.0
        ));
    }
    Ok(())
}

fn lower_column_end(elem: &ElementData, model: &Model) -> Result<usize, String> {
    let axis = frame_axis_ends(elem, model)?;
    if axis[0][2] == axis[1][2] {
        return Err(format!(
            "柱 ID {}: 柱脚の物理フェースを確定できません",
            elem.id.0
        ));
    }
    Ok(usize::from(axis[0][2] > axis[1][2]))
}

fn rc_rank_for_unembedded_base(
    elem: &ElementData,
    model: &Model,
    faces: &[[f64; 6]; 2],
    evidence: &SrcColumnConfirmation,
) -> Result<MemberRank, String> {
    use sepika_core::rc_rebar_geom::RectEdge;
    let fail = || {
        format!("柱 ID {}: 非埋込み柱脚のRC柱扱いに必要な断面・Fc・塑性変形時の急激な耐力低下の確認がありません", elem.id.0)
    };
    let brittle = evidence.brittle_rc.ok_or_else(fail)?;
    let Some(SectionShape::SrcColumnRect { b, d, rebar, .. }) =
        model.element_section(elem).and_then(|s| s.shape.as_ref())
    else {
        return Err(fail());
    };
    if rebar.is_unset() || rebar.validate(*b, *d).is_err() {
        return Err(fail());
    }
    let fc = model
        .element_material(elem)
        .and_then(|m| m.fc)
        .filter(|&v| positive(v))
        .ok_or_else(fail)?;
    let clear_span_mm = sepika_design_jp::rc::rc_member_clear_span(elem, model)
        .map_err(|e| format!("柱 ID {}: {e}", elem.id.0))?;
    let area_mm2 = b * d;
    let stress = area_mm2 * fc;
    if !positive(stress) {
        return Err(fail());
    }
    let axis_rank = |width_mm, depth_mm, edge, shear_component: usize| {
        let tension = rebar.edge_steel(edge, *b, *d);
        let shear_n = f64::abs(faces[0][shear_component]).max(faces[1][shear_component].abs());
        rc_column_type(
            clear_span_mm / depth_mm,
            column_compression_n(faces) / stress,
            100.0 * tension.area_mm2 / (width_mm * tension.effective_depth_mm),
            shear_n.abs() / stress,
            brittle,
        )
    };
    let ranks = [
        axis_rank(*b, *d, RectEdge::Top, 1),
        axis_rank(*d, *b, RectEdge::Left, 2),
    ];
    Ok(sepika_design_jp::secondary::member_rank::worst_rank(&ranks).unwrap())
}
