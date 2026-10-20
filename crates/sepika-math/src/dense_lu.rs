use faer::{
    linalg::solvers::{PartialPivLu, Solve},
    Mat,
};

/// 小密行列の分解・求解失敗。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DenseSolveError {
    #[error("密行列の寸法が一致しません")]
    Dimension,
    #[error("密行列または解が非有限です")]
    NonFinite,
    #[error("密行列が特異です: pivot={pivot}, threshold={threshold}")]
    Singular { pivot: f64, threshold: f64 },
    #[error("密行列求解の後退誤差が許容値を超えました: {error}")]
    Residual { error: f64 },
}

/// 行優先正方行列の部分ピボットLU。正則化を行わず、同じ因子で複数右辺を解く。
pub struct CheckedDenseLu {
    factor: Option<PartialPivLu<f64>>,
    matrix: Vec<f64>,
    n: usize,
}

impl CheckedDenseLu {
    /// 行優先の `n×n` 行列を分解する。相対ピボット閾値は `64*n*EPSILON`。
    pub fn factorize(matrix: &[f64], n: usize) -> Result<Self, DenseSolveError> {
        if n.checked_mul(n) != Some(matrix.len()) {
            return Err(DenseSolveError::Dimension);
        }
        if matrix.iter().any(|x| !x.is_finite()) {
            return Err(DenseSolveError::NonFinite);
        }
        let factor = if n == 0 {
            None
        } else {
            let scale = matrix.iter().map(|x| x.abs()).fold(0.0, f64::max);
            let lu = Mat::from_fn(n, n, |i, j| matrix[i * n + j]).partial_piv_lu();
            let threshold = scale * (64.0 * n as f64 * f64::EPSILON);
            for i in 0..n {
                let pivot = lu.U()[(i, i)].abs();
                if !pivot.is_finite() {
                    return Err(DenseSolveError::NonFinite);
                }
                if pivot <= threshold {
                    return Err(DenseSolveError::Singular { pivot, threshold });
                }
            }
            Some(lu)
        };
        Ok(Self {
            factor,
            matrix: matrix.to_vec(),
            n,
        })
    }

    /// 行優先の `n×nrhs` 右辺を解き、同じ配置で返す。後退誤差も検査する。
    pub fn solve(&self, rhs: &[f64], nrhs: usize) -> Result<Vec<f64>, DenseSolveError> {
        let n = self.n;
        if n.checked_mul(nrhs) != Some(rhs.len()) {
            return Err(DenseSolveError::Dimension);
        }
        if rhs.iter().any(|x| !x.is_finite()) {
            return Err(DenseSolveError::NonFinite);
        }
        if n == 0 || nrhs == 0 {
            return Ok(Vec::new());
        }
        let mut x = Mat::from_fn(n, nrhs, |i, j| rhs[i * nrhs + j]);
        self.factor.as_ref().unwrap().solve_in_place(x.as_mut());
        let mut out = vec![0.0; rhs.len()];
        for i in 0..n {
            for j in 0..nrhs {
                let value = x[(i, j)];
                if !value.is_finite() {
                    return Err(DenseSolveError::NonFinite);
                }
                out[i * nrhs + j] = value;
            }
        }
        for i in 0..n {
            for j in 0..nrhs {
                let mut residual = -rhs[i * nrhs + j];
                let mut scale = rhs[i * nrhs + j].abs();
                for k in 0..n {
                    let product = self.matrix[i * n + k] * out[k * nrhs + j];
                    residual += product;
                    scale += product.abs();
                }
                if !residual.is_finite() || !scale.is_finite() {
                    return Err(DenseSolveError::NonFinite);
                }
                let error = if scale == 0.0 {
                    residual.abs()
                } else {
                    residual.abs() / scale
                };
                if error > 256.0 * n as f64 * f64::EPSILON {
                    return Err(DenseSolveError::Residual { error });
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pivoted_indefinite_matrix_solves_multiple_independent_right_hand_sides() {
        let lu = CheckedDenseLu::factorize(&[0.0, 2.0, 3.0, 4.0], 2).unwrap();
        let actual = lu.solve(&[4.0, -2.0, 11.0, 2.0], 2).unwrap();
        for (x, y) in actual.iter().zip([1.0, 2.0, 2.0, -1.0]) {
            assert!((x - y).abs() < 1e-12);
        }
        assert_eq!(lu.solve(&[0.0, 3.0], 1).unwrap(), vec![1.0, 0.0]);
    }

    #[test]
    fn empty_dimension_and_invalid_inputs_are_explicit() {
        assert!(CheckedDenseLu::factorize(&[], 0)
            .unwrap()
            .solve(&[], 12)
            .unwrap()
            .is_empty());
        assert!(matches!(
            CheckedDenseLu::factorize(&[1.0], 2),
            Err(DenseSolveError::Dimension)
        ));
        assert!(matches!(
            CheckedDenseLu::factorize(&[f64::NAN], 1),
            Err(DenseSolveError::NonFinite)
        ));
        assert!(matches!(
            CheckedDenseLu::factorize(&[1.0, 2.0, 2.0, 4.0], 2),
            Err(DenseSolveError::Singular { .. })
        ));
        let lu = CheckedDenseLu::factorize(&[1.0], 1).unwrap();
        assert!(matches!(lu.solve(&[], 1), Err(DenseSolveError::Dimension)));
        assert!(matches!(
            lu.solve(&[f64::INFINITY], 1),
            Err(DenseSolveError::NonFinite)
        ));
    }
}
