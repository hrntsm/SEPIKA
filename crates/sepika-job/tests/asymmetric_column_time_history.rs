use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::model::MassMethod;
use sepika_element::frame::beam::BeamElement;
use sepika_job::compute::compute_time_history;
use sepika_job::time_history_restart::{
    compute_checkpoint, resume_checkpoint, LinearTimeHistoryCheckpoint,
};
use sepika_job::AnalysisSettings;
use sepika_solver::dynamic::timehistory::GroundMotion;

#[path = "fixtures/asymmetric_column_model.rs"]
mod fixture;

const DT_S: f64 = 0.01;
const LENGTH_MM: f64 = 3000.0;
const MASS: f64 = 20.0;
const AXIS_X: f64 = 0.6;
const AXIS_Z: f64 = 0.8;
const N_KN: [f64; 4] = [-800.0, -400.0, -1.0, -1.0];
const MZ_KNM: [f64; 4] = [0.0, 125.0, 130.0, -145.0];

fn wave_for_displacements(displacement: [f64; 4], stiffness: f64) -> Vec<f64> {
    let mut ground_accel = vec![0.0];
    let (mut u, mut v, mut a) = (0.0, 0.0, 0.0);
    for next_u in displacement {
        let avg_a = (2.0 * (next_u - u) / DT_S - 2.0 * v) / DT_S;
        let next_a = 2.0 * avg_a - a;
        ground_accel.push(-(next_a + stiffness / MASS * next_u));
        v += avg_a * DT_S;
        (u, a) = (next_u, next_a);
    }
    ground_accel
}

#[test]
fn peak_section_failure_between_force_extrema_survives_frame_thinning_and_restart() {
    let mut model = fixture::column_model();
    model.nodes[1].coord = [AXIS_X * LENGTH_MM, 0.0, AXIS_Z * LENGTH_MM];
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.nodes[1].restraint.set_free(Dof::Ux);
    model.nodes[1].restraint.set_free(Dof::Uy);
    model.nodes[1].mass = Some([MASS, MASS, 0.0, 0.0, 0.0, 0.0]);
    model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
    model.mass_method = MassMethod::LumpedOnly;

    // 断面力を指定するため、節点の変位と入力波を回転拘束時の線形剛性から逆算する。
    let stiffness = BeamElement::new(&model.elements[0], &model).local_stiffness();
    let axial_stiffness = stiffness.get(6, 6);
    let bending_stiffness = stiffness.get(7, 7);
    let i_end_moment_per_mm = -stiffness.get(5, 7);
    let x_stiffness = axial_stiffness * AXIS_X.powi(2) + stiffness.get(8, 8) * AXIS_Z.powi(2);
    let wave = GroundMotion {
        dt: DT_S,
        accel_x: wave_for_displacements(
            N_KN.map(|n| n * 1e3 / (axial_stiffness * AXIS_X)),
            x_stiffness,
        ),
        accel_y: Some(wave_for_displacements(
            MZ_KNM.map(|mz| mz * 1e6 / i_end_moment_per_mm),
            bending_stiffness,
        )),
        accel_theta: None,
    };
    let fully_recorded = AnalysisSettings {
        th_damping: 0.0,
        th_record_every: 1,
        ..Default::default()
    };
    let sparse = AnalysisSettings {
        th_record_every: 3,
        ..fully_recorded
    };
    let observed = compute_time_history(model.clone(), fully_recorded, wave.clone())
        .unwrap()
        .recording
        .unwrap();
    for (index, (n, mz)) in N_KN.into_iter().zip(MZ_KNM).enumerate() {
        let [n_n, _, _, _, _, mz_nmm] =
            observed.member_forces[index + 1][0].as_ref().unwrap().at[0].1;
        assert!((n_n / 1e3 - n).abs() < 1e-8, "{index}: {n_n}");
        assert!((mz_nmm / 1e6 - mz).abs() < 1e-8, "{index}: {mz_nmm}");
    }

    let result = compute_time_history(model.clone(), sparse, wave.clone()).unwrap();
    let record = result.recording.as_ref().unwrap();
    assert!(!record.frame_time.contains(&(2.0 * DT_S)));
    assert_eq!(
        record.asymmetric_column_sections,
        observed.asymmetric_column_sections
    );
    let column = &record.asymmetric_column_sections[&0];
    assert_eq!(column.committed_count, 5);
    for (position, (xi, governing_step, ratio)) in column.positions.iter().zip([
        (0.0, 2.0, 1.065_016_34),
        (0.5, 1.0, 0.453_658_76),
        (1.0, 4.0, 0.972_863_95),
    ]) {
        let peak = position.peak.as_ref().unwrap();
        assert_eq!(position.xi, xi);
        assert_eq!(position.observed_count, 5);
        assert_eq!(peak.time_s, governing_step * DT_S);
        assert!((peak.ratio - ratio).abs() < 1e-7, "{xi}: {peak:?}");
        assert!(position
            .unavailable
            .iter()
            .any(|reason| reason.contains("曲げねじり座屈")));
    }
    assert!(!record.axial_force_peaks[0]
        .snapshots
        .iter()
        .any(|snap| snap.time == 2.0 * DT_S));
    assert!(!record.strong_axis_moment_peaks[&0]
        .snapshots
        .iter()
        .any(|snap| snap.time == 2.0 * DT_S && snap.positions.contains(&0)));

    let checkpoint = compute_checkpoint(model.clone(), sparse, wave.clone(), 2).unwrap();
    let checkpoint =
        LinearTimeHistoryCheckpoint::from_bytes(&checkpoint.to_bytes().unwrap()).unwrap();
    let resumed = resume_checkpoint(model, sparse, wave, checkpoint).unwrap();
    assert_eq!(
        resumed.recording.unwrap().asymmetric_column_sections,
        record.asymmetric_column_sections
    );
}
