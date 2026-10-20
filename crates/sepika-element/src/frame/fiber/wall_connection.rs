use super::FiberBeam;
use crate::behavior::LocalMat;
use crate::wall::side_column::local_equilibrium::{
    solve_local_equilibrium, solve_local_equilibrium_damped, LocalEquilibriumError,
    LocalEquilibriumOptions, LocalTrialResponse,
};
use crate::wall::side_column::release_connection::ReleaseConnection;
use sepika_core::model::EndCondition;
use sepika_math::dense_lu::CheckedDenseLu;
use smallvec::SmallVec;

impl FiberBeam {
    /// 初期状態に壁法線まわりの両端自由解放を追加する。法線は全体座標で指定する。
    /// 準静的な回転接続であり、元端条件・材料断面軸・軸履歴を保持する。
    pub fn configure_wall_release(&mut self, normal_global: [f64; 3]) -> Result<(), String> {
        if self.connection_started {
            return Err("壁接続は最初の状態更新・checkpoint復元より前に設定してください".into());
        }
        let scale = normal_global.iter().map(|v| v.abs()).fold(0.0, f64::max);
        if scale == 0.0 || normal_global.iter().any(|v| !v.is_finite()) {
            return Err("壁法線が不正です".into());
        }
        let normal = normal_global.map(|v| v / scale);
        let local: [f64; 3] = std::array::from_fn(|i| {
            self.axis.rot[i]
                .iter()
                .zip(normal)
                .map(|(r, n)| r * n)
                .sum()
        });
        let conditions = std::array::from_fn(|end| {
            self.releases
                .iter()
                .find(|r| r.dof == end * 6 + 4)
                .map_or(EndCondition::Fixed, |r| {
                    if r.spring == 0.0 {
                        EndCondition::Pinned
                    } else {
                        EndCondition::SemiRigid { k_theta: r.spring }
                    }
                })
        });
        let torsion = std::array::from_fn(|end| {
            self.releases
                .iter()
                .any(|r| r.dof == end * 6 + 3 && r.spring == 0.0)
        });
        let mut connection = ReleaseConnection::new(
            [local[1], local[2]],
            conditions,
            torsion,
            self.torsion_j > 0.0 && self.g > 0.0,
        )
        .map_err(|error| error.to_string())?;
        let raw = std::array::from_fn(|i| {
            std::array::from_fn(|j| self.initial_elastic_stiffness.get(i, j))
        });
        connection
            .fix_isolated_torsion_gauge(&raw)
            .map_err(|error| error.to_string())?;
        self.trial_int = SmallVec::from_elem(0.0, connection.b_columns().len());
        self.committed_int = self.trial_int.clone();
        self.wall_connection = Some(connection);
        Ok(())
    }

    pub(super) fn internal_rotation_count(&self) -> usize {
        self.wall_connection
            .as_ref()
            .map_or(self.releases.len(), |c| c.b_columns().len())
    }

    fn wall_connection_loads(&self, connection: &ReleaseConnection) -> ([f64; 12], Vec<f64>) {
        let qf = self.flex_member_load();
        let d = crate::frame::rigid_arm::to_flex_force(
            &self.rigid_member_load(),
            self.rigid_i,
            self.rigid_j,
        );
        let pa = std::array::from_fn(|i| {
            d[i] + (0..12).map(|j| connection.a()[j][i] * qf[j]).sum::<f64>()
        });
        let pb = connection
            .b_columns()
            .iter()
            .map(|column| column.iter().zip(qf).map(|(b, q)| b * q).sum())
            .collect();
        (pa, pb)
    }

    pub(super) fn wall_connection_response(
        &self,
        connection: &ReleaseConnection,
        tangent: &LocalMat,
    ) -> Result<LocalTrialResponse, String> {
        let u = self.flex_disp();
        let (pa, pb) = self.wall_connection_loads(connection);
        let e = connection
            .flexible_displacement(&u, &self.trial_int)
            .map_err(|error| error.to_string())?;
        let force = self.elem_internal_force(&e);
        let tangent = std::array::from_fn(|i| std::array::from_fn(|j| tangent.get(i, j)));
        connection
            .trial_response(&u, &self.trial_int, &force, &tangent, &pa, &pb)
            .map_err(|error| error.to_string())
    }

    pub(super) fn wall_connection_tangent(
        &self,
        connection: &ReleaseConnection,
        tangent: &LocalMat,
    ) -> Result<LocalMat, String> {
        let response = self.wall_connection_response(connection, tangent)?;
        let n = response.residual.len();
        let lu = CheckedDenseLu::factorize(&response.kbb, n)
            .map_err(|error| format!("外側壁接続の接線: {error}"))?;
        let derivative = lu
            .solve(&response.kba, 12)
            .map_err(|error| format!("外側壁接続の接線: {error}"))?;
        let mut result = LocalMat::zeros(12);
        for i in 0..12 {
            for j in 0..12 {
                let value = response.kaa[i][j]
                    - (0..n)
                        .map(|a| response.kab[i * n + a] * derivative[a * 12 + j])
                        .sum::<f64>();
                if !value.is_finite() {
                    return Err("外側壁接続の接線が非有限です".into());
                }
                result.set(i, j, value);
            }
        }
        Ok(result)
    }

    pub(super) fn solve_wall_connection(&mut self) -> Result<(), String> {
        let connection = self.wall_connection.clone().unwrap();
        let u = self.flex_disp();
        let n = connection.b_columns().len();
        let (pa, pb) = self.wall_connection_loads(&connection);
        let restart_rotations = self.committed_int.to_vec();
        let mut b = restart_rotations.clone();
        let e = connection
            .flexible_displacement(&u, &b)
            .map_err(|error| error.to_string())?;
        let k = &self.initial_elastic_stiffness;
        let force = std::array::from_fn(|i| (0..12).map(|j| k.get(i, j) * e[j]).sum());
        let matrix = std::array::from_fn(|i| std::array::from_fn(|j| k.get(i, j)));
        let predictor = connection
            .trial_response(&u, &b, &force, &matrix, &pa, &pb)
            .map_err(|error| error.to_string())?;
        let lu = CheckedDenseLu::factorize(&predictor.kbb, n)
            .map_err(|error| format!("外側壁接続の初期予測: {error}"))?;
        let correction = lu
            .solve(&predictor.residual, 1)
            .map_err(|error| format!("外側壁接続の初期予測: {error}"))?;
        for (rotation, delta) in b.iter_mut().zip(correction) {
            *rotation -= delta;
        }
        let reference = (0..12)
            .filter(|i| i % 6 >= 3)
            .map(|i| (0..12).map(|j| (k.get(i, j) * e[j]).abs()).sum::<f64>())
            .chain(pb.iter().map(|v| v.abs()))
            .fold(1.0, f64::max);
        let options = LocalEquilibriumOptions {
            reference_moment_nmm: reference,
            absolute_tolerance_nmm: 1e-7,
            relative_tolerance: 1e-10,
            max_iterations: 40,
        };
        let mut evaluate = |b: &[f64]| {
            self.trial_int = SmallVec::from_slice(b);
            if let Some(h) = &mut self.hinge {
                h.trial_eta = h.committed_eta;
                h.trial_plastic = h.committed_plastic;
            }
            let e = connection
                .flexible_displacement(&u, b)
                .map_err(|error| LocalEquilibriumError::Evaluation(error.to_string()))?;
            self.update_trial_state(&e)
                .map_err(LocalEquilibriumError::Evaluation)?;
            let tangent = if let Some(h) = &self.hinge {
                self.hinge_tangent(h)
                    .map_err(LocalEquilibriumError::Evaluation)?
            } else {
                self.elem_tangent()
            };
            self.wall_connection_response(&connection, &tangent)
                .map_err(LocalEquilibriumError::Evaluation)
        };
        let solution = match solve_local_equilibrium(&b, options, &mut evaluate) {
            Err(LocalEquilibriumError::NotConverged { .. }) => {
                solve_local_equilibrium_damped(&restart_rotations, options, &mut evaluate).map_err(|error| format!("外側壁接続（通常Newton最大{}回後、確定回転から減衰Newton最大{}回を1回再試行）: {error}", options.max_iterations, options.max_iterations))?
            }
            result => result.map_err(|error| format!("外側壁接続: {error}"))?,
        };
        self.trial_int = SmallVec::from_slice(&solution.rotations_rad);
        Ok(())
    }
}
