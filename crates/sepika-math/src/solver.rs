use faer::sparse::SparseColMat;

/// 疎行列ソルバの共通インタフェース。
pub trait LinearSolver: Send + Sync {
    fn factorize(&mut self, k: &SparseColMat<usize, f64>) -> Result<(), SolveError>;
    fn solve(&self, rhs: &[f64]) -> Result<Vec<f64>, SolveError>;

    /// [`Self::solve`] のバッファ再利用版。解を `out` に書き込む。
    ///
    /// 既定実装は `solve` を呼んで結果をコピーするだけ。
    fn solve_into(&self, rhs: &[f64], out: &mut Vec<f64>) -> Result<(), SolveError> {
        *out = self.solve(rhs)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SolveError {
    #[error("not factorized yet")]
    NotFactorized,
    #[error("matrix not positive definite")]
    NotPositiveDefinite,
    #[error("dimension mismatch: K={k}, rhs={rhs}")]
    DimMismatch { k: usize, rhs: usize },
    #[error("faer error: {0}")]
    Backend(String),
    /// 入力モデル起因のエラー。メッセージはユーザー向け診断文（日本語）とする。
    #[error("{0}")]
    InvalidInput(String),
    /// 反復法・固有値解析が規定回数内に収束しなかった。
    #[error("収束しませんでした: {0}")]
    NonConvergence(String),
}

#[derive(Clone, Copy, Debug)]
pub enum SolverBackend {
    DirectSparseCholesky,
    DirectSparseLu,
    IterativePcg {
        tol: f64,
        max_iter: usize,
    },
    /// 自由度数に応じて疎 Cholesky / PCG を自動選択する。PCG 非収束時は疎 Cholesky へフォールバックする。
    Auto,
}

/// 因子分解済みソルバに単一 RHS 列を与えて解く共通処理（内部用）。
pub(crate) fn solve_dense_column<S: faer::linalg::solvers::Solve<f64>>(
    factor: &S,
    rhs: &[f64],
    n: usize,
) -> Result<Vec<f64>, SolveError> {
    if rhs.len() != n {
        return Err(SolveError::DimMismatch {
            k: n,
            rhs: rhs.len(),
        });
    }
    let b = faer::Mat::from_fn(n, 1, |i, _| rhs[i]);
    let x = factor.solve(b.as_ref());
    Ok((0..n).map(|i| x[(i, 0)]).collect())
}

/// [`solve_dense_column`] のバッファ再利用版（内部用）。
pub(crate) fn solve_dense_column_into<S: faer::linalg::solvers::Solve<f64>>(
    factor: &S,
    rhs: &[f64],
    n: usize,
    scratch: &mut faer::Mat<f64>,
    out: &mut Vec<f64>,
) -> Result<(), SolveError> {
    if rhs.len() != n {
        return Err(SolveError::DimMismatch {
            k: n,
            rhs: rhs.len(),
        });
    }
    if scratch.nrows() != n || scratch.ncols() != 1 {
        *scratch = faer::Mat::zeros(n, 1);
    }
    for i in 0..n {
        scratch[(i, 0)] = rhs[i];
    }
    factor.solve_in_place(scratch.as_mut());
    if out.len() != n {
        out.resize(n, 0.0);
    }
    for i in 0..n {
        out[i] = scratch[(i, 0)];
    }
    Ok(())
}

/// スパースパターンの不変シグネチャ（内部用）。値は含めない。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SparsityPattern {
    col_ptr: Vec<usize>,
    row_idx: Vec<usize>,
}

impl SparsityPattern {
    pub(crate) fn of(k: &SparseColMat<usize, f64>) -> Self {
        let sym = k.symbolic();
        Self {
            col_ptr: sym.col_ptr().to_vec(),
            row_idx: sym.row_idx().to_vec(),
        }
    }

    /// キャッシュ済みパターンと一致するか。
    pub(crate) fn matches(&self, k: &SparseColMat<usize, f64>) -> bool {
        let sym = k.symbolic();
        self.col_ptr == sym.col_ptr() && self.row_idx == sym.row_idx()
    }
}

pub fn make_solver(backend: SolverBackend) -> Box<dyn LinearSolver> {
    match backend {
        SolverBackend::DirectSparseCholesky => Box::new(crate::cholesky::CholeskySolver::default()),
        SolverBackend::IterativePcg { tol, max_iter } => {
            Box::new(crate::pcg::PcgSolver::new(tol, max_iter))
        }
        SolverBackend::DirectSparseLu => Box::new(crate::lu::LuSolver::default()),
        SolverBackend::Auto => Box::new(crate::auto::AutoSolver::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto::AutoSolver;
    use crate::sparse::{assemble_csc, Triplet};

    fn solvers() -> Vec<(&'static str, Box<dyn LinearSolver>)> {
        vec![
            ("Cholesky", make_solver(SolverBackend::DirectSparseCholesky)),
            ("LU", make_solver(SolverBackend::DirectSparseLu)),
            (
                "PCG",
                make_solver(SolverBackend::IterativePcg {
                    tol: 1e-12,
                    max_iter: 10,
                }),
            ),
            ("Auto/direct", make_solver(SolverBackend::Auto)),
            ("Auto/PCG", Box::new(AutoSolver::with_params(0, 1e-12, 10))),
        ]
    }

    #[test]
    fn linear_solver_lifecycle_contract() {
        let diagonal = |scale: f64| {
            assemble_csc(
                2,
                vec![
                    Triplet {
                        row: 0,
                        col: 0,
                        val: 2.0 * scale,
                    },
                    Triplet {
                        row: 1,
                        col: 1,
                        val: 3.0 * scale,
                    },
                ],
            )
        };
        for (backend, mut solver) in solvers() {
            let mut out = vec![f64::NAN];
            assert!(
                matches!(solver.solve(&[4.0, 9.0]), Err(SolveError::NotFactorized)),
                "{backend}"
            );
            assert!(
                matches!(
                    solver.solve_into(&[4.0, 9.0], &mut out),
                    Err(SolveError::NotFactorized)
                ),
                "{backend}"
            );
            solver.factorize(&diagonal(1.0)).unwrap();
            assert!(
                matches!(
                    solver.solve(&[4.0]),
                    Err(SolveError::DimMismatch { k: 2, rhs: 1 })
                ),
                "{backend}"
            );
            assert!(
                matches!(
                    solver.solve_into(&[4.0], &mut out),
                    Err(SolveError::DimMismatch { k: 2, rhs: 1 })
                ),
                "{backend}"
            );
            for (rhs, expected) in [([4.0, 9.0], [2.0, 3.0]), ([8.0, 18.0], [4.0, 6.0])] {
                let solution = solver.solve(&rhs).unwrap();
                solver.solve_into(&rhs, &mut out).unwrap();
                assert_eq!(out, solution, "{backend}");
                for (actual, expected) in out.iter().zip(expected) {
                    approx::assert_relative_eq!(*actual, expected, max_relative = 1e-10);
                }
            }
            solver.factorize(&diagonal(2.0)).unwrap();
            solver.solve_into(&[8.0, 18.0], &mut out).unwrap();
            for (actual, expected) in out.iter().zip([2.0, 3.0]) {
                approx::assert_relative_eq!(*actual, expected, max_relative = 1e-10);
            }
        }
    }
}
