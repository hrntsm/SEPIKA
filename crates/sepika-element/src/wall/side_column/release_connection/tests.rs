use super::*;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance * expected.abs().max(1.0),
        "actual={actual}, expected={expected}"
    );
}

fn fixed(normal: [f64; 2]) -> ReleaseConnection {
    ReleaseConnection::new(normal, [EndCondition::Fixed; 2], [false; 2], true).unwrap()
}

#[test]
fn fixed_releases_only_the_normal_and_keeps_original_translation_and_section_axes() {
    for normal in [[1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [-1.0, -1.0]] {
        let connection = fixed(normal);
        assert_eq!(connection.spring_diag(), [0.0; 2]);
        let u = std::array::from_fn(|i| (i + 1) as f64);
        let norm = normal[0].hypot(normal[1]);
        let [ny, nz] = normal.map(|x| x / norm);
        let e = connection.flexible_displacement(&u, &[0.7, -0.2]).unwrap();
        for (end, rotation) in [0.7, -0.2].into_iter().enumerate() {
            let offset = end * 6;
            assert_eq!(&e[offset..offset + 4], &u[offset..offset + 4]);
            close(ny * e[offset + 4] + nz * e[offset + 5], rotation, 1e-14);
            close(
                -nz * e[offset + 4] + ny * e[offset + 5],
                -nz * u[offset + 4] + ny * u[offset + 5],
                1e-14,
            );
        }
    }
}

#[test]
fn opposite_normals_describe_the_same_flexible_displacement() {
    let u = std::array::from_fn(|i| i as f64 / 10.0);
    let positive = fixed([3.0, 4.0]);
    let negative = fixed([-3.0, -4.0]);
    let ep = positive.flexible_displacement(&u, &[0.2, 0.3]).unwrap();
    let en = negative.flexible_displacement(&u, &[-0.2, -0.3]).unwrap();
    assert_eq!(ep, en);
}

#[test]
fn subnormal_normals_preserve_direction_projection_and_work() {
    let d = f64::from_bits(1);
    let u = std::array::from_fn(|i| (i as f64 + 0.3).cos());
    let b = [0.2, -0.6];
    let f = std::array::from_fn(|i| (i as f64 + 0.7).sin());
    for signs in [[1.0, 1.0], [1.0, -1.0], [-1.0, 1.0], [-1.0, -1.0]] {
        let reference = fixed(signs);
        for scale in [d, f64::MIN_POSITIVE, f64::MAX] {
            let connection = fixed(signs.map(|sign| sign * scale));
            assert_eq!(connection.a(), reference.a());
            assert_eq!(connection.b_columns(), reference.b_columns());
            assert_eq!(connection.c_rows(), reference.c_rows());
            let e = connection.flexible_displacement(&u, &b).unwrap();
            assert_eq!(e, reference.flexible_displacement(&u, &b).unwrap());
            for (i, row) in connection.a().iter().enumerate() {
                for (j, expected) in row.iter().enumerate() {
                    let squared: f64 = row
                        .iter()
                        .enumerate()
                        .map(|(k, value)| value * connection.a()[k][j])
                        .sum();
                    close(squared, *expected, 1e-14);
                    close(*expected, connection.a()[j][i], 1e-14);
                }
            }
            let response = connection
                .trial_response(
                    &[0.0; 12],
                    &[0.0; 2],
                    &f,
                    &[[0.0; 12]; 12],
                    &[0.0; 12],
                    &[0.0; 2],
                )
                .unwrap();
            close(
                dot(&f, &e),
                dot(&response.retained_force, &u)
                    + response
                        .residual
                        .iter()
                        .zip(b)
                        .map(|(force, rotation)| force * rotation)
                        .sum::<f64>(),
                1e-14,
            );
        }
    }
}

#[test]
fn pin_zero_spring_finite_spring_and_end_conditions_keep_each_rotation_once() {
    let pin =
        ReleaseConnection::new([1.0, 1.0], [EndCondition::Pinned; 2], [false; 2], true).unwrap();
    let zero = ReleaseConnection::new(
        [1.0, 1.0],
        [EndCondition::SemiRigid { k_theta: 0.0 }; 2],
        [false; 2],
        true,
    )
    .unwrap();
    assert_eq!(pin.a(), zero.a());
    assert_eq!(pin.b_columns(), zero.b_columns());
    assert_eq!(pin.c_rows(), zero.c_rows());
    assert_eq!(pin.spring_diag(), [0.0; 6]);
    assert_eq!(zero.spring_diag(), [0.0; 6]);
    let mixed = ReleaseConnection::new(
        [0.0, 1.0],
        [
            EndCondition::Fixed,
            EndCondition::SemiRigid { k_theta: 7.0 },
        ],
        [true; 2],
        true,
    )
    .unwrap();
    assert_eq!(mixed.spring_diag(), [0.0, 0.0, 7.0, 0.0, 7.0]);
    let no_torsion = ReleaseConnection::new(
        [0.0, 1.0],
        [
            EndCondition::Pinned,
            EndCondition::SemiRigid { k_theta: 7.0 },
        ],
        [true; 2],
        false,
    )
    .unwrap();
    assert_eq!(no_torsion.spring_diag(), [0.0, 0.0, 0.0, 7.0]);
    let u = std::array::from_fn(|i| i as f64);
    let e = no_torsion.flexible_displacement(&u, &[0.0; 4]).unwrap();
    assert_eq!(e[3], u[3]);
    assert_eq!(e[9], u[9]);
}

fn raw_response(e: &[f64; 12]) -> ([f64; 12], [[f64; 12]; 12]) {
    let force = std::array::from_fn(|i| (i + 2) as f64 * e[i] + e[i].powi(3));
    let tangent = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            if i == j {
                (i + 2) as f64 + 3.0 * e[i].powi(2)
            } else {
                0.0
            }
        })
    });
    (force, tangent)
}

// 直列の中間回転は法線方向に自由なので、その方向の元ばね変形が0となる。
fn uncondensed_energy(
    state: &[f64],
    normal: [f64; 2],
    condition: [EndCondition; 2],
    torsion: [bool; 2],
    pa: &[f64; 12],
    pb: &[f64],
) -> f64 {
    let norm = normal[0].hypot(normal[1]);
    let [ny, nz] = normal.map(|x| x / norm);
    let mut e: [f64; 12] = state[..12].try_into().unwrap();
    let mut index = 12;
    let mut spring_energy = 0.0;
    for end in 0..2 {
        let offset = end * 6;
        let free_x = torsion[end] && matches!(condition[end], EndCondition::Fixed);
        let (rx, rn, rt) = match condition[end] {
            EndCondition::Fixed => {
                let rx = if free_x {
                    let value = state[index];
                    index += 1;
                    value
                } else {
                    state[offset + 3]
                };
                let rn = state[index];
                index += 1;
                (rx, rn, -nz * state[offset + 4] + ny * state[offset + 5])
            }
            _ => {
                let value = (state[index], state[index + 1], state[index + 2]);
                index += 3;
                value
            }
        };
        e[offset + 3] = rx;
        e[offset + 4] = ny * rn - nz * rt;
        e[offset + 5] = nz * rn + ny * rt;
        if let EndCondition::SemiRigid { k_theta } = condition[end] {
            let delta = [
                e[offset + 3] - state[offset + 3],
                e[offset + 4] - state[offset + 4],
                e[offset + 5] - state[offset + 5],
            ];
            let normal_slip = ny * delta[1] + nz * delta[2];
            let intermediate = [
                state[offset + 3],
                state[offset + 4] + ny * normal_slip,
                state[offset + 5] + nz * normal_slip,
            ];
            spring_energy += 0.5
                * k_theta
                * (0..3)
                    .map(|i| (e[offset + 3 + i] - intermediate[i]).powi(2))
                    .sum::<f64>();
        }
    }
    let material: f64 = e
        .iter()
        .enumerate()
        .map(|(i, x)| 0.5 * (i + 2) as f64 * x * x + x.powi(4) / 4.0)
        .sum();
    let external: f64 = state
        .iter()
        .zip(pa.iter().chain(pb))
        .map(|(u, p)| u * p)
        .sum();
    material + spring_energy - external
}

#[test]
fn forces_match_independent_original_isotropic_springs_and_free_series_normal_energy() {
    for normal in [[1.0, 0.0], [0.0, -1.0], [1.0, 1.0], [-3.0, -4.0]] {
        for condition in [
            [EndCondition::Fixed; 2],
            [EndCondition::Pinned; 2],
            [EndCondition::SemiRigid { k_theta: 0.0 }; 2],
            [
                EndCondition::SemiRigid { k_theta: 11.0 },
                EndCondition::Fixed,
            ],
            [EndCondition::SemiRigid { k_theta: 11.0 }; 2],
        ] {
            let torsion = [true, false];
            let connection = ReleaseConnection::new(normal, condition, torsion, true).unwrap();
            let state: Vec<_> = (0..12 + connection.spring_diag().len())
                .map(|i| (i as f64 - 7.0) * 0.03)
                .collect();
            let u = state[..12].try_into().unwrap();
            let b = &state[12..];
            let pa = std::array::from_fn(|i| i as f64 * 0.02);
            let pb: Vec<_> = b
                .iter()
                .enumerate()
                .map(|(i, _)| -0.04 * (i + 1) as f64)
                .collect();
            let e = connection.flexible_displacement(u, b).unwrap();
            let (f, k) = raw_response(&e);
            let response = connection.trial_response(u, b, &f, &k, &pa, &pb).unwrap();
            for (i, force) in response
                .retained_force
                .iter()
                .chain(&response.residual)
                .enumerate()
            {
                let mut plus = state.clone();
                let mut minus = state.clone();
                plus[i] += 1e-6;
                minus[i] -= 1e-6;
                let expected = (uncondensed_energy(&plus, normal, condition, torsion, &pa, &pb)
                    - uncondensed_energy(&minus, normal, condition, torsion, &pa, &pb))
                    / 2e-6;
                close(*force, expected, 2e-9);
            }
        }
    }
}

fn block(response: &LocalTrialResponse, n: usize, i: usize, j: usize) -> f64 {
    match (i < 12, j < 12) {
        (true, true) => response.kaa[i][j],
        (true, false) => response.kab[i * n + j - 12],
        (false, true) => response.kba[(i - 12) * 12 + j],
        (false, false) => response.kbb[(i - 12) * n + j - 12],
    }
}

#[test]
fn all_four_tangent_blocks_differentiate_nonsymmetric_raw_trial_and_load_residual() {
    let connection = ReleaseConnection::new(
        [3.0, 4.0],
        [
            EndCondition::SemiRigid { k_theta: 8.0 },
            EndCondition::Fixed,
        ],
        [false; 2],
        true,
    )
    .unwrap();
    let n = connection.spring_diag().len();
    let state: Vec<_> = (0..12 + n).map(|i| i as f64 / 100.0).collect();
    let raw_k: [[f64; 12]; 12] =
        std::array::from_fn(|i| std::array::from_fn(|j| ((2 * i + j + 1) as f64).sin()));
    let evaluate = |state: &[f64]| {
        let u = state[..12].try_into().unwrap();
        let b = &state[12..];
        let e = connection.flexible_displacement(u, b).unwrap();
        let f = std::array::from_fn(|i| dot(&raw_k[i], &e) + i as f64);
        connection
            .trial_response(u, b, &f, &raw_k, &[0.4; 12], &vec![0.3; n])
            .unwrap()
    };
    let response = evaluate(&state);
    for j in 0..12 + n {
        let mut plus = state.clone();
        let mut minus = state.clone();
        plus[j] += 1e-6;
        minus[j] -= 1e-6;
        let rp = evaluate(&plus);
        let rm = evaluate(&minus);
        for (i, (fp, fm)) in rp
            .retained_force
            .iter()
            .chain(&rp.residual)
            .zip(rm.retained_force.iter().chain(&rm.residual))
            .enumerate()
        {
            close(block(&response, n, i, j), (fp - fm) / 2e-6, 3e-9);
        }
    }
    let zero_raw = connection
        .trial_response(
            &[0.0; 12],
            &vec![0.0; n],
            &[0.0; 12],
            &[[0.0; 12]; 12],
            &[2.0; 12],
            &vec![3.0; n],
        )
        .unwrap();
    assert_eq!(zero_raw.retained_force, [-2.0; 12]);
    assert_eq!(zero_raw.residual, vec![-3.0; n]);
}

#[test]
fn mapping_is_work_conjugate_without_rotating_translations_or_section_forces() {
    let connection = fixed([1.0, 1.0]);
    let du = std::array::from_fn(|i| (i as f64 + 0.4).cos());
    let db = [0.3, -0.7];
    let de = connection.flexible_displacement(&du, &db).unwrap();
    let f = std::array::from_fn(|i| (i as f64).sin());
    let response = connection
        .trial_response(
            &[0.0; 12],
            &[0.0; 2],
            &f,
            &[[0.0; 12]; 12],
            &[0.0; 12],
            &[0.0; 2],
        )
        .unwrap();
    close(
        dot(&f, &de),
        dot(&response.retained_force, &du)
            + response
                .residual
                .iter()
                .zip(db)
                .map(|(f, d)| f * d)
                .sum::<f64>(),
        1e-14,
    );
}

#[test]
fn invalid_normal_spring_dimensions_and_nonfinite_trial_are_explicit_errors() {
    for normal in [[0.0, 0.0], [f64::NAN, 1.0], [1.0, f64::INFINITY]] {
        assert_eq!(
            ReleaseConnection::new(normal, [EndCondition::Fixed; 2], [false; 2], true).unwrap_err(),
            ReleaseConnectionError::InvalidNormal
        );
    }
    for k_theta in [-1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            ReleaseConnection::new(
                [1.0, 0.0],
                [EndCondition::SemiRigid { k_theta }; 2],
                [true; 2],
                false
            )
            .unwrap_err(),
            ReleaseConnectionError::InvalidSpring
        );
    }
    let connection = fixed([1.0, 0.0]);
    assert_eq!(
        connection
            .flexible_displacement(&[0.0; 12], &[])
            .unwrap_err(),
        ReleaseConnectionError::Dimension
    );
    assert_eq!(
        connection
            .flexible_displacement(&[f64::NAN; 12], &[0.0; 2])
            .unwrap_err(),
        ReleaseConnectionError::NonFinite
    );
    assert_eq!(
        connection
            .trial_response(
                &[0.0; 12],
                &[0.0; 2],
                &[0.0; 12],
                &[[0.0; 12]; 12],
                &[0.0; 12],
                &[]
            )
            .unwrap_err(),
        ReleaseConnectionError::Dimension
    );
    assert_eq!(
        connection
            .trial_response(
                &[0.0; 12],
                &[0.0; 2],
                &[f64::INFINITY; 12],
                &[[0.0; 12]; 12],
                &[0.0; 12],
                &[0.0; 2]
            )
            .unwrap_err(),
        ReleaseConnectionError::NonFinite
    );
    let mut overflow = [f64::MAX; 12];
    overflow[5] = -f64::MAX;
    assert_eq!(
        fixed([1.0, 1.0])
            .flexible_displacement(&overflow, &[f64::MAX; 2])
            .unwrap_err(),
        ReleaseConnectionError::NonFinite
    );
}

#[test]
fn original_connections_do_not_add_wall_release() {
    for condition in [
        EndCondition::Fixed,
        EndCondition::Pinned,
        EndCondition::SemiRigid { k_theta: 7.0 },
    ] {
        let connection =
            ReleaseConnection::from_end_conditions([condition; 2], [false; 2], true).unwrap();
        let n = connection.b_columns().len();
        assert_eq!(
            n,
            if matches!(condition, EndCondition::Fixed) {
                0
            } else {
                6
            }
        );
        let u = std::array::from_fn(|i| i as f64);
        let b: Vec<_> = connection.c_rows().iter().map(|c| dot(c, &u)).collect();
        assert_eq!(connection.flexible_displacement(&u, &b).unwrap(), u);
        assert_eq!(
            connection.spring_diag(),
            vec![
                if let EndCondition::SemiRigid { k_theta } = condition {
                    k_theta
                } else {
                    0.0
                };
                n
            ]
        );
    }
}

#[test]
fn only_a_finite_positive_isolated_common_torsion_is_removed() {
    let mut k = [[0.0; 12]; 12];
    k[3][3] = 8.0;
    k[9][9] = 8.0;
    k[3][9] = -8.0;
    k[9][3] = -8.0;
    let connection =
        || ReleaseConnection::new([0.6, 0.8], [EndCondition::Pinned; 2], [false; 2], true).unwrap();
    let mut c = connection();
    c.fix_isolated_torsion_gauge(&k).unwrap();
    assert_eq!(c.b_columns().len(), 5);
    for change in [0, 1, 2, 3] {
        let mut invalid = k;
        match change {
            0 => invalid[3][0] = 1e-30,
            1 => invalid[3][3] = 0.0,
            2 => invalid[9][3] = 0.0,
            _ => invalid[5][5] = f64::NAN,
        }
        let mut c = connection();
        let previous = c.clone();
        assert_eq!(
            c.fix_isolated_torsion_gauge(&invalid),
            Err(ReleaseConnectionError::InvalidTorsionGauge)
        );
        assert_eq!(c, previous);
    }
}
