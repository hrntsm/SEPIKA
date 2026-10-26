use sepika_job::{
    compute::{compute_time_history, effective_time_history_wave},
    AnalysisSettings, JobError,
};
use sepika_solver::dynamic::timehistory::GroundMotion;

fn wave() -> GroundMotion {
    GroundMotion {
        dt: 0.01,
        accel_x: vec![0.0, 1.0, 0.0],
        accel_y: None,
        accel_theta: Some(vec![3.0; 3]),
    }
}

#[test]
fn disabled_phase_difference_preserves_all_input_components() {
    let cfg = AnalysisSettings {
        phase_diff_dir_y: true,
        ..Default::default()
    };
    let result = effective_time_history_wave(&cfg, wave()).unwrap();
    assert_eq!(result.accel_x, vec![0.0, 1.0, 0.0]);
    assert!(result.accel_y.is_none());
    assert_eq!(result.accel_theta.unwrap(), vec![3.0; 3]);
}

#[test]
fn y_phase_difference_requires_y_acceleration() {
    let cfg = AnalysisSettings {
        phase_diff_enabled: true,
        phase_diff_dir_y: true,
        ..Default::default()
    };
    for result in [
        effective_time_history_wave(&cfg, wave()).map(|_| ()),
        compute_time_history(Default::default(), cfg, wave()).map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(JobError::InvalidInput(reason)) if reason.contains("Y方向加速度"))
        );
    }
}

#[test]
fn phase_difference_uses_the_selected_component() {
    let cfg = AnalysisSettings {
        phase_diff_enabled: true,
        phase_diff_length_m: 20.0,
        phase_diff_vs: 200.0,
        phase_diff_incidence_deg: 30.0,
        ..Default::default()
    };
    let mut input = wave();
    input.accel_y = Some(vec![0.0, 2.0, 0.0]);
    let x = effective_time_history_wave(&cfg, wave()).unwrap();
    let y = effective_time_history_wave(
        &AnalysisSettings {
            phase_diff_dir_y: true,
            ..cfg
        },
        input,
    )
    .unwrap();
    assert_eq!(x.accel_theta.unwrap(), vec![0.0, 0.00005, 0.0]);
    assert_eq!(y.accel_theta.unwrap(), vec![0.0, 0.0001, 0.0]);
}
