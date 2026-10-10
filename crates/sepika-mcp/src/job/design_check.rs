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

/// 保存組合せまたは単独ケースを検定する。地震単独指定は検定用G+Pを自動合成する。
/// 自動合成に必要な重力ケースが欠落・失敗した場合は検定を拒否する。
pub(crate) fn compute_design_check_job(
    model: &Model,
    params: &JobParams,
) -> Result<JobOutcome, JobError> {
    use sepika_core::load_combo::{
        case_design_state, combination_design_state, LoadAction, LoadDuration,
    };
    if params.load_case.is_some() && params.load_combination.is_some() {
        return Err(JobError::InvalidInput(
            "load_caseとload_combinationは同時指定できません".into(),
        ));
    }
    let (work, notices) = model_prepared_for_analysis(model, params)?;
    let (lc_id, terms, source, state) = if let Some(index) = params.load_combination {
        let combo = work.combinations.get(index).ok_or_else(|| {
            JobError::InvalidInput(format!("保存組合せindex {index} がありません"))
        })?;

        (
            None,
            combo.terms.clone(),
            "saved_combination",
            combination_design_state(combo, &work.load_cases),
        )
    } else {
        let lc = resolve_load_case(&work, params.load_case)?;
        if lc.kind == LoadCaseKind::Seismic {
            if work.load_cases.iter().any(|c| c.kind == LoadCaseKind::Snow) {
                return Err(JobError::InvalidInput(
                    "積雪ケースがある地震検定は地域条件に応じた保存組合せを指定してください".into(),
                ));
            }
            let mut terms: Vec<_> = sepika_job::gravity_case_ids_for_design(&work)
                .into_iter()
                .map(|id| (id, 1.0))
                .collect();
            terms.push((lc.id, 1.0));
            let combo = sepika_core::model::LoadCombination {
                name: String::new(),
                terms: terms.clone(),
            };
            let state = combination_design_state(&combo, &work.load_cases)
                .map_err(JobError::InvalidInput)?;
            (
                Some(lc.id),
                terms,
                "automatic_gravity_combination",
                Ok(state),
            )
        } else {
            (
                Some(lc.id),
                vec![(lc.id, 1.0)],
                "single_case",
                case_design_state(lc.kind),
            )
        }
    };
    let mut solved = Vec::new();
    let mut failures = Vec::new();
    for (id, factor) in &terms {
        match sepika_job::compute::compute_linear_static(work.clone(), *id) {
            Ok(result) => solved.push((result, *factor)),
            Err(error) => failures.push(format!("case:{}: {error}", id.0)),
        }
    }
    if !failures.is_empty() {
        return Err(JobError::InvalidInput(format!(
            "必要応力が不足するため検定を拒否します: {}",
            failures.join("; ")
        )));
    }
    let refs: Vec<_> = solved.iter().map(|(r, f)| (r, *f)).collect();
    let result = sepika_solver::statics::linear::superpose_static(&refs);
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
    let case = (source == "single_case").then(|| lc_id.unwrap().0);

    let term = match state.as_ref().map(|s| s.duration) {
        Ok(LoadDuration::Short) => LoadTerm::Short,
        _ => LoadTerm::Long,
    };
    let mut load_error = state.as_ref().err().cloned();
    if state
        .as_ref()
        .is_ok_and(|s| !s.combination && s.duration == LoadDuration::Short)
    {
        load_error = Some("単独の雪・風応力です。G+Pを含む保存組合せを指定してください".into());
    }
    if state.as_ref().is_ok_and(|s| s.combination)
        && work.stress_cfg.tension_only_iteration
        && work.elements.iter().any(|e| {
            matches!(
                e.kind,
                sepika_core::model::ElementKind::Brace { tension_only: true }
            )
        })
    {
        load_error =
            Some("引張専用ブレースの別ケース合成は同一解を保証できないため未検定です".into());
    }
    let gravity_terms = sepika_job::design_gravity_terms(model, &terms);
    let long_member_forces = if state
        .as_ref()
        .is_ok_and(|s| s.combination && s.action == LoadAction::Seismic)
    {
        Some(
            sepika_job::complete_design_gravity_forces(&gravity_terms, |id| {
                sepika_job::compute::compute_linear_static(work.clone(), id)
                    .ok()
                    .map(|r| r.member_forces)
            })
            .map_err(|missing| {
                JobError::InvalidInput(format!("検定用重力応力が不足しています: {missing:?}"))
            })?,
        )
    } else {
        None
    };
    let q0_by_elem = sepika_job::simple_beam_q0_by_terms(model, &gravity_terms);
    let check_forces = result.member_forces.clone();
    let member_force_rows = flatten_member_force_rows(&check_forces);

    let wall_case = match params.load_combination {
        Some(index) => format!("combo:{index}:{}", work.combinations[index].name),
        None if source == "automatic_gravity_combination" => {
            format!("auto:G+P+case:{}", lc_id.unwrap().0)
        }
        None => format!("case:{}", lc_id.unwrap().0),
    };
    let mut report = sepika_design_jp::run_member_design_checks(
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

    if let Some(reason) = &load_error {
        report.skip_for_load_state(reason);
    }
    let mut summary =
        assemble_design_check_summary(&report, case, term, long_member_forces.is_some(), 0);
    summary["load_target"] = serde_json::json!({ "source": source, "combination_index": params.load_combination, "requested_case": lc_id.map(|id| id.0), "terms": terms, "gravity_reference_terms": gravity_terms, "state": state.as_ref().ok(), "diagnostic": load_error, "legal_conditions_verified": false });
    summary["floor_scope"] = serde_json::json!({ "selected_combination_checked": false, "long_term_approximation": "GUIの固定＋用途別積載の独立略算。MCPでは未実行" });
    if state.is_err() {
        summary["term"] = serde_json::Value::Null;
    }
    attach_prepare_notices(&mut summary, notices);
    Ok(JobOutcome::DesignCheck {
        case,
        member_force_rows,
        summary,
    })
}

fn assemble_design_check_summary(
    report: &sepika_design_jp::MemberDesignCheckReport,
    case: Option<u32>,
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
        "case": case,
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
        let summary = assemble_design_check_summary(&report, Some(5), LoadTerm::Short, false, 0);
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
        let summary = assemble_design_check_summary(&report, Some(5), LoadTerm::Short, false, 0);
        assert_eq!(summary["wall_summary"]["n_skipped"], 2);
        report.wall_checks[0].outcome = CheckOutcome::Skipped {
            reason: "壁応答欠落".into(),
        };
        report.wall_checks[0].skip_kind = Some(WallSkipKind::MissingResponse);
        let summary = assemble_design_check_summary(&report, Some(5), LoadTerm::Short, false, 0);
        assert!(summary["max_ratio"].is_null());
        assert!(summary["wall_summary_by_kind"]["AllowableShear"]["max_ratio"].is_null());
        let empty =
            assemble_design_check_summary(&Default::default(), Some(5), LoadTerm::Short, false, 0);
        assert!(empty["max_ratio"].is_null());
        assert_eq!(empty["all_checked_and_ok"], false);
    }
}
