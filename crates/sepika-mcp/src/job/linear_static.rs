//! 線形静的解析ジョブの純粋計算。
//!
//! - [`compute_linear_static_job`] — LinearStatic ジョブの純粋計算部分。

use super::{
    attach_prepare_notices, flatten_member_force_rows, model_prepared_for_analysis,
    resolve_load_case, JobOutcome, JobParams,
};
use sepika_core::model::Model;
use sepika_job::JobError;

/// LinearStatic ジョブの純粋計算部分。
pub(crate) fn compute_linear_static_job(
    model: &Model,
    params: &JobParams,
) -> Result<JobOutcome, JobError> {
    let (work, notices) = model_prepared_for_analysis(model, params)?;
    let case = resolve_load_case(&work, params.load_case)?;
    let lc_id = case.id;
    let seismic_notice = if sepika_job::compute::missing_seismic_horizontal_load(case) {
        let prefix = format!("{} の Ai 地震力を再生成できません:", case.name);
        notices
            .iter()
            .find(|notice| notice.starts_with(&prefix))
            .or_else(|| {
                if params.ai_mode == sepika_solver::statics::analysis::AiMode::SemiPrecise
                    && params.design_period.is_none()
                {
                    notices.iter().find(|notice| {
                        notice.starts_with(
                            "精算周期(固有値解析)が選択されていますが固有値解析が未実行です。",
                        )
                    })
                } else {
                    None
                }
            })
    } else {
        None
    };
    let result = sepika_job::compute::compute_linear_static(work.clone(), lc_id).map_err(
        |error| match (error, seismic_notice) {
            (JobError::InvalidInput(message), Some(notice)) => {
                JobError::InvalidInput(format!("{message}\n{notice}"))
            }
            (error, _) => error,
        },
    )?;
    let model = &work;
    let lc_id = lc_id.0;

    let node_ids: Vec<u32> = model.nodes.iter().map(|n| n.id.0).collect();
    let member_force_rows = flatten_member_force_rows(&result.member_forces);
    let max_abs_disp = result
        .disp
        .iter()
        .flat_map(|d| d.iter())
        .fold(0.0_f64, |m, v| m.max(v.abs()));

    let mut summary = serde_json::json!({
        "kind": "LinearStatic",
        "case": lc_id,
        "n_nodes": node_ids.len(),
        "n_member_force_rows": member_force_rows.len(),
        "max_abs_disp": max_abs_disp,
    });
    attach_prepare_notices(&mut summary, notices);
    Ok(JobOutcome::LinearStatic {
        case: lc_id,
        node_ids,
        disp: result.disp,
        member_force_rows,
        summary,
    })
}
