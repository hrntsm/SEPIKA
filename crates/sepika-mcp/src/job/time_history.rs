//! 時刻歴応答解析ジョブの純粋計算。
//!
//! - [`compute_time_history_job`] — TimeHistory ジョブの純粋計算部分。

use super::{attach_prepare_notices, model_prepared_for_analysis, JobDir, JobOutcome, JobParams};
use sepika_core::model::Model;
use sepika_job::{settings::ThDir, JobError};

/// TimeHistory ジョブの純粋計算部分。
/// MCP からは減衰モデル・積分法・非線形の切り替えを受け付けていない。
pub(crate) fn compute_time_history_job(
    model: &Model,
    params: &JobParams,
) -> Result<JobOutcome, JobError> {
    let (work, notices) = model_prepared_for_analysis(model, params)?;

    let cfg = sepika_job::AnalysisSettings {
        th_dt: params.dt,
        th_duration: params.duration,
        th_period: params.period,
        th_amp: params.amp,
        th_dir: match params.dir {
            JobDir::X => ThDir::X,
            JobDir::Y => ThDir::Y,
        },
        ..Default::default()
    };
    let wave = sepika_job::sample_ground_motion(&cfg);

    let result = sepika_job::compute::compute_time_history(work, cfg, wave)?;

    let peak_disp = result
        .history
        .node_disp
        .iter()
        .fold(0.0_f64, |m, v| m.max(v.abs()));
    let mut summary = serde_json::json!({
        "kind": "TimeHistory",
        "peak_disp": peak_disp,
        "record_dir_y": result.history.record_dir_y,
        "n_steps": result.time.len(),
    });
    attach_prepare_notices(&mut summary, notices);
    Ok(JobOutcome::TimeHistory { summary })
}

#[cfg(test)]
mod tests {
    use super::super::rc_wall617_fixture as fixture;
    use super::*;
    #[test]
    fn linear_mcp_time_history_does_not_require_rc_wall_capacity() {
        let mut model = fixture::input_model();
        if let Some(sepika_core::section_shape::SectionShape::RcWall { pwh_ratio, .. }) =
            &mut model.sections[2].shape
        {
            *pwh_ratio = None;
        }
        let params = JobParams {
            duration: 0.04,
            ..Default::default()
        };
        let JobOutcome::TimeHistory { summary } =
            compute_time_history_job(&model, &params).unwrap()
        else {
            panic!()
        };
        assert!(summary["n_steps"].as_u64().unwrap() > 0);
        assert!(summary.get("wall_capacities_n").is_none());
        let (work, _) = model_prepared_for_analysis(&model, &params).unwrap();
        let cfg = sepika_job::AnalysisSettings {
            th_duration: 0.04,
            ..Default::default()
        };
        let direct = sepika_job::compute::compute_time_history(
            work,
            cfg,
            sepika_job::sample_ground_motion(&cfg),
        )
        .unwrap();
        assert!(!direct.nonlinear);
        assert!(direct.wall_capacities.is_empty());
    }
}
