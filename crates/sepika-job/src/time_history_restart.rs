//! 線形時刻歴の確定状態と全区間結果の保存・再開。

use crate::{AnalysisSettings, JobError, JobResult};
use sepika_core::{dof::DofMap, model::Model};
use sepika_solver::{
    common::constraint::Reducer,
    dynamic::timehistory::{
        linear_time_history_from_state, linear_time_history_until, GroundMotion, NewmarkCfg,
        ResponseResult, StoryResponse, TimeStepState,
    },
    statics::analysis::Analysis,
};
use sha2::{Digest, Sha256};

const MAGIC: &[u8] = b"SEPIKA-LINEAR-RESTART\x01";

/// 確定状態と開始からの結果。保存波形は全区間の未加工入力 [mm/s²]。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct LinearTimeHistoryCheckpoint {
    contract: u32,
    model: Model,
    cfg: AnalysisSettings,
    wave: GroundMotion,
    mapping: Vec<usize>,
    transform: Vec<Vec<(usize, f64)>>,
    damping: Vec<u8>,
    state: TimeStepState,
    result: ResponseResult,
}

fn invalid(message: &str) -> JobError {
    JobError::InvalidInput(message.into())
}

fn encode(value: &impl serde::Serialize) -> JobResult<Vec<u8>> {
    bincode::serialize(value).map_err(|_| invalid("再開状態を保存できません。"))
}

impl LinearTimeHistoryCheckpoint {
    /// 先頭からの確定済み積分数と時刻 [s] を返す。
    pub fn progress(&self) -> (u64, f64) {
        (self.state.step, self.state.time)
    }

    /// 保存された全波形を返す。再開操作では現設定と照合する。
    pub fn wave(&self) -> GroundMotion {
        self.wave.clone()
    }

    /// 識別子と破損検査値を付けた専用形式で保存する。
    pub fn to_bytes(&self) -> JobResult<Vec<u8>> {
        self.validate()?;
        let payload =
            rmp_serde::to_vec_named(self).map_err(|_| invalid("再開状態を保存できません。"))?;
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&Sha256::digest(&payload));
        bytes.extend(payload);
        Ok(bytes)
    }

    /// 専用形式と破損検査値・計算契約を検査する。不一致は読込を拒否する。
    pub fn from_bytes(bytes: &[u8]) -> JobResult<Self> {
        let header = MAGIC.len() + 32;
        if bytes.len() < header || !bytes.starts_with(MAGIC) {
            return Err(invalid("線形時刻歴の再開状態ファイルではありません。"));
        }
        let payload = &bytes[header..];
        if Sha256::digest(payload)[..] != bytes[MAGIC.len()..header] {
            return Err(invalid("再開状態ファイルが破損しています。"));
        }
        let checkpoint: Self = rmp_serde::from_slice(payload)
            .map_err(|_| invalid("再開状態の保存形式が不正です。"))?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    fn validate(&self) -> JobResult<()> {
        if self.contract != sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION {
            return Err(invalid(
                "再開状態の計算契約版が一致しません。再計算してください。",
            ));
        }
        let n = self.wave.accel_x.len();
        if n == 0 || self.state.step >= n as u64 {
            return Err(invalid("再開ステップが波形の範囲外です。"));
        }
        if self.state.step == (n - 1) as u64 {
            return Err(invalid("最終ステップのため再開する区間がありません。"));
        }
        let step = self.state.step as usize;
        let dimension = self.state.disp_red.len();
        if self.cfg.th_nonlinear
            || self.result.nonlinear
            || dimension == 0
            || self.state.vel_red.len() != dimension
            || self.state.accel_red.len() != dimension
            || self.mapping.len() != self.transform.len()
            || self
                .transform
                .iter()
                .flatten()
                .any(|&(i, v)| i >= dimension || !v.is_finite())
            || !self.wave.dt.is_finite()
            || self.wave.dt <= 0.0
            || self.state.time != step as f64 * self.wave.dt
            || self
                .state
                .disp_red
                .iter()
                .chain(&self.state.vel_red)
                .chain(&self.state.accel_red)
                .any(|v| !v.is_finite())
            || self.result.time.len() != step + 1
            || self
                .result
                .time
                .iter()
                .enumerate()
                .any(|(i, &t)| t != i as f64 * self.wave.dt)
            || self.result.history.node_disp.len() != step + 1
            || self.result.history.base_shear.len() != step + 1
            || self.result.history.top_drift_angle.len() != step + 1
            || self.result.peak_disp.len() != self.model.nodes.len()
            || self.result.story_drift_angle.len() != self.model.layer_count()
            || self.result.cumulative_ductility.len() != self.model.elements.len()
            || self.result.recording.is_none()
        {
            return Err(invalid("再開状態の時刻・応答・前半記録が不正です。"));
        }
        let recording = self.result.recording.as_ref().unwrap();
        let every = if self.cfg.th_record_every == 0 {
            ((n - 1) / 1000).max(1)
        } else {
            self.cfg.th_record_every
        };
        let times: Vec<_> = (0..=step)
            .step_by(every)
            .map(|s| s as f64 * self.wave.dt)
            .collect();
        let frames = times.len();
        let valid_forces = |forces: &[Option<sepika_element::frame::beam::MemberForces>]| {
            forces.len() == self.model.elements.len()
                && forces.iter().flatten().all(|f| {
                    !f.at.is_empty()
                        && f.at.iter().all(|(x, v)| {
                            (0.0..=1.0).contains(x) && v.iter().all(|v| v.is_finite())
                        })
                })
        };
        if recording.record_every != every
            || recording.frame_time != times
            || recording.node_disp.len() != frames
            || recording.member_forces.len() != frames
            || recording.node_disp.iter().any(|row| {
                row.len() != self.model.nodes.len() || row.iter().flatten().any(|v| !v.is_finite())
            })
            || recording.member_forces.iter().any(|row| !valid_forces(row))
            || !valid_forces(&recording.peak_member_forces)
            || !valid_story(&recording.story_x, self.model.layer_count(), frames)
            || !valid_story(&recording.story_y, self.model.layer_count(), frames)
            || self
                .result
                .history
                .node_disp
                .iter()
                .chain(&self.result.history.base_shear)
                .chain(&self.result.history.top_drift_angle)
                .any(|v| !v.is_finite())
            || self
                .result
                .peak_disp
                .iter()
                .flatten()
                .chain(&self.result.story_drift_angle)
                .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(invalid("再開状態の前半記録・ピークが不正です。"));
        }
        Ok(())
    }
}

fn valid_story(story: &StoryResponse, count: usize, frames: usize) -> bool {
    story.stories.len() == count
        && story.story_weight.len() == count
        && [
            &story.story_shear,
            &story.floor_accel,
            &story.floor_vel,
            &story.floor_disp,
        ]
        .iter()
        .all(|rows| {
            rows.len() == frames
                && rows
                    .iter()
                    .all(|row| row.len() == count && row.iter().all(|v| v.is_finite()))
        })
        && [
            &story.peak_shear_coeff,
            &story.peak_story_shear,
            &story.peak_floor_accel,
            &story.peak_floor_vel,
            &story.peak_floor_disp,
        ]
        .iter()
        .all(|row| row.len() == count && row.iter().all(|v| v.is_finite() && *v >= 0.0))
}

fn prepare(model: Model, cfg: AnalysisSettings) -> JobResult<Model> {
    if cfg.th_nonlinear {
        return Err(invalid(
            "途中状態の保存・再開は線形時刻歴だけに対応しています。",
        ));
    }
    crate::weight_preparation::require_current_generated_mass(&model)?;
    model
        .validate_wall_weight_generation()
        .map_err(JobError::InvalidInput)?;
    model
        .validate_damper_mass_placement()
        .map_err(JobError::InvalidInput)?;
    Ok(sepika_core::wall_expand::expand_wall_elements_owned(model).0)
}

/// 全波形を保持し、指定確定ステップまで計算する。最終点の指定は拒否する。
pub fn compute_checkpoint(
    model: Model,
    cfg: AnalysisSettings,
    wave: GroundMotion,
    step: u64,
) -> JobResult<LinearTimeHistoryCheckpoint> {
    if step == 0 || wave.accel_x.len() < 2 || step >= (wave.accel_x.len() - 1) as u64 {
        return Err(invalid(
            "途中保存ステップは1以上、最終ステップ未満にしてください。",
        ));
    }
    let model = prepare(model, cfg)?;
    let effective = crate::compute::effective_time_history_wave(&cfg, wave.clone())?;
    let analysis = Analysis::prepare(&model).map_err(|e| JobError::Prepare(e.to_string()))?;
    let damping = crate::compute::time_history_damping(&analysis, cfg)?;
    let dof = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dof);
    if reducer.n_indep == 0 {
        return Err(invalid("再開可能な自由度がありません。"));
    }
    let zero = vec![0.0; reducer.n_indep];
    let (result, state) = linear_time_history_until(
        &model,
        &dof,
        &reducer,
        &effective,
        &NewmarkCfg::average_accel(),
        &damping,
        &zero,
        &zero,
        false,
        (cfg.th_record_every > 0).then_some(cfg.th_record_every),
        Some(step),
    )
    .map_err(crate::compute::dynamic_solve_error)?;
    let checkpoint = LinearTimeHistoryCheckpoint {
        contract: sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION,
        model: model.calculation_model(),
        cfg,
        wave,
        mapping: (0..dof.n_active()).map(|i| dof.global(i as u32)).collect(),
        transform: reducer.t_rows,
        damping: encode(&damping)?,
        state,
        result,
    };
    checkpoint.validate()?;
    Ok(checkpoint)
}

/// 現在の計算入力・全波形・設定・自由度対応を照合し、全区間の結果を返す。
/// 最終ステップ保存、範囲外・不正状態、計算契約の不一致は拒否する。
pub fn resume_checkpoint(
    model: Model,
    cfg: AnalysisSettings,
    wave: GroundMotion,
    checkpoint: LinearTimeHistoryCheckpoint,
) -> JobResult<ResponseResult> {
    checkpoint.validate()?;
    let model = prepare(model, cfg)?;
    if encode(&model.calculation_model())? != encode(&checkpoint.model)? {
        return Err(invalid("再開状態と現在のモデルの計算入力が一致しません。"));
    }
    if encode(&cfg)? != encode(&checkpoint.cfg)? || encode(&wave)? != encode(&checkpoint.wave)? {
        return Err(invalid(
            "再開状態と現在の波形・時間刻み・計算設定が一致しません。",
        ));
    }
    let analysis = Analysis::prepare(&model).map_err(|e| JobError::Prepare(e.to_string()))?;
    let damping = crate::compute::time_history_damping(&analysis, cfg)?;
    let dof = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dof);
    let mapping: Vec<_> = (0..dof.n_active()).map(|i| dof.global(i as u32)).collect();
    if mapping != checkpoint.mapping
        || reducer.t_rows != checkpoint.transform
        || encode(&damping)? != checkpoint.damping
    {
        return Err(invalid("再開状態と現在の自由度対応・減衰が一致しません。"));
    }
    let effective = crate::compute::effective_time_history_wave(&cfg, wave)?;
    let (continuation, _) = linear_time_history_from_state(
        &model,
        &dof,
        &reducer,
        &effective,
        &NewmarkCfg::average_accel(),
        &damping,
        &checkpoint.state,
        false,
        (cfg.th_record_every > 0).then_some(cfg.th_record_every),
    )
    .map_err(crate::compute::dynamic_solve_error)?;
    Ok(merge(checkpoint.result, continuation))
}

fn peaks(before: &mut [f64], after: &[f64]) {
    for (a, b) in before.iter_mut().zip(after) {
        *a = a.max(*b);
    }
}

fn merge_story(a: &mut StoryResponse, b: StoryResponse) {
    a.story_shear.extend(b.story_shear.into_iter().skip(1));
    a.floor_accel.extend(b.floor_accel.into_iter().skip(1));
    a.floor_vel.extend(b.floor_vel.into_iter().skip(1));
    a.floor_disp.extend(b.floor_disp.into_iter().skip(1));
    peaks(&mut a.peak_shear_coeff, &b.peak_shear_coeff);
    peaks(&mut a.peak_story_shear, &b.peak_story_shear);
    peaks(&mut a.peak_floor_accel, &b.peak_floor_accel);
    peaks(&mut a.peak_floor_vel, &b.peak_floor_vel);
    peaks(&mut a.peak_floor_disp, &b.peak_floor_disp);
}

fn merge(mut a: ResponseResult, b: ResponseResult) -> ResponseResult {
    a.time.extend(b.time.into_iter().skip(1));
    a.history
        .node_disp
        .extend(b.history.node_disp.into_iter().skip(1));
    a.history
        .base_shear
        .extend(b.history.base_shear.into_iter().skip(1));
    a.history
        .top_drift_angle
        .extend(b.history.top_drift_angle.into_iter().skip(1));
    for (a, b) in a.peak_disp.iter_mut().zip(b.peak_disp) {
        peaks(a, &b);
    }
    peaks(&mut a.story_drift_angle, &b.story_drift_angle);
    let ar = a.recording.as_mut().unwrap();
    let br = b.recording.unwrap();
    ar.frame_time.extend(br.frame_time.into_iter().skip(1));
    ar.node_disp.extend(br.node_disp.into_iter().skip(1));
    ar.member_forces
        .extend(br.member_forces.into_iter().skip(1));
    merge_story(&mut ar.story_x, br.story_x);
    merge_story(&mut ar.story_y, br.story_y);
    for (a, b) in ar.peak_member_forces.iter_mut().zip(br.peak_member_forces) {
        match (a.as_mut(), b) {
            (Some(a), Some(b)) => {
                for (a, b) in a.at.iter_mut().zip(b.at) {
                    for (a, b) in a.1.iter_mut().zip(b.1) {
                        if b.abs() > a.abs() {
                            *a = b;
                        }
                    }
                }
            }
            (None, b) => *a = b,
            _ => {}
        }
    }
    a
}
