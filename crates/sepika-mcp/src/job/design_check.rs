//! 断面検定・接合部検定ジョブの純粋計算。
//!
//! - [`compute_design_check_job`] — DesignCheck ジョブの純粋計算部分。

use super::{
    attach_prepare_notices, flatten_member_force_rows, model_prepared_for_analysis,
    resolve_load_case, JobOutcome, JobParams,
};
use sepika_core::model::{LoadCaseKind, Model};
use sepika_design_jp::{BondMethod, LoadTerm, MemberDesignCheckOptions, QdMethod, SteelFbBasis};
use sepika_job::JobError;

/// DesignCheck ジョブの純粋計算部分。
/// 検定条件は荷重ケース種別で決める。地震時短期では重力ケースを別途解析して組合せ内力で検定する。
/// 重力ケースの再解析が一部失敗してもジョブ全体は落とさない。
pub(crate) fn compute_design_check_job(
    model: &Model,
    params: &JobParams,
) -> Result<JobOutcome, JobError> {
    let (work, notices) = model_prepared_for_analysis(model, params)?;
    let lc = resolve_load_case(&work, params.load_case)?;
    let lc_id = lc.id;
    let result = sepika_job::compute::compute_linear_static(work.clone(), lc_id)?;
    let expanded_storage;
    let wall_index;
    let model: &Model = if sepika_load::wall_expand::model_has_wall_plates_to_expand(&work) {
        let (expanded, index, _wall_report) = sepika_load::wall_expand::expand_wall_elements(&work);
        wall_index = Some(index);
        expanded_storage = expanded;
        &expanded_storage
    } else {
        wall_index = None;
        &work
    };
    let lc_id_u32 = lc_id.0;

    let term = match lc.kind {
        LoadCaseKind::Seismic | LoadCaseKind::Wind => LoadTerm::Short,
        _ => LoadTerm::Long,
    };

    let mut gravity_failed = 0usize;
    let (long_member_forces, q0_by_elem, check_forces) = if lc.kind == LoadCaseKind::Seismic {
        let gravity_ids = sepika_job::gravity_case_ids_for_seismic_weight(model);
        let mut gravity_results = Vec::new();
        for gid in &gravity_ids {
            if *gid == lc_id {
                continue;
            }
            match sepika_job::compute::compute_linear_static(work.clone(), *gid) {
                Ok(g) => gravity_results.push(g.member_forces),
                Err(_) => gravity_failed += 1,
            }
        }
        let long = if gravity_results.is_empty() {
            None
        } else {
            Some(sepika_job::sum_member_forces_lists(&gravity_results))
        };
        let q0 = sepika_job::simple_beam_q0_by_gravity_cases(model);
        let combo = if let Some(ref lf) = long {
            sepika_job::sum_member_forces_lists(&[lf.clone(), result.member_forces.clone()])
        } else {
            result.member_forces.clone()
        };
        (long, q0, combo)
    } else {
        (None, Default::default(), result.member_forces.clone())
    };

    let member_force_rows = flatten_member_force_rows(&check_forces);

    let wall_case = format!("case:{}", lc_id.0);
    let report = sepika_design_jp::run_member_design_checks(
        model,
        &check_forces,
        &result.panel_moments,
        &MemberDesignCheckOptions {
            term,
            wall_index: wall_index.as_ref(),
            wall_case: &wall_case,
            rc_damage_control: true,
            bond_method: BondMethod::default(),
            qd_method: QdMethod::default(),
            long_member_forces: long_member_forces.as_deref(),
            q_simple_by_elem: Some(&q0_by_elem),
            girder_group_overrides: None,
            steel_fb_basis: SteelFbBasis::default(),
        },
    );

    let mut summary = assemble_design_check_summary(
        &report,
        lc_id_u32,
        term,
        long_member_forces.is_some(),
        gravity_failed,
    );
    attach_prepare_notices(&mut summary, notices);
    Ok(JobOutcome::DesignCheck {
        case: lc_id_u32,
        member_force_rows,
        summary,
    })
}

fn assemble_design_check_summary(
    report: &sepika_design_jp::MemberDesignCheckReport,
    lc_id_u32: u32,
    term: LoadTerm,
    qd_wired: bool,
    gravity_failed: usize,
) -> serde_json::Value {
    let mut n_checks = 0usize;
    let mut n_ng = 0usize;
    let mut n_skipped = 0usize;
    let mut member_skipped = Vec::new();
    let mut max_ratio: Option<f64> = None;

    for (elem, pos, outcome) in &report.member_checks {
        n_checks += 1;
        match outcome {
            sepika_design_jp::CheckOutcome::Checked(cr) => {
                if !cr.ok() {
                    n_ng += 1;
                }
                if !cr.components.is_empty() {
                    max_ratio = Some(max_ratio.map_or(cr.ratio(), |r| r.max(cr.ratio())));
                }
            }
            sepika_design_jp::CheckOutcome::Skipped { reason } => {
                n_skipped += 1;
                member_skipped
                    .push(serde_json::json!({"elem": elem.0, "position": pos, "reason": reason}));
            }
        }
    }

    let n_joint_checks = report.joint_checks.len();
    let mut n_joint_ng = 0usize;
    let mut n_joint_skipped = 0usize;
    let mut joint_skipped = Vec::new();
    for (node, label, outcome) in &report.joint_checks {
        match outcome {
            sepika_design_jp::CheckOutcome::Checked(cr) => {
                if !cr.ok() {
                    n_joint_ng += 1;
                }
                if !cr.components.is_empty() {
                    max_ratio = Some(max_ratio.map_or(cr.ratio(), |r| r.max(cr.ratio())));
                }
            }
            sepika_design_jp::CheckOutcome::Skipped { reason } => {
                n_joint_skipped += 1;
                joint_skipped.push(serde_json::json!({
                    "node": node.0,
                    "label": label,
                    "reason": reason,
                }));
            }
        }
    }

    let wall_summary =
        sepika_design_jp::wall_check::WallCheckSummary::from_checks(&report.wall_checks);
    if let Some(ratio) = wall_summary.max_ratio {
        max_ratio = Some(max_ratio.map_or(ratio, |r| r.max(ratio)));
    }
    serde_json::json!({
        "wall_checks": report.wall_checks,
        "wall_summary": wall_summary,
        "wall_summary_by_kind": {
            "AllowableShear": sepika_design_jp::wall_check::WallCheckSummary::for_kind(&report.wall_checks, sepika_design_jp::wall_check::WallCheckKind::AllowableShear),
            "ReferenceSkeleton": sepika_design_jp::wall_check::WallCheckSummary::for_kind(&report.wall_checks, sepika_design_jp::wall_check::WallCheckKind::ReferenceSkeleton),
        },
        "kind": "DesignCheck",
        "case": lc_id_u32,
        "term": match term {
            LoadTerm::Long => "long",
            LoadTerm::Short => "short",
        },
        "n_checks": n_checks,
        "n_ng": n_ng,
        "n_skipped": n_skipped,
        "member_skipped": member_skipped,
        "all_checked_and_ok": n_checks + n_joint_checks + wall_summary.n_checks > 0 && n_skipped + n_joint_skipped + wall_summary.n_skipped == 0 && n_ng + n_joint_ng + wall_summary.n_ng == 0,
        "all_members_checked_and_ok": n_checks > 0 && n_skipped == 0 && n_ng == 0,
        "n_joint_checks": n_joint_checks,
        "n_joint_ng": n_joint_ng,
        "n_joint_skipped": n_joint_skipped,
        "joint_skipped": joint_skipped,
        "max_ratio": max_ratio,
        "qd_wired": qd_wired,
        "gravity_failed": gravity_failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::{ElemId, NodeId, WallPlateId};
    use sepika_design_jp::wall_check::{WallCheck, WallCheckKind, WallSkipKind};
    use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};
    #[test]
    fn wall_output_assembly_keeps_independent_status_keys_counts_and_null_max() {
        let row = |id, outcome, why| WallCheck {
            plate: Some(WallPlateId(id)),
            elem: Some(ElemId(10 + id)),
            node: Some(NodeId(0)),
            case: "case:5".into(),
            kind: WallCheckKind::AllowableShear,
            seismic_target: true,
            skip_kind: why,
            outcome,
        };
        let mut report = sepika_design_jp::MemberDesignCheckReport::default();
        report.wall_checks.push(row(
            0,
            CheckOutcome::Checked(CheckResult {
                basis: "独立検証".into(),
                detail: String::new(),
                components: vec![CheckComponent {
                    kind: CheckKind::Shear,
                    ratio: 0.5,
                    detail: String::new(),
                }],
            }),
            None,
        ));
        report.wall_checks.push(row(
            1,
            CheckOutcome::Skipped {
                reason: "国内鋼板式未確定".into(),
            },
            Some(WallSkipKind::NotImplemented),
        ));
        let summary = assemble_design_check_summary(&report, 5, LoadTerm::Short, false, 0);
        assert_eq!(summary["wall_summary"]["n_ok"], 1);
        assert_eq!(summary["wall_summary"]["n_skipped"], 1);
        assert_eq!(summary["wall_summary"]["n_walls"], 2);
        assert_eq!(summary["all_checked_and_ok"], false);
        assert_eq!(summary["wall_checks"][1]["plate"], 1);
        assert_eq!(summary["wall_checks"][1]["elem"], 11);
        assert_eq!(summary["wall_checks"][1]["case"], "case:5");
        assert_eq!(summary["max_ratio"], 0.5);
        report.wall_checks.push(row(
            2,
            CheckOutcome::Skipped {
                reason: "壁材料欠落".into(),
            },
            Some(WallSkipKind::MissingInput),
        ));
        let summary = assemble_design_check_summary(&report, 5, LoadTerm::Short, false, 0);
        assert_eq!(summary["wall_summary"]["n_skipped"], 2);
        report.wall_checks[0].outcome = CheckOutcome::Skipped {
            reason: "壁応答欠落".into(),
        };
        report.wall_checks[0].skip_kind = Some(WallSkipKind::MissingResponse);
        let summary = assemble_design_check_summary(&report, 5, LoadTerm::Short, false, 0);
        assert!(summary["max_ratio"].is_null());
        assert!(summary["wall_summary_by_kind"]["AllowableShear"]["max_ratio"].is_null());
        let empty =
            assemble_design_check_summary(&Default::default(), 5, LoadTerm::Short, false, 0);
        assert!(empty["max_ratio"].is_null());
        assert_eq!(empty["all_checked_and_ok"], false);
    }
}
