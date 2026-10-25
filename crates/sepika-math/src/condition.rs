// LAPACK 3.12.1 DLACN2 から対称行列用に移植。著作権・利用条件は ../LICENSE-LAPACK。

pub(crate) fn estimate_symmetric_one_norm(n: usize, mut apply: impl FnMut(&mut [f64])) -> f64 {
    let mut apply_finite = |x: &mut [f64]| {
        apply(x);
        x.iter().all(|value| value.is_finite())
    };
    let mut x = vec![1.0 / n as f64; n];
    if !apply_finite(&mut x) {
        return f64::INFINITY;
    }
    if n == 1 {
        return x[0].abs();
    }
    let mut estimate = absolute_sum(&x);
    let mut signs: Vec<f64> = x.iter().copied().map(sign).collect();
    x.copy_from_slice(&signs);
    if !apply_finite(&mut x) {
        return f64::INFINITY;
    }
    let mut index = largest_absolute_index(&x);
    let mut iteration = 2;
    loop {
        x.fill(0.0);
        x[index] = 1.0;
        if !apply_finite(&mut x) {
            return f64::INFINITY;
        }
        let previous_estimate = estimate;
        estimate = absolute_sum(&x);
        if x.iter()
            .zip(&signs)
            .all(|(&value, &old)| sign(value) == old)
            || estimate <= previous_estimate
        {
            break;
        }
        for (value, old) in x.iter_mut().zip(&mut signs) {
            *value = sign(*value);
            *old = *value;
        }
        if !apply_finite(&mut x) {
            return f64::INFINITY;
        }
        let previous_index = index;
        index = largest_absolute_index(&x);
        if x[previous_index] == x[index].abs() || iteration >= 5 {
            break;
        }
        iteration += 1;
    }
    for (i, value) in x.iter_mut().enumerate() {
        *value = if i % 2 == 0 { 1.0 } else { -1.0 } * (1.0 + i as f64 / (n - 1) as f64);
    }
    if !apply_finite(&mut x) {
        return f64::INFINITY;
    }
    let final_estimate = 2.0 * (absolute_sum(&x) / (3.0 * n as f64));
    if !estimate.is_finite() || !final_estimate.is_finite() {
        return f64::INFINITY;
    }
    estimate.max(final_estimate)
}

fn sign(value: f64) -> f64 {
    if value >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

fn absolute_sum(x: &[f64]) -> f64 {
    x.iter().map(|value| value.abs()).sum()
}

fn largest_absolute_index(x: &[f64]) -> usize {
    let mut index = 0;
    for i in 1..x.len() {
        if x[i].abs() > x[index].abs() {
            index = i;
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate(matrix: &[&[f64]]) -> (f64, usize) {
        let mut calls = 0;
        let estimate = estimate_symmetric_one_norm(matrix.len(), |x| {
            calls += 1;
            let result: Vec<f64> = matrix
                .iter()
                .map(|row| row.iter().zip(x.iter()).map(|(a, b)| a * b).sum())
                .collect();
            x.copy_from_slice(&result);
        });
        (estimate, calls)
    }

    #[test]
    fn known_symmetric_norms_include_negative_entries_and_single_dimension() {
        for (matrix, expected) in [
            (vec![&[-3.0][..]], 3.0),
            (vec![&[3.0, -1.0][..], &[-1.0, 2.0][..]], 4.0),
            (
                vec![
                    &[4.0, 0.0, 0.0][..],
                    &[0.0, 2.0, 0.0][..],
                    &[0.0, 0.0, 1.0][..],
                ],
                4.0,
            ),
        ] {
            let (actual, calls) = estimate(&matrix);
            approx::assert_relative_eq!(actual, expected, max_relative = 1e-14);
            assert!(calls <= 11);
        }
    }

    #[test]
    fn alternating_final_probe_detects_subspace_hidden_from_initial_and_coordinate_probes() {
        let (actual, calls) = estimate(&[&[0.0, 0.0, 0.0], &[0.0, 1.0, -1.0], &[0.0, -1.0, 1.0]]);
        approx::assert_relative_eq!(actual, 14.0 / 9.0, max_relative = 1e-14);
        assert_eq!(calls, 4);
        assert!(actual <= 2.0);
    }

    #[test]
    fn nonfinite_operator_results_never_become_a_finite_estimate() {
        for invalid in [f64::NAN, f64::INFINITY] {
            let estimate = estimate_symmetric_one_norm(2, |x| x[0] = invalid);
            assert!(!estimate.is_finite());
        }
    }
}
