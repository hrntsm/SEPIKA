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

    let mut summary = pushover_summary(&result);
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
        "qu_kN": force_kn(result.qu),
        "mechanism": mechanism,
        "n_steps": result.steps.len(),
        "wall_run": result.wall_run,
        "wall_history": result.wall_history,
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
                ..Default::default()
            },
        )
        .expect("MCP壁解析");
        let JobOutcome::Pushover { summary } = outcome else {
            panic!("壁結果");
        };
        assert!(summary["wall_run"]["input_generation"].is_array());
        assert!(summary["wall_run"]["run_id"].is_string());
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
