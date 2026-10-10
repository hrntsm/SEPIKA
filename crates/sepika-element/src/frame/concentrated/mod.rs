use crate::behavior::{Ctx, ElementBehavior, LocalMat, LocalVec, MassOption};
use sepika_core::dof::DofMap;

use sepika_material::uniaxial::UniaxialMaterial;
use smallvec::SmallVec;
use std::any::Any;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpringModel {
    OneComponent,
    TwoComponent,
}

/// 端バネの N-M 相関パラメータ。
#[derive(Clone, Copy, Debug)]
pub struct MnInteraction {
    /// N=0 での降伏モーメント [N·mm]。
    pub my0: f64,
    /// 軸許容耐力 [N]（正値。`new` で 1 N 以上へクランプする）。
    pub n_allow: f64,
}

impl MnInteraction {
    /// 軸許容耐力 `n_allow` [N] を正値（1 N 以上）へクランプして構築する。
    pub fn new(my0: f64, n_allow: f64) -> Self {
        MnInteraction {
            my0,
            n_allow: n_allow.max(1.0),
        }
    }

    /// 軸力 `axial_force` [N]（引張正）に対する曲げ許容モーメント [N·mm]。
    /// `my0·(1 − |N|/N許容)`、下限は `0.02·my0`。
    pub fn moment_limit(&self, axial_force: f64) -> f64 {
        (self.my0 * (1.0 - axial_force.abs() / self.n_allow)).max(0.02 * self.my0)
    }
}

/// 材端集中ばね梁。
pub struct ConcentratedSpringBeam {
    pub elastic: crate::frame::beam::BeamElement,
    pub spring_i: Box<dyn UniaxialMaterial>,
    pub spring_j: Box<dyn UniaxialMaterial>,
    initial_spring_stiffness_i: f64,
    initial_spring_stiffness_j: f64,
    pub model: SpringModel,
    total_rotation_reference: Option<f64>,
    /// N-M 相関。
    pub mn: Option<MnInteraction>,
    /// ばね変形の確定値。
    rot_i: f64,
    rot_j: f64,
    /// ばね変形のトライアル値。
    trial_rot_i: f64,
    trial_rot_j: f64,
    /// 可撓端回転の確定値。
    thb_i: f64,
    thb_j: f64,
    /// 可撓端回転のトライアル値。
    trial_thb_i: f64,
    trial_thb_j: f64,
    flex_stiffness_cache: std::sync::OnceLock<LocalMat>,
}

impl ConcentratedSpringBeam {
    pub fn new(
        elastic: crate::frame::beam::BeamElement,
        spring_i: Box<dyn UniaxialMaterial>,
        spring_j: Box<dyn UniaxialMaterial>,
        model: SpringModel,
    ) -> Self {
        Self {
            initial_spring_stiffness_i: spring_i.probe(0.0).1,
            initial_spring_stiffness_j: spring_j.probe(0.0).1,
            elastic,
            spring_i,
            spring_j,
            model,
            total_rotation_reference: None,
            mn: None,
            rot_i: 0.0,
            rot_j: 0.0,
            trial_rot_i: 0.0,
            trial_rot_j: 0.0,
            thb_i: 0.0,
            thb_j: 0.0,
            trial_thb_i: 0.0,
            trial_thb_j: 0.0,
            flex_stiffness_cache: std::sync::OnceLock::new(),
        }
    }

    pub fn new_one_component(
        elastic: crate::frame::beam::BeamElement,
        spring_i: Box<dyn UniaxialMaterial>,
        spring_j: Box<dyn UniaxialMaterial>,
    ) -> Self {
        Self::new(elastic, spring_i, spring_j, SpringModel::OneComponent)
    }

    /// 総角材料からM/Sを控除して接続する。初期剛性S[N·mm/rad]を持つ同一逆対称基準に限定する。
    pub fn with_total_rotation_reference(mut self, s_nmm: f64) -> Self {
        assert!(s_nmm.is_finite() && s_nmm > 0.0);
        self.total_rotation_reference = Some(s_nmm);
        self.flex_stiffness_cache = std::sync::OnceLock::new();
        self.initial_spring_stiffness_i = s_nmm;
        self.initial_spring_stiffness_j = s_nmm;
        self
    }

    fn total_material_probe(&self, end: usize, theta: f64) -> (f64, f64) {
        let material = if end == 0 {
            &self.spring_i
        } else {
            &self.spring_j
        };
        let (m, kt) = material.probe(theta);
        if theta == 0.0 && kt == 0.0 {
            (
                m,
                self.total_rotation_reference.expect("総角基準が必要です"),
            )
        } else {
            (m, kt)
        }
    }

    pub fn with_mn_interaction(mut self, my0: f64, n_allow: f64) -> Self {
        self.mn = Some(MnInteraction::new(my0, n_allow));
        self
    }

    /// 現在の軸力 [N]（引張正）。
    fn current_axial_force(&self, du_local: Option<&[f64; 12]>) -> f64 {
        let ul = self.elastic.axis.rotate_to_local(&self.elastic.trial_disp);
        let mut d = ul[6] - ul[0];
        if let Some(du) = du_local {
            d += du[6] - du[0];
        }
        self.elastic.e * self.elastic.a / self.elastic.length.max(1.0) * d
    }

    fn apply_mn_interaction(&mut self, du_local: Option<&[f64; 12]>) {
        let Some(mn) = self.mn else {
            return;
        };
        let n = self.current_axial_force(du_local);
        let m_lim = mn.moment_limit(n);
        self.spring_i.set_yield(m_lim);
        self.spring_j.set_yield(m_lim);
    }

    fn k_flex(&self) -> &LocalMat {
        self.flex_stiffness_cache.get_or_init(|| {
            if self.total_rotation_reference.is_some() {
                self.elastic.local_stiffness_flex_rc_reference()
            } else {
                self.elastic.local_stiffness_flex()
            }
        })
    }

    fn u_flex_local(&self) -> [f64; 12] {
        let u_local = self.elastic.axis.rotate_to_local(&self.elastic.trial_disp);
        let (li, lj) = self.elastic.rigid_lengths();
        crate::frame::rigid_arm::to_flex_disp(&u_local, li, lj)
    }

    fn solve_internal_equilibrium(&mut self) {
        if let Some(s) = self.total_rotation_reference {
            self.solve_total_rotation_equilibrium(s);
            return;
        }
        let k_flex = self.k_flex();
        let u_flex = self.u_flex_local();
        let er = SPRING_ROT_DOFS;
        let thn = [u_flex[er[0]], u_flex[er[1]]];
        let mut thb = [self.trial_thb_i, self.trial_thb_j];

        for _ in 0..50 {
            let mut uh = u_flex;
            uh[er[0]] = thb[0];
            uh[er[1]] = thb[1];
            let mut mb = [0.0_f64; 2];
            for (k, &e) in er.iter().enumerate() {
                let mut s = 0.0;
                for (j, &u) in uh.iter().enumerate() {
                    s += k_flex.get(e, j) * u;
                }
                mb[k] = s;
            }
            let g = [thn[0] - thb[0], thn[1] - thb[1]];
            let (ms_i, kt_i) = self.spring_i.probe(g[0]);
            let (ms_j, kt_j) = self.spring_j.probe(g[1]);
            let r = [mb[0] - ms_i, mb[1] - ms_j];
            let scale = mb[0]
                .abs()
                .max(mb[1].abs())
                .max(ms_i.abs())
                .max(ms_j.abs())
                .max(1.0);
            if r[0].abs().max(r[1].abs()) < 1e-9 * scale {
                break;
            }
            let j00 = k_flex.get(er[0], er[0]) + kt_i;
            let j01 = k_flex.get(er[0], er[1]);
            let j10 = k_flex.get(er[1], er[0]);
            let j11 = k_flex.get(er[1], er[1]) + kt_j;
            let det = j00 * j11 - j01 * j10;
            if det.abs() < 1e-30 {
                break;
            }
            thb[0] -= (j11 * r[0] - j01 * r[1]) / det;
            thb[1] -= (-j10 * r[0] + j00 * r[1]) / det;
        }

        self.trial_thb_i = thb[0];
        self.trial_thb_j = thb[1];
        self.trial_rot_i = thn[0] - thb[0];
        self.trial_rot_j = thn[1] - thb[1];
        self.spring_i.trial(self.trial_rot_i);
        self.spring_j.trial(self.trial_rot_j);
    }
    fn solve_total_rotation_equilibrium(&mut self, s: f64) {
        let k = self.k_flex();
        let u = self.u_flex_local();
        let er = SPRING_ROT_DOFS;
        let mut thb = [self.trial_thb_i, self.trial_thb_j];
        for _ in 0..50 {
            let mut ub = u;
            ub[er[0]] = thb[0];
            ub[er[1]] = thb[1];
            let mb: [f64; 2] =
                std::array::from_fn(|end| (0..12).map(|j| k.get(er[end], j) * ub[j]).sum());
            let total: [f64; 2] = std::array::from_fn(|end| u[er[end]] - thb[end] + mb[end] / s);
            let probes = [
                self.total_material_probe(0, total[0]),
                self.total_material_probe(1, total[1]),
            ];
            let r = [mb[0] - probes[0].0, mb[1] - probes[1].0];
            let scale = mb[0]
                .abs()
                .max(mb[1].abs())
                .max(probes[0].0.abs())
                .max(probes[1].0.abs())
                .max(1.0);
            if r[0].abs().max(r[1].abs()) < 1e-9 * scale {
                break;
            }
            let j: [[f64; 2]; 2] = std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    (1.0 - probes[i].1 / s) * k.get(er[i], er[j])
                        + if i == j { probes[i].1 } else { 0.0 }
                })
            });
            let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
            assert!(
                det.is_finite() && det != 0.0,
                "RC総角基準の局所釣合が特異です"
            );
            thb[0] -= (j[1][1] * r[0] - j[0][1] * r[1]) / det;
            thb[1] -= (-j[1][0] * r[0] + j[0][0] * r[1]) / det;
        }
        let mut ub = u;
        ub[er[0]] = thb[0];
        ub[er[1]] = thb[1];
        let mb: [f64; 2] =
            std::array::from_fn(|end| (0..12).map(|j| k.get(er[end], j) * ub[j]).sum());
        self.trial_thb_i = thb[0];
        self.trial_thb_j = thb[1];
        self.trial_rot_i = u[er[0]] - thb[0] + mb[0] / s;
        self.trial_rot_j = u[er[1]] - thb[1] + mb[1] / s;
        self.spring_i.trial(self.trial_rot_i);
        self.spring_j.trial(self.trial_rot_j);
    }
}

/// 材端曲げばねが作用する局所回転自由度（局所 DOF 5・11）。
const SPRING_ROT_DOFS: [usize; 2] = [5, 11];

fn condense_springs(k_elem: &LocalMat, k_i: f64, k_j: f64) -> LocalMat {
    let releases = [(SPRING_ROT_DOFS[0], k_i), (SPRING_ROT_DOFS[1], k_j)];
    crate::frame::prismatic::condense_end_releases(k_elem, &releases).unwrap_or_else(|| {
        let mut kaa = LocalMat {
            n: k_elem.n,
            data: k_elem.data.clone(),
        };
        for &(dof, stiffness) in &releases {
            kaa.set(dof, dof, kaa.get(dof, dof) + stiffness);
        }
        kaa
    })
}

fn condense_total_rotation(k: &LocalMat, kt: [f64; 2], s: f64) -> LocalMat {
    let er = SPRING_ROT_DOFS;
    let b = [1.0 - kt[0] / s, 1.0 - kt[1] / s];
    let a00 = kt[0] + b[0] * k.get(er[0], er[0]);
    let a01 = b[0] * k.get(er[0], er[1]);
    let a10 = b[1] * k.get(er[1], er[0]);
    let a11 = kt[1] + b[1] * k.get(er[1], er[1]);
    let det = a00 * a11 - a01 * a10;
    assert!(det.is_finite() && det != 0.0, "RC追加柔性の縮約が特異です");
    let mut result = k.clone();
    for col in 0..k.n {
        let v0 = b[0] * k.get(er[0], col);
        let v1 = b[1] * k.get(er[1], col);
        let d0 = (a11 * v0 - a01 * v1) / det;
        let d1 = (-a10 * v0 + a00 * v1) / det;
        for row in 0..k.n {
            result.set(
                row,
                col,
                k.get(row, col) - k.get(row, er[0]) * d0 - k.get(row, er[1]) * d1,
            );
        }
    }
    result
}

fn compute_kstar(
    elastic: &crate::frame::beam::BeamElement,
    k_flex: &LocalMat,
    kti: f64,
    ktj: f64,
) -> LocalMat {
    let k_end = condense_springs(k_flex, kti, ktj);
    let (li, lj) = elastic.rigid_lengths();
    elastic.apply_rigid_zone_transform(&k_end, li, lj)
}

impl ElementBehavior for ConcentratedSpringBeam {
    fn n_dof(&self) -> usize {
        12
    }

    fn global_dofs(&self, dof: &DofMap) -> SmallVec<[usize; 24]> {
        crate::behavior::node_global_dofs(&self.elastic.nodes, dof)
    }

    fn tangent_stiffness(&self, _ctx: &Ctx) -> LocalMat {
        if let Some(s) = self.total_rotation_reference {
            let kt = [
                self.total_material_probe(0, self.trial_rot_i).1,
                self.total_material_probe(1, self.trial_rot_j).1,
            ];
            let local = condense_total_rotation(self.k_flex(), kt, s);
            let (li, lj) = self.elastic.rigid_lengths();
            return self
                .elastic
                .axis
                .to_global(&self.elastic.apply_rigid_zone_transform(&local, li, lj));
        }
        let kti = self.spring_i.probe(self.trial_rot_i).1;
        let ktj = self.spring_j.probe(self.trial_rot_j).1;

        let k_local = match self.model {
            SpringModel::OneComponent => compute_kstar(&self.elastic, self.k_flex(), kti, ktj),
            SpringModel::TwoComponent => unimplemented!(
                "TwoComponent spring model is not yet implemented. Use OneComponent."
            ),
        };
        self.elastic.axis.to_global(&k_local)
    }

    fn internal_force(&self, _ctx: &Ctx) -> LocalVec {
        let k_flex = self.k_flex();
        let u_flex = self.u_flex_local();
        let er = SPRING_ROT_DOFS;
        let mut uh = u_flex;
        uh[er[0]] = self.trial_thb_i;
        uh[er[1]] = self.trial_thb_j;

        let mut f_flex = [0.0_f64; 12];
        for (i, f) in f_flex.iter_mut().enumerate() {
            let mut s = 0.0;
            for (j, &u) in uh.iter().enumerate() {
                s += k_flex.get(i, j) * u;
            }
            *f = s;
        }
        let ms_i = self.spring_i.probe(self.trial_rot_i).0;
        let ms_j = self.spring_j.probe(self.trial_rot_j).0;
        f_flex[er[0]] = ms_i;
        f_flex[er[1]] = ms_j;

        let (li, lj) = self.elastic.rigid_lengths();
        let f_node = crate::frame::rigid_arm::to_node_force(&f_flex, li, lj);
        let f_global = self.elastic.axis.rotate_to_global(&f_node);
        LocalVec {
            data: SmallVec::from_slice(&f_global),
        }
    }

    fn state_member_forces(&self, ctx: &Ctx) -> Option<crate::frame::beam::MemberForces> {
        let f_global = self.internal_force(ctx);
        let arr: [f64; 12] = std::array::from_fn(|i| f_global.data[i]);
        let f_local = self.elastic.axis.rotate_to_local(&arr);
        Some(crate::frame::beam::member_forces_from_end_forces(
            &f_local,
            self.elastic.length,
            &self.elastic.eval_sections,
        ))
    }

    fn end_spring_rotations(&self) -> Option<[f64; 2]> {
        if let Some(s) = self.total_rotation_reference {
            return Some([
                self.rot_i - self.spring_i.probe(self.rot_i).0 / s,
                self.rot_j - self.spring_j.probe(self.rot_j).0 / s,
            ]);
        }
        Some([self.rot_i, self.rot_j])
    }

    fn update_state(&mut self, du: &LocalVec, commit: bool, _ctx: &Ctx) {
        let du_global: [f64; 12] = std::array::from_fn(|i| du.data[i]);
        let du_local = self.elastic.axis.rotate_to_local(&du_global);
        self.apply_mn_interaction(Some(&du_local));
        self.elastic.update_state(du, commit, _ctx);
        self.solve_internal_equilibrium();
        if commit {
            self.spring_i.commit();
            self.spring_j.commit();
            self.rot_i = self.trial_rot_i;
            self.rot_j = self.trial_rot_j;
            self.thb_i = self.trial_thb_i;
            self.thb_j = self.trial_thb_j;
        }
    }

    fn mass_matrix(&self, opt: MassOption) -> LocalMat {
        match opt {
            MassOption::Lumped => self.elastic.mass_matrix(opt),
            MassOption::Consistent => {
                let mass_properties = self
                    .elastic
                    .mass_properties_error
                    .as_ref()
                    .map_or(self.elastic.mass_properties, |error| {
                        panic!("質量特性を解決できません: {error}")
                    });
                let (li, lj) = self.elastic.rigid_lengths();
                let flex_length = self.elastic.length - li - lj;
                let phi_y = if flex_length > 0.0 && self.elastic.g > 0.0 && self.elastic.as_z > 0.0
                {
                    12.0 * self.elastic.e * self.elastic.iy
                        / (self.elastic.g * self.elastic.as_z * flex_length.powi(2))
                } else {
                    0.0
                };
                let phi_z = if self.total_rotation_reference.is_none()
                    && flex_length > 0.0
                    && self.elastic.g > 0.0
                    && self.elastic.as_y > 0.0
                {
                    12.0 * self.elastic.e * self.elastic.iz
                        / (self.elastic.g * self.elastic.as_y * flex_length.powi(2))
                } else {
                    0.0
                };
                let flex = crate::frame::prismatic::consistent_mass_timoshenko(
                    mass_properties,
                    flex_length,
                    phi_z,
                    phi_y,
                );
                if self.total_rotation_reference.is_some() {
                    let mass = crate::frame::prismatic::mass_without_end_releases(
                        &flex,
                        mass_properties,
                        li,
                        lj,
                    );
                    return self.elastic.axis.to_global(&mass);
                }
                let releases = [
                    (SPRING_ROT_DOFS[0], self.initial_spring_stiffness_i),
                    (SPRING_ROT_DOFS[1], self.initial_spring_stiffness_j),
                ];
                let mm = crate::frame::prismatic::condense_end_releases_with_mass(
                    self.k_flex(),
                    &flex,
                    mass_properties,
                    li,
                    lj,
                    &releases,
                )
                .unwrap_or_else(|| {
                    panic!(
                        "ConcentratedSpringBeam の端部解放質量を縮約できません: Kbb が特異です（解放条件を確認してください）"
                    )
                });
                self.elastic.axis.to_global(&mm)
            }
        }
    }

    fn geometric_stiffness(&self, n: f64) -> LocalMat {
        self.elastic.geometric_stiffness(n)
    }

    fn snapshot_state(&self) -> Box<dyn Any> {
        let materials: Vec<Box<dyn UniaxialMaterial>> =
            vec![self.spring_i.clone_box(), self.spring_j.clone_box()];
        Box::new((
            materials,
            [self.rot_i, self.rot_j, self.trial_rot_i, self.trial_rot_j],
            [self.thb_i, self.thb_j, self.trial_thb_i, self.trial_thb_j],
            self.elastic.committed_disp,
            self.elastic.trial_disp,
        ))
    }

    fn restore_state(&mut self, state: &dyn Any) {
        type Snapshot = (
            Vec<Box<dyn UniaxialMaterial>>,
            [f64; 4],
            [f64; 4],
            [f64; 12],
            [f64; 12],
        );
        let snapshot =
            crate::behavior::downcast_snapshot::<Snapshot>("ConcentratedSpringBeam", state);
        if snapshot.0.len() == 2 {
            self.spring_i = snapshot.0[0].clone_box();
            self.spring_j = snapshot.0[1].clone_box();
        }
        [self.rot_i, self.rot_j, self.trial_rot_i, self.trial_rot_j] = snapshot.1;
        [self.thb_i, self.thb_j, self.trial_thb_i, self.trial_thb_j] = snapshot.2;
        self.elastic.committed_disp = snapshot.3;
        self.elastic.trial_disp = snapshot.4;
    }

    fn commit_state(&mut self) {
        self.elastic.commit_state();
        self.spring_i.commit();
        self.spring_j.commit();
        self.rot_i = self.trial_rot_i;
        self.rot_j = self.trial_rot_j;
        self.thb_i = self.trial_thb_i;
        self.thb_j = self.trial_thb_j;
    }

    fn revert_state(&mut self) {
        self.elastic.revert_state();
        self.spring_i.revert();
        self.spring_j.revert();
        self.trial_rot_i = self.rot_i;
        self.trial_rot_j = self.rot_j;
        self.trial_thb_i = self.thb_i;
        self.trial_thb_j = self.thb_j;
    }

    fn serialize_checkpoint(&self) -> Vec<u8> {
        let cp = ConcentratedSpringCheckpoint {
            rot_i: self.rot_i,
            rot_j: self.rot_j,
            trial_rot_i: self.trial_rot_i,
            trial_rot_j: self.trial_rot_j,
            thb_i: self.thb_i,
            thb_j: self.thb_j,
            trial_thb_i: self.trial_thb_i,
            trial_thb_j: self.trial_thb_j,
            spring_i: self.spring_i.serialize_state(),
            spring_j: self.spring_j.serialize_state(),
            elastic_committed_disp: self.elastic.committed_disp,
            elastic_trial_disp: self.elastic.trial_disp,
        };
        bincode::serialize(&cp).expect("serialize checkpoint")
    }

    fn deserialize_checkpoint(
        &mut self,
        data: &[u8],
    ) -> Result<(), crate::behavior::CheckpointError> {
        let cp: ConcentratedSpringCheckpoint = bincode::deserialize(data)
            .map_err(|e| crate::behavior::CheckpointError::Decode(e.to_string()))?;
        self.rot_i = cp.rot_i;
        self.rot_j = cp.rot_j;
        self.trial_rot_i = cp.trial_rot_i;
        self.trial_rot_j = cp.trial_rot_j;
        self.thb_i = cp.thb_i;
        self.thb_j = cp.thb_j;
        self.trial_thb_i = cp.trial_thb_i;
        self.trial_thb_j = cp.trial_thb_j;
        self.spring_i.deserialize_state(&cp.spring_i)?;
        self.spring_j.deserialize_state(&cp.spring_j)?;
        self.elastic.committed_disp = cp.elastic_committed_disp;
        self.elastic.trial_disp = cp.elastic_trial_disp;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ConcentratedSpringCheckpoint {
    rot_i: f64,
    rot_j: f64,
    trial_rot_i: f64,
    trial_rot_j: f64,
    thb_i: f64,
    thb_j: f64,
    trial_thb_i: f64,
    trial_thb_j: f64,
    spring_i: Vec<u8>,
    spring_j: Vec<u8>,
    elastic_committed_disp: [f64; 12],
    elastic_trial_disp: [f64; 12],
}

#[cfg(test)]
mod tests;
