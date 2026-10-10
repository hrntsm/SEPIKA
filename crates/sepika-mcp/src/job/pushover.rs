//! 増分解析（プッシュオーバー解析）ジョブの純粋計算。
//!
//! - [`compute_pushover_job`] — Pushover ジョブの純粋計算部分。

use super::{attach_prepare_notices, JobDir, JobOutcome, JobParams};
use sepika_core::model::Model;
use sepika_core::units::to_display::force_kn;
use sepika_job::JobError;

/// Pushover ジョブの純粋計算部分。
pub(crate) fn compute_pushover_job(
    model: Model,
    params: &JobParams,
) -> Result<JobOutcome, JobError> {
    let input_model = model.clone();
    let mut work = model;
    let prepare_settings = params.analysis_settings_for_prepare();
    let prepare_report = sepika_job::prepare::prepare_model_for_analysis(
        &mut work,
        &prepare_settings,
        params.design_period,
    )?;
    let target = params.pushover_target();
    let cfg = sepika_job::AnalysisSettings {
        push_dir: match params.dir {
            JobDir::X => sepika_solver::statics::analysis::SeismicDir::X,
            JobDir::Y => sepika_solver::statics::analysis::SeismicDir::Y,
        },
        push_steps: params.steps,
        push_use_max_disp: target.max_disp.is_some(),
        push_max_disp: target.max_disp.unwrap_or_default(),
        push_use_drift_angle: target.max_drift_angle.is_some(),
        push_drift_denom: target
            .max_drift_angle
            .map(|a| 1.0 / a.max(f64::MIN_POSITIVE))
            .unwrap_or(200.0),
        ..prepare_settings
    };
    let input = serde_json::to_vec(&(input_model, cfg)).expect("増分解析の入力識別");
    let mut result = sepika_job::compute::compute_pushover(work, cfg)?;
    result.identify_wall_input(input);

    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    for (purpose, step) in [
        (EvaluationPurpose::Ds, params.ds_step),
        (EvaluationPurpose::HoldingCapacity, params.capacity_step),
    ] {
        if let Some(step) = step {
            let point = result
                .evaluation_point(purpose, cfg.push_dir, step, "MCPで明示指定したstep".into())
                .map_err(JobError::InvalidInput)?;
            if purpose == EvaluationPurpose::Ds {
                result.ds_evaluation = Some(point);
            } else {
                result.capacity_evaluation = Some(point);
            }
        }
    }
    let mut summary = pushover_summary(&result);
    summary["analysis_conditions"] = serde_json::to_value(cfg).expect("解析条件の直列化");
    attach_prepare_notices(&mut summary, prepare_report.notices);
    Ok(JobOutcome::Pushover { summary })
}

fn pushover_summary(
    result: &sepika_solver::nonlinear::pushover::PushoverResult,
) -> serde_json::Value {
    let mechanism = match result.mechanism {
        sepika_solver::nonlinear::pushover::MechanismType::Overall => "Overall".to_string(),
        sepika_solver::nonlinear::pushover::MechanismType::StoryCollapse { layer } => {
            format!("StoryCollapse(layer={layer})")
        }
        sepika_solver::nonlinear::pushover::MechanismType::Partial => "Partial".to_string(),
    };
    serde_json::json!({
        "kind": "Pushover",
        "max_base_shear_kN": force_kn(result.qu),
        "mechanism": mechanism,
        "n_steps": result.steps.len(),
        "wall_run": result.wall_run,
        "wall_history": result.wall_history,
        "confirmed_history": result.confirmed_history,
        "steps": result.steps,
        "capacity_curve": result.capacity_curve,
        "ds_evaluation": result.ds_evaluation,
        "capacity_evaluation": result.capacity_evaluation,
        "termination": result.termination,
        "control": result.control,
        "ds_story_evaluation": result.ds_evaluation.as_ref().map(|p| result.evaluate_stories(p, sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose::Ds)),
        "capacity_story_evaluation": result.capacity_evaluation.as_ref().map(|p| result.evaluate_stories(p, sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose::HoldingCapacity)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, WallPlateId};
    use sepika_solver::nonlinear::pushover::wall_response::WallUnavailableReason;
    #[test]
    fn mcp_wall_history_has_plate_run_input_and_null_distinct_from_zero() {
        let mut model = crate::tests::rc_column_model();
        let mut bottom = model.nodes[0].clone();
        bottom.id = NodeId(2);
        bottom.coord = [4000.0, 0.0, 0.0];
        let mut top = model.nodes[1].clone();
        top.id = NodeId(3);
        top.coord = [4000.0, 0.0, 3000.0];
        model.nodes.extend([bottom, top]);
        let mut girder = model.sections[0].clone();
        girder.id = SectionId(1);
        girder.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
        model.sections.push(girder);
        for (id, nodes, section) in [(1, [0, 2], 1), (2, [1, 3], 1), (3, [2, 3], 0)] {
            let mut e = model.elements[0].clone();
            e.id = ElemId(id);
            e.nodes = nodes.map(NodeId).into_iter().collect();
            e.section = Some(SectionId(section));
            model.elements.push(e);
        }
        let mut section = sepika_core::section_shape::SectionShape::RcWall {
            thickness: 180.0,
            ps: 0.002,
            pwh_ratio: Some(0.002),
        }
        .to_section(SectionId(2), "壁".into());
        section.material = Some(MaterialId(0));
        section.rebar_material = Some(MaterialId(1));
        section.shear_rebar_material = Some(MaterialId(1));
        model.sections.push(section);
        model.add_enclosed_wall_plate_from_nodes(
            &[NodeId(0), NodeId(2), NodeId(3), NodeId(1)],
            sepika_core::model::WallPlate {
                id: WallPlateId(0),
                shape: sepika_core::model::WallPlateShape::Enclosed,
                section: Some(SectionId(2)),
                self_weight_shares: vec![],
                opening_area: 0.0,
                opening_weight: 0.0,
                openings: vec![],
                loads: vec![],
                slit: Default::default(),
            },
        );
        model.stories = sepika_load::story_gen::generate_stories_with_opts(
            &model,
            &[],
            false,
            model.mass_method,
        )
        .expect("階生成")
        .stories;
        let upper_floor = model.stories.last_mut().unwrap();
        upper_floor.weight_override = Some(500_000.0);
        upper_floor.seismic_weight = Some(500_000.0);
        let outcome = compute_pushover_job(
            model,
            &JobParams {
                steps: 3,
                max_disp: Some(1.0),
                ds_step: Some(1),
                capacity_step: Some(1),
                ..Default::default()
            },
        )
        .expect("MCP壁解析");
        let JobOutcome::Pushover { summary } = outcome else {
            panic!("壁結果");
        };
        assert!(summary["wall_run"]["input_generation"].is_array());
        assert!(summary["wall_run"]["run_id"].is_string());
        assert_eq!(summary["ds_evaluation"]["step"], 1);
        assert_eq!(summary["capacity_evaluation"]["step"], 1);
        assert_eq!(summary["ds_evaluation"]["purpose"], "Ds");
        assert!(summary["analysis_conditions"].is_object());
        let ds = &summary["ds_story_evaluation"]["Ok"][0];
        assert!(ds["qu_n"].is_number(), "{}", summary["ds_story_evaluation"]);
        assert!(ds["residual_n"].as_f64().unwrap().abs() <= ds["tolerance_n"].as_f64().unwrap());
        let confirmed = summary["confirmed_history"].as_array().unwrap();
        assert_eq!(
            confirmed.len(),
            summary["n_steps"].as_u64().unwrap() as usize
        );
        let initial: sepika_solver::nonlinear::pushover::story_response::StoryCut =
            serde_json::from_value(confirmed[0]["cuts"][0].clone()).unwrap();
        assert!(initial.evaluate().unwrap_err().contains("分母"));
        assert!((ds["qu_n"].as_f64().unwrap() - 100_000.0).abs() < 0.001);
        for r in confirmed {
            assert_eq!(r["run_id"], summary["wall_run"]["run_id"]);
            assert_eq!(
                r["input_generation"],
                summary["wall_run"]["input_generation"]
            );
            assert!(r["cuts"][0]["reference_n"].is_number());
            assert!(r["cuts"][0]["support_n"].is_number());
        }
        let records = summary["wall_history"].as_array().unwrap();
        assert_eq!(records.len(), summary["n_steps"].as_u64().unwrap() as usize);
        for (step, r) in records.iter().enumerate() {
            assert_eq!(r["step"], step);
            assert_eq!(r["plate"], 0);
            assert!(r["unavailable"].is_null());
            let response = &r["response"];
            assert!(response["qw_n"].is_number());
            assert!(response["qdir_n"].is_number());
            assert!(response["material_shear_strain"].is_null());
            assert_eq!(
                response["material_shear_unavailable"],
                "MaterialShearNotRecovered"
            );
            assert!(response["line_events"].is_null());
            assert_eq!(
                response["line_events_unavailable"],
                "LineEventsNotApplicable"
            );
        }
        let mut result:sepika_solver::nonlinear::pushover::PushoverResult=serde_json::from_value(serde_json::json!({
            "steps":[],"capacity_curve":[],"hinges":[],"shear_yields":[],"mechanism":"Partial","qu":0.0,"member_response":[],
            "wall_history":records,"wall_run":summary["wall_run"]
        })).unwrap();
        result.qu = 150_000.0;
        let unselected = pushover_summary(&result);
        assert_eq!(unselected["max_base_shear_kN"], 150.0);
        assert!(unselected.get("qu_kN").is_none());
        assert!(unselected["capacity_story_evaluation"].is_null());
        result.steps = serde_json::from_value(summary["steps"].clone()).unwrap();
        result.capacity_curve = serde_json::from_value(summary["capacity_curve"].clone()).unwrap();
        result.confirmed_history =
            serde_json::from_value(summary["confirmed_history"].clone()).unwrap();
        result.capacity_evaluation =
            serde_json::from_value(summary["capacity_evaluation"].clone()).unwrap();
        let record = result
            .confirmed_history
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r.step == 1)
            .unwrap();
        for cut in &mut record.cuts {
            let total: f64 = cut.forces.iter().map(|f| f.force_n).sum();
            for force in &mut cut.forces {
                force.force_n *= 120_000.0 / total;
            }
            cut.external_n = 120_000.0;
        }
        let selected = pushover_summary(&result);
        assert_eq!(selected["max_base_shear_kN"], 150.0);
        assert!(
            (selected["capacity_story_evaluation"]["Ok"][0]["qu_n"]
                .as_f64()
                .unwrap()
                - 120_000.0)
                .abs()
                < 1e-6
        );
        for (wall, frame, external, reason, beta, residual) in [
            (90.0, 60.0, 120.0, "釣合い残差", "0.6", "30"),
            (160.0, -10.0, 150.0, "範囲外", "1.0666666666666667", "0"),
            (0.0, 0.0, 0.0, "分母", "未定義", "0"),
            (-10.0, 160.0, 150.0, "範囲外", "-0.06666666666666667", "0"),
        ] {
            use sepika_solver::nonlinear::pushover::story_response::{CutForce, ForceGroup};
            let point = result.capacity_evaluation.as_ref().unwrap();
            let identity = format!("run={}", point.run_id);
            let cut = &mut result
                .confirmed_history
                .as_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r.step == point.step)
                .unwrap()
                .cuts[0];
            cut.forces = vec![
                CutForce {
                    elem: ElemId(0),
                    group: ForceGroup::Wall,
                    force_n: wall,
                },
                CutForce {
                    elem: ElemId(1),
                    group: ForceGroup::Frame,
                    force_n: frame,
                },
            ];
            cut.external_n = external;
            cut.reference_n = 100.0;
            cut.support_n = 2.0;
            cut.tolerance_n = 1e-6;
            let published = pushover_summary(&result);
            let error = published["capacity_story_evaluation"]["Err"]
                .as_str()
                .unwrap();
            for fragment in [
                reason.to_string(),
                "purpose=HoldingCapacity".into(),
                identity,
                "direction=X".into(),
                "step=1".into(),
                format!("Qu={} N", wall + frame),
                format!("Wall={wall} N"),
                "Brace=0 N".into(),
                format!("Frame={frame} N"),
                format!("上層外力={external} N"),
                "基準外力=100 N".into(),
                "支持ばね内力=2 N".into(),
                format!("残差={residual} N"),
                "許容差=0.000001 N".into(),
                format!("βu={beta} [-]"),
            ] {
                assert!(error.contains(&fragment), "{fragment}: {error}");
            }
            assert!(published["capacity_story_evaluation"].get("Ok").is_none());
        }
        let r = result.wall_history.as_mut().unwrap().first_mut().unwrap();
        r.response.as_mut().unwrap().qdir_n = 0.0;
        assert_eq!(
            pushover_summary(&result)["wall_history"][0]["response"]["qdir_n"],
            0.0
        );
        let r = result.wall_history.as_mut().unwrap().first_mut().unwrap();
        r.response = None;
        r.unavailable = Some(WallUnavailableReason::InvalidForce);
        let json = pushover_summary(&result);
        assert!(json["wall_history"][0]["response"].is_null());
        assert_eq!(json["wall_history"][0]["unavailable"], "InvalidForce");
        result.wall_history = None;
        assert!(pushover_summary(&result)["wall_history"].is_null());
    }
}
