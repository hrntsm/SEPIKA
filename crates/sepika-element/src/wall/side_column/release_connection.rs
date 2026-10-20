use sepika_core::model::EndCondition;
use sepika_math::dense_lu::CheckedDenseLu;

use crate::behavior::LocalMat;

use super::local_equilibrium::LocalTrialResponse;

/// 元端部条件の回転ばねと、任意の壁法線まわりの直列自由解放を表す。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReleaseConnection {
    a: [[f64; 12]; 12],
    b: Vec<[f64; 12]>,
    c: Vec<[f64; 12]>,
    spring_diag: Vec<f64>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ReleaseConnectionError {
    #[error("壁法線の局所y/z成分が不正です")]
    InvalidNormal,
    #[error("端部回転ばね剛性は有限かつ0以上でなければなりません")]
    InvalidSpring,
    #[error("共通ねじりゲージには有限正の非連成ねじり剛性が必要です")]
    InvalidTorsionGauge,
    #[error("内部回転または荷重の寸法が一致しません")]
    Dimension,
    #[error("接続のtrialに非有限値があります")]
    NonFinite,
    #[error("初期接続の内部回転を縮約できません: {0}")]
    Singular(String),
    #[error("共通ねじり回転は慣性を持つため整合質量ではゲージ除去できません")]
    InertialTorsionGauge,
}

impl ReleaseConnection {
    /// 法線は局所y/z成分を正規化して用いる。内部回転は端ごとにx・法線・接線の順。
    /// `k_theta` は `[Nmm/rad]`。0はピンと同値、負値・非有限値は拒否する。
    /// `has_torsion=false` はx内部回転を作らず、ねじり追加解放指定は元Fixed時だけ作用する。
    pub fn new(
        normal_yz: [f64; 2],
        end_cond: [EndCondition; 2],
        torsion_release: [bool; 2],
        has_torsion: bool,
    ) -> Result<Self, ReleaseConnectionError> {
        Self::build(Some(normal_yz), end_cond, torsion_release, has_torsion)
    }

    /// 壁解放を追加せず元端条件だけを表す。ばね剛性は `[Nmm/rad]`。
    pub fn from_end_conditions(
        end_cond: [EndCondition; 2],
        torsion_release: [bool; 2],
        has_torsion: bool,
    ) -> Result<Self, ReleaseConnectionError> {
        Self::build(None, end_cond, torsion_release, has_torsion)
    }

    fn build(
        wall_normal_yz: Option<[f64; 2]>,
        end_cond: [EndCondition; 2],
        torsion_release: [bool; 2],
        has_torsion: bool,
    ) -> Result<Self, ReleaseConnectionError> {
        let normal_yz = wall_normal_yz.unwrap_or([1.0, 0.0]);
        let scale = normal_yz[0].abs().max(normal_yz[1].abs());
        if normal_yz.iter().any(|value| !value.is_finite()) || scale == 0.0 {
            return Err(ReleaseConnectionError::InvalidNormal);
        }
        let scaled = normal_yz.map(|value| value / scale);
        let norm = scaled[0].hypot(scaled[1]);
        let [ny, nz] = scaled.map(|value| value / norm);
        let directions = [[1.0, 0.0, 0.0], [0.0, ny, nz], [0.0, -nz, ny]];
        let mut result = Self {
            a: std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 })),
            b: Vec::new(),
            c: Vec::new(),
            spring_diag: Vec::new(),
        };
        for (end, condition) in end_cond.into_iter().enumerate() {
            let original_spring = match condition {
                EndCondition::Fixed => None,
                EndCondition::Pinned => Some(0.0),
                EndCondition::SemiRigid { k_theta } => {
                    if !k_theta.is_finite() || k_theta < 0.0 {
                        return Err(ReleaseConnectionError::InvalidSpring);
                    }
                    Some(k_theta)
                }
            };
            for (axis, direction) in directions.iter().enumerate() {
                if axis == 0 && !has_torsion {
                    continue;
                }
                let spring = if (axis == 1 && wall_normal_yz.is_some())
                    || (axis == 0
                        && torsion_release[end]
                        && matches!(condition, EndCondition::Fixed))
                {
                    Some(0.0)
                } else {
                    original_spring
                };
                if let Some(spring) = spring {
                    let mut v = [0.0; 12];
                    v[end * 6 + 3..end * 6 + 6].copy_from_slice(direction);
                    for (i, row) in result.a.iter_mut().enumerate() {
                        for (j, value) in row.iter_mut().enumerate() {
                            *value -= v[i] * v[j];
                        }
                    }
                    result.b.push(v);
                    result.c.push(v);
                    result.spring_diag.push(spring);
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn fix_isolated_torsion_gauge(
        &mut self,
        raw_tangent: &[[f64; 12]; 12],
    ) -> Result<(), ReleaseConnectionError> {
        let slots: Vec<_> = [3, 9]
            .into_iter()
            .map(|slot| {
                self.b.iter().enumerate().find_map(|(index, column)| {
                    (column[slot] == 1.0 && self.spring_diag[index] == 0.0).then_some(index)
                })
            })
            .collect();
        if let [Some(_), Some(index)] = slots.as_slice() {
            let t = raw_tangent[3][3];
            if raw_tangent.iter().flatten().any(|v| !v.is_finite())
                || t <= 0.0
                || raw_tangent[9][9] != t
                || raw_tangent[3][9] != -t
                || raw_tangent[9][3] != -t
                || (0..12).filter(|i| ![3, 9].contains(i)).any(|i| {
                    [
                        raw_tangent[3][i],
                        raw_tangent[9][i],
                        raw_tangent[i][3],
                        raw_tangent[i][9],
                    ]
                    .iter()
                    .any(|v| *v != 0.0)
                })
            {
                return Err(ReleaseConnectionError::InvalidTorsionGauge);
            }
            // 両端自由ねじりの共通回転だけが無応力のゲージであり、j端を0に固定して商空間で解く。
            self.b.remove(*index);
            self.c.remove(*index);
            self.spring_diag.remove(*index);
        }
        Ok(())
    }

    /// `e = A u + B b` のA。並進 `[mm]`・回転 `[rad]` は元の部材局所軸を用いる。
    pub fn a(&self) -> &[[f64; 12]; 12] {
        &self.a
    }

    /// Bの列。内部回転は `[rad]`。
    pub fn b_columns(&self) -> &[[f64; 12]] {
        &self.b
    }

    /// ばね変形 `b - C u` のCの行。
    pub fn c_rows(&self) -> &[[f64; 12]] {
        &self.c
    }

    /// 有限ばねの対角値 `[Nmm/rad]`。自由解放は0。
    pub fn spring_diag(&self) -> &[f64] {
        &self.spring_diag
    }

    pub(crate) fn initial_linear_mapping(
        &self,
        raw: &LocalMat,
    ) -> Result<(LocalMat, LocalMat), ReleaseConnectionError> {
        let n = self.b.len();
        let tangent = std::array::from_fn(|i| std::array::from_fn(|j| raw.get(i, j)));
        let response = self.trial_response(
            &[0.0; 12],
            &vec![0.0; n],
            &[0.0; 12],
            &tangent,
            &[0.0; 12],
            &vec![0.0; n],
        )?;
        let derivative = if n == 0 {
            Vec::new()
        } else {
            CheckedDenseLu::factorize(&response.kbb, n)
                .and_then(|lu| lu.solve(&response.kba, 12))
                .map_err(|error| ReleaseConnectionError::Singular(error.to_string()))?
        };
        let mut stiffness = LocalMat::zeros(12);
        let mut mapping = LocalMat::zeros(12);
        for i in 0..12 {
            for j in 0..12 {
                stiffness.set(
                    i,
                    j,
                    response.kaa[i][j]
                        - (0..n)
                            .map(|a| response.kab[i * n + a] * derivative[a * 12 + j])
                            .sum::<f64>(),
                );
                mapping.set(
                    i,
                    j,
                    self.a[i][j]
                        - (0..n)
                            .map(|a| self.b[a][i] * derivative[a * 12 + j])
                            .sum::<f64>(),
                );
            }
        }
        if stiffness
            .data
            .iter()
            .chain(&mapping.data)
            .any(|v| !v.is_finite())
        {
            return Err(ReleaseConnectionError::NonFinite);
        }
        Ok((stiffness, mapping))
    }

    pub(crate) fn initial_consistent_mass(
        &self,
        raw_stiffness: &LocalMat,
        raw_mass: &LocalMat,
    ) -> Result<LocalMat, ReleaseConnectionError> {
        // 静的な無応力ねじり回転にも慣性があり、その固定で質量を消去できない。
        if [3, 9]
            .into_iter()
            .any(|slot| self.a[slot][slot] == 0.0 && self.b.iter().all(|v| v[slot] == 0.0))
        {
            return Err(ReleaseConnectionError::InertialTorsionGauge);
        }
        if raw_mass.data.iter().any(|v| !v.is_finite()) {
            return Err(ReleaseConnectionError::NonFinite);
        }
        let (_, mapping) = self.initial_linear_mapping(raw_stiffness)?;
        let mut result = LocalMat::zeros(12);
        for i in 0..12 {
            for j in 0..12 {
                let value = (0..12)
                    .map(|a| {
                        (0..12)
                            .map(|b| mapping.get(a, i) * raw_mass.get(a, b) * mapping.get(b, j))
                            .sum::<f64>()
                    })
                    .sum();
                result.set(i, j, value);
            }
        }
        if result.data.iter().any(|v| !v.is_finite()) {
            return Err(ReleaseConnectionError::NonFinite);
        }
        Ok(result)
    }

    /// 元の断面局所軸で柔域変位を返す。剛域・全体座標の変換は呼出側で行う。
    pub fn flexible_displacement(
        &self,
        u: &[f64; 12],
        b: &[f64],
    ) -> Result<[f64; 12], ReleaseConnectionError> {
        self.check_state(u, b)?;
        let result = std::array::from_fn(|i| {
            dot(&self.a[i], u)
                + self
                    .b
                    .iter()
                    .zip(b)
                    .map(|(column, value)| column[i] * value)
                    .sum::<f64>()
        });
        if result.iter().any(|x| !x.is_finite()) {
            return Err(ReleaseConnectionError::NonFinite);
        }
        Ok(result)
    }

    /// 同じ柔域trialの内力・接線から荷重込み残差と全4接線ブロックを返す。
    /// `pa/pb` は変位に依存しない仕事共役外荷重。材料trial・履歴の確定は行わない。
    pub fn trial_response(
        &self,
        u: &[f64; 12],
        b: &[f64],
        raw_force: &[f64; 12],
        raw_tangent: &[[f64; 12]; 12],
        pa: &[f64; 12],
        pb: &[f64],
    ) -> Result<LocalTrialResponse, ReleaseConnectionError> {
        self.check_state(u, b)?;
        let n = self.b.len();
        if pb.len() != n {
            return Err(ReleaseConnectionError::Dimension);
        }
        if raw_force
            .iter()
            .chain(raw_tangent.iter().flatten())
            .chain(pa)
            .chain(pb)
            .any(|x| !x.is_finite())
        {
            return Err(ReleaseConnectionError::NonFinite);
        }
        let a_columns: [[f64; 12]; 12] =
            std::array::from_fn(|j| std::array::from_fn(|i| self.a[i][j]));
        let columns: Vec<_> = a_columns.iter().chain(&self.b).collect();
        let mut force: Vec<_> = columns.iter().map(|v| dot(v, raw_force)).collect();
        let mut tangent = vec![vec![0.0; 12 + n]; 12 + n];
        for (i, left) in columns.iter().enumerate() {
            for (j, right) in columns.iter().enumerate() {
                tangent[i][j] = raw_tangent
                    .iter()
                    .zip(left.iter())
                    .map(|(row, value)| value * dot(row, right))
                    .sum();
            }
        }
        for (k, ((c, stiffness), rotation)) in
            self.c.iter().zip(&self.spring_diag).zip(b).enumerate()
        {
            let moment = stiffness * (rotation - dot(c, u));
            let mut gradient = vec![0.0; 12 + n];
            for (value, c_value) in gradient[..12].iter_mut().zip(c) {
                *value = -c_value;
            }
            gradient[12 + k] = 1.0;
            for (i, value) in force.iter_mut().enumerate() {
                *value += gradient[i] * moment;
                for (j, entry) in tangent[i].iter_mut().enumerate() {
                    *entry += stiffness * gradient[i] * gradient[j];
                }
            }
        }
        for (value, load) in force.iter_mut().zip(pa.iter().chain(pb)) {
            *value -= load;
        }
        if force
            .iter()
            .chain(tangent.iter().flatten())
            .any(|x| !x.is_finite())
        {
            return Err(ReleaseConnectionError::NonFinite);
        }
        Ok(LocalTrialResponse {
            retained_force: std::array::from_fn(|i| force[i]),
            residual: force[12..].to_vec(),
            kaa: std::array::from_fn(|i| std::array::from_fn(|j| tangent[i][j])),
            kab: tangent[..12]
                .iter()
                .flat_map(|row| row[12..].iter().copied())
                .collect(),
            kba: tangent[12..]
                .iter()
                .flat_map(|row| row[..12].iter().copied())
                .collect(),
            kbb: tangent[12..]
                .iter()
                .flat_map(|row| row[12..].iter().copied())
                .collect(),
        })
    }

    fn check_state(&self, u: &[f64; 12], b: &[f64]) -> Result<(), ReleaseConnectionError> {
        if b.len() != self.b.len() {
            return Err(ReleaseConnectionError::Dimension);
        }
        if u.iter().chain(b).any(|x| !x.is_finite()) {
            return Err(ReleaseConnectionError::NonFinite);
        }
        Ok(())
    }
}

fn dot(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests;
