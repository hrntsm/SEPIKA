use crate::solver::{LinearSolver, SolveError};
use faer::sparse::linalg::solvers::{Lu, SymbolicLu};
use faer::sparse::SparseColMat;

/// 疎 LU 直接法ソルバ。対称正定値でない系にも使える。
/// 分解失敗後の求解は `NotFactorized` を返す。
#[derive(Default)]
pub struct LuSolver {
    factor: Option<Lu<usize, f64>>,
    n: usize,
}

impl LinearSolver for LuSolver {
    fn factorize(&mut self, k: &SparseColMat<usize, f64>) -> Result<(), SolveError> {
        self.factor = None;
        self.n = k.nrows();
        let symbolic = SymbolicLu::try_new(k.symbolic())
            .map_err(|e| SolveError::Backend(format!("symbolic: {e:?}")))?;
        let lu = Lu::try_new_with_symbolic(symbolic, k.as_ref())
            .map_err(|e| SolveError::Backend(format!("LU factorize: {e:?}")))?;
        self.factor = Some(lu);
        Ok(())
    }

    fn solve(&self, rhs: &[f64]) -> Result<Vec<f64>, SolveError> {
        let lu = self.factor.as_ref().ok_or(SolveError::NotFactorized)?;
        crate::solver::solve_dense_column(lu, rhs, self.n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse::{assemble_csc, Triplet};

    #[test]
    fn indefinite_matrix_preserves_lu_contract() {
        let k = assemble_csc(
            2,
            vec![
                Triplet {
                    row: 0,
                    col: 0,
                    val: 1.0,
                },
                Triplet {
                    row: 0,
                    col: 1,
                    val: 2.0,
                },
                Triplet {
                    row: 1,
                    col: 0,
                    val: 2.0,
                },
                Triplet {
                    row: 1,
                    col: 1,
                    val: 1.0,
                },
            ],
        );
        let mut solver = LuSolver::default();
        solver.factorize(&k).unwrap();
        for rhs in [[3.0, 3.0], [0.0, 0.0]] {
            let x = solver.solve(&rhs).unwrap();
            let mut out = Vec::new();
            solver.solve_into(&rhs, &mut out).unwrap();
            assert_eq!(x, out);
            for value in x {
                assert!(value.is_finite());
                approx::assert_relative_eq!(value, rhs[0] / 3.0, max_relative = 1.0e-9);
            }
        }
    }

    /// 非対称行列も解ける（Cholesky では対象外のケース）。
    #[test]
    fn test_lu_unsymmetric() {
        faer::set_global_parallelism(faer::Par::Seq);
        let k = assemble_csc(
            2,
            vec![
                Triplet {
                    row: 0,
                    col: 0,
                    val: 2.0,
                },
                Triplet {
                    row: 0,
                    col: 1,
                    val: 1.0,
                },
                Triplet {
                    row: 1,
                    col: 0,
                    val: 0.5,
                },
                Triplet {
                    row: 1,
                    col: 1,
                    val: 3.0,
                },
            ],
        );
        let mut solver = LuSolver::default();
        solver.factorize(&k).unwrap();
        // [2 1; 0.5 3] x = [4; 6.5] -> x = [1; 2]
        let x = solver.solve(&[4.0, 6.5]).unwrap();
        approx::assert_relative_eq!(x[0], 1.0, max_relative = 1e-9);
        approx::assert_relative_eq!(x[1], 2.0, max_relative = 1e-9);
    }

    #[test]
    fn failed_refactorization_invalidates_previous_factor_and_can_recover() {
        let valid = assemble_csc(
            2,
            vec![
                Triplet {
                    row: 0,
                    col: 0,
                    val: 2.0,
                },
                Triplet {
                    row: 1,
                    col: 1,
                    val: 3.0,
                },
            ],
        );
        let mut solver = LuSolver::default();
        let mut out = Vec::new();
        for n in [2, 3] {
            solver.factorize(&valid).unwrap();
            assert_eq!(solver.solve(&[4.0, 9.0]).unwrap(), vec![2.0, 3.0]);
            let singular = assemble_csc(
                n,
                vec![Triplet {
                    row: 0,
                    col: 0,
                    val: 1.0,
                }],
            );
            assert!(solver.factorize(&singular).is_err());
            for rhs in [vec![1.0; 2], vec![1.0; n]] {
                assert!(matches!(solver.solve(&rhs), Err(SolveError::NotFactorized)));
                assert!(matches!(
                    solver.solve_into(&rhs, &mut out),
                    Err(SolveError::NotFactorized)
                ));
            }
            solver.factorize(&valid).unwrap();
            solver.solve_into(&[4.0, 9.0], &mut out).unwrap();
            assert_eq!(out, vec![2.0, 3.0]);
        }
    }
}
