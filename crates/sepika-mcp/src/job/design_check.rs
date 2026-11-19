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
    sepika_design_jp::validate_model_steel_inputs(model).map_err(JobError::InvalidInput)?;
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
    let model: &Model = if sepika_core::wall_expand::model_has_wall_plates_to_expand(&work) {
        let (expanded, index, _wall_report) = sepika_core::wall_expand::expand_wall_elements(&work);
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
    let q0_by_elem = sepika_job::simple_beam_q0_by_terms(model, &gravity_terms)
        .map_err(JobError::InvalidInput)?;
    let check_forces = &result.member_forces;
    let member_force_rows = flatten_member_force_rows(check_forces);

    let wall_case = match params.load_combination {
        Some(index) => format!("combo:{index}:{}", work.combinations[index].name),
        None if source == "automatic_gravity_combination" => {
            format!("auto:G+P+case:{}", lc_id.unwrap().0)
        }
        None => format!("case:{}", lc_id.unwrap().0),
    };
    let mut report = sepika_design_jp::run_member_design_checks(
        model,
        check_forces,
        &result.panel_moments,
        &MemberDesignCheckOptions {
            term,
            wall_index: wall_index.as_ref(),
            wall_case: &wall_case,
            load_terms: Some(&terms),
            rc_damage_control: true,
            bond_method: BondMethod::default(),
            qd_method: QdMethod::default(),
            long_member_forces: long_member_forces.as_deref(),
            q_simple_by_elem: Some(&q0_by_elem),
            girder_group_overrides: None,
            steel_fb_basis: SteelFbBasis::default(),
        },
    )
    .map_err(JobError::InvalidInput)?;

    if let Some(reason) = &load_error {
        report.skip_for_load_state(reason);
    }
    let mut summary =
        assemble_design_check_summary(&report, case, term, long_member_forces.is_some());
    summary["load_target"] = serde_json::json!({ "source": source, "combination_index": params.load_combination, "requested_case": lc_id.map(|id| id.0), "terms": terms, "gravity_reference_terms": gravity_terms, "state": state.as_ref().ok(), "diagnostic": load_error, "legal_conditions_verified": false });
    summary["floor_scope"] = serde_json::json!({ "selected_combination_checked": false, "long_term_approximation": "GUIの固定＋用途別積載の独立略算。MCPでは未実行" });
    summary["secondary_post_scope"] = serde_json::json!({"checked":false,"reason":"鋼製間柱の独立検定はGUI専用です。MCPの主架構検定には含まれません"});
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
) -> serde_json::Value {
    let mut n_checks = 0usize;
    let mut n_ng = 0usize;
    let mut n_skipped = 0usize;
    let mut member_skipped = Vec::new();
    let mut rc_deflection_checks = Vec::new();
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
                for component in cr
                    .components
                    .iter()
                    .filter(|c| c.kind == sepika_design_jp::CheckKind::Deflection)
                {
                    rc_deflection_checks.push(serde_json::json!({
                        "elem": elem.0, "position": pos, "ratio": component.ratio, "detail": component.detail,
                    }));
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
        "lateral_bracing_references": report.lateral_bracing_references.iter().map(|(elem, reference)| serde_json::json!({ "elem": elem.0, "reference": reference, "actual_bracing_verified": false })).collect::<Vec<_>>(),
        "steel_deflection_references": report.steel_deflection_references.iter().map(|(elem, detail)| serde_json::json!({ "elem": elem.0, "detail": detail })).collect::<Vec<_>>(),
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
        "rc_deflection_checks": rc_deflection_checks,
        "all_checked_and_ok": n_checks + n_joint_checks + wall_summary.n_checks > 0 && n_skipped + n_joint_skipped + wall_summary.n_skipped == 0 && n_ng + n_joint_ng + wall_summary.n_ng == 0,
        "all_members_checked_and_ok": n_checks > 0 && n_skipped == 0 && n_ng == 0,
        "n_joint_checks": n_joint_checks,
        "n_joint_ng": n_joint_ng,
        "n_joint_skipped": n_joint_skipped,
        "joint_skipped": joint_skipped,
        "max_ratio": max_ratio,
        "qd_wired": qd_wired,
        "gravity_failed": 0,
    })
}

#[cfg(test)]
mod tests {
    use super::super::rc_wall_cracking_model as crack_fixture;
    use super::super::rc_wall_cracking_reference as crack_reference;
    use super::super::rc_wall_side_column_model as fixture;
    use super::*;
    use sepika_core::ids::{ElemId, NodeId, WallPlateId};
    use sepika_design_jp::wall_check::{WallCheck, WallCheckKind, WallSkipKind};
    use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};

    mod steel_column_k_input {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/steel_column_k_model.rs"
        ));
    }

    mod steel_h_column_combined_input {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/steel_h_column_combined_model.rs"
        ));
    }

    #[test]
    fn h_column_combination_reason_reaches_mcp_design_job_summary() {
        let run = |weak_shear_n: f64| {
            let model = steel_h_column_combined_input::cantilever(weak_shear_n);
            let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                &model,
                &JobParams {
                    load_case: Some(0),
                    ..Default::default()
                },
            )
            .unwrap() else {
                panic!()
            };
            summary
        };
        let summary = run(365_000.0);
        assert_eq!(summary["n_checks"], 3, "{summary}");
        assert!(summary["max_ratio"].is_null(), "{summary}");
        assert_eq!(summary["n_skipped"], 3, "{summary}");
        assert_eq!(summary["all_members_checked_and_ok"], false);
        let skipped = summary["member_skipped"].as_array().unwrap();
        assert_eq!(skipped.len(), 3, "{summary}");
        for (row, xi) in skipped.iter().zip([0.0, 0.5, 1.0]) {
            assert_eq!(row["elem"], 0);
            assert_eq!(row["position"], xi);
            assert!(
                row["reason"]
                    .as_str()
                    .unwrap()
                    .contains("フランジ内の設計用公称応力度の分布を規準から確定できず"),
                "{row}"
            );
        }
        let without_weak_shear = run(0.0);
        assert_eq!(without_weak_shear["n_checks"], 3, "{without_weak_shear}");
        assert_eq!(without_weak_shear["n_skipped"], 0, "{without_weak_shear}");
    }

    #[test]
    fn cft_column_mcp_reports_missing_axis_for_case_and_combination() {
        use crate::tests::cft_column_buckling_input::{cantilever, direct_lengths};
        let mut model = cantilever(true, 100_000.0);
        for combo in [false, true] {
            model.steel_design_attrs = vec![direct_lengths(ElemId(0), None, Some(6000.0))];
            let run = |model: &Model| {
                let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                    model,
                    &JobParams {
                        load_case: (!combo).then_some(0),
                        load_combination: combo.then_some(0),
                        ..Default::default()
                    },
                )
                .unwrap() else {
                    panic!()
                };
                summary
            };
            let summary = run(&model);
            let skipped = summary["member_skipped"].as_array().unwrap();
            assert_eq!(skipped.len(), 3, "{summary}");
            for row in skipped {
                assert_eq!(row["elem"], 0);
                assert!(row["reason"].as_str().unwrap().contains("強軸の座屈長さ"));
            }
            assert_eq!(summary["all_members_checked_and_ok"], false);
            model.steel_design_attrs[0].lk_y_direct = Some(6000.0);
            let summary = run(&model);
            assert_eq!(summary["n_checks"], 3, "{summary}");
            assert_eq!(summary["n_skipped"], 0, "{summary}");
            assert_eq!(summary["all_members_checked_and_ok"], true);
        }
    }

    #[test]
    fn steel_buckling_mcp_distinguishes_defaults_direct_and_invalid_for_case_and_combination() {
        use sepika_core::ids::LoadCaseId;
        use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination};

        let mut model = steel_column_k_input::portal(1.0);
        model.load_cases.push(LoadCase {
            origin: Default::default(),
            id: LoadCaseId(1),
            name: "P=0".into(),
            kind: LoadCaseKind::Live,
            nodal: vec![],
            member: vec![],
        });
        model.combinations.push(LoadCombination {
            name: "長期".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
        });
        for combo in [false, true] {
            let run = |model: &Model| {
                let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                    model,
                    &JobParams {
                        load_case: (!combo).then_some(0),
                        load_combination: combo.then_some(0),
                        ..Default::default()
                    },
                )
                .unwrap() else {
                    panic!()
                };
                summary
            };
            let mut model = model.clone();
            model.steel_design_attrs[1].lk_z_direct = None;
            let summary = run(&model);
            assert_eq!(summary["n_checks"], 9, "{summary}");
            assert_eq!(summary["n_skipped"], 0, "{summary}");
            assert_eq!(summary["all_members_checked_and_ok"], true, "{summary}");

            model.steel_design_attrs[0].lk_y_direct = Some(6983.630277482522);
            let summary = run(&model);
            assert_eq!(summary["n_skipped"], 0, "{summary}");
            assert_eq!(summary["n_ng"], 3, "{summary}");
            assert_eq!(summary["all_members_checked_and_ok"], false, "{summary}");

            model.steel_design_attrs[1].lk_z_direct = Some(0.0);
            let summary = run(&model);
            let skipped = summary["member_skipped"].as_array().unwrap();
            assert_eq!(skipped.len(), 3, "{summary}");
            for row in skipped {
                assert_eq!(row["elem"], 1);
                assert!(row["reason"]
                    .as_str()
                    .unwrap()
                    .contains("弱軸の座屈長さの直接指定"));
            }
            assert_eq!(summary["all_members_checked_and_ok"], false);
        }
    }

    mod column_input {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_model.rs"
        ));
    }
    mod column_reference {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_reference.rs"
        ));
    }

    mod tension_input {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/rc_column_tension_model.rs"
        ));
    }
    mod tension_reference {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/rc_column_tension_reference.rs"
        ));
    }

    mod asymmetric_beam_input {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/rc_beam_asymmetric_model.rs"
        ));
    }

    mod brb_input {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/brb_model.rs"
        ));
    }

    #[test]
    fn brb_product_mcp_counts_unchecked_and_limit_exceedance() {
        for (reduction, ratio, reason) in [
            (None, None, Some("端部低減距離 L1が未入力")),
            (Some(f64::INFINITY), None, Some("端部低減距離 L1には有限")),
            (Some(2500.0), None, Some("控除後の座屈長さ L−2L1")),
            (Some(0.0), Some(5000.0 / 4400.0), None),
            (Some(300.0), Some(1.0), None),
        ] {
            let mut model = brb_input::model();
            model.brb_attrs[0].length_reduction = reduction;
            let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                &model,
                &JobParams {
                    load_case: Some(0),
                    ..Default::default()
                },
            )
            .unwrap() else {
                panic!()
            };
            assert_eq!(summary["n_checks"], 2, "{summary}");
            assert_eq!(summary["max_ratio"].as_f64(), ratio);
            let skipped = summary["member_skipped"].as_array().unwrap();
            if let Some(reason) = reason {
                assert_eq!(summary["n_skipped"], 2);
                assert_eq!(summary["n_ng"], 0);
                assert_eq!(summary["all_members_checked_and_ok"], false);
                assert_eq!(skipped.len(), 2);
                for (row, position) in skipped.iter().zip([0.0, 1.0]) {
                    assert_eq!(row["elem"], brb_input::BRACE.0);
                    assert_eq!(row["position"].as_f64(), Some(position));
                    assert!(row["reason"].as_str().unwrap().contains(reason), "{row}");
                }
            } else {
                let ok = ratio == Some(1.0);
                assert_eq!(summary["n_skipped"], 0);
                assert!(skipped.is_empty());
                assert_eq!(summary["n_ng"], if ok { 0 } else { 2 });
                assert_eq!(summary["all_members_checked_and_ok"], ok);
            }
        }
    }

    #[test]
    fn rc_beam_asymmetric_mcp_case_and_combination_report_unchecked() {
        for asymmetric in [false, true] {
            let model = asymmetric_beam_input::model(asymmetric);
            for combo in [false, true] {
                let JobOutcome::DesignCheck {
                    summary,
                    member_force_rows,
                    ..
                } = compute_design_check_job(
                    &model,
                    &JobParams {
                        load_case: (!combo).then_some(0),
                        load_combination: combo.then_some(0),
                        ..Default::default()
                    },
                )
                .unwrap()
                else {
                    panic!()
                };
                assert_eq!(summary["n_checks"], if asymmetric { 3 } else { 4 });
                assert_eq!(member_force_rows.len(), 3);
                assert!(
                    member_force_rows
                        .iter()
                        .all(|(_, _, f)| (f[5] - 45e6).abs() < 1e-6),
                    "{member_force_rows:?}"
                );
                assert_eq!(summary["all_checked_and_ok"], false);
                assert_eq!(summary["all_members_checked_and_ok"], false);
                if asymmetric {
                    assert_eq!(summary["n_skipped"], 3);
                    assert_eq!(summary["n_ng"], 0);
                    assert!(summary["max_ratio"].is_null());
                    for row in summary["member_skipped"].as_array().unwrap() {
                        let reason = row["reason"].as_str().unwrap();
                        assert!(
                            reason.contains("正曲げ")
                                && reason.contains("重心までの距離が一致しません")
                        );
                    }
                } else {
                    assert_eq!(summary["n_skipped"], 1, "{summary}");
                    assert!(summary["member_skipped"][0]["reason"]
                        .as_str()
                        .unwrap()
                        .contains("長期たわみ未算定"));
                    assert_eq!(summary["n_ng"], 3, "{summary}");
                    assert!(
                        summary["max_ratio"].as_f64().unwrap()
                            >= 45e6 / 39_177_363.507_517_874 - 1e-8
                    );
                }
            }
        }
    }

    #[test]
    fn mcp_long_term_rc_reports_calculated_deflection_for_case_and_combination() {
        use sepika_core::dof::{Dof, Dof6Mask};
        use sepika_core::model::MemberLoad;

        let mut model = asymmetric_beam_input::model(false);
        model.materials[0].young = 23_000.0;
        model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
        for node in &mut model.nodes {
            node.restraint = Dof6Mask::FIXED;
            node.restraint.set_free(Dof::Ry);
        }
        model.load_cases[0].name = "たわみ検証用固定荷重".into();
        model.load_cases[0].nodal.clear();
        model.load_cases[0].member = vec![MemberLoad::full_length_uniform(
            ElemId(0),
            [0.0, 0.0, 1.0],
            3000.0,
            15.0,
        )];

        for combo in [false, true] {
            let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                &model,
                &JobParams {
                    load_case: (!combo).then_some(0),
                    load_combination: combo.then_some(0),
                    ..Default::default()
                },
            )
            .unwrap() else {
                panic!("DesignCheck");
            };
            let rows = summary["rc_deflection_checks"].as_array().unwrap();
            assert_eq!(rows.len(), 1, "{summary}");
            let row = &rows[0];
            assert_eq!(row["elem"], 0);
            assert_eq!(row["position"], 0.5);
            // 単純支持・等分布の閉形式 5wL⁴/(384EI) を 250×8/L 倍。
            approx::assert_abs_diff_eq!(
                row["ratio"].as_f64().unwrap(),
                0.5133730510837876,
                epsilon = 1e-10
            );
            let detail = row["detail"].as_str().unwrap();
            assert!(detail.contains("δ=0.770060 mm, v=-0.770060 mm"), "{detail}");
            assert!(detail.contains("x=1500.000 mm"), "{detail}");
            assert_eq!(summary["member_skipped"], serde_json::json!([]));
        }
    }

    #[test]
    fn mcp_long_term_girder_reports_unavailable_deflection_with_material_specific_status() {
        use sepika_core::dof::{Dof, Dof6Mask};
        use sepika_core::ids::{MaterialId, SectionId};
        use sepika_core::model::{FrameSectionUse, MaterialCategory, MemberLoad, MemberLoadKind};
        use sepika_core::section_shape::SectionShape;

        let mut model = asymmetric_beam_input::model(false);
        model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
        for node in &mut model.nodes {
            node.restraint = Dof6Mask::FIXED;
            node.restraint.set_free(Dof::Ry);
        }
        model.load_cases[0].nodal.clear();
        model.load_cases[0].member.push(MemberLoad::manual(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Point {
                a: 1_500.0,
                p: 1_000.0,
            },
        ));

        for steel in [false, true] {
            if steel {
                let mut material =
                    asymmetric_beam_input::material("SN400B", MaterialCategory::Steel);
                material.id = MaterialId(0);
                model.materials[0] = material;
                let mut section = SectionShape::SteelH {
                    height: 400.0,
                    width: 200.0,
                    web_thick: 8.0,
                    flange_thick: 13.0,
                    root_r: Some(0.0),
                }
                .to_section(SectionId(0), "鋼梁".into());
                section.material = Some(MaterialId(0));
                section.frame_use = Some(FrameSectionUse::Girder);
                model.sections[0] = section;
            }
            let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                &model,
                &JobParams {
                    load_combination: Some(0),
                    ..Default::default()
                },
            )
            .unwrap() else {
                panic!("DesignCheck");
            };
            assert_eq!(summary["n_ng"], 0, "{summary}");
            assert_eq!(summary["rc_deflection_checks"], serde_json::json!([]));
            assert_eq!(summary["all_members_checked_and_ok"], steel, "{summary}");
            if steel {
                assert_eq!(summary["n_skipped"], 0, "{summary}");
                assert_eq!(summary["steel_deflection_references"][0]["elem"], 0);
                let detail = summary["steel_deflection_references"][0]["detail"]
                    .as_str()
                    .unwrap();
                assert!(
                    detail.contains("未算定") && detail.contains("集中荷重"),
                    "{detail}"
                );
            } else {
                assert_eq!(summary["n_skipped"], 1, "{summary}");
                assert_eq!(summary["member_skipped"][0]["position"], 0.5);
                let reason = summary["member_skipped"][0]["reason"].as_str().unwrap();
                assert!(
                    reason.contains("長期たわみ未算定") && reason.contains("集中荷重"),
                    "{reason}"
                );
            }
        }
    }

    #[test]
    fn rc_column_tension_mcp_reports_ng_for_case_and_combination() {
        for circle in [false, true] {
            let mut model = tension_input::tension_model(
                column_input::column_model(circle, 0.0),
                1_000_000.0,
                [0.0, 0.0],
            );
            model.load_cases.push(sepika_core::model::LoadCase {
                origin: Default::default(),
                id: sepika_core::ids::LoadCaseId(1),
                name: "架構用積載".into(),
                kind: sepika_core::model::LoadCaseKind::Live,
                nodal: vec![],
                member: vec![],
            });
            model
                .combinations
                .push(sepika_core::model::LoadCombination {
                    name: "長期".into(),
                    terms: vec![
                        (sepika_core::ids::LoadCaseId(0), 1.0),
                        (sepika_core::ids::LoadCaseId(1), 1.0),
                    ],
                });
            for combo in [false, true] {
                let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                    &model,
                    &JobParams {
                        load_case: if combo { None } else { Some(0) },
                        load_combination: if combo { Some(0) } else { None },
                        ..Default::default()
                    },
                )
                .unwrap() else {
                    panic!()
                };
                assert_eq!(summary["n_checks"], 3);
                assert_eq!(summary["n_ng"], 3, "{circle} combo={combo}: {summary}");
                assert_eq!(summary["n_skipped"], 0);
                assert_eq!(summary["all_checked_and_ok"], false);
                assert_eq!(summary["all_members_checked_and_ok"], false);
                let ratio = summary["max_ratio"].as_f64().unwrap();
                assert!(
                    ratio > 2.0 && ratio <= 1_000_000.0 / tension_reference::tension_limit_n(true),
                    "{summary}"
                );
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&summary.to_string()).unwrap(),
                    summary
                );
            }
        }
    }

    #[test]
    fn rc_column_tension_outside_nm_curve_is_not_reported_as_passing_in_mcp() {
        for circle in [false, true] {
            let model = tension_input::tension_model(
                column_input::column_model(circle, 0.0),
                0.99 * tension_reference::tension_limit_n(true),
                [1_000_000.0, 2_000_000.0],
            );
            let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                &model,
                &JobParams {
                    load_case: Some(0),
                    ..Default::default()
                },
            )
            .unwrap() else {
                panic!()
            };
            assert_eq!(summary["n_skipped"], 3);
            assert_eq!(summary["n_ng"], 0);
            assert_eq!(summary["all_checked_and_ok"], false);
            assert!(summary["max_ratio"].is_null());
            for row in summary["member_skipped"].as_array().unwrap() {
                assert!(row["reason"].as_str().unwrap().contains("検定不能"));
            }
        }
    }

    #[test]
    fn rc_column_boundary_mcp_case_and_combination_are_unchecked_in_json() {
        for circle in [false, true] {
            let mut model =
                column_input::column_model(circle, 1.01 * column_reference::axial_limit_n(circle));
            model
                .combinations
                .push(sepika_core::model::LoadCombination {
                    name: "長期".into(),
                    terms: vec![(sepika_core::ids::LoadCaseId(0), 1.0)],
                });
            for combo in [false, true] {
                let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
                    &model,
                    &JobParams {
                        load_case: if combo { None } else { Some(0) },
                        load_combination: if combo { Some(0) } else { None },
                        ..Default::default()
                    },
                )
                .unwrap() else {
                    panic!()
                };
                assert_eq!(summary["n_checks"], 3);
                assert_eq!(summary["n_skipped"], 3);
                assert_eq!(summary["n_ng"], 0);
                assert_eq!(summary["all_checked_and_ok"], false);
                assert_eq!(summary["all_members_checked_and_ok"], false);
                assert!(summary["max_ratio"].is_null());
                for row in summary["member_skipped"].as_array().unwrap() {
                    assert!(row["reason"].as_str().unwrap().contains("検定不能"));
                }
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&summary.to_string()).unwrap(),
                    summary
                );
            }
        }
    }

    #[test]
    fn steel601_mcp_rejects_invalid_assigned_steel_with_target_before_preparation() {
        use sepika_core::ids::{MaterialId, SectionId};
        use sepika_core::model::{Material, MaterialCategory};
        use sepika_core::section_shape::SectionShape;
        let model = fixture::input_model();
        assert!(matches!(
            compute_design_check_job(
                &model,
                &JobParams {
                    load_case: Some(0),
                    ..Default::default()
                }
            )
            .unwrap(),
            JobOutcome::DesignCheck { .. }
        ));
        for case in [
            "unassigned",
            "missing-reference",
            "non-steel",
            "direct-missing-fy",
            "direct-zero-fy",
            "direct-negative-fy",
            "direct-nan-fy",
            "direct-infinite-fy",
            "known-missing-thickness",
            "known-invalid-thickness",
            "known-out-of-range",
        ] {
            let mut work = model.clone();
            let material = MaterialId(work.materials.len() as u32);
            work.materials.push(Material {
                id: material,
                name: "直接入力鋼材".into(),
                category: MaterialCategory::Steel,
                young: 205000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(235.0),
                strength_factor: None,
                concrete_class: Default::default(),
            });
            let section = SectionId(work.sections.len() as u32);
            let mut steel_section = SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(0.0),
            }
            .to_section(section, "入力検査鋼梁".into());
            steel_section.material = Some(material);
            work.sections.push(steel_section);
            let target = work.elements[0].id;
            work.elements[0].section = Some(section);
            let sec = &mut work.sections[section.index()];
            let steel = &mut work.materials[material.index()];
            match case {
                "unassigned" => sec.material = None,
                "missing-reference" => sec.material = Some(MaterialId(999)),
                "non-steel" => steel.category = MaterialCategory::Concrete,
                "direct-missing-fy" => steel.fy = None,
                "direct-zero-fy" => steel.fy = Some(0.0),
                "direct-negative-fy" => steel.fy = Some(-1.0),
                "direct-nan-fy" => steel.fy = Some(f64::NAN),
                "direct-infinite-fy" => steel.fy = Some(f64::INFINITY),
                "known-missing-thickness" | "known-invalid-thickness" | "known-out-of-range" => {
                    steel.name = "SS400".into();
                    sec.shape = None;
                    sec.thickness = match case {
                        "known-missing-thickness" => None,
                        "known-invalid-thickness" => Some(-1.0),
                        _ => Some(101.0),
                    };
                }
                _ => unreachable!(),
            }
            let error = compute_design_check_job(&work, &JobParams::default())
                .err()
                .expect(case)
                .to_string();
            let expected = match case {
                "direct-missing-fy" | "direct-zero-fy" | "direct-negative-fy" | "direct-nan-fy"
                | "direct-infinite-fy" => "fy",
                "known-missing-thickness" | "known-invalid-thickness" => "板厚",
                "known-out-of-range" => "適用範囲",
                _ => "鋼材",
            };
            assert!(
                error.contains(&format!("部材 {}", target.0)) && error.contains(expected),
                "{case}: {error}"
            );
        }
    }

    #[test]
    fn mcp_design_crack_points_match_independent_fixture_in_json() {
        let JobOutcome::DesignCheck { summary, .. } = compute_design_check_job(
            &crack_fixture::input_model(fixture::input_model()),
            &JobParams {
                load_case: Some(0),
                ..Default::default()
            },
        )
        .unwrap() else {
            panic!()
        };
        let checks: Vec<WallCheck> =
            serde_json::from_value(summary["wall_checks"].clone()).unwrap();
        let reference = checks
            .iter()
            .find(|w| w.kind == WallCheckKind::ReferenceSkeleton)
            .unwrap();
        assert!(
            matches!(reference.outcome, CheckOutcome::Checked(_)),
            "{:?}",
            reference
        );
        crack_reference::assert_reference(reference.reference_skeleton.as_ref().unwrap());
        assert!(checks
            .iter()
            .filter(|w| w.kind == WallCheckKind::AllowableShear)
            .all(|w| w.reference_skeleton.is_none()));
    }

    #[test]
    fn mcp_rc_wall_static_case_and_combo_preserve_solved_batch_and_face_state() {
        use sepika_core::ids::LoadCaseId;
        use sepika_core::model::{LoadCase, LoadCombination, NodalLoad};
        let mut model = fixture::input_model();
        if let Some(sepika_core::section_shape::SectionShape::RcWall { pwh_ratio, .. }) =
            &mut model.sections[2].shape
        {
            *pwh_ratio = Some(0.006);
        }
        model.load_cases.push(LoadCase {
            origin: Default::default(),
            id: LoadCaseId(20),
            name: "独立水平P".into(),
            kind: LoadCaseKind::Live,
            nodal: vec![NodalLoad::manual(
                NodeId(2),
                [100_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            )],
            member: Vec::new(),
        });
        model.combinations.push(LoadCombination {
            name: "617同一バッチ".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(20), 1.0)],
        });
        let mut baseline: [Option<[f64; 6]>; 2] = [None, None];
        for kind in [
            sepika_core::model::ElementKind::Beam,
            sepika_core::model::ElementKind::Fiber,
            sepika_core::model::ElementKind::MultiSpring,
        ] {
            model.elements[0].kind = kind;
            let before = serde_json::to_vec(&model).unwrap();
            for combination in [false, true] {
                let params = JobParams {
                    load_case: (!combination).then_some(20),
                    load_combination: combination.then_some(0),
                    ..Default::default()
                };
                let JobOutcome::DesignCheck {
                    summary,
                    member_force_rows,
                    ..
                } = compute_design_check_job(&model, &params).unwrap()
                else {
                    panic!()
                };
                let (work, _) = model_prepared_for_analysis(&model, &params).unwrap();
                let terms = if combination {
                    work.combinations[0].terms.clone()
                } else {
                    vec![(LoadCaseId(20), 1.0)]
                };
                let solved: Vec<_> = terms
                    .iter()
                    .map(|(id, factor)| {
                        (
                            sepika_job::compute::compute_linear_static(work.clone(), *id).unwrap(),
                            *factor,
                        )
                    })
                    .collect();
                let references: Vec<_> = solved.iter().map(|(r, f)| (r, *f)).collect();
                let direct = sepika_solver::statics::linear::superpose_static(&references);
                assert_eq!(
                    serde_json::to_value(member_force_rows).unwrap(),
                    serde_json::to_value(flatten_member_force_rows(&direct.member_forces)).unwrap()
                );
                let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&work);
                let wall = expanded
                    .elements
                    .iter()
                    .find(|e| e.kind == sepika_core::model::ElementKind::Wall)
                    .unwrap();
                let batch: Vec<_> = direct
                    .member_forces
                    .iter()
                    .map(|(id, f)| (*id, f.at.as_slice()))
                    .collect();
                let face = sepika_element::wall::static_face::StaticWallFace::new(wall, &expanded)
                    .unwrap()
                    .recover(&batch)
                    .unwrap();
                assert_eq!(face.physical_area_mm2, 1_332_000.0);
                assert!((face.shear_n.abs() - 100_000.0).abs() < 1e-6);
                let checks: Vec<WallCheck> =
                    serde_json::from_value(summary["wall_checks"].clone()).unwrap();
                let reference = checks
                    .iter()
                    .find(|w| w.kind == WallCheckKind::ReferenceSkeleton)
                    .unwrap();
                assert!(reference.case.contains(if combination {
                    "combo:0:617"
                } else {
                    "case:20"
                }));
                let CheckOutcome::Checked(check) = &reference.outcome else {
                    panic!("{:?}", reference)
                };
                let detail = &check.components[0].detail;
                for expected in [
                    format!("Nface={:.1} kN", face.compression_n / 1000.0),
                    format!("Mface={:.1} kNm", face.moment_nmm / 1e6),
                    format!("Qface={:.1} kN", face.shear_n / 1000.0),
                ] {
                    assert!(detail.contains(&expected), "{detail}");
                }
                let skeleton = reference.reference_skeleton.as_ref().unwrap();
                let (crack_displacement_mm, crack_force_n) = skeleton.displacement_points[1];
                let observed = [
                    face.compression_n,
                    face.shear_n,
                    face.moment_nmm,
                    face.physical_area_mm2,
                    face.depth_mm,
                    crack_force_n / crack_displacement_mm,
                ];
                assert!(observed.iter().all(|value| value.is_finite()));
                if let Some(expected) = baseline[usize::from(combination)] {
                    for (actual, expected) in observed.into_iter().zip(expected) {
                        assert!(
                            (actual - expected).abs() <= 1e-8 * expected.abs().max(1.0),
                            "{kind:?} combination={combination}: {actual} != {expected}"
                        );
                    }
                } else {
                    baseline[usize::from(combination)] = Some(observed);
                }
            }
            assert_eq!(serde_json::to_vec(&model).unwrap(), before);
            assert!(compute_design_check_job(
                &model,
                &JobParams {
                    load_case: Some(999),
                    ..Default::default()
                }
            )
            .is_err());
        }
    }
    #[test]
    fn wall_output_assembly_keeps_independent_status_keys_counts_and_null_max() {
        let row = |id, outcome, why| WallCheck {
            plate: Some(WallPlateId(id)),
            elem: Some(ElemId(10 + id)),
            node: Some(NodeId(0)),
            case: "case:5".into(),
            kind: WallCheckKind::AllowableShear,
            reference_skeleton: None,
            shear_method: None,
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
        let summary = assemble_design_check_summary(&report, Some(5), LoadTerm::Short, false);
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
        let summary = assemble_design_check_summary(&report, Some(5), LoadTerm::Short, false);
        assert_eq!(summary["wall_summary"]["n_skipped"], 2);
        report.wall_checks[0].outcome = CheckOutcome::Skipped {
            reason: "壁応答欠落".into(),
        };
        report.wall_checks[0].skip_kind = Some(WallSkipKind::MissingResponse);
        let summary = assemble_design_check_summary(&report, Some(5), LoadTerm::Short, false);
        assert!(summary["max_ratio"].is_null());
        assert!(summary["wall_summary_by_kind"]["AllowableShear"]["max_ratio"].is_null());
        let empty =
            assemble_design_check_summary(&Default::default(), Some(5), LoadTerm::Short, false);
        assert!(empty["max_ratio"].is_null());
        assert_eq!(empty["all_checked_and_ok"], false);
    }
    #[test]
    fn panel_505_mcp_summary_retains_skipped_plane_and_denies_all_ok() {
        let report = sepika_design_jp::MemberDesignCheckReport {
            joint_checks: vec![
                (
                    NodeId(0),
                    "パネルゾーン(S) X-Z".into(),
                    CheckOutcome::Skipped {
                        reason: "接合部 ID 0 梁 db=580/480 上位置差=50 下位置差=-50 段違い".into(),
                    },
                ),
                (
                    NodeId(0),
                    "パネルゾーン(S) Y-Z".into(),
                    CheckOutcome::Checked(CheckResult {
                        basis: "標準形式".into(),
                        detail: String::new(),
                        components: vec![CheckComponent {
                            kind: CheckKind::Shear,
                            ratio: 0.5,
                            detail: String::new(),
                        }],
                    }),
                ),
            ],
            ..Default::default()
        };
        let summary = assemble_design_check_summary(&report, Some(0), LoadTerm::Short, false);
        assert_eq!(summary["all_checked_and_ok"], false);
        assert_eq!(summary["n_joint_checks"], 2);
        assert_eq!(summary["n_joint_skipped"], 1);
        assert_eq!(summary["n_joint_ng"], 0);
        assert_eq!(summary["max_ratio"], 0.5);
        assert!(summary["joint_skipped"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("上位置差=50"));
    }
}

#[cfg(test)]
mod rc_joint_1377_tests {
    mod fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/rc_joint_model.rs"
        ));
    }
    #[test]
    fn rc_joint_1377_summary_retains_unresolved_joint_and_denies_all_ok() {
        let model = fixture::model();
        let report = sepika_design_jp::MemberDesignCheckReport {
            joint_checks: sepika_design_jp::joint_wiring::collect_joint_checks(
                &model,
                &[],
                sepika_design_jp::LoadTerm::Short,
            ),
            ..Default::default()
        };
        let summary = super::assemble_design_check_summary(
            &report,
            Some(0),
            sepika_design_jp::LoadTerm::Short,
            false,
        );
        assert_eq!(summary["all_checked_and_ok"], false);
        assert_eq!(summary["n_joint_checks"], 1);
        assert_eq!(summary["n_joint_skipped"], 1);
        assert_eq!(summary["n_joint_ng"], 0);
        assert!(summary["max_ratio"].is_null());
        assert_eq!(summary["joint_skipped"][0]["node"], 0);
        assert_eq!(summary["joint_skipped"][0]["label"], "接合部終局(RC)");
        assert!(summary["joint_skipped"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("検討目的・設計状態"));
    }
}
