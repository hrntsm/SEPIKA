use sepika_math::dense_lu::{CheckedDenseLu, DenseSolveError};

/// 保持12自由度と内部回転（最大6）の同一trial応答。接線ブロックは行優先。
#[derive(Clone, Debug)]
pub struct LocalTrialResponse {
    pub retained_force: [f64; 12],
    /// 荷重を差し引いた内部回転残差 `[Nmm]`。
    pub residual: Vec<f64>,
    pub kaa: [[f64; 12]; 12],
    pub kab: Vec<f64>,
    pub kba: Vec<f64>,
    pub kbb: Vec<f64>,
}

/// 局所残差の許容値は `absolute_tolerance_nmm + relative_tolerance*reference_moment_nmm`。
#[derive(Clone, Copy, Debug)]
pub struct LocalEquilibriumOptions {
    pub reference_moment_nmm: f64,
    pub absolute_tolerance_nmm: f64,
    pub relative_tolerance: f64,
    /// Newton増分の最大回数。0でも初期状態を評価する。
    pub max_iterations: usize,
}

/// 局所失敗理由。要素ID・解析種別は呼出側で付与する。
#[derive(Debug, thiserror::Error)]
pub enum LocalEquilibriumError {
    #[error("局所釣合いの寸法が一致しません")]
    Dimension,
    #[error("局所釣合いの許容値が不正です")]
    InvalidOptions,
    #[error("局所釣合いに非有限値があります")]
    NonFinite,
    #[error(transparent)]
    LinearSolve(#[from] DenseSolveError),
    #[error("局所釣合い未収束: iterations={iterations}, residual={residual_nmm} Nmm, tolerance={tolerance_nmm} Nmm, reference={reference_moment_nmm} Nmm")]
    NotConverged {
        iterations: usize,
        residual_nmm: f64,
        tolerance_nmm: f64,
        reference_moment_nmm: f64,
    },
    #[error("局所trial評価失敗: {0}")]
    Evaluation(String),
}

/// 収束状態の内力・Schur接線・内部回転。commitは行わない。
#[derive(Debug)]
pub struct LocalEquilibriumSolution {
    pub rotations_rad: Vec<f64>,
    pub retained_force: [f64; 12],
    pub tangent: [[f64; 12]; 12],
    pub residual_nmm: f64,
    pub iterations: usize,
}

/// 各評価で同じcommitを復元し、荷重込み残差と全微分を同一trialから返すこと。
/// 呼出側は失敗時の材料trial破棄と、材料・内部回転の同時commit/revertを所有する。
pub fn solve_local_equilibrium(
    initial_rotations_rad: &[f64],
    options: LocalEquilibriumOptions,
    evaluate: impl FnMut(&[f64]) -> Result<LocalTrialResponse, LocalEquilibriumError>,
) -> Result<LocalEquilibriumSolution, LocalEquilibriumError> {
    solve_with_step_control(initial_rotations_rad, options, false, evaluate)
}

pub(crate) fn solve_local_equilibrium_damped(
    initial_rotations_rad: &[f64],
    options: LocalEquilibriumOptions,
    evaluate: impl FnMut(&[f64]) -> Result<LocalTrialResponse, LocalEquilibriumError>,
) -> Result<LocalEquilibriumSolution, LocalEquilibriumError> {
    solve_with_step_control(initial_rotations_rad, options, true, evaluate)
}

fn solve_with_step_control(
    initial_rotations_rad: &[f64],
    options: LocalEquilibriumOptions,
    damped: bool,
    mut evaluate: impl FnMut(&[f64]) -> Result<LocalTrialResponse, LocalEquilibriumError>,
) -> Result<LocalEquilibriumSolution, LocalEquilibriumError> {
    let n = initial_rotations_rad.len();
    if n > 6 {
        return Err(LocalEquilibriumError::Dimension);
    }
    if initial_rotations_rad.iter().any(|x| !x.is_finite()) {
        return Err(LocalEquilibriumError::NonFinite);
    }
    if !options.reference_moment_nmm.is_finite()
        || options.reference_moment_nmm <= 0.0
        || !options.absolute_tolerance_nmm.is_finite()
        || options.absolute_tolerance_nmm < 0.0
        || !options.relative_tolerance.is_finite()
        || options.relative_tolerance < 0.0
    {
        return Err(LocalEquilibriumError::InvalidOptions);
    }
    let tolerance_nmm =
        options.absolute_tolerance_nmm + options.relative_tolerance * options.reference_moment_nmm;
    if !tolerance_nmm.is_finite() {
        return Err(LocalEquilibriumError::InvalidOptions);
    }
    let mut b = initial_rotations_rad.to_vec();
    for iterations in 0..=options.max_iterations {
        let response = evaluate(&b)?;
        if response.residual.len() != n
            || response.kab.len() != 12 * n
            || response.kba.len() != n * 12
            || response.kbb.len() != n * n
        {
            return Err(LocalEquilibriumError::Dimension);
        }
        if response
            .retained_force
            .iter()
            .chain(response.residual.iter())
            .chain(response.kaa.iter().flatten())
            .chain(response.kab.iter())
            .chain(response.kba.iter())
            .chain(response.kbb.iter())
            .any(|x| !x.is_finite())
        {
            return Err(LocalEquilibriumError::NonFinite);
        }
        let residual_nmm = response
            .residual
            .iter()
            .map(|x| x.abs())
            .fold(0.0, f64::max);
        let lu = CheckedDenseLu::factorize(&response.kbb, n)?;
        if residual_nmm <= tolerance_nmm {
            let x = lu.solve(&response.kba, 12)?;
            let mut tangent = response.kaa;
            for (i, row) in tangent.iter_mut().enumerate() {
                for (j, value) in row.iter_mut().enumerate() {
                    for k in 0..n {
                        *value -= response.kab[i * n + k] * x[k * 12 + j];
                    }
                    if !value.is_finite() {
                        return Err(LocalEquilibriumError::NonFinite);
                    }
                }
            }
            return Ok(LocalEquilibriumSolution {
                rotations_rad: b,
                retained_force: response.retained_force,
                tangent,
                residual_nmm,
                iterations,
            });
        }
        if iterations == options.max_iterations {
            return Err(LocalEquilibriumError::NotConverged {
                iterations,
                residual_nmm,
                tolerance_nmm,
                reference_moment_nmm: options.reference_moment_nmm,
            });
        }
        let delta = lu.solve(&response.residual, 1)?;
        if !damped {
            for (value, increment) in b.iter_mut().zip(delta) {
                *value -= increment;
                if !value.is_finite() {
                    return Err(LocalEquilibriumError::NonFinite);
                }
            }
            continue;
        }
        let mut accepted = None;
        for cut in 0..=12 {
            let scale = 0.5_f64.powi(cut);
            let candidate: Vec<_> = b
                .iter()
                .zip(&delta)
                .map(|(value, increment)| value - scale * increment)
                .collect();
            if candidate.iter().any(|value| !value.is_finite()) {
                return Err(LocalEquilibriumError::NonFinite);
            }
            let trial = evaluate(&candidate)?;
            if trial.residual.len() != n {
                return Err(LocalEquilibriumError::Dimension);
            }
            if trial.residual.iter().any(|value| !value.is_finite()) {
                return Err(LocalEquilibriumError::NonFinite);
            }
            let norm = trial
                .residual
                .iter()
                .map(|value| value.abs())
                .fold(0.0_f64, f64::max);
            if norm <= tolerance_nmm || norm < residual_nmm * (1.0 - 1e-4 * scale) {
                accepted = Some(candidate);
                break;
            }
        }
        b = accepted.ok_or(LocalEquilibriumError::NotConverged {
            iterations,
            residual_nmm,
            tolerance_nmm,
            reference_moment_nmm: options.reference_moment_nmm,
        })?;
    }
    unreachable!()
}

#[cfg(test)]
mod tests;
