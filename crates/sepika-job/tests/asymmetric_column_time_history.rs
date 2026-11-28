use sepika_core::dof::{Dof, Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{MassMethod, MemberLoad, MemberLoadKind};
use sepika_design_jp::steel::{frame_buckling_lengths, SteelDesign};
use sepika_design_jp::{DesignCtx, LoadTerm, MemberForcesAt, MemberKind};
use sepika_element::frame::beam::BeamElement;
use sepika_job::compute::{compute_confirmed_time_history, compute_time_history};
use sepika_job::time_history_check::TimeHistoryColumnCheck;
use sepika_job::time_history_restart::{
    compute_checkpoint, resume_checkpoint, LinearTimeHistoryCheckpoint,
};
use sepika_job::AnalysisSettings;
use sepika_solver::common::constraint::Reducer;
use sepika_solver::dynamic::damping::{Damping, DampingAccumulation};
use sepika_solver::dynamic::timehistory::{
    nonlinear_time_history_with_force_observer, GroundMotion, NewmarkCfg, NonlinearThCfg,
};

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

    let solved = compute_confirmed_time_history(model.clone(), sparse, wave.clone()).unwrap();
    let record = solved.response().recording.as_ref().unwrap();
    assert!(!record.frame_time.contains(&(2.0 * DT_S)));
    assert_eq!(
        record.asymmetric_column_sections,
        observed.asymmetric_column_sections
    );
    let TimeHistoryColumnCheck::AsymmetricSection(Ok(column)) =
        solved.column_check(model.elements[0].id)
    else {
        panic!("対象柱には解析で算定した断面照査がある");
    };
    assert_eq!(column.committed_count, 5);
    assert_eq!(column.positions.len(), 3);
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

    let elem = &model.elements[0];
    let (lk_y, lk_z) = frame_buckling_lengths(&model, elem, MemberKind::Column, None);
    let ratio_at_i = |cut| {
        SteelDesign
            .asymmetric_column_section_check(
                &MemberForcesAt::from_local(0.0, cut),
                model.element_section(elem).unwrap(),
                model.element_material(elem).unwrap(),
                &DesignCtx {
                    term: LoadTerm::Short,
                    kind: MemberKind::Column,
                    length: LENGTH_MM,
                    lk_y: Some(lk_y),
                    lk_z: Some(lk_z),
                    ..Default::default()
                },
            )
            .unwrap()
            .0
    };
    let at_i = |step: usize| observed.member_forces[step][0].as_ref().unwrap().at[0].1;
    assert_eq!(ratio_at_i(at_i(0)), 0.0);
    let peak = column.positions[0].peak.as_ref().unwrap();
    let simultaneous = at_i(2);
    assert_eq!((peak.n_n, peak.mz_nmm), (simultaneous[0], simultaneous[5]));
    assert!((peak.ratio - ratio_at_i(simultaneous)).abs() < 1e-12);
    let mut mixed = simultaneous;
    mixed[0] = at_i(1)[0];
    assert!((peak.ratio - ratio_at_i(mixed)).abs() > 0.1);

    let checkpoint = compute_checkpoint(model.clone(), sparse, wave.clone(), 2).unwrap();
    let checkpoint =
        LinearTimeHistoryCheckpoint::from_bytes(&checkpoint.to_bytes().unwrap()).unwrap();
    let resumed = resume_checkpoint(model, sparse, wave, checkpoint).unwrap();
    assert_eq!(
        resumed.recording.unwrap().asymmetric_column_sections,
        record.asymmetric_column_sections
    );
}

#[test]
fn column_checks_follow_each_run_when_two_columns_change_element_order() {
    let mut model = fixture::column_model();
    model.nodes[1].mass = Some([MASS, MASS, MASS, 0.0, 0.0, 0.0]);
    model.mass_method = MassMethod::LumpedOnly;
    let mut other_column = model.elements[0].clone();
    other_column.id = ElemId(1);
    other_column.rigid_zone.face_j = Some(750.0);
    let mut other_nodes = model.nodes.clone();
    for node in &mut other_nodes {
        node.id = NodeId(node.id.0 + 2);
        node.coord[0] = 3_000.0;
    }
    other_column.nodes = [NodeId(2), NodeId(3)].into_iter().collect();
    model.nodes.extend(other_nodes);
    model.elements.push(other_column);
    let mut reordered = model.clone();
    reordered.elements.swap(0, 1);
    reordered.elements[0].id = ElemId(0);
    reordered.elements[1].id = ElemId(1);
    for (input, end_positions) in [(model, [1.0, 0.75]), (reordered, [0.75, 1.0])] {
        let solved = compute_confirmed_time_history(
            input,
            AnalysisSettings {
                th_damping: 0.0,
                ..Default::default()
            },
            GroundMotion {
                dt: DT_S,
                accel_x: vec![0.0, 0.0],
                accel_y: None,
                accel_theta: None,
            },
        )
        .unwrap();
        for (id, last_position) in [ElemId(0), ElemId(1)].into_iter().zip(end_positions) {
            let TimeHistoryColumnCheck::AsymmetricSection(Ok(history)) = solved.column_check(id)
            else {
                panic!("対象柱の断面照査を取得できない");
            };
            let position = history
                .positions
                .iter()
                .find(|position| position.xi == last_position)
                .unwrap();
            assert!(position.peak.as_ref().is_some_and(|peak| peak.ratio == 0.0));
            assert_eq!(
                history.positions.iter().any(|position| position.xi == 0.75),
                last_position == 0.75
            );
        }
    }
}

#[test]
fn nonlinear_j_face_does_not_check_the_rounded_right_cut_at_a_point_load() {
    let length_mm = 8_395.6;
    let face_j_mm = 498.3;
    let point_mm = length_mm - face_j_mm;
    let xi = 1.0 - face_j_mm / length_mm;
    assert_eq!(xi, point_mm / length_mm);
    assert!(xi * length_mm > point_mm);
    assert_nonlinear_j_face_cut(length_mm, face_j_mm, point_mm, 0.0, false);
}

#[test]
fn nonlinear_j_face_does_not_take_a_point_load_left_limit_at_a_different_face() {
    let length_mm = 8_000.0;
    let face_j_mm = 262.375;
    let face_mm = 7_737.625_f64;
    let point_mm = face_mm.next_down();
    let xi = 1.0 - face_j_mm / length_mm;
    assert_eq!(length_mm - face_j_mm, face_mm);
    assert_eq!(xi * length_mm, point_mm);
    assert_nonlinear_j_face_cut(length_mm, face_j_mm, point_mm, -150e3, false);
}

#[test]
fn nonlinear_j_face_checks_the_rounded_cut_on_the_same_side_of_a_point_load() {
    let length_mm = 8_000.0;
    let face_j_mm = 262.375;
    let face_mm = 7_737.625;
    let point_mm = face_mm + 2.0_f64.powi(-36);
    let xi = 1.0 - face_j_mm / length_mm;
    assert_eq!(length_mm - face_j_mm, face_mm);
    assert_eq!(xi * length_mm, 7_737.624_999_999_999);
    assert!(xi * length_mm < face_mm && face_mm < point_mm);
    assert_nonlinear_j_face_cut(length_mm, face_j_mm, point_mm, -150e3, true);
}

fn assert_nonlinear_j_face_cut(
    length_mm: f64,
    face_j_mm: f64,
    point_mm: f64,
    recorded_n: f64,
    same_cut: bool,
) {
    let mut model = fixture::column_model();
    let xi = 1.0 - face_j_mm / length_mm;
    model.nodes[1].coord[2] = length_mm;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.nodes[1].restraint.set_free(Dof::Uz);
    model.nodes[1].mass = Some([0.0, 0.0, MASS, 0.0, 0.0, 0.0]);
    model.mass_method = MassMethod::LumpedOnly;
    let elem = &mut model.elements[0];
    elem.rigid_zone.face_j = Some(face_j_mm);
    let id = elem.id;
    model.load_cases[0].nodal.clear();
    model.load_cases[0].member = vec![MemberLoad::manual(
        id,
        [0.0, 0.0, 1.0],
        MemberLoadKind::Point {
            a: point_mm,
            p: -150e3,
        },
    )];
    let wave = GroundMotion {
        dt: DT_S,
        accel_x: vec![0.0; 3],
        accel_y: None,
        accel_theta: None,
    };

    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    let mut observed = Vec::new();
    let response = nonlinear_time_history_with_force_observer(
        &model,
        &dofmap,
        &reducer,
        &wave,
        &NewmarkCfg::average_accel(),
        &Damping::MassProportional { h: 0.0, omega: 1.0 },
        DampingAccumulation::NonCumulative,
        &[0.0],
        &[0.0],
        NonlinearThCfg {
            apply_long_term: true,
            ..Default::default()
        },
        Some(&mut |step| {
            observed.push(step.step());
            let member = step.member(id).unwrap();
            let face = member.recorded_section_forces(xi);
            if same_cut {
                assert!((face.unwrap()[0] - recorded_n).abs() < 1e-7);
            } else {
                assert!(matches!(face, Err(reason) if reason.contains("切断側を確定できません")));
            }
            assert!((member.recorded_section_forces(0.0).unwrap()[0] + 150e3).abs() < 1e-7);
            assert!(member.recorded_section_forces(1.0).unwrap()[0].abs() < 1e-7);
        }),
    )
    .unwrap();
    assert_eq!(observed, [0, 1, 2]);
    let recorded = response.recording.unwrap();
    let peak = recorded.peak_member_forces[0].as_ref().unwrap();
    let (_, force) = peak.at.iter().find(|(at, _)| *at == xi).unwrap();
    assert!((force[0] - recorded_n).abs() < 1e-7);

    let solved = compute_confirmed_time_history(
        model,
        AnalysisSettings {
            th_nonlinear: true,
            th_apply_long_term: true,
            th_damping: 0.0,
            th_record_every: 2,
            ..Default::default()
        },
        wave,
    )
    .unwrap();
    let recording = solved.response().recording.as_ref().unwrap();
    assert!(!recording.frame_time.contains(&DT_S));
    let TimeHistoryColumnCheck::AsymmetricSection(Ok(history)) = solved.column_check(id) else {
        panic!("上下非対称柱の断面照査を取得できない");
    };
    assert_eq!(history.committed_count, 3);
    let face = history
        .positions
        .iter()
        .find(|position| position.xi == xi)
        .unwrap();
    let start = &history.positions[0];
    assert_eq!(start.observed_count, 3);
    if same_cut {
        assert_eq!(face.observed_count, 3);
        let peak = face.peak.as_ref().unwrap();
        let reference = start.peak.as_ref().unwrap();
        assert!((peak.n_n - recorded_n).abs() < 1e-7);
        assert!(peak.mz_nmm.abs() < 1e-7);
        assert_eq!(peak.time_s, reference.time_s);
        assert!(peak.ratio > 0.0 && (peak.ratio - reference.ratio).abs() < 1e-12);
        assert_eq!(face.unavailable, start.unavailable);
        assert!(face
            .unavailable
            .iter()
            .any(|reason| reason.contains("曲げねじり座屈")));
    } else {
        assert_eq!(face.observed_count, 0);
        assert!(face.peak.is_none());
        assert!(face
            .unavailable
            .iter()
            .any(|reason| reason.contains("切断側を確定できません")));
    }
}
