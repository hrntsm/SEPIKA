//! 側柱の要素本体（面内両端ピンの柱）。
//!
//! 静的縮約による剛性計算と `ElementBehavior` 実装、および解放曲げ面を表す
//! `ReleaseAxis`。

use super::release_connection::ReleaseConnection;
use crate::behavior::{Ctx, LocalMat, LocalVec, MassOption};
use crate::frame::beam::BeamElement;

/// 解放する局所曲げ面（回転自由度）。
///
/// - `LocalY`: 局所 y 軸回りの回転（ry, 要素ローカル自由度 4・10）を解放。
///   曲げ面は局所 x-z 面（たわみ方向 = 局所 z 軸）。
/// - `LocalZ`: 局所 z 軸回りの回転（rz, 要素ローカル自由度 5・11）を解放。
///   曲げ面は局所 x-y 面（たわみ方向 = 局所 y 軸）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReleaseAxis {
    LocalY,
    LocalZ,
    /// 柱の局所 y・z 成分で表した解放回転軸の単位ベクトル。
    LocalDirection([f64; 2]),
}

/// 壁面内曲げに対応する両端回転を静縮約した側柱。
pub struct InPlaneReleasedColumn {
    pub(super) inner: BeamElement,
    release_axis: ReleaseAxis,
}

impl InPlaneReleasedColumn {
    pub fn new(inner: BeamElement, release_axis: ReleaseAxis) -> Self {
        Self {
            inner,
            release_axis,
        }
    }

    fn try_release_connection(&self) -> Result<ReleaseConnection, String> {
        let normal = match self.release_axis {
            ReleaseAxis::LocalY => [1.0, 0.0],
            ReleaseAxis::LocalZ => [0.0, 1.0],
            ReleaseAxis::LocalDirection(n) => n,
        };
        let mut connection = ReleaseConnection::new(
            normal,
            self.inner.end_cond,
            self.inner.torsion_release,
            self.inner.j > 0.0 && self.inner.g > 0.0,
        )
        .map_err(|error| format!("壁柱の端部接続が不正です: {error}"))?;
        let raw = self.inner.local_stiffness_flex_raw();
        let tangent = std::array::from_fn(|i| std::array::from_fn(|j| raw.get(i, j)));
        connection
            .fix_isolated_torsion_gauge(&tangent)
            .map_err(|error| format!("壁柱の端部接続が不正です: {error}"))?;
        Ok(connection)
    }

    fn released_local_stiffness(&self) -> LocalMat {
        let (stiffness, _) = self
            .try_release_connection()
            .unwrap_or_else(|error| panic!("{error}"))
            .initial_linear_mapping(&self.inner.local_stiffness_flex_raw())
            .unwrap_or_else(|error| panic!("壁柱の端部解放剛性を縮約できません: {error}"));
        let (li, lj) = self.inner.rigid_lengths();
        crate::frame::rigid_arm::transform_stiffness(&stiffness, li, lj)
    }

    fn released_local_mass(&self) -> Result<LocalMat, String> {
        if let Some(error) = &self.inner.mass_properties_error {
            return Err(format!("質量特性を解決できません: {error}"));
        }
        let properties = self.inner.mass_properties;
        let (li, lj) = self.inner.rigid_lengths();
        let length = self.inner.length - li - lj;
        let phi_y = if self.inner.g > 0.0 && self.inner.as_z > 0.0 {
            12.0 * self.inner.e * self.inner.iy / (self.inner.g * self.inner.as_z * length.powi(2))
        } else {
            0.0
        };
        let phi_z = if self.inner.g > 0.0 && self.inner.as_y > 0.0 {
            12.0 * self.inner.e * self.inner.iz / (self.inner.g * self.inner.as_y * length.powi(2))
        } else {
            0.0
        };
        let raw_mass =
            crate::frame::prismatic::consistent_mass_timoshenko(properties, length, phi_z, phi_y);
        let mass = self
            .try_release_connection()?
            .initial_consistent_mass(&self.inner.local_stiffness_flex_raw(), &raw_mass)
            .map_err(|error| format!("壁柱の端部解放質量を縮約できません: {error}"))?;
        Ok(crate::frame::prismatic::mass_without_end_releases(
            &mass, properties, li, lj,
        ))
    }

    /// 縮約後の局所剛性を用いた断面力の復元。
    fn recover_forces_released(
        &self,
        u_elem_global: &[f64; 12],
    ) -> crate::frame::beam::MemberForces {
        let u_local = self.inner.axis.rotate_to_local(u_elem_global);
        let k_local = self.released_local_stiffness();
        let mut f_local = [0.0; 12];
        for (i, fi) in f_local.iter_mut().enumerate() {
            let mut s = 0.0;
            for (j, &uj) in u_local.iter().enumerate() {
                s += k_local.get(i, j) * uj;
            }
            *fi = s;
        }

        crate::frame::beam::member_forces_from_end_forces(
            &f_local,
            self.inner.length,
            &self.inner.eval_sections,
        )
    }
}

crate::behavior::forward_element_behavior!(InPlaneReleasedColumn, inner, {
    n_dof: forward,
    global_dofs: forward,
    tangent_stiffness: custom,
    internal_force: custom,
    update_state: custom,
    update_state_with_member_loads: custom,
    mass_matrix: custom,
    try_mass_matrix: custom,
    owns_frame_member_loads: custom,
    frame_load_response: custom,
    state_frame_load_response: custom,
    recover_forces: custom,
    state_member_forces: custom,
    geometric_stiffness: forward,
    snapshot_state: custom,
    restore_state: custom,
    commit_state: custom,
    revert_state: custom,
    serialize_checkpoint: custom,
    deserialize_checkpoint: custom,
    panel_moments_from: forward,
    ductility_probe: forward,
    fiber_section_states: forward,
    end_spring_rotations: forward,
    set_time_step: forward,
}, custom {
    fn owns_frame_member_loads(&self) -> bool { true }
    fn snapshot_state(&self) -> Box<dyn std::any::Any> { Box::new(self.inner.elastic_load_state()) }
    fn restore_state(&mut self, state: &dyn std::any::Any) {
        self.inner.restore_elastic_load_state(crate::behavior::downcast_snapshot::<crate::frame::beam::state::ElasticLoadState>("InPlaneReleasedColumn", state).clone());
    }
    fn commit_state(&mut self) {
        self.inner.committed_disp = self.inner.trial_disp;
        self.inner.committed_member_loads = self.inner.trial_member_loads.clone();
    }
    fn revert_state(&mut self) {
        self.inner.trial_disp = self.inner.committed_disp;
        self.inner.trial_member_loads = self.inner.committed_member_loads.clone();
    }

    fn frame_load_response(&self, u_elem: &[f64], loads: &[sepika_core::model::MemberLoad]) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        self.inner.loaded_response(u_elem, loads, Some(self.try_release_connection()?)).map(Some)
    }

    fn update_state(&mut self, du: &LocalVec, commit: bool, ctx: &Ctx) -> Result<(), crate::behavior::ElementUpdateError> {
        self.update_state_with_member_loads(du, &self.inner.trial_member_loads.clone(), commit, ctx)
    }

    fn update_state_with_member_loads(&mut self, du: &LocalVec, loads: &[sepika_core::model::MemberLoad], commit: bool, _ctx: &Ctx) -> Result<(), crate::behavior::ElementUpdateError> {
        let connection = self.try_release_connection().map_err(|error| crate::behavior::ElementUpdateError(format!("部材 {:?}: {error}", self.inner.id)))?;
        self.inner.update_loaded_state(du, loads, commit, Some(connection))
    }

    fn state_frame_load_response(&self) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        let connection = self.try_release_connection().map_err(|error| format!("部材 {:?}: {error}", self.inner.id))?;
        self.inner.current_loaded_response(Some(connection)).map(Some)
    }

    fn serialize_checkpoint(&self) -> Vec<u8> {
        self.inner.serialize_loaded_checkpoint(Some(self.try_release_connection().unwrap_or_else(|error| panic!("{error}"))))
    }

    fn deserialize_checkpoint(&mut self, data: &[u8]) -> Result<(), crate::behavior::CheckpointError> {
        let connection = self.try_release_connection().map_err(|error| crate::behavior::CheckpointError::Decode(format!("部材 {:?}: {error}", self.inner.id)))?;
        self.inner.restore_loaded_checkpoint(data, Some(connection))
    }

    fn tangent_stiffness(&self, _ctx: &Ctx) -> LocalMat {
        self.inner.axis.to_global(&self.released_local_stiffness())
    }

    fn mass_matrix(&self, opt: MassOption) -> LocalMat {
        self.try_mass_matrix(opt).unwrap_or_else(|error| panic!("{error}"))
    }

    fn try_mass_matrix(&self, opt: MassOption) -> Result<LocalMat, String> {
        match opt {
            MassOption::Lumped => self.inner.try_mass_matrix(opt),
            MassOption::Consistent => Ok(self.inner.axis.to_global(&self.released_local_mass()?)),
        }
    }

    fn internal_force(&self, _ctx: &Ctx) -> LocalVec {
        self.state_frame_load_response().unwrap_or_else(|error| panic!("{error}")).expect("壁側柱の荷重応答").nodal_residual
    }

    fn recover_forces(&self, u_elem: &[f64]) -> Option<crate::frame::beam::MemberForces> {
        if u_elem.len() < 12 {
            return None;
        }
        let mut arr = [0.0; 12];
        arr.copy_from_slice(&u_elem[..12]);
        Some(self.recover_forces_released(&arr))
    }

    fn state_member_forces(&self, _ctx: &Ctx) -> Option<crate::frame::beam::MemberForces> {
        Some(self.state_frame_load_response().unwrap_or_else(|error| panic!("{error}")).expect("壁側柱の荷重応答").section.recover(&self.inner.eval_sections).unwrap_or_else(|error| panic!("{error}")))
    }
});
