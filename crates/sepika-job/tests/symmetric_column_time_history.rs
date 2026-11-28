use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::model::{
    EndCondition, MassMethod, MemberLoad, MemberLoadKind, Model, SteelDesignAttr,
};
use sepika_element::frame::beam::BeamElement;
use sepika_job::compute::{compute_confirmed_time_history, compute_time_history};
use sepika_job::time_history_check::TimeHistoryColumnCheck;
use sepika_job::time_history_restart::{
    compute_checkpoint, resume_checkpoint, LinearTimeHistoryCheckpoint,
};
use sepika_job::AnalysisSettings;
use sepika_solver::common::member_response::ConfirmedLoadState;
use sepika_solver::dynamic::damping::Damping;
use sepika_solver::dynamic::timehistory::{GroundMotion, NewmarkCfg};
use sepika_solver::statics::analysis::Analysis;

#[path = "../../sepika-design-jp/tests/fixtures/steel_h_column_combined_model.rs"]
mod combined_column;

fn confirm_supports(model: &mut Model) {
    model.steel_design_attrs.push(SteelDesignAttr {
        elem: model.elements[0].id,
        joint_flange_loss: 0.0,
        joint_web_loss: 0.0,
        scallop_web_loss: 0.0,
        lb_direct: None,
        lateral_brace_count: None,
        column_lateral_buckling_end_supports: model.column_end_supports(&model.elements[0]),
        lk_y_direct: None,
        lk_z_direct: None,
        c_direct: Some(2.3),
    });
}

#[test]
fn unrecorded_linear_bending_failure_survives_checkpoint_round_trip_and_restart() {
    let mut model = combined_column::cantilever(0.0, 0.0, 0.0);
    model.load_cases.clear();
    model.elements[0].end_cond[1] = EndCondition::Pinned;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.nodes[1].restraint.set_free(Dof::Ux);
    model.nodes[1].mass = Some([20.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    model.mass_method = MassMethod::LumpedOnly;
    confirm_supports(&mut model);

    // 平均加速度法の初回応答を 259 kN·m に置き、次の時刻で変位をゼロに戻す。
    let k_local = BeamElement::new(&model.elements[0], &model).local_stiffness();
    let u_mm = 259e6 / k_local.get(5, 7).abs();
    let dt = 0.01_f64;
    let wave = GroundMotion {
        dt,
        accel_x: vec![
            0.0,
            -(4.0 / dt.powi(2) + k_local.get(7, 7) / 20.0) * u_mm,
            16.0 / dt.powi(2) * u_mm,
        ],
        accel_y: None,
        accel_theta: None,
    };
    let dense = AnalysisSettings {
        th_damping: 0.0,
        th_record_every: 1,
        ..Default::default()
    };
    let sparse = AnalysisSettings {
        th_record_every: 2,
        ..dense
    };
    let full = compute_time_history(model.clone(), dense, wave.clone())
        .unwrap()
        .recording
        .unwrap();
    let solved = compute_confirmed_time_history(model.clone(), sparse, wave.clone()).unwrap();
    let recording = solved.response().recording.as_ref().unwrap();
    assert!(!recording.frame_time.contains(&dt));
    assert_eq!(
        recording.symmetric_column_bending,
        full.symmetric_column_bending
    );
    let column_id = model.elements[0].id;
    let TimeHistoryColumnCheck::SymmetricBending(Ok(item)) = solved.column_check(column_id) else {
        panic!("対象柱には解析で算定した横座屈照査がある");
    };
    assert_eq!(
        (item.committed_count, item.evaluated_count),
        (3, 3),
        "{item:?}"
    );
    assert!(item.unavailable.is_empty(), "{item:?}");
    let peak = item.peak.as_ref().unwrap();
    assert_eq!((peak.time_s, peak.max_x_mm), (dt, 0.0), "{peak:?}");
    assert!((peak.max_mz_nmm.abs() - 259e6).abs() < 1e-5, "{peak:?}");
    assert!((peak.c - 1.75).abs() < 1e-12, "{peak:?}");
    assert!((peak.ratio - 1.007_72).abs() < 1e-5, "{peak:?}");

    let elem = &model.elements[0];
    let bounds = [0.0, item.lb_mm.unwrap()];
    let mut confirmed_times = Vec::new();
    Analysis::prepare(&model)
        .unwrap()
        .time_history_with_force_observer(
            &wave,
            NewmarkCfg::average_accel(),
            Damping::MassProportional { h: 0.0, omega: 1.0 },
            Some(2),
            Some(&mut |snapshot| {
                confirmed_times.push(snapshot.time_s());
                if snapshot.step() != peak.step {
                    return;
                }
                let member = snapshot.member(elem.id).unwrap();
                assert_eq!(member.member_id(), elem.id);
                assert_eq!(
                    member.load_state(),
                    ConfirmedLoadState::TimeHistory {
                        step: peak.step,
                        time_s: peak.time_s,
                        applied_long_term: false,
                    }
                );
                assert_eq!(bounds, [0.0, 3000.0]);
                let shorter = [0.0, 3_000.0_f64.next_down()];
                assert!(matches!(member.strong_bending_span(shorter, shorter),
                    Err(reason) if reason.contains("解析応答の全長と一致しません")));
                let span = member.strong_bending_span(bounds, bounds).unwrap();
                assert!(span.monotonic && span.unverified_distribution.is_none());
                assert_eq!(span.end_moments_nmm, peak.end_moments_nmm);
                assert!((span.end_moments_nmm[0].abs() - 259e6).abs() < 1e-5);
                let recorded_mz = member.recorded_section_forces(0.0).unwrap()[5];
                assert!((span.end_moments_nmm[0] - recorded_mz).abs() < 1e-5);
                assert!(member.strong_bending_span([1.0, 3000.0], bounds).is_err());
            }),
        )
        .unwrap();
    assert_eq!(confirmed_times, [0.0, dt, 2.0 * dt]);

    let checkpoint = compute_checkpoint(model.clone(), sparse, wave.clone(), 1).unwrap();
    let checkpoint =
        LinearTimeHistoryCheckpoint::from_bytes(&checkpoint.to_bytes().unwrap()).unwrap();
    let resumed = resume_checkpoint(model, sparse, wave, checkpoint)
        .unwrap()
        .recording
        .unwrap();
    assert_eq!(
        resumed.symmetric_column_bending,
        recording.symmetric_column_bending
    );
}

#[test]
fn nonlinear_history_uses_the_actual_point_load_to_find_an_off_center_maximum() {
    let mut model = combined_column::cantilever(0.0, 0.0, 0.0);
    model.elements[0].end_cond = [EndCondition::Pinned; 2];
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.nodes[1].restraint.set_free(Dof::Uy);
    model.nodes[1].support_spring = Some([0.0, 10_000.0, 0.0, 0.0, 0.0, 0.0]);
    model.nodes[1].mass = Some([0.0, 20.0, 0.0, 0.0, 0.0, 0.0]);
    model.mass_method = MassMethod::LumpedOnly;
    let case = &mut model.load_cases[0];
    case.nodal.clear();
    case.member = vec![MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Point { a: 1000.0, p: 60e3 },
    )];
    confirm_supports(&mut model);
    let result = compute_time_history(
        model,
        AnalysisSettings {
            th_nonlinear: true,
            th_apply_long_term: true,
            th_damping: 0.0,
            th_record_every: 2,
            ..Default::default()
        },
        GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0; 3],
            accel_y: None,
            accel_theta: None,
        },
    )
    .unwrap();
    let recording = result.recording.unwrap();
    let item = &recording.symmetric_column_bending[&0];
    assert_eq!(
        (item.committed_count, item.evaluated_count),
        (3, 3),
        "{item:?}"
    );
    assert!(item.unavailable.is_empty(), "{item:?}");
    let peak = item.peak.as_ref().unwrap();
    assert_eq!(peak.max_x_mm, 1000.0);
    assert!((peak.max_mz_nmm.abs() - 40e6).abs() < 1e-6, "{peak:?}");
    assert_eq!((peak.monotonic, peak.c, peak.p_lambda_b), (false, 1.0, 0.3));
    for m in peak.end_moments_nmm {
        assert!(m.abs() < 1e-6, "{peak:?}");
    }
}
