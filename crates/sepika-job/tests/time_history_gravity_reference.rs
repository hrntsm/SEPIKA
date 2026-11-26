use sepika_core::ids::LoadCaseId;
use sepika_core::load_combo::{effective_combination_terms, is_gravity_combination};
use sepika_core::model::{AnalysisKind, Model};
use sepika_core::section_shape::StandardRebar;
use sepika_element::factory::{build_hinge_view, StrengthBasis};
use sepika_element::frame::beam::MemberForces;
use sepika_job::{compute::compute_time_history, AnalysisSettings};
use sepika_solver::dynamic::timehistory::GroundMotion;
use sepika_solver::statics::analysis::Analysis;

#[path = "fixtures/time_history_gravity_reference_model.rs"]
mod frame_model;
use frame_model::{frame, BEAM_D_MM, COLUMN_B_MM, COLUMN_H_MM, CONCRETE_E_MPA, CONCRETE_POISSON};

const FORCE_TOL_N: f64 = 0.1;
const N: usize = 0;
const QY: usize = 1;
const MZ: usize = 5;

type EndForces = [[f64; 6]; 2];

fn ends(forces: &MemberForces) -> EndForces {
    [0.0, 1.0].map(|xi| forces.at.iter().find(|(x, _)| *x == xi).unwrap().1)
}

fn near(observed: f64, expected: f64, tolerance: f64) {
    assert!(
        (observed - expected).abs() < tolerance,
        "observed {observed}, expected {expected} ± {tolerance}"
    );
}

fn signed_forces(ends: &[EndForces; 4]) -> [f64; 4] {
    [ends[0][1][QY], ends[1][0][QY], ends[2][0][N], ends[3][0][N]]
}

fn check_equilibrium(model: &Model, end_forces: &[EndForces; 4], load_n: f64) {
    let [q_left, q_right, n_left, n_right] = signed_forces(end_forces);
    near(q_left - q_right, load_n, FORCE_TOL_N);
    near(-n_left - n_right, load_n, FORCE_TOL_N);
    near(q_left, -n_left, FORCE_TOL_N);
    near(q_right, n_right, FORCE_TOL_N);
    for (beam, [i, j]) in model.elements[..2].iter().zip(&end_forces[..2]) {
        let length_mm = model.member_length(beam);
        near(i[QY], j[QY], FORCE_TOL_N);
        near(j[MZ] - i[MZ], i[QY] * length_mm, FORCE_TOL_N * length_mm);
        near(i[MZ] + j[MZ], 0.0, FORCE_TOL_N * length_mm);
    }
    for [i, j] in &end_forces[2..] {
        near(i[N], j[N], FORCE_TOL_N);
    }
}

fn analyze(model: &Model, load_n: f64) -> ([EndForces; 4], [EndForces; 4]) {
    let combo = &model.combinations[0];
    assert!(is_gravity_combination(&combo.name, model));
    assert_eq!(
        effective_combination_terms(combo, &model.load_cases).unwrap(),
        [(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)]
    );
    let linear = Analysis::prepare(model)
        .unwrap()
        .linear_combination(combo)
        .unwrap();
    let linear_ends = std::array::from_fn(|i| {
        let id = model.elements[i].id;
        ends(
            &linear
                .member_forces
                .iter()
                .find(|(key, _)| *key == id)
                .unwrap()
                .1,
        )
    });
    let th = compute_time_history(
        model.clone(),
        AnalysisSettings {
            th_nonlinear: true,
            th_apply_long_term: true,
            th_damping: 0.0,
            th_record_every: 1,
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
    assert!(th.nonlinear && th.applied_long_term);
    assert_eq!(th.non_converged_steps, 0);
    let rec = th.recording.unwrap();
    assert_eq!(rec.frame_time, [0.0, 0.01, 0.02]);
    let recorded_ends =
        |frame: usize| std::array::from_fn(|i| ends(rec.member_forces[frame][i].as_ref().unwrap()));
    let initial_ends = recorded_ends(0);
    check_equilibrium(model, &linear_ends, load_n);
    for (step, time_s) in rec.frame_time.iter().enumerate() {
        let forces = recorded_ends(step);
        check_equilibrium(model, &forces, load_n);
        for (value, initial) in signed_forces(&forces)
            .into_iter()
            .zip(signed_forces(&initial_ends))
        {
            near(value, initial, FORCE_TOL_N);
        }
        println!(
            "t={time_s} s, Qy(left,right), N(left,right) = {:?} N",
            signed_forces(&forces)
        );
    }
    println!(
        "P={load_n} N, Qy(left,right), N(left,right) = {:?} N (linear); Uz(center) = {:.9} / {:.9} mm (linear/initial)",
        signed_forces(&linear_ends), linear.disp[3][2], rec.node_disp[0][3][2]
    );
    (linear_ends, initial_ends)
}

fn check_cracking(model: &Model, end_forces: &[EndForces; 4], cracked: [bool; 2]) {
    for (index, is_cracked) in cracked.into_iter().enumerate() {
        let view = build_hinge_view(
            &model.elements[index],
            model,
            StrengthBasis::Nominal,
            AnalysisKind::TimeHistory,
            0.0,
            6,
            6,
        )
        .unwrap();
        let backbone = view.total_backbone.unwrap();
        let mc_nmm = backbone[1][1];
        let my_nmm = backbone[2][1];
        for end in end_forces[index] {
            assert!(end[MZ].abs() < my_nmm);
            assert_eq!(end[MZ].abs() > mc_nmm, is_cracked);
        }
        println!(
            "beam {index}, |Mz|={:.6}, Mc={:.6}, My={:.6} kN·m",
            end_forces[index][0][MZ].abs() / 1e6,
            mc_nmm / 1e6,
            my_nmm / 1e6
        );
    }
}

#[test]
fn cracking_redistributes_the_same_gravity_load_in_an_equal_span_frame() {
    let spans_mm = [5_000.0; 2];
    for (load_n, cracked) in [(20_000.0, false), (98_000.0, true)] {
        let model = frame(
            load_n,
            spans_mm,
            500.0,
            [StandardRebar::D25, StandardRebar::D35],
        );
        let (linear, initial) = analyze(&model, load_n);
        let half_n = load_n / 2.0;
        let expected = [half_n, -half_n, -half_n, -half_n];
        for (observed, expected) in signed_forces(&linear).into_iter().zip(expected) {
            near(observed, expected, FORCE_TOL_N);
        }
        check_cracking(&model, &initial, [cracked, false]);
        if cracked {
            // 収束許容幅を超える再配分を確認する。区間内の正確な再配分量は独立期待値としない。
            assert!(initial[0][1][QY] < half_n - 1_000.0);
            assert!(-initial[1][0][QY] > half_n + 1_000.0);
        } else {
            for (observed, expected) in signed_forces(&initial).into_iter().zip(expected) {
                near(observed, expected, FORCE_TOL_N);
            }
        }
    }
}

#[test]
fn shear_flexibility_changes_gravity_partition_even_before_cracking_in_an_unequal_span_frame() {
    let load_n = 20_000.0;
    let spans_mm = [4_000.0, 6_000.0];
    let beam_b_mm = 400.0;
    let model = frame(load_n, spans_mm, beam_b_mm, [StandardRebar::D29; 2]);
    let (linear, initial) = analyze(&model, load_n);
    check_cracking(&model, &initial, [false; 2]);
    for (end_forces, include_shear) in [(&linear, true), (&initial, false)] {
        // 鉛直変位が共通な二経路へ、片側ずつ曲げ・せん断・軸の直列柔性を適用する。
        let k = spans_mm.map(|span_mm| {
            let ei_nmm2 = CONCRETE_E_MPA * beam_b_mm * BEAM_D_MM.powi(3) / 12.0;
            let g_mpa = CONCRETE_E_MPA / (2.0 * (1.0 + CONCRETE_POISSON));
            let effective_area_mm2 = beam_b_mm * BEAM_D_MM / 1.2;
            let bending = span_mm.powi(3) / (12.0 * ei_nmm2);
            let shear = if include_shear {
                span_mm / (g_mpa * effective_area_mm2)
            } else {
                0.0
            };
            let axial = COLUMN_H_MM / (CONCRETE_E_MPA * COLUMN_B_MM.powi(2));
            (bending + shear + axial).recip()
        });
        let [left, right] = k.map(|stiffness| load_n * stiffness / (k[0] + k[1]));
        let expected = [left, -right, -left, -right];
        for (observed, expected) in signed_forces(end_forces).into_iter().zip(expected) {
            near(observed, expected, FORCE_TOL_N);
        }
    }
}
