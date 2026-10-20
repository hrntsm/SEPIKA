use super::{FiberBeam, GaussPoint, HingeState, HINGE_SIGN, HINGE_SLOTS};
use crate::behavior::LocalMat;
use sepika_math::dense_lu::CheckedDenseLu;

impl HingeState {
    pub(super) fn new(lp: f64, k_el: LocalMat, points: &[GaussPoint]) -> Self {
        Self {
            lp,
            k_el,
            d0: std::array::from_fn(|end| points[end].cached_stiff),
            trial_eta: [0.0; 6],
            committed_eta: [0.0; 6],
            trial_plastic: [0.0; 6],
            committed_plastic: [0.0; 6],
        }
    }
}

struct SectionTrial {
    force: [f64; 6],
    plastic: [f64; 6],
    plastic_derivative: [f64; 36],
    jacobian: [f64; 36],
    residual: [f64; 6],
    normalized_residual: f64,
    scales: [f64; 6],
}

impl FiberBeam {
    pub(super) fn constitutive_failure(&self, error: &str) -> ! {
        panic!(
            "Fiber/MS 要素 {:?}（節点 {:?}）の構成則を解けません: {error}",
            self.element_id, self.nodes
        )
    }

    fn update_hinge_section_trial(&mut self, end: usize, eta: &[f64]) {
        let gp = &mut self.gauss_points[end];
        for (i, fiber) in gp.section.fibers.iter().enumerate() {
            let strain = eta[0] + eta[1] * fiber.z - eta[2] * fiber.y;
            gp.mats[i].revert();
            let (stress, tangent) = gp.mats[i].trial(strain);
            gp.trial_stress[i] = stress;
            gp.trial_et[i] = tangent;
        }
        gp.refresh_response();
    }

    pub(super) fn hinge_uhat(h: &HingeState, u: &[f64; 12]) -> [f64; 12] {
        let mut uhat = *u;
        for a in 0..6 {
            uhat[HINGE_SLOTS[a]] -= HINGE_SIGN[a] * h.trial_plastic[a];
        }
        uhat
    }

    pub(super) fn hinge_internal_force(h: &HingeState, u: &[f64; 12]) -> [f64; 12] {
        let uhat = Self::hinge_uhat(h, u);
        std::array::from_fn(|i| (0..12).map(|j| h.k_el.get(i, j) * uhat[j]).sum())
    }

    fn hinge_section_system(&self, eta: &[f64; 6], u: &[f64; 12]) -> Result<SectionTrial, String> {
        let h = self.hinge.as_ref().unwrap();
        let mut result = SectionTrial {
            force: [0.0; 6],
            plastic: [0.0; 6],
            plastic_derivative: [0.0; 36],
            jacobian: [0.0; 36],
            residual: [0.0; 6],
            normalized_residual: 0.0,
            scales: [1.0; 6],
        };
        for end in 0..2 {
            let gp = &self.gauss_points[end];
            let initial: Vec<f64> = h.d0[end].iter().flatten().copied().collect();
            let lu = CheckedDenseLu::factorize(&initial, 3)
                .map_err(|error| format!("内側端{end}の初期断面剛性: {error}"))?;
            let elastic = lu
                .solve(&gp.cached_force, 1)
                .map_err(|error| format!("内側端{end}の弾性変形控除: {error}"))?;
            let tangent: Vec<f64> = gp.cached_stiff.iter().flatten().copied().collect();
            let elastic_derivative = lu
                .solve(&tangent, 3)
                .map_err(|error| format!("内側端{end}の断面接線: {error}"))?;
            for a in 0..3 {
                let row = end * 3 + a;
                result.force[row] = gp.cached_force[a];
                result.plastic[row] = h.lp * (eta[row] - elastic[a]);
                for b in 0..3 {
                    let col = end * 3 + b;
                    result.plastic_derivative[row * 6 + col] =
                        h.lp * (f64::from(a == b) - elastic_derivative[a * 3 + b]);
                    result.jacobian[row * 6 + col] = gp.cached_stiff[a][b];
                }
            }
        }
        let mut uhat = *u;
        for a in 0..6 {
            uhat[HINGE_SLOTS[a]] -= HINGE_SIGN[a] * result.plastic[a];
        }
        let qf = self
            .member_load_state
            .trial_load_map
            .as_ref()
            .map(|map| *map.flex_load())
            .unwrap_or([0.0; 12]);
        for a in 0..6 {
            let demand: f64 = (0..12)
                .map(|j| HINGE_SIGN[a] * h.k_el.get(HINGE_SLOTS[a], j) * uhat[j])
                .sum::<f64>()
                - HINGE_SIGN[a] * qf[HINGE_SLOTS[a]];
            result.residual[a] = result.force[a] - demand;
            let gp = &self.gauss_points[a / 3];
            let section_scale: f64 = gp
                .section
                .fibers
                .iter()
                .zip(&gp.trial_stress)
                .map(|(fiber, stress)| {
                    let lever = match a % 3 {
                        0 => 1.0,
                        1 => fiber.z,
                        _ => fiber.y,
                    };
                    (stress * fiber.area * lever).abs()
                })
                .sum();
            let demand_scale: f64 = (0..12)
                .map(|j| (h.k_el.get(HINGE_SLOTS[a], j) * uhat[j]).abs())
                .sum();
            if !section_scale.is_finite() || !demand_scale.is_finite() {
                return Err("内側断面釣合いの残差尺度が非有限です".into());
            }
            let scale = 1.0
                + result.force[a]
                    .abs()
                    .max(demand.abs())
                    .max(section_scale)
                    .max(demand_scale)
                    .max(qf[HINGE_SLOTS[a]].abs());
            result.scales[a] = scale;
            result.normalized_residual = result
                .normalized_residual
                .max(result.residual[a].abs() / scale);
            for b in 0..6 {
                for c in 0..6 {
                    result.jacobian[a * 6 + b] += HINGE_SIGN[a]
                        * HINGE_SIGN[c]
                        * h.k_el.get(HINGE_SLOTS[a], HINGE_SLOTS[c])
                        * result.plastic_derivative[c * 6 + b];
                }
            }
        }
        if result
            .force
            .iter()
            .chain(result.plastic.iter())
            .chain(result.residual.iter())
            .chain(result.jacobian.iter())
            .chain(result.plastic_derivative.iter())
            .chain(result.scales.iter())
            .any(|x| !x.is_finite())
        {
            return Err("内側断面釣合いに非有限値があります".into());
        }
        if !result.normalized_residual.is_finite() {
            return Err("内側断面釣合いの正規化残差が非有限です".into());
        }
        Ok(result)
    }

    fn hinge_scales(&self) -> [f64; 6] {
        let h = self.hinge.as_ref().unwrap();
        std::array::from_fn(|a| h.d0[a / 3][a % 3][a % 3].abs().sqrt())
    }

    pub(super) fn solve_hinges(&mut self, u: &[f64; 12]) -> Result<(), String> {
        let mut eta = self.hinge.as_ref().unwrap().trial_eta;
        for iteration in 0..80 {
            for end in 0..2 {
                self.update_hinge_section_trial(end, &eta[end * 3..end * 3 + 3]);
            }
            let response = self.hinge_section_system(&eta, u)?;
            let scales = self.hinge_scales();
            let normalized_jacobian: Vec<f64> = (0..36)
                .map(|i| response.jacobian[i] / scales[i / 6] / scales[i % 6])
                .collect();
            let lu = CheckedDenseLu::factorize(&normalized_jacobian, 6).map_err(|error| {
                format!(
                    "内側断面釣合いのJacobian: {error}; iteration={iteration}, residual={:?}",
                    response.residual
                )
            })?;
            if response.normalized_residual <= 1.0e-10 {
                let h = self.hinge.as_mut().unwrap();
                h.trial_eta = eta;
                h.trial_plastic = response.plastic;
                return Ok(());
            }
            let correction = lu
                .solve(
                    &(0..6)
                        .map(|i| response.residual[i] / scales[i])
                        .collect::<Vec<_>>(),
                    1,
                )
                .map_err(|error| format!("内側断面釣合いの増分: {error}"))?;
            let merit: f64 = (0..6)
                .map(|a| (response.residual[a] / response.scales[a]).powi(2))
                .sum();
            let mut accepted = false;
            let mut step = 1.0;
            for _ in 0..30 {
                let candidate = std::array::from_fn(|a| eta[a] - step * correction[a] / scales[a]);
                for end in 0..2 {
                    self.update_hinge_section_trial(end, &candidate[end * 3..end * 3 + 3]);
                }
                if let Ok(trial) = self.hinge_section_system(&candidate, u) {
                    let trial_merit: f64 = (0..6)
                        .map(|a| (trial.residual[a] / response.scales[a]).powi(2))
                        .sum();
                    if trial_merit < merit {
                        eta = candidate;
                        accepted = true;
                        break;
                    }
                }
                step *= 0.5;
            }
            if !accepted {
                // 応力が不連続に変わるひび割れ境界では、残差単調減少だけを要求すると別の材料分岐へ進めない。
                eta = std::array::from_fn(|a| eta[a] - correction[a] / scales[a]);
            }
            if iteration == 79 {
                return Err(format!(
                    "内側断面釣合いが80反復で収束しません: residual={:?}, normalized={}",
                    response.residual, response.normalized_residual
                ));
            }
        }
        unreachable!()
    }

    pub(super) fn hinge_tangent(&self, h: &HingeState) -> Result<LocalMat, String> {
        let u = self.elem_disp(&self.flex_disp());
        let response = self.hinge_section_system(&h.trial_eta, &u)?;
        let scales = self.hinge_scales();
        let normalized_jacobian: Vec<f64> = (0..36)
            .map(|i| response.jacobian[i] / scales[i / 6] / scales[i % 6])
            .collect();
        let lu = CheckedDenseLu::factorize(&normalized_jacobian, 6)
            .map_err(|error| format!("内側断面接線のJacobian: {error}"))?;
        let mut rhs = [0.0; 72];
        for a in 0..6 {
            for j in 0..12 {
                rhs[a * 12 + j] = HINGE_SIGN[a] * h.k_el.get(HINGE_SLOTS[a], j) / scales[a];
            }
        }
        let derivative = lu
            .solve(&rhs, 12)
            .map_err(|error| format!("内側断面の整合接線: {error}"))?;
        let mut tangent = h.k_el.clone();
        for i in 0..12 {
            for j in 0..12 {
                let mut correction = 0.0;
                for a in 0..6 {
                    for b in 0..6 {
                        correction += h.k_el.get(i, HINGE_SLOTS[a])
                            * HINGE_SIGN[a]
                            * response.plastic_derivative[a * 6 + b]
                            * derivative[b * 12 + j]
                            / scales[b];
                    }
                }
                tangent.set(i, j, tangent.get(i, j) - correction);
            }
        }
        if tangent.data.iter().any(|value| !value.is_finite()) {
            return Err("整合接線が非有限です".into());
        }
        Ok(tangent)
    }

    pub(super) fn flex_member_load(&self) -> [f64; 12] {
        self.member_load_state
            .trial_load_map
            .as_ref()
            .map_or([0.0; 12], |map| *map.flex_load())
    }

    pub(super) fn rigid_member_load(&self) -> [f64; 12] {
        self.member_load_state
            .trial_load_map
            .as_ref()
            .map_or([0.0; 12], |map| *map.rigid_node_load())
    }

    pub(super) fn current_loaded_response(
        &self,
    ) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        let Some(map) = &self.member_load_state.trial_load_map else {
            return Ok(None);
        };
        self.current_response_from_map(map).map(Some)
    }

    pub(super) fn current_response_from_map(
        &self,
        map: &crate::frame::member_load::FrameDeadLoadMap,
    ) -> Result<crate::behavior::LoadedFrameResponse, String> {
        let e = self.elem_disp(&self.flex_disp());
        let raw = self.elem_internal_force(&e);
        let gf = std::array::from_fn(|i| raw[i] - map.flex_load()[i]);
        let node = crate::frame::rigid_arm::to_node_force(&gf, self.rigid_i, self.rigid_j);
        let physical = std::array::from_fn(|i| node[i] - map.rigid_node_load()[i]);
        let global = self.axis.rotate_to_global(&physical);
        if gf
            .iter()
            .chain(&physical)
            .chain(&global)
            .any(|v| !v.is_finite())
        {
            return Err("荷重込み物理材端力が非有限です".into());
        }
        Ok(crate::behavior::LoadedFrameResponse {
            nodal_residual: crate::behavior::LocalVec {
                data: smallvec::SmallVec::from_slice(&self.trial_global_force()?),
            },
            physical_frame_end_force_global: global,
            flex_face_force_local: gf,
            section: crate::frame::member_load::LoadedFrameSection::new(map.clone(), physical),
        })
    }

    pub(super) fn trial_global_force(&self) -> Result<[f64; 12], String> {
        let u_flex = self.flex_disp();
        let u_elem = self.elem_disp(&u_flex);
        let mut force = self.elem_internal_force(&u_elem);
        if u_flex
            .iter()
            .chain(&u_elem)
            .chain(&force)
            .any(|value| !value.is_finite())
        {
            return Err("可撓端変位または全12成分内力が非有限です".into());
        }
        if let Some(connection) = &self.wall_connection {
            let tangent = LocalMat::zeros(12);
            force = self
                .wall_connection_response(connection, &tangent)?
                .retained_force;
        } else {
            let qf = self.flex_member_load();
            for i in 0..12 {
                force[i] -= qf[i];
            }
            for release in &self.releases {
                force[release.dof] = release.spring * (u_flex[release.dof] - u_elem[release.dof]);
            }
            let d = crate::frame::rigid_arm::to_flex_force(
                &self.rigid_member_load(),
                self.rigid_i,
                self.rigid_j,
            );
            for i in 0..12 {
                force[i] -= d[i];
            }
        }
        let node_force = crate::frame::rigid_arm::to_node_force(&force, self.rigid_i, self.rigid_j);
        let global_force = self.axis.rotate_to_global(&node_force);
        if force
            .iter()
            .chain(&node_force)
            .chain(&global_force)
            .any(|value| !value.is_finite())
        {
            return Err("端部接続・剛域・全体変換後の全12成分内力が非有限です".into());
        }
        Ok(global_force)
    }
}

impl FiberBeam {
    pub(super) fn active_release_indices(&self) -> Vec<usize> {
        let pinned_torsion = [3, 9].iter().all(|slot| {
            self.releases
                .iter()
                .any(|r| r.dof == *slot && r.spring == 0.0)
        });
        self.releases
            .iter()
            .enumerate()
            .filter_map(|(index, release)| {
                // 両端自由ねじりの共通回転は無応力の座標ゲージであり、ばねで拘束しない。
                (!(pinned_torsion && release.dof == 9)).then_some(index)
            })
            .collect()
    }

    pub(super) fn condensed_release_tangent(&self, element: &LocalMat) -> Result<LocalMat, String> {
        if let Some(connection) = &self.wall_connection {
            return self.wall_connection_tangent(connection, element);
        }
        let indices = self.active_release_indices();
        let n = indices.len();
        let mut tangent = element.clone();
        for release in &self.releases {
            for i in 0..12 {
                tangent.set(release.dof, i, 0.0);
                tangent.set(i, release.dof, 0.0);
            }
            tangent.set(release.dof, release.dof, release.spring);
        }
        let mut kbb = vec![0.0; n * n];
        let mut kba = vec![0.0; n * 12];
        let mut kab = vec![0.0; 12 * n];
        for (a, &index) in indices.iter().enumerate() {
            let release = &self.releases[index];
            for (b, &other) in indices.iter().enumerate() {
                kbb[a * n + b] = element.get(release.dof, self.releases[other].dof);
            }
            kbb[a * n + a] += release.spring;
            for i in 0..12 {
                if self.releases.iter().all(|r| r.dof != i) {
                    kba[a * 12 + i] = element.get(release.dof, i);
                    kab[i * n + a] = element.get(i, release.dof);
                }
            }
            kba[a * 12 + release.dof] = -release.spring;
            kab[release.dof * n + a] = -release.spring;
        }
        let lu = CheckedDenseLu::factorize(&kbb, n)
            .map_err(|error| format!("外側端部接続の整合接線: {error}"))?;
        let solved = lu
            .solve(&kba, 12)
            .map_err(|error| format!("外側端部接続の整合接線: {error}"))?;
        for i in 0..12 {
            for j in 0..12 {
                let correction: f64 = (0..n).map(|a| kab[i * n + a] * solved[a * 12 + j]).sum();
                tangent.set(i, j, tangent.get(i, j) - correction);
            }
        }
        if tangent.data.iter().any(|value| !value.is_finite()) {
            return Err("整合接線が非有限です".into());
        }
        Ok(tangent)
    }
}
