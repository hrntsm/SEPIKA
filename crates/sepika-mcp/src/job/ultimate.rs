//! 終局検定ジョブの純粋計算。
//!
//! - [`compute_ultimate_check_job`] — 終局検定ジョブ（靭性保証型耐震設計指針）。

use super::{attach_prepare_notices, model_prepared_for_analysis, JobOutcome, JobParams};
use sepika_core::load_combo::ultimate_gravity_terms;
use sepika_core::model::{ElementKind, Model};
use sepika_job::JobError;

/// 保存した長期架構用組合せによる静的・両端ヒンジ略算。必要な重力応力が不足すれば拒否する。
pub(crate) fn compute_ultimate_check_job(
    model: &Model,
    params: &JobParams,
) -> Result<JobOutcome, JobError> {
    // 荷重同期が不足ケースを自動生成する場合も、利用者が保存した構成だけを採用する。
    let terms = ultimate_gravity_terms(model).map_err(JobError::InvalidInput)?;
    let combination_index = model.ultimate_gravity_combination;
    let (work, notices) = model_prepared_for_analysis(model, params)?;
    if work.stress_cfg.tension_only_iteration
        && work
            .elements
            .iter()
            .any(|e| matches!(e.kind, ElementKind::Brace { tension_only: true }))
    {
        return Err(JobError::InvalidInput(
            "gravity_superposition_unsupported: 引張専用ブレースの反復解析では各ケースの有効状態が同一と確認できないため、長期架構用の重力応力を合成できません"
                .into(),
        ));
    }
    let mut demand = sepika_job::complete_ultimate_static_demand(&work, |case| {
        sepika_job::compute::compute_linear_static(work.clone(), case)
            .map(|result| result.member_forces)
            .map_err(|error| error.to_string())
    })
    .map_err(JobError::InvalidInput)?;
    let axial: Vec<(sepika_core::ids::ElemId, f64)> =
        demand.iter().map(|(id, d)| (*id, d.n_axial)).collect();

    // MCP の静的終局検定の対象は CFT 柱のため、RC 断面があるジョブは需要不足として拒否する。
    for (_, member) in &mut demand {
        member.q_long = None;
        member.q_long_weak = None;
    }
    let opts = sepika_design_jp::ultimate::UltimateShearOptions::default();
    let checks = sepika_design_jp::ultimate::collect_rc_ultimate_checks(&work, &demand, &opts)
        .map_err(JobError::InvalidInput)?;
    let cft_checks = sepika_design_jp::ultimate::collect_cft_ultimate_checks(&work, &axial)
        .map_err(JobError::InvalidInput)?;

    let n_checks = checks.len();
    let n_ng = checks.iter().filter(|c| !c.ok).count();
    let min_shear_margin = checks
        .iter()
        .map(|c| c.shear_margin)
        .filter(|m| m.is_finite())
        .fold(f64::INFINITY, f64::min);
    let members: Vec<serde_json::Value> = checks
        .iter()
        .map(|c| {
            serde_json::json!({
                "elem": c.elem.0,
                "kind": format!("{:?}", c.kind),
                "mu": c.mu,
                "qmu": c.qmu,
                "qsu": c.qsu,
                "qbu": c.qbu,
                "shear_margin": c.shear_margin,
                "bond_margin": c.bond_margin,
                "ok": c.ok,
            })
        })
        .collect();

    let cft_members: Vec<serde_json::Value> = cft_checks
        .iter()
        .map(|c| {
            serde_json::json!({
                "elem": c.elem.0,
                "class": format!("{:?}", c.class),
                "ncu": c.ncu,
                "ntu": c.ntu,
                "mu_nm": c.mu_nm.as_ref().ok(),
                "mu_nm_unavailable_reason": c.mu_nm.as_ref().err(),
                "n_design": c.n_design,
                "axial_margin": c.axial_margin,
                "axial_ok": c.ok,
            })
        })
        .collect();

    let mut summary = serde_json::json!({
        "kind": "UltimateCheck",
        "load_target": {
            "source": "saved_combination",
            "combination_index": combination_index,
            "mode": "static_hinge",
            "purpose": "frame_long",
            "terms": terms,
        },
        "n_checks": n_checks,
        "n_ng": n_ng,
        "min_shear_margin": if min_shear_margin.is_finite() { serde_json::json!(min_shear_margin) } else { serde_json::Value::Null },
        "members": members,
        "n_cft_checks": cft_checks.len(),
        "n_cft_axial_ng": cft_checks.iter().filter(|c| !c.ok).count(),
        "n_cft_mu_unavailable": cft_checks.iter().filter(|c| c.mu_nm.is_err()).count(),
        "all_checks_calculated_and_ok": (!checks.is_empty() || !cft_checks.is_empty()) && n_ng == 0 && cft_checks.iter().all(|c| c.ok && c.mu_nm.is_ok()),
        "cft_members": cft_members,
    });
    attach_prepare_notices(&mut summary, notices);
    Ok(JobOutcome::UltimateCheck { summary })
}
