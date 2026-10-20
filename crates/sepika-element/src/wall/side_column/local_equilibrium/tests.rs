use super::*;

fn options() -> LocalEquilibriumOptions {
    LocalEquilibriumOptions {
        reference_moment_nmm: 1e6,
        absolute_tolerance_nmm: 1e-8,
        relative_tolerance: 1e-12,
        max_iterations: 30,
    }
}

fn empty(n: usize) -> LocalTrialResponse {
    LocalTrialResponse {
        retained_force: [0.0; 12],
        residual: vec![0.0; n],
        kaa: [[0.0; 12]; 12],
        kab: vec![0.0; 12 * n],
        kba: vec![0.0; n * 12],
        kbb: vec![0.0; n * n],
    }
}

fn quartic(a: f64, b: &[f64]) -> Result<LocalTrialResponse, LocalEquilibriumError> {
    let mut r = empty(1);
    r.retained_force[0] = 1e8 * (a - b[0]);
    r.residual[0] = -r.retained_force[0] + 1e12 * b[0].powi(3);
    r.kaa[0][0] = 1e8;
    r.kab[0] = -1e8;
    r.kba[0] = -1e8;
    r.kbb[0] = 1e8 + 3e12 * b[0].powi(2);
    Ok(r)
}

fn solve_quartic(a: f64) -> LocalEquilibriumSolution {
    solve_local_equilibrium(&[0.0], options(), |b| quartic(a, b)).unwrap()
}

fn reference_rotation(a: f64) -> f64 {
    let (mut low, mut high) = (-1.0, 1.0);
    for _ in 0..100 {
        let middle = (low + high) * 0.5;
        if middle + 10000.0 * middle * middle * middle > a {
            high = middle;
        } else {
            low = middle;
        }
    }
    (low + high) * 0.5
}

#[test]
fn quartic_energy_has_independent_positive_and_negative_force_tangent_and_work_anchors() {
    for sign in [-1.0, 1.0] {
        let s = solve_quartic(sign * 0.02);
        assert!((s.rotations_rad[0] - sign * 0.01).abs() < 1e-11);
        assert!((s.retained_force[0] - sign * 1e6).abs() < 1e-3);
        assert!((s.tangent[0][0] / 75e6 - 1.0).abs() < 1e-9);
        assert!(s.residual_nmm < 1e-3);
        assert!((s.tangent[0][0] * sign * 0.02 - s.retained_force[0]).abs() > 499999.0);
        let b = s.rotations_rad[0];
        let energy = 0.5 * 1e8 * (sign * 0.02 - b).powi(2) + 0.25 * 1e12 * b.powi(4);
        assert!((energy - 7500.0).abs() < 1e-5);
    }
    let work = |steps: usize| {
        let da = 0.02 / steps as f64;
        let mut result = 0.0;
        for i in 0..=steps {
            let a = i as f64 * da;
            let force = solve_quartic(a).retained_force[0];
            result += force * da * if i == 0 || i == steps { 0.5 } else { 1.0 };
        }
        result
    };
    let coarse = work(200);
    let fine = work(400);
    assert!((fine - 7500.0).abs() < (coarse - 7500.0).abs() / 3.9);
    assert!((fine - 7500.0).abs() < 0.02);
    eprintln!("A0/A1 f=±1e6 K=75e6 W=7500; A3 work200={coarse}, work400={fine} Nmm");
}

#[test]
fn every_finite_difference_point_reequilibrates_and_matches_bisection_reference() {
    let mut errors = Vec::new();
    for h in [1e-4, 1e-5, 1e-6] {
        let plus = solve_quartic(0.02 + h);
        let minus = solve_quartic(0.02 - h);
        for (a, s) in [(0.02 + h, &plus), (0.02 - h, &minus)] {
            assert!((s.rotations_rad[0] - reference_rotation(a)).abs() < 1e-11);
            assert!(s.residual_nmm < 1e-3);
        }
        let derivative = (plus.retained_force[0] - minus.retained_force[0]) / (2.0 * h);
        errors.push((derivative / 75e6 - 1.0).abs());
        eprintln!(
            "A2 h={h} FD={derivative} reference=75000000 relative_error={}",
            errors.last().unwrap()
        );
    }
    assert!(errors[1] < errors[0]);
    assert!(errors[2] < errors[1]);
    assert!(errors[2] < 1e-6);
}

#[test]
fn loaded_residual_and_its_displacement_derivative_are_condensed_together() {
    // W=1e8(a-b)²/2+4e8b²/2-(1e5+2e7a)b。
    let solve = |a: f64| {
        solve_local_equilibrium(&[0.0], options(), |b| {
            let mut r = empty(1);
            r.residual[0] = 5e8 * b[0] - 1.2e8 * a - 1e5;
            r.retained_force[0] = 1e8 * a - 1.2e8 * b[0];
            r.kaa[0][0] = 1e8;
            r.kab[0] = -1.2e8;
            r.kba[0] = -1.2e8;
            r.kbb[0] = 5e8;
            Ok(r)
        })
        .unwrap()
    };
    let s = solve(0.0);
    assert!((s.rotations_rad[0] - 0.0002).abs() < 1e-14);
    assert!((s.retained_force[0] + 24000.0).abs() < 1e-6);
    assert!((s.tangent[0][0] - 71.2e6).abs() < 1e-6);
    let fd = (solve(1e-6).retained_force[0] - solve(-1e-6).retained_force[0]) / 2e-6;
    assert!((fd / 71.2e6 - 1.0).abs() < 1e-10);
}

#[test]
fn coupled_two_end_block_keeps_off_diagonals_and_all_retained_directions() {
    let mut response = empty(2);
    response.kbb = vec![4.0, 1.0, 1.0, 3.0];
    for i in 0..12 {
        response.kaa[i][i] = 100.0;
        response.kab[2 * i] = 1.0;
        response.kab[2 * i + 1] = 2.0;
        response.kba[i] = 1.0;
        response.kba[12 + i] = 2.0;
    }
    let s = solve_local_equilibrium(&[0.0, 0.0], options(), |b| {
        let mut r = response.clone();
        r.residual = vec![4.0 * b[0] + b[1] - 6.0, b[0] + 3.0 * b[1] - 7.0];
        r.retained_force = [b[0] + 2.0 * b[1]; 12];
        Ok(r)
    })
    .unwrap();
    assert!((s.rotations_rad[0] - 1.0).abs() < 1e-12);
    assert!((s.rotations_rad[1] - 2.0).abs() < 1e-12);
    for i in 0..12 {
        assert!((s.retained_force[i] - 5.0).abs() < 1e-12);
        for j in 0..12 {
            let expected = if i == j { 100.0 } else { 0.0 } - 15.0 / 11.0;
            assert!((s.tangent[i][j] - expected).abs() < 1e-12);
        }
    }
}

#[test]
fn nonsymmetric_blocks_preserve_force_and_ordered_schur_products() {
    let s = solve_local_equilibrium(&[0.0, 0.0], options(), |b| {
        let mut r = empty(2);
        r.kaa[0][0] = 10.0;
        r.kaa[0][1] = 2.0;
        r.kaa[1][0] = 1.0;
        r.kaa[1][1] = 12.0;
        r.kab[0] = 1.0;
        r.kab[1] = 3.0;
        r.kab[2] = 4.0;
        r.kab[3] = 2.0;
        r.kba[0] = 2.0;
        r.kba[1] = 5.0;
        r.kba[12] = 1.0;
        r.kba[13] = 4.0;
        r.kbb = vec![3.0, 1.0, -1.0, 2.0];
        r.residual = vec![3.0 * b[0] + b[1] - 8.0, -b[0] + 2.0 * b[1]];
        r.retained_force[0] = 18.0 + b[0] + 3.0 * b[1];
        r.retained_force[1] = -10.0 + 4.0 * b[0] + 2.0 * b[1];
        Ok(r)
    })
    .unwrap();
    for (actual, expected) in s.rotations_rad.iter().zip([16.0 / 7.0, 8.0 / 7.0]) {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert!((s.retained_force[0] - 166.0 / 7.0).abs() < 1e-12);
    assert!((s.retained_force[1] - 10.0 / 7.0).abs() < 1e-12);
    for (i, row) in [[52.0 / 7.0, -43.0 / 7.0], [-15.0 / 7.0, 26.0 / 7.0]]
        .iter()
        .enumerate()
    {
        for (j, expected) in row.iter().enumerate() {
            assert!((s.tangent[i][j] - expected).abs() < 1e-12);
        }
    }
}

#[test]
fn callback_failure_after_a_successful_newton_evaluation_is_propagated() {
    let mut calls = 0;
    let result = solve_local_equilibrium(&[0.0], options(), |b| {
        calls += 1;
        if calls == 2 {
            return Err(LocalEquilibriumError::Evaluation(
                "second trial failed".into(),
            ));
        }
        quartic(0.02, b)
    });
    assert_eq!(calls, 2);
    assert!(
        matches!(result,Err(LocalEquilibriumError::Evaluation(reason)) if reason=="second trial failed")
    );
}

#[test]
fn no_internal_rotation_preserves_force_and_tangent_without_solving() {
    let mut r = empty(0);
    r.retained_force[8] = 17.0;
    r.kaa[8][8] = 23.0;
    let s = solve_local_equilibrium(&[], options(), |_| Ok(r.clone())).unwrap();
    assert_eq!(s.retained_force[8], 17.0);
    assert_eq!(s.tangent[8][8], 23.0);
    assert_eq!(s.iterations, 0);
}

#[test]
fn singular_nonfinite_notconverged_dimensions_and_evaluation_failures_never_return_success() {
    let singular = solve_local_equilibrium(&[0.0], options(), |_| Ok(empty(1)));
    assert!(matches!(
        singular,
        Err(LocalEquilibriumError::LinearSolve(
            DenseSolveError::Singular { .. }
        ))
    ));
    let nonfinite = solve_local_equilibrium(&[0.0], options(), |_| {
        let mut r = empty(1);
        r.retained_force[0] = f64::NAN;
        Ok(r)
    });
    assert!(matches!(nonfinite, Err(LocalEquilibriumError::NonFinite)));
    let mut capped = options();
    capped.max_iterations = 0;
    let failure = solve_local_equilibrium(&[0.0], capped, |b| quartic(0.02, b));
    assert!(matches!(
        failure,
        Err(LocalEquilibriumError::NotConverged { iterations: 0, .. })
    ));
    assert!(matches!(
        solve_local_equilibrium(&[0.0], options(), |_| Ok(empty(0))),
        Err(LocalEquilibriumError::Dimension)
    ));
    assert!(matches!(
        solve_local_equilibrium(&[], options(), |_| Err(LocalEquilibriumError::Evaluation(
            "material".into()
        ))),
        Err(LocalEquilibriumError::Evaluation(_))
    ));
    let retry = solve_quartic(0.02);
    assert!((retry.retained_force[0] - 1e6).abs() < 1e-3);
}
