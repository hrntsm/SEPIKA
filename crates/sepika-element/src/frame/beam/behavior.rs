//! [`ElementBehavior`] トレイト実装（自由度写像・接線/幾何剛性・内力・質量行列）。

use super::element::BeamElement;
use crate::behavior::{Ctx, ElementBehavior, LocalMat, MassOption};
use sepika_core::dof::DofMap;
use smallvec::SmallVec;

impl ElementBehavior for BeamElement {
    fn n_dof(&self) -> usize {
        12
    }

    fn global_dofs(&self, dof: &DofMap) -> SmallVec<[usize; 24]> {
        crate::behavior::node_global_dofs(&self.nodes, dof)
    }

    fn tangent_stiffness(&self, _ctx: &Ctx) -> LocalMat {
        self.axis.to_global(&self.local_stiffness())
    }

    fn geometric_stiffness(&self, n: f64) -> LocalMat {
        let (li, lj) = self.rigid_lengths();
        let kg_node =
            crate::frame::prismatic::geometric_stiffness(n, self.length - li - lj, li, lj);
        self.axis.to_global(&kg_node)
    }

    fn owns_frame_member_loads(&self) -> bool {
        true
    }

    fn frame_load_response(
        &self,
        displacement: &[f64],
        loads: &[sepika_core::model::MemberLoad],
    ) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        self.loaded_response(displacement, loads, None).map(Some)
    }

    fn state_frame_load_response(
        &self,
    ) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        self.current_loaded_response(None).map(Some)
    }

    fn internal_force(&self, _ctx: &Ctx) -> crate::behavior::LocalVec {
        self.current_loaded_response(None)
            .unwrap_or_else(|error| panic!("{error}"))
            .nodal_residual
    }

    fn update_state(
        &mut self,
        du: &crate::behavior::LocalVec,
        commit: bool,
        _ctx: &Ctx,
    ) -> Result<(), crate::behavior::ElementUpdateError> {
        self.update_loaded_state(du, &self.trial_member_loads.clone(), commit, None)
    }

    fn update_state_with_member_loads(
        &mut self,
        du: &crate::behavior::LocalVec,
        loads: &[sepika_core::model::MemberLoad],
        commit: bool,
        _ctx: &Ctx,
    ) -> Result<(), crate::behavior::ElementUpdateError> {
        self.update_loaded_state(du, loads, commit, None)
    }

    fn commit_state(&mut self) {
        self.committed_disp = self.trial_disp;
        self.committed_member_loads = self.trial_member_loads.clone();
    }

    fn revert_state(&mut self) {
        self.trial_disp = self.committed_disp;
        self.trial_member_loads = self.committed_member_loads.clone();
    }

    fn snapshot_state(&self) -> Box<dyn std::any::Any> {
        Box::new(self.elastic_load_state())
    }

    fn restore_state(&mut self, state: &dyn std::any::Any) {
        self.restore_elastic_load_state(
            crate::behavior::downcast_snapshot::<super::state::ElasticLoadState>(
                "BeamElement",
                state,
            )
            .clone(),
        );
    }

    fn serialize_checkpoint(&self) -> Vec<u8> {
        self.serialize_loaded_checkpoint(None)
    }

    fn deserialize_checkpoint(
        &mut self,
        data: &[u8],
    ) -> Result<(), crate::behavior::CheckpointError> {
        self.restore_loaded_checkpoint(data, None)
    }

    fn mass_matrix(&self, opt: MassOption) -> LocalMat {
        self.try_mass_matrix(opt)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn try_mass_matrix(&self, opt: MassOption) -> Result<LocalMat, String> {
        if let Some(error) = &self.mass_properties_error {
            return Err(format!("質量特性を解決できません: {error}"));
        }
        let mass_properties = self.mass_properties;
        match opt {
            MassOption::Lumped => Ok(crate::frame::prismatic::lumped_mass(
                mass_properties.total_mass(self.length),
            )),
            MassOption::Consistent => {
                let (li, lj) = self.rigid_lengths();
                let flex_length = self.length - li - lj;
                let phi_y = if flex_length > 0.0 && self.g > 0.0 && self.as_z > 0.0 {
                    12.0 * self.e * self.iy / (self.g * self.as_z * flex_length.powi(2))
                } else {
                    0.0
                };
                let phi_z = if flex_length > 0.0 && self.g > 0.0 && self.as_y > 0.0 {
                    12.0 * self.e * self.iz / (self.g * self.as_y * flex_length.powi(2))
                } else {
                    0.0
                };
                let flex = crate::frame::prismatic::consistent_mass_timoshenko(
                    mass_properties,
                    flex_length,
                    phi_z,
                    phi_y,
                );
                let k_flex = self.checked_stiffness_flex_raw()?;
                let releases = self.end_releases();
                let mm = crate::frame::prismatic::condense_end_releases_with_mass(
                    &k_flex,
                    &flex,
                    mass_properties,
                    li,
                    lj,
                    &releases,
                )
                .ok_or_else(|| format!(
                    "部材 ID {} の弾性梁 (E={}, G={}, J={}, L={}) の BeamElement の端部解放質量を縮約できません: Kbb が特異または縮約演算が非有限です（解放条件を確認してください）",
                    self.id.0, self.e, self.g, self.j, self.length
                ))?;
                Ok(self.axis.to_global(&mm))
            }
        }
    }

    fn recover_forces(&self, u_elem: &[f64]) -> Option<crate::frame::beam::MemberForces> {
        if u_elem.len() < 12 {
            return None;
        }
        let mut arr = [0.0; 12];
        arr.copy_from_slice(&u_elem[..12]);
        Some(self.recover_forces(&arr))
    }

    fn state_member_forces(&self, _ctx: &Ctx) -> Option<crate::frame::beam::MemberForces> {
        Some(
            self.current_loaded_response(None)
                .unwrap_or_else(|error| panic!("{error}"))
                .section
                .recover(&self.eval_sections)
                .unwrap_or_else(|error| panic!("{error}")),
        )
    }
}
