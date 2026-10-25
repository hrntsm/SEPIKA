use crate::solver::{LinearSolver, SolveError, SparsityPattern};
use faer::linalg::solvers::Solve;
use faer::sparse::linalg::solvers::{Llt, SymbolicLlt};
use faer::sparse::SparseColMat;
use faer::Side;
use std::sync::Mutex;

/// 疎 Cholesky 直接法。平衡化後の推定逆条件数が機械精度未満なら分解を拒否する。
/// 分解失敗後の求解は `NotFactorized` を返す。同じ疎構造では symbolic 分解を再利用する。
pub struct CholeskySolver {
    factor: Option<Llt<usize, f64>>,
    symbolic_cache: Option<(SymbolicLlt<usize>, SparsityPattern)>,
    diagonal_sqrt: Vec<f64>,
    n: usize,
    scratch: Mutex<faer::Mat<f64>>,
}

impl Default for CholeskySolver {
    fn default() -> Self {
        Self {
            factor: None,
            symbolic_cache: None,
            diagonal_sqrt: Vec::new(),
            n: 0,
            scratch: Mutex::new(faer::Mat::new()),
        }
    }
}

impl LinearSolver for CholeskySolver {
    fn factorize(&mut self, k: &SparseColMat<usize, f64>) -> Result<(), SolveError> {
        self.factor = None;
        self.n = k.nrows();
        self.diagonal_sqrt = vec![0.0; self.n];
        let (pattern, values) = k.parts();
        for col in 0..self.n {
            for (offset, row) in pattern.row_idx_of_col_raw(col).iter().copied().enumerate() {
                if row == col {
                    let value = values[pattern.col_range(col).start + offset];
                    if !value.is_finite() || value <= 0.0 {
                        return Err(SolveError::NotPositiveDefinite);
                    }
                    self.diagonal_sqrt[col] = value.sqrt();
                }
            }
            if self.diagonal_sqrt[col] == 0.0 {
                return Err(SolveError::NotPositiveDefinite);
            }
        }
        let mut scaled = k.clone();
        let (pattern, values) = scaled.parts_mut();
        let mut column_sums = vec![0.0; self.n];
        for col in 0..self.n {
            for (offset, row) in pattern.row_idx_of_col_raw(col).iter().copied().enumerate() {
                if row < col {
                    continue;
                }
                let value = &mut values[pattern.col_range(col).start + offset];
                *value = if row == col {
                    1.0
                } else {
                    (*value / self.diagonal_sqrt[row]) / self.diagonal_sqrt[col]
                };
                if !value.is_finite() {
                    return Err(SolveError::NotPositiveDefinite);
                }
                column_sums[col] += value.abs();
                if row != col {
                    column_sums[row] += value.abs();
                }
            }
        }
        let norm = column_sums.into_iter().fold(0.0_f64, f64::max);
        let symbolic = match &self.symbolic_cache {
            Some((sym, cached_pattern)) if cached_pattern.matches(k) => sym.clone(),
            _ => {
                let sym = SymbolicLlt::try_new(k.symbolic(), Side::Lower)
                    .map_err(|e| SolveError::Backend(format!("symbolic: {e:?}")))?;
                self.symbolic_cache = Some((sym.clone(), SparsityPattern::of(k)));
                sym
            }
        };
        let factor = Llt::try_new_with_symbolic(symbolic, scaled.as_ref(), Side::Lower)
            .map_err(|_| SolveError::NotPositiveDefinite)?;
        if self.n > 0 {
            let mut column = faer::Mat::zeros(self.n, 1);
            let inverse_norm = crate::condition::estimate_symmetric_one_norm(self.n, |x| {
                for (i, value) in x.iter().enumerate() {
                    column[(i, 0)] = *value;
                }
                factor.solve_in_place(column.as_mut());
                for (i, value) in x.iter_mut().enumerate() {
                    *value = column[(i, 0)];
                }
            });
            let rcond = (1.0 / norm) / inverse_norm;
            if !rcond.is_finite() || rcond < f64::EPSILON / 2.0 {
                return Err(SolveError::NotPositiveDefinite);
            }
        }
        self.factor = Some(factor);
        Ok(())
    }

    fn solve(&self, rhs: &[f64]) -> Result<Vec<f64>, SolveError> {
        let mut out = Vec::new();
        self.solve_into(rhs, &mut out)?;
        Ok(out)
    }

    fn solve_into(&self, rhs: &[f64], out: &mut Vec<f64>) -> Result<(), SolveError> {
        let factor = self.factor.as_ref().ok_or(SolveError::NotFactorized)?;
        if rhs.len() != self.n {
            return Err(SolveError::DimMismatch {
                k: self.n,
                rhs: rhs.len(),
            });
        }
        let mut scratch = self.scratch.lock().expect("スクラッチのロックに失敗");
        if scratch.nrows() != self.n || scratch.ncols() != 1 {
            *scratch = faer::Mat::zeros(self.n, 1);
        }
        for i in 0..self.n {
            scratch[(i, 0)] = rhs[i] / self.diagonal_sqrt[i];
        }
        factor.solve_in_place(scratch.as_mut());
        out.resize(self.n, 0.0);
        for i in 0..self.n {
            out[i] = scratch[(i, 0)] / self.diagonal_sqrt[i];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{make_solver, SolverBackend};
    use crate::sparse::{assemble_csc, Triplet};

    #[test]
    fn test_2dof_spring() {
        faer::set_global_parallelism(faer::Par::Seq);
        let k = assemble_csc(
            2,
            vec![
                Triplet {
                    row: 0,
                    col: 0,
                    val: 300.0,
                },
                Triplet {
                    row: 1,
                    col: 0,
                    val: -200.0,
                },
                Triplet {
                    row: 0,
                    col: 1,
                    val: -200.0,
                },
                Triplet {
                    row: 1,
                    col: 1,
                    val: 200.0,
                },
            ],
        );
        let mut solver = make_solver(SolverBackend::DirectSparseCholesky);
        solver.factorize(&k).unwrap();
        let x = solver.solve(&[0.0, 1000.0]).unwrap();
        approx::assert_relative_eq!(x[0], 10.0, max_relative = 1e-9);
        approx::assert_relative_eq!(x[1], 15.0, max_relative = 1e-9);
    }

    #[test]
    fn test_2dof_deterministic() {
        faer::set_global_parallelism(faer::Par::Seq);
        let k = assemble_csc(
            2,
            vec![
                Triplet {
                    row: 0,
                    col: 0,
                    val: 300.0,
                },
                Triplet {
                    row: 1,
                    col: 0,
                    val: -200.0,
                },
                Triplet {
                    row: 0,
                    col: 1,
                    val: -200.0,
                },
                Triplet {
                    row: 1,
                    col: 1,
                    val: 200.0,
                },
            ],
        );
        let mut solver0 = make_solver(SolverBackend::DirectSparseCholesky);
        solver0.factorize(&k).unwrap();
        let x0 = solver0.solve(&[0.0, 1000.0]).unwrap();
        let mut solver = make_solver(SolverBackend::DirectSparseCholesky);
        solver.factorize(&k).unwrap();
        let x = solver.solve(&[0.0, 1000.0]).unwrap();
        assert_eq!(x, x0);
    }

    /// 3 自由度のバネ連成系（三重対角パターン）。行列の値だけを差し替えて
    /// symbolic キャッシュの再利用を検証するために使う。
    fn spring_chain_3dof(k12: f64, k23: f64) -> SparseColMat<usize, f64> {
        assemble_csc(
            3,
            vec![
                Triplet {
                    row: 0,
                    col: 0,
                    val: 300.0,
                },
                Triplet {
                    row: 1,
                    col: 0,
                    val: -k12,
                },
                Triplet {
                    row: 0,
                    col: 1,
                    val: -k12,
                },
                Triplet {
                    row: 1,
                    col: 1,
                    val: k12 + k23,
                },
                Triplet {
                    row: 2,
                    col: 1,
                    val: -k23,
                },
                Triplet {
                    row: 1,
                    col: 2,
                    val: -k23,
                },
                Triplet {
                    row: 2,
                    col: 2,
                    val: k23 + 50.0,
                },
            ],
        )
    }

    /// symbolic キャッシュの本体テスト: 同一スパースパターンで 2 回目の
    /// `factorize`（値は変わる）が、そのパターンを毎回新規に symbolic 分解した
    /// 場合とビット一致すること。AMD 順序付けを使い回しても数値結果が変わらない
    /// ことの検証（決定性の要件）。
    #[test]
    fn test_reused_symbolic_matches_fresh_bit_exact() {
        faer::set_global_parallelism(faer::Par::Seq);
        let k1 = spring_chain_3dof(200.0, 100.0);
        let k2 = spring_chain_3dof(80.0, 260.0);
        let rhs = [1.0, -2.0, 3.5];

        // 1 回目で symbolic を構築し、2 回目（同一パターン・別の値）で再利用させる。
        let mut reused = CholeskySolver::default();
        reused.factorize(&k1).unwrap();
        reused.factorize(&k2).unwrap();
        let x_reused = reused.solve(&rhs).unwrap();

        // 比較対象: k2 のみで新規に factorize したソルバ（symbolic を毎回作り直す）。
        let mut fresh = CholeskySolver::default();
        fresh.factorize(&k2).unwrap();
        let x_fresh = fresh.solve(&rhs).unwrap();

        assert_eq!(
            x_reused, x_fresh,
            "symbolic 再利用と毎回新規構築でビット不一致"
        );
    }

    /// スパースパターンが変わった場合は symbolic を自動的に作り直し、
    /// 引き続き正しい解が得られること（キャッシュ無効化のフォールバック確認）。
    #[test]
    fn test_pattern_change_recomputes_symbolic() {
        faer::set_global_parallelism(faer::Par::Seq);
        let k_tridiag = spring_chain_3dof(200.0, 100.0);
        // (0,2)/(2,0) を追加した別パターン（正定値を保つ対角優位な値）。
        let k_dense = assemble_csc(
            3,
            vec![
                Triplet {
                    row: 0,
                    col: 0,
                    val: 400.0,
                },
                Triplet {
                    row: 1,
                    col: 0,
                    val: -100.0,
                },
                Triplet {
                    row: 0,
                    col: 1,
                    val: -100.0,
                },
                Triplet {
                    row: 2,
                    col: 0,
                    val: -50.0,
                },
                Triplet {
                    row: 0,
                    col: 2,
                    val: -50.0,
                },
                Triplet {
                    row: 1,
                    col: 1,
                    val: 300.0,
                },
                Triplet {
                    row: 2,
                    col: 1,
                    val: -30.0,
                },
                Triplet {
                    row: 1,
                    col: 2,
                    val: -30.0,
                },
                Triplet {
                    row: 2,
                    col: 2,
                    val: 200.0,
                },
            ],
        );
        let rhs = [10.0, 0.0, 0.0];

        let mut solver = CholeskySolver::default();
        solver.factorize(&k_tridiag).unwrap();
        let _ = solver.solve(&rhs).unwrap();
        // パターンが変わる（(0,2)/(2,0) が追加される）→ symbolic を作り直すはず。
        solver.factorize(&k_dense).unwrap();
        let x = solver.solve(&rhs).unwrap();

        let mut fresh = CholeskySolver::default();
        fresh.factorize(&k_dense).unwrap();
        let x_fresh = fresh.solve(&rhs).unwrap();
        assert_eq!(x, x_fresh);
    }
}

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    use crate::sparse::{assemble_csc, Triplet};

    fn matrix(values: &[&[f64]]) -> SparseColMat<usize, f64> {
        assemble_csc(
            values.len(),
            values
                .iter()
                .enumerate()
                .flat_map(|(row, entries)| {
                    entries
                        .iter()
                        .enumerate()
                        .map(move |(col, &val)| Triplet { row, col, val })
                })
                .collect(),
        )
    }

    #[test]
    #[ignore = "条件数診断による分解コストを比較する手動計測"]
    fn condition_estimation_cost() {
        use std::hint::black_box;
        use std::time::Instant;
        faer::set_global_parallelism(faer::Par::Seq);
        for side in [8, 16] {
            let n = side * side * side;
            let mut entries = Vec::new();
            for z in 0..side {
                for y in 0..side {
                    for x in 0..side {
                        let col = x + side * (y + side * z);
                        entries.push(Triplet {
                            row: col,
                            col,
                            val: 6.1,
                        });
                        for neighbor in [
                            (x + 1 < side).then_some(col + 1),
                            (y + 1 < side).then_some(col + side),
                            (z + 1 < side).then_some(col + side * side),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            entries.push(Triplet {
                                row: neighbor,
                                col,
                                val: -1.0,
                            });
                            entries.push(Triplet {
                                row: col,
                                col: neighbor,
                                val: -1.0,
                            });
                        }
                    }
                }
            }
            let k = assemble_csc(n, entries);
            let symbolic = SymbolicLlt::try_new(k.symbolic(), Side::Lower).unwrap();
            black_box(
                Llt::try_new_with_symbolic(symbolic.clone(), k.as_ref(), Side::Lower).unwrap(),
            );
            let mut solver = CholeskySolver::default();
            solver.factorize(&k).unwrap();
            let start = Instant::now();
            for _ in 0..5 {
                black_box(
                    Llt::try_new_with_symbolic(symbolic.clone(), k.as_ref(), Side::Lower).unwrap(),
                );
            }
            let baseline = start.elapsed().as_secs_f64() / 5.0;
            let start = Instant::now();
            for _ in 0..5 {
                solver.factorize(black_box(&k)).unwrap();
            }
            let checked = start.elapsed().as_secs_f64() / 5.0;
            println!(
                "n={n}, nnz={}, baseline_ms={:.3}, checked_ms={:.3}, ratio={:.3}",
                k.parts().1.len(),
                baseline * 1000.0,
                checked * 1000.0,
                checked / baseline
            );
        }
    }

    #[test]
    fn rounded_mechanism_is_rejected_for_scaled_and_sign_reversed_coordinates() {
        for scale in [
            [1.0, 1.0],
            [2.0_f64.powi(-100), 2.0_f64.powi(100)],
            [2.0_f64.powi(100), 2.0_f64.powi(-100)],
        ] {
            for sign in [-1.0, 1.0] {
                let k = matrix(&[
                    &[scale[0] * scale[0], sign * scale[0] * scale[1]],
                    &[
                        sign * scale[0] * scale[1],
                        scale[1] * scale[1] * (1.0 + f64::EPSILON),
                    ],
                ]);
                let mut solver = CholeskySolver::default();
                assert!(matches!(
                    solver.factorize(&k),
                    Err(SolveError::NotPositiveDefinite)
                ));
            }
        }
    }

    #[test]
    fn lower_triangle_is_the_only_input_to_factorization_and_condition_diagnosis() {
        let mut solver = CholeskySolver::default();
        for upper in [0.0, f64::NAN, 1e300] {
            solver
                .factorize(&matrix(&[&[4.0, upper], &[1.0, 9.0]]))
                .unwrap();
            let x = solver.solve(&[6.0, 19.0]).unwrap();
            approx::assert_relative_eq!(x[0], 1.0, max_relative = 1e-14);
            approx::assert_relative_eq!(x[1], 2.0, max_relative = 1e-14);
        }
        assert!(solver
            .factorize(&matrix(&[&[4.0, 1.0], &[f64::NAN, 9.0]]))
            .is_err());
    }

    #[test]
    fn coordinate_permutations_preserve_weak_finite_solutions() {
        let values = [
            [1.0, -1.0, 0.0],
            [-1.0, 1.0 + 1e-10, 0.0],
            [0.0, 0.0, 1e120],
        ];
        for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
            let entries = (0..3)
                .flat_map(|row| {
                    (0..3).map(move |col| Triplet {
                        row,
                        col,
                        val: values[order[row]][order[col]],
                    })
                })
                .collect();
            let mut solver = CholeskySolver::default();
            solver.factorize(&assemble_csc(3, entries)).unwrap();
            let rhs = order.map(|i| [0.0, 1e-10, 1e120][i]);
            let x = solver.solve(&rhs).unwrap();
            for value in x {
                approx::assert_relative_eq!(value, 1.0, max_relative = 1e-5);
            }
        }
    }

    #[test]
    fn finite_weak_stiffness_is_accepted_independently_of_other_blocks_and_units() {
        for n in [2, 10000] {
            for scale in [[1.0, 1.0], [1e-60, 1e60], [1e60, 1e-60]] {
                let stiffness = 1e-10;
                let mut entries = vec![
                    Triplet {
                        row: 0,
                        col: 0,
                        val: scale[0] * scale[0],
                    },
                    Triplet {
                        row: 0,
                        col: 1,
                        val: -scale[0] * scale[1],
                    },
                    Triplet {
                        row: 1,
                        col: 0,
                        val: -scale[0] * scale[1],
                    },
                    Triplet {
                        row: 1,
                        col: 1,
                        val: (1.0 + stiffness) * scale[1] * scale[1],
                    },
                ];
                entries.extend((2..n).map(|i| Triplet {
                    row: i,
                    col: i,
                    val: 1e-200,
                }));
                let k = assemble_csc(n, entries);
                let mut solver = CholeskySolver::default();
                solver.factorize(&k).unwrap();
                let mut rhs = vec![1e-200; n];
                rhs[0] = 0.0;
                rhs[1] = stiffness * scale[1];
                let result = solver.solve(&rhs).unwrap();
                for (i, value) in result.iter().enumerate() {
                    let expected = if i < 2 { 1.0 / scale[i] } else { 1.0 };
                    approx::assert_relative_eq!(*value, expected, max_relative = 1e-5);
                }
            }
        }
    }

    #[test]
    fn dense_factor_preserves_finite_weak_stiffness() {
        let n = 192;
        for weak in [1e-8, f64::EPSILON] {
            let k = assemble_csc(
                n,
                (0..n)
                    .flat_map(|col| {
                        (0..n).map(move |row| Triplet {
                            row,
                            col,
                            val: if row == col { 1.0 + weak } else { 1.0 },
                        })
                    })
                    .collect(),
            );
            let mut solver = CholeskySolver::default();
            let symbolic = faer::sparse::linalg::cholesky::factorize_symbolic_cholesky(
                k.symbolic(),
                Side::Lower,
                Default::default(),
                Default::default(),
            )
            .unwrap();
            assert!(matches!(
                symbolic.raw(),
                faer::sparse::linalg::cholesky::SymbolicCholeskyRaw::Supernodal(_)
            ));
            let result = solver.factorize(&k);
            if weak > f64::EPSILON {
                result.unwrap();
                let x = solver.solve(&vec![n as f64 + weak; n]).unwrap();
                for value in x {
                    approx::assert_relative_eq!(value, 1.0, max_relative = 1e-5);
                }
            } else {
                assert!(matches!(result, Err(SolveError::NotPositiveDefinite)));
            }
        }
    }

    #[test]
    fn failed_refactorization_invalidates_all_previous_solutions() {
        let mut solver = CholeskySolver::default();
        solver
            .factorize(&matrix(&[&[2.0, 0.0], &[0.0, 3.0]]))
            .unwrap();
        for bad in [
            matrix(&[&[1.0, 1.0], &[1.0, 1.0 + f64::EPSILON]]),
            matrix(&[&[0.0]]),
        ] {
            assert!(solver.factorize(&bad).is_err());
            assert!(matches!(
                solver.solve(&[1.0, 1.0]),
                Err(SolveError::NotFactorized)
            ));
            assert!(matches!(
                solver.solve_into(&[1.0, 1.0], &mut vec![]),
                Err(SolveError::NotFactorized)
            ));
        }
        solver.factorize(&matrix(&[&[2.0]])).unwrap();
        approx::assert_relative_eq!(solver.solve(&[4.0]).unwrap()[0], 2.0, max_relative = 1e-14);
    }

    #[test]
    fn non_spd_matrices_are_rejected_even_with_unexcited_or_zero_loads() {
        for values in [
            [&[1.0, 0.0][..], &[0.0, 0.0][..]],
            [&[1.0, 2.0][..], &[2.0, 1.0][..]],
        ] {
            for rhs in [[1.0, 0.0], [1.0, 1.0], [0.0, 0.0]] {
                let mut solver = CholeskySolver::default();
                let result = solver
                    .factorize(&matrix(&values))
                    .and_then(|()| solver.solve(&rhs));
                assert!(matches!(result, Err(SolveError::NotPositiveDefinite)));
            }
        }
    }

    #[test]
    fn original_f64_residual_and_solution_for_small_and_zero_rhs() {
        let values = [&[1.0][..]];
        let mut solver = CholeskySolver::default();
        solver.factorize(&matrix(&values)).unwrap();
        for b in [1.0e-16, 0.0] {
            let rhs = [b];
            let solution = solver.solve(&rhs).unwrap();
            let mut out = Vec::new();
            solver.solve_into(&rhs, &mut out).unwrap();
            assert_eq!(out, solution);
            assert!(solution[0].is_finite());
            assert!((solution[0] - b).abs() <= 1.0e-6 * b.abs());
            let residual = (values[0][0] * solution[0] - rhs[0]).abs();
            assert!(residual <= 1.0e-6 * b.abs());
            if b != 0.0 {
                assert!(b.abs() > 1.0e-6 * b.abs(), "ゼロ解を試験許容で拒否する");
            }
        }
    }

    #[test]
    fn spd_multiple_rhs_and_updated_pattern_have_original_f64_residuals() {
        let mut solver = CholeskySolver::default();
        for values in [
            [&[4.0, 1.0][..], &[1.0, 3.0][..]],
            [&[8.0, 2.0][..], &[2.0, 6.0][..]],
        ] {
            solver.factorize(&matrix(&values)).unwrap();
            for expected in [[1.0, 2.0], [-2.0, 3.0], [0.0, 0.0]] {
                let rhs: Vec<f64> = values
                    .iter()
                    .map(|row| row.iter().zip(expected).map(|(k, x)| k * x).sum())
                    .collect();
                let solution = solver.solve(&rhs).unwrap();
                let mut out = vec![f64::NAN];
                solver.solve_into(&rhs, &mut out).unwrap();
                assert_eq!(solution, out);
                for (i, row) in values.iter().enumerate() {
                    assert!(solution[i].is_finite());
                    assert!((solution[i] - expected[i]).abs() <= 1.0e-6 * expected[i].abs());
                    let residual =
                        (row.iter().zip(&solution).map(|(k, x)| k * x).sum::<f64>() - rhs[i]).abs();
                    assert!(residual <= 1.0e-6 * rhs[i].abs());
                }
            }
        }
    }
}
