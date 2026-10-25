//! 梁の実材軸端と接続節点の固定初期幾何による適合。
use crate::behavior::{Ctx, ElementBehavior, LocalMat, LocalVec, MassOption};
use sepika_core::model::{ElementData, Model};
use smallvec::SmallVec;

fn offsets(data: &ElementData, model: &Model) -> Option<[[f64; 3]; 2]> {
    model
        .primary_beam_axis_ends(data)
        .unwrap_or_else(|reason| panic!("{reason}"))
        .map(|ends| {
            std::array::from_fn(|side| {
                sepika_core::geom::vec3::sub(
                    ends[side],
                    model.node(data.nodes[side]).unwrap().coord,
                )
            })
        })
}

/// 実材軸端の12成分の荷重を接続節点と既存仕口パネル自由度へ写す。
pub fn transform_nodal_load(data: &ElementData, model: &Model, force: &[f64; 12]) -> LocalVec {
    let force = offsets(data, model).map_or(*force, |r| force_to_node(force, r));
    crate::frame::panel_offset::transform_nodal_load(data, model, &force)
}

pub(crate) fn connect(
    inner: Box<dyn ElementBehavior>,
    data: &ElementData,
    model: &Model,
) -> Box<dyn ElementBehavior> {
    match offsets(data, model) {
        Some(r) if r.iter().flatten().any(|v| *v != 0.0) => Box::new(EccentricMember { inner, r }),
        _ => inner,
    }
}

fn coupling(r: [[f64; 3]; 2]) -> [(usize, usize, f64); 12] {
    std::array::from_fn(|index| {
        let side = index / 6;
        let base = side * 6;
        let [x, y, z] = r[side];
        let (row, col, value) = [
            (0, 4, z),
            (0, 5, -y),
            (1, 3, -z),
            (1, 5, x),
            (2, 3, y),
            (2, 4, -x),
        ][index % 6];
        (base + row, base + col, value)
    })
}

fn force_to_node(force: &[f64; 12], r: [[f64; 3]; 2]) -> [f64; 12] {
    let mut result = *force;
    for (row, col, value) in coupling(r) {
        result[col] += value * force[row];
    }
    result
}

struct EccentricMember {
    inner: Box<dyn ElementBehavior>,
    r: [[f64; 3]; 2],
}

impl EccentricMember {
    fn to_inner(&self, u: &[f64]) -> LocalVec {
        let mut result = SmallVec::from_slice(u);
        for (row, col, value) in coupling(self.r) {
            result[row] += value * u[col];
        }
        LocalVec { data: result }
    }

    fn to_node(&self, force: &LocalVec) -> LocalVec {
        let force: &[f64; 12] = force
            .data
            .as_slice()
            .try_into()
            .expect("偏心接続の内側は12自由度");
        LocalVec {
            data: SmallVec::from_slice(&force_to_node(force, self.r)),
        }
    }

    fn transform_matrix(&self, matrix: &LocalMat) -> LocalMat {
        let mut result = LocalMat {
            n: matrix.n,
            data: matrix.data.clone(),
        };
        let terms = coupling(self.r);
        for &(row, col, value) in &terms {
            for i in 0..12 {
                result.set(i, col, result.get(i, col) + matrix.get(i, row) * value);
                result.set(col, i, result.get(col, i) + value * matrix.get(row, i));
            }
        }
        for &(r1, c1, z1) in &terms {
            for &(r2, c2, z2) in &terms {
                result.set(c1, c2, result.get(c1, c2) + z1 * matrix.get(r1, r2) * z2);
            }
        }
        result
    }
}

crate::behavior::forward_element_behavior!(EccentricMember, inner, {
    n_dof: forward,
    global_dofs: forward,
    tangent_stiffness: custom,
    internal_force: custom,
    update_state: custom,
    update_state_with_member_loads: custom,
    mass_matrix: custom,
    try_mass_matrix: custom,
    owns_frame_member_loads: forward,
    frame_load_response: custom,
    state_frame_load_response: custom,
    recover_forces: custom,
    state_member_forces: forward,
    geometric_stiffness: custom,
    snapshot_state: forward,
    restore_state: forward,
    commit_state: forward,
    revert_state: forward,
    serialize_checkpoint: forward,
    deserialize_checkpoint: forward,
    panel_moments_from: forward,
    ductility_probe: forward,
    fiber_section_states: forward,
    end_spring_rotations: forward,
    set_time_step: forward,
}, custom {
    fn tangent_stiffness(&self, ctx: &Ctx) -> LocalMat {
        self.transform_matrix(&self.inner.tangent_stiffness(ctx))
    }
    fn geometric_stiffness(&self, n: f64) -> LocalMat {
        self.transform_matrix(&self.inner.geometric_stiffness(n))
    }
    fn internal_force(&self, ctx: &Ctx) -> LocalVec {
        self.to_node(&self.inner.internal_force(ctx))
    }
    fn mass_matrix(&self, option: MassOption) -> LocalMat {
        self.try_mass_matrix(option).unwrap_or_else(|reason| panic!("{reason}"))
    }
    fn try_mass_matrix(&self, option: MassOption) -> Result<LocalMat, String> {
        Ok(self.transform_matrix(&self.inner.try_mass_matrix(option)?))
    }
    fn update_state(&mut self, du: &LocalVec, commit: bool, ctx: &Ctx) -> Result<(), crate::behavior::ElementUpdateError> {
        crate::behavior::validate_trial_increment(du, 12)?;
        self.inner.update_state(&self.to_inner(&du.data), commit, ctx)
    }
    fn update_state_with_member_loads(&mut self, du: &LocalVec, loads: &[sepika_core::model::MemberLoad], commit: bool, ctx: &Ctx) -> Result<(), crate::behavior::ElementUpdateError> {
        crate::behavior::validate_trial_increment(du, 12)?;
        self.inner.update_state_with_member_loads(&self.to_inner(&du.data), loads, commit, ctx)
    }
    fn recover_forces(&self, displacement: &[f64]) -> Option<crate::frame::beam::MemberForces> {
        self.inner.recover_forces(&self.to_inner(displacement).data)
    }
    fn frame_load_response(&self, displacement: &[f64], loads: &[sepika_core::model::MemberLoad]) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        if displacement.len() != 12 || displacement.iter().any(|v| !v.is_finite()) {
            return Err("偏心接続部材の荷重応答変位が不正です".into());
        }
        self.inner.frame_load_response(&self.to_inner(displacement).data, loads).map(|response| response.map(|mut response| {
            response.nodal_residual = self.to_node(&response.nodal_residual);
            response
        }))
    }
    fn state_frame_load_response(&self) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        self.inner.state_frame_load_response().map(|response| response.map(|mut response| {
            response.nodal_residual = self.to_node(&response.nodal_residual);
            response
        }))
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_arm_keeps_sign_and_virtual_work_and_parallel_force_has_no_moment() {
        for e in [-100.0, 0.0, 100.0] {
            let r = [[0.0, 0.0, e]; 2];
            let mut force = [0.0; 12];
            force[0] = 1000.0;
            let mapped = force_to_node(&force, r);
            assert_eq!(mapped[0], 1000.0);
            assert_eq!(mapped[4], 1000.0 * e);
            let mut displacement = [0.0; 12];
            displacement[4] = 0.001;
            let mut actual = displacement;
            for (row, col, value) in coupling(r) {
                actual[row] += value * displacement[col];
            }
            assert_eq!(actual[0], 0.001 * e);
            assert_eq!(actual[0] * force[0], displacement[4] * mapped[4]);
            force[0] = 0.0;
            force[2] = 1000.0;
            assert_eq!(force_to_node(&force, r), force);
        }
    }
}
