use super::*;
use approx::assert_relative_eq;
use sepika_core::model::Model;
use sepika_math::cholesky::CholeskySolver;
use sepika_math::solver::LinearSolver;
use sepika_math::sparse::{assemble_csc, Triplet};

const E: f64 = 200000.0;
const FY: f64 = 200.0;
const HP: f64 = 200000.0 / 99.0;
const LP_AREA: f64 = 300.0 * 250.0;
const B: [[f64; 3]; 4] = [
    [1.0, -20.0, 10.0],
    [1.0, -20.0, -10.0],
    [1.0, 20.0, 10.0],
    [1.0, 20.0, -10.0],
];
const BTB: [f64; 3] = [4.0, 1600.0, 400.0];

const AXIAL_KE: f64 = 200000000.0 / 3000.0;
const AXIAL_Y: f64 = 200000.0;
const AXIAL_HA: f64 = 1000.0 * HP / 600.0;
const AXIAL_CASES: [(Option<[f64; 3]>, Option<f64>); 5] = [
    (None, None),
    (Some([0.0, 0.0, 1.0]), None),
    (Some([0.0, 0.6, 0.8]), None),
    (Some([0.0, 0.6, 0.8]), Some(1e7)),
    (Some([0.0, 0.6, 0.8]), Some(0.0)),
];

fn axial_member(
    normal: Option<[f64; 3]>,
    spring: Option<f64>,
    hardening: f64,
) -> crate::frame::multi_spring::MultiSpringElement {
    use sepika_core::model::EndCondition;
    let mut inner = super::constitutive_tests::bilinear_member();
    for gp in &mut inner.gauss_points {
        for material in &mut gp.mats {
            *material = Box::new(sepika_material::uniaxial::Bilinear::new(E, FY, hardening));
        }
    }
    let condition = spring.map_or(EndCondition::Fixed, |k_theta| {
        if k_theta == 0.0 {
            EndCondition::Pinned
        } else {
            EndCondition::SemiRigid { k_theta }
        }
    });
    inner.releases = resolve_end_releases(&[condition, EndCondition::Fixed], [false; 2], false);
    inner.trial_int = SmallVec::from_elem(0.0, inner.releases.len());
    inner.committed_int = inner.trial_int.clone();
    if let Some(n) = normal {
        inner.configure_wall_release(n).unwrap();
    }
    crate::frame::multi_spring::MultiSpringElement { inner }
}

fn axial_behavior(
    member: &mut crate::frame::multi_spring::MultiSpringElement,
    ms: bool,
) -> &mut dyn ElementBehavior {
    if ms {
        member
    } else {
        &mut member.inner
    }
}

fn axial_increment(delta_mm: f64) -> LocalVec {
    let mut data = smallvec::smallvec![0.0; 12];
    data[6] = delta_mm;
    LocalVec { data }
}

#[test]
fn independent_plastic_axial_event_partition_preserves_work_at_every_history_commit() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for (normal, spring) in AXIAL_CASES {
        let reference = PrimalReference::with_original(normal, spring);
        for ms in [false, true] {
            for sign in [-1.0, 1.0] {
                let mut member = axial_member(normal, spring, 0.01);
                let mut delta_mm: f64 = 0.0;
                let mut p_mm = 0.0;
                let mut force_n = 0.0;
                let mut epc = [0.0; 8];
                let mut observed_epc = [0.0; 8];
                let mut work_nmm = 0.0;
                let mut observed_work_nmm = 0.0;
                let mut observed_force_n = 0.0;
                let mut dissipation_nmm = 0.0;
                let mut observed_dissipation_nmm = 0.0;
                let mut primal_dissipation_nmm = 0.0;
                let mut max_energy_error_nmm = 0.0_f64;
                let mut commits = 0;
                let mut events = 0;
                for target in [1.5, 6.0, 4.5, 6.0, 0.0, -6.0, -4.5, -6.0, 6.0] {
                    let target_mm = sign * target;
                    let direction = (target_mm - delta_mm).signum();
                    let yield_mm = p_mm + (AXIAL_HA * p_mm + direction * AXIAL_Y) / AXIAL_KE;
                    let mut endpoints = Vec::new();
                    if direction * (yield_mm - delta_mm) > 1e-12
                        && direction * (target_mm - yield_mm) >= -1e-12
                    {
                        events += 1;
                    }
                    if direction * (yield_mm - delta_mm) > 1e-12
                        && direction * (target_mm - yield_mm) > 1e-12
                    {
                        endpoints.push(yield_mm);
                    }
                    endpoints.push(target_mm);
                    for next_mm in endpoints {
                        let next_p_mm = if direction * (next_mm - yield_mm) > 1e-12 {
                            (AXIAL_KE * next_mm - direction * AXIAL_Y) / (AXIAL_KE + AXIAL_HA)
                        } else {
                            p_mm
                        };
                        let next_force_n = AXIAL_KE * (next_mm - next_p_mm);
                        work_nmm += 0.5 * (force_n + next_force_n) * (next_mm - delta_mm);
                        dissipation_nmm += AXIAL_Y * (next_p_mm - p_mm).abs();
                        let expected = reference.solve(next_mm, &epc);
                        let behavior = axial_behavior(&mut member, ms);
                        behavior
                            .update_state(&axial_increment(next_mm - delta_mm), true, &ctx)
                            .unwrap();
                        let actual = behavior.internal_force(&ctx);
                        let states = behavior.fiber_section_states().unwrap();
                        observed_work_nmm +=
                            0.5 * (observed_force_n + actual.data[6]) * (next_mm - delta_mm);
                        for i in 0..12 {
                            let axial = if i == 0 {
                                -next_force_n
                            } else if i == 6 {
                                next_force_n
                            } else {
                                0.0
                            };
                            assert_relative_eq!(actual.data[i], axial, epsilon = 1e-5);
                            assert_relative_eq!(expected.force[i], axial, epsilon = 1e-5);
                        }
                        let mut observed_ep = [0.0; 8];
                        for (end, state) in states.iter().enumerate() {
                            for (i, fiber) in state.fibers.iter().enumerate() {
                                let index = end * 4 + i;
                                let stress = member.inner.gauss_points[end].trial_stress[i];
                                let ep = fiber.strain - stress / E;
                                observed_ep[index] = ep;
                                assert_relative_eq!(ep, next_p_mm / 600.0, epsilon = 1e-11);
                                assert_relative_eq!(expected.ep[index], ep, epsilon = 1e-11);
                                assert_relative_eq!(stress, next_force_n / 1000.0, epsilon = 1e-8);
                                assert!((stress - HP * ep).abs() <= FY + 1e-8);
                                observed_dissipation_nmm +=
                                    LP_AREA * FY * (ep - observed_epc[index]).abs();
                            }
                        }
                        let hinge = member.inner.hinge.as_ref().unwrap();
                        for i in 0..6 {
                            let p = if i % 3 == 0 { next_p_mm / 2.0 } else { 0.0 };
                            let eta = if i % 3 == 0 {
                                next_p_mm / 600.0 + next_force_n / 2e8
                            } else {
                                0.0
                            };
                            assert_relative_eq!(hinge.trial_eta[i], eta, epsilon = 1e-11);
                            assert_relative_eq!(hinge.trial_plastic[i], p, epsilon = 1e-9);
                        }
                        assert!(member.inner.trial_int.iter().all(|b| b.abs() < 1e-12));
                        let observed_p_mm = hinge.trial_plastic[0] + hinge.trial_plastic[3];
                        let stored_nmm = next_force_n.powi(2) / (2.0 * AXIAL_KE)
                            + AXIAL_HA * next_p_mm.powi(2) / 2.0;
                        let observed_stored_nmm = actual.data[6].powi(2) / (2.0 * AXIAL_KE)
                            + LP_AREA * HP * dot(&observed_ep, &observed_ep) / 2.0;
                        primal_dissipation_nmm += expected.dissipation_nmm;
                        assert_relative_eq!(observed_p_mm, next_p_mm, epsilon = 1e-9);
                        assert_relative_eq!(expected.stored_nmm, stored_nmm, epsilon = 1e-4);
                        assert_relative_eq!(
                            expected.hardening_nmm,
                            AXIAL_HA * next_p_mm.powi(2) / 2.0,
                            epsilon = 1e-4
                        );
                        assert_relative_eq!(
                            primal_dissipation_nmm,
                            dissipation_nmm,
                            epsilon = 1e-4
                        );
                        assert_relative_eq!(
                            observed_dissipation_nmm,
                            dissipation_nmm,
                            epsilon = 1e-4
                        );
                        assert_relative_eq!(work_nmm, stored_nmm + dissipation_nmm, epsilon = 1e-4);
                        let energy_error =
                            observed_work_nmm - observed_stored_nmm - observed_dissipation_nmm;
                        max_energy_error_nmm = max_energy_error_nmm.max(energy_error.abs());
                        assert_relative_eq!(observed_work_nmm, work_nmm, epsilon = 1e-4);
                        assert!(energy_error.abs() < 1e-4, "W-U-D={energy_error} Nmm");
                        assert!(dissipation_nmm >= 0.0);
                        epc = expected.ep;
                        observed_epc = observed_ep;
                        delta_mm = next_mm;
                        p_mm = next_p_mm;
                        force_n = next_force_n;
                        observed_force_n = actual.data[6];
                        commits += 1;
                    }
                }
                assert!(events >= 3);
                println!("I3 work normal={normal:?} spring={spring:?} ms={ms} sign={sign} commits={commits} events={events} W={work_nmm:.12e} D={dissipation_nmm:.12e} max_W-U-D={max_energy_error_nmm:.12e} Nmm");
            }
        }
    }
}

#[test]
fn independent_plastic_axial_initial_and_reverse_yield_have_distinct_one_sided_tangents() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let kt = AXIAL_KE * AXIAL_HA / (AXIAL_KE + AXIAL_HA);
    for (normal, spring) in AXIAL_CASES {
        let reference = PrimalReference::with_original(normal, spring);
        for ms in [false, true] {
            for sign in [-1.0, 1.0] {
                for reverse in [false, true] {
                    let mut member = axial_member(normal, spring, 0.01);
                    let preload_mm = if reverse { sign * 6.0 } else { sign * 1.5 };
                    let preload = reference.solve(preload_mm, &[0.0; 8]);
                    let direction = if reverse { -sign } else { sign };
                    let pc_mm = 300.0 * (preload.ep[0] + preload.ep[4]);
                    let yield_mm = pc_mm + (AXIAL_HA * pc_mm + direction * AXIAL_Y) / AXIAL_KE;
                    let yield_n = AXIAL_HA * pc_mm + direction * AXIAL_Y;
                    let behavior = axial_behavior(&mut member, ms);
                    behavior
                        .update_state(&axial_increment(preload_mm), true, &ctx)
                        .unwrap();
                    let committed = behavior.snapshot_state();
                    let mut max_tangent_error = 0.0_f64;
                    for side in [-1.0, 1.0] {
                        let step_mm = 1e-5;
                        let target_mm = yield_mm + direction * side * step_mm;
                        let expected_k = if side < 0.0 { AXIAL_KE } else { kt };
                        let expected_n = yield_n + expected_k * direction * side * step_mm;
                        let expected = reference.solve(target_mm, &preload.ep);
                        let behavior = axial_behavior(&mut member, ms);
                        behavior.restore_state(committed.as_ref());
                        behavior
                            .update_state(&axial_increment(target_mm - preload_mm), false, &ctx)
                            .unwrap();
                        let actual_n = behavior.internal_force(&ctx).data[6];
                        let actual_k = behavior.tangent_stiffness(&ctx).get(6, 6);
                        let one_sided_k = (actual_n - yield_n) / (direction * side * step_mm);
                        let primal_k = reference.branch_tangent(&expected.branches)[6][6];
                        assert_relative_eq!(expected.force[6], expected_n, epsilon = 1e-5);
                        assert_relative_eq!(actual_n, expected_n, epsilon = 1e-5);
                        assert_relative_eq!(actual_k, expected_k, epsilon = 1e-5);
                        assert_relative_eq!(primal_k, expected_k, epsilon = 1e-5);
                        assert_relative_eq!(one_sided_k, expected_k, epsilon = 0.02);
                        for branch in expected.branches {
                            assert_eq!(branch, if side < 0.0 { 0 } else { direction as i8 });
                        }
                        max_tangent_error = max_tangent_error.max((one_sided_k - expected_k).abs());
                    }
                    println!("I3 yield normal={normal:?} spring={spring:?} ms={ms} sign={sign} reverse={reverse} yield_delta={yield_mm:.12e} mm max_one_sided_error={max_tangent_error:.12e} N/mm");
                }
            }
        }
    }
}

#[test]
fn independent_plastic_perfectly_plastic_nonunique_end_partition_fails_atomically_and_retries() {
    let r = self_stress_projector();
    let k = euler_stiffness();
    let direction = [1.0; 4];
    let projected: [f64; 3] =
        std::array::from_fn(|a| (0..4).map(|i| B[i][a] * direction[i] / BTB[a]).sum());
    let mut null_elastic_displacement = [0.0; 12];
    null_elastic_displacement[0] = 300.0 * projected[0];
    null_elastic_displacement[6] = 300.0 * projected[0];
    for row in r {
        assert_eq!(dot(&row, &direction), 0.0);
    }
    for row in k {
        assert_eq!(dot(&row, &null_elastic_displacement), 0.0);
    }
    assert_eq!(projected, [1.0, 0.0, 0.0]);
    for (ep_i, ep_j) in [(0.005, 0.005), (0.004, 0.006)] {
        let p_mm = 300.0 * (ep_i + ep_j);
        let n = AXIAL_KE * (6.0 - p_mm);
        assert_relative_eq!(n, AXIAL_Y, epsilon = 1e-9);
        for ep in [ep_i, ep_j] {
            let eta = [ep + FY / E, 0.0, 0.0];
            let stress = B.map(|b| E * (dot(&b, &eta) - ep));
            for value in stress {
                assert_relative_eq!(value, FY, epsilon = 1e-10);
                assert!(ep > 0.0);
            }
            for a in 0..3 {
                let q: f64 = (0..4).map(|i| 250.0 * B[i][a] * stress[i]).sum();
                assert_relative_eq!(q, if a == 0 { AXIAL_Y } else { 0.0 }, epsilon = 1e-8);
            }
        }
    }
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for (normal, spring) in AXIAL_CASES {
        for ms in [false, true] {
            for sign in [-1.0, 1.0] {
                let mut member = axial_member(normal, spring, 0.0);
                let mut control = axial_member(normal, spring, 0.0);
                let behavior = axial_behavior(&mut member, ms);
                behavior
                    .update_state(&axial_increment(sign * 1.5), true, &ctx)
                    .unwrap();
                let committed_cp = behavior.serialize_checkpoint();
                behavior
                    .update_state(&axial_increment(sign * 0.25), false, &ctx)
                    .unwrap();
                let trial_cp = behavior.serialize_checkpoint();
                let trial_force = behavior.internal_force(&ctx).data;
                let trial_tangent = behavior.tangent_stiffness(&ctx).data;
                let connection_started = member.inner.connection_started;
                let behavior = axial_behavior(&mut member, ms);
                let error = behavior
                    .update_state(&axial_increment(sign * 4.25), true, &ctx)
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("内側断面釣合いのJacobian"), "{error}");
                assert_eq!(behavior.serialize_checkpoint(), trial_cp);
                assert_eq!(behavior.internal_force(&ctx).data, trial_force);
                assert_eq!(behavior.tangent_stiffness(&ctx).data, trial_tangent);
                assert_eq!(member.inner.connection_started, connection_started);
                let behavior = axial_behavior(&mut member, ms);
                behavior.revert_state();
                assert_eq!(behavior.serialize_checkpoint(), committed_cp);
                behavior
                    .update_state(&axial_increment(sign * 0.5), true, &ctx)
                    .unwrap();
                let retried_cp = behavior.serialize_checkpoint();
                let retried_force = behavior.internal_force(&ctx).data;
                let retried_tangent = behavior.tangent_stiffness(&ctx).data;
                assert_relative_eq!(retried_force[6], sign * 133333.3333333333, epsilon = 1e-5);
                let behavior = axial_behavior(&mut control, ms);
                behavior
                    .update_state(&axial_increment(sign * 1.5), true, &ctx)
                    .unwrap();
                behavior
                    .update_state(&axial_increment(sign * 0.5), true, &ctx)
                    .unwrap();
                assert_eq!(behavior.serialize_checkpoint(), retried_cp);
                assert_eq!(behavior.internal_force(&ctx).data, retried_force);
                assert_eq!(behavior.tangent_stiffness(&ctx).data, retried_tangent);
                assert_eq!(
                    member.inner.connection_started,
                    control.inner.connection_started
                );
                println!("I3 nonunique rollback normal={normal:?} spring={spring:?} ms={ms} sign={sign} CP/cache/config exact retry_N={} N error={error}", retried_force[6]);
            }
        }
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn self_stress_projector() -> [[f64; 4]; 4] {
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            f64::from(i == j) - (0..3).map(|a| B[i][a] * B[j][a] / BTB[a]).sum::<f64>()
        })
    })
}

fn euler_stiffness() -> [[f64; 12]; 12] {
    let mut k = [[0.0; 12]; 12];
    for (i, j, s) in [(0, 0, 1.0), (6, 6, 1.0), (0, 6, -1.0), (6, 0, -1.0)] {
        k[i][j] = s * 200000000.0 / 3000.0;
    }
    let l: f64 = 3000.0;
    let block = [
        [12.0, 6.0 * l, -12.0, 6.0 * l],
        [6.0 * l, 4.0 * l * l, -6.0 * l, 2.0 * l * l],
        [-12.0, -6.0 * l, 12.0, -6.0 * l],
        [6.0 * l, 2.0 * l * l, -6.0 * l, 4.0 * l * l],
    ];
    for (slots, signs, ei) in [
        ([1, 5, 7, 11], [1.0; 4], 2e10),
        ([2, 4, 8, 10], [1.0, -1.0, 1.0, -1.0], 8e10),
    ] {
        for i in 0..4 {
            for j in 0..4 {
                k[slots[i]][slots[j]] = ei / l.powi(3) * signs[i] * signs[j] * block[i][j];
            }
        }
    }
    k
}

fn spd_solve(h: &[Vec<f64>], rhs: &[f64]) -> Vec<f64> {
    if rhs.is_empty() {
        return Vec::new();
    }
    let scales: Vec<_> = (0..rhs.len()).map(|i| h[i][i].sqrt()).collect();
    let entries = h
        .iter()
        .enumerate()
        .flat_map(|(i, row)| {
            let scales = &scales;
            row.iter().enumerate().map(move |(j, v)| Triplet {
                row: i,
                col: j,
                val: v / (scales[i] * scales[j]),
            })
        })
        .collect();
    let mut solver = CholeskySolver::default();
    solver.factorize(&assemble_csc(rhs.len(), entries)).unwrap();
    let normalized: Vec<_> = rhs.iter().zip(&scales).map(|(r, s)| r / s).collect();
    let solution: Vec<_> = solver
        .solve(&normalized)
        .unwrap()
        .iter()
        .zip(scales)
        .map(|(x, s)| x / s)
        .collect();
    for (row, r) in h.iter().zip(rhs) {
        let residual = dot(row, &solution) - r;
        let scale = dot(
            &row.iter().map(|x| x.abs()).collect::<Vec<_>>(),
            &solution.iter().map(|x| x.abs()).collect::<Vec<_>>(),
        ) + r.abs();
        assert!(
            residual.abs() <= 1e-7 + 1e-12 * scale,
            "linear residual={residual}"
        );
    }
    solution
}

struct PrimalReference {
    k: [[f64; 12]; 12],
    columns: Vec<[f64; 12]>,
    h: Vec<Vec<f64>>,
    normal: Option<[f64; 3]>,
    springs: Vec<f64>,
}

struct ReferenceTrial {
    ep: [f64; 8],
    beta: Vec<f64>,
    force: [f64; 12],
    eta: [f64; 6],
    plastic: [f64; 6],
    strain: [f64; 8],
    stress: [f64; 8],
    stored_nmm: f64,
    hardening_nmm: f64,
    dissipation_nmm: f64,
    branches: [i8; 8],
}

impl PrimalReference {
    fn with_original(normal: Option<[f64; 3]>, original_i_spring: Option<f64>) -> Self {
        let k = euler_stiffness();
        let mut columns = vec![[0.0; 12]; 8];
        let mut springs = vec![0.0; 8];
        for (i, column) in columns.iter_mut().take(8).enumerate() {
            for (a, slot) in [0, 4, 5].into_iter().enumerate() {
                column[i / 4 * 6 + slot] =
                    if i < 4 { 300.0 } else { -300.0 } * B[i % 4][a] / BTB[a];
            }
        }
        if let Some(n) = normal {
            for end in 0..2 {
                let mut column = [0.0; 12];
                column[end * 6 + 4] = n[1];
                column[end * 6 + 5] = n[2];
                columns.push(column);
                springs.push(0.0);
                if end == 0 {
                    if let Some(spring) = original_i_spring {
                        let mut column = [0.0; 12];
                        column[4] = -n[2];
                        column[5] = n[1];
                        columns.push(column);
                        springs.push(spring);
                    }
                }
            }
        } else if let Some(spring) = original_i_spring {
            for slot in [4, 5] {
                let mut column = [0.0; 12];
                column[slot] = 1.0;
                columns.push(column);
                springs.push(spring);
            }
        }
        let r = self_stress_projector();
        let h: Vec<Vec<_>> = columns
            .iter()
            .enumerate()
            .map(|(i, ci)| {
                columns
                    .iter()
                    .enumerate()
                    .map(|(j, cj)| {
                        let elastic: f64 = (0..12).map(|a| ci[a] * dot(&k[a], cj)).sum();
                        elastic
                            + if i == j { springs[i] } else { 0.0 }
                            + if i < 8 && j < 8 && i / 4 == j / 4 {
                                LP_AREA * (E * r[i % 4][j % 4] + if i == j { HP } else { 0.0 })
                            } else {
                                0.0
                            }
                    })
                    .collect()
            })
            .collect();
        spd_solve(&h, &vec![0.0; h.len()]);
        Self {
            k,
            columns,
            h,
            normal,
            springs,
        }
    }

    fn solve(&self, delta_mm: f64, epc: &[f64; 8]) -> ReferenceTrial {
        let mut u = [0.0; 12];
        u[6] = delta_mm;
        self.solve_displacement(&u, epc)
    }

    fn solve_displacement(&self, u: &[f64; 12], epc: &[f64; 8]) -> ReferenceTrial {
        let n = self.columns.len();
        let g: Vec<_> = self
            .columns
            .iter()
            .map(|c| (0..12).map(|a| c[a] * dot(&self.k[a], u)).sum::<f64>())
            .collect();
        let mut branches = [0_i8; 8];
        let mut visited = Vec::new();
        for _ in 0..64 {
            assert!(!visited.contains(&branches), "reference active-set cycle");
            visited.push(branches);
            let free: Vec<_> = (0..n).filter(|&i| i >= 8 || branches[i] != 0).collect();
            let mut x = vec![0.0; n];
            x[..8].copy_from_slice(epc);
            let rhs: Vec<_> = free
                .iter()
                .map(|&i| {
                    -g[i]
                        - if i < 8 {
                            LP_AREA * FY * f64::from(branches[i])
                        } else {
                            0.0
                        }
                        - (0..8)
                            .filter(|&j| branches[j] == 0)
                            .map(|j| self.h[i][j] * epc[j])
                            .sum::<f64>()
                })
                .collect();
            let matrix: Vec<Vec<_>> = free
                .iter()
                .map(|&i| free.iter().map(|&j| self.h[i][j]).collect())
                .collect();
            for (&i, value) in free.iter().zip(spd_solve(&matrix, &rhs)) {
                x[i] = value;
            }
            let gradient: Vec<_> = self
                .h
                .iter()
                .zip(&g)
                .map(|(row, gi)| dot(row, &x) + gi)
                .collect();
            let mut next = branches;
            for i in 0..8 {
                if branches[i] == 0 && gradient[i].abs() > LP_AREA * FY + 1e-5 {
                    next[i] = if gradient[i] > 0.0 { -1 } else { 1 };
                } else if branches[i] != 0 && f64::from(branches[i]) * (x[i] - epc[i]) < -1e-12 {
                    next[i] = 0;
                }
            }
            if next != branches {
                branches = next;
                continue;
            }
            for i in 0..n {
                let residual = if i < 8 {
                    gradient[i] + LP_AREA * FY * f64::from(branches[i])
                } else {
                    gradient[i]
                };
                if i >= 8 || branches[i] != 0 {
                    assert!(residual.abs() < 1e-4, "primal gradient={residual}");
                }
            }
            let mut v = *u;
            for (c, value) in self.columns.iter().zip(&x) {
                for a in 0..12 {
                    v[a] += c[a] * value;
                }
            }
            let force = std::array::from_fn(|a| dot(&self.k[a], &v));
            let mut eta = [0.0; 6];
            let mut plastic = [0.0; 6];
            let mut strain = [0.0; 8];
            let mut stress = [0.0; 8];
            let r = self_stress_projector();
            let hardening_nmm = LP_AREA * HP * dot(&x[..8], &x[..8]) / 2.0;
            let mut stored_nmm = dot(&v, &force) / 2.0 + hardening_nmm;
            for (i, beta) in x.iter().enumerate().skip(8) {
                stored_nmm += self.springs[i] * beta * beta / 2.0;
                assert!(
                    (dot(&self.columns[i], &force) + self.springs[i] * beta).abs() < 1e-5,
                    "original connection residual"
                );
            }
            for end in 0..2 {
                let ep = &x[end * 4..end * 4 + 4];
                stored_nmm +=
                    LP_AREA * E / 2.0 * (0..4).map(|i| ep[i] * dot(&r[i], ep)).sum::<f64>();
                for (a, slot) in [0, 4, 5].into_iter().enumerate() {
                    let projected: f64 = (0..4).map(|i| B[i][a] * ep[i] / BTB[a]).sum();
                    plastic[end * 3 + a] = 300.0 * projected;
                    eta[end * 3 + a] = projected
                        + if end == 0 { -1.0 } else { 1.0 } * force[end * 6 + slot]
                            / (E * 250.0 * BTB[a]);
                }
                for i in 0..4 {
                    let index = end * 4 + i;
                    strain[index] = dot(&B[i], &eta[end * 3..end * 3 + 3]);
                    stress[index] = E * (strain[index] - ep[i]);
                    let shifted = stress[index] - HP * ep[i];
                    if branches[index] == 0 {
                        assert!(shifted.abs() <= FY + 1e-8);
                    } else {
                        assert_relative_eq!(
                            shifted,
                            f64::from(branches[index]) * FY,
                            epsilon = 1e-8
                        );
                        assert!(f64::from(branches[index]) * (ep[i] - epc[index]) >= -1e-12);
                    }
                }
                for (a, slot) in [0, 4, 5].into_iter().enumerate() {
                    let section: f64 = (0..4).map(|i| 250.0 * B[i][a] * stress[end * 4 + i]).sum();
                    assert_relative_eq!(
                        section,
                        if end == 0 { -1.0 } else { 1.0 } * force[end * 6 + slot],
                        epsilon = 1e-5
                    );
                }
                if let Some(normal) = self.normal {
                    assert!(dot(&normal, &force[end * 6 + 3..end * 6 + 6]).abs() < 1e-5);
                }
            }
            return ReferenceTrial {
                ep: std::array::from_fn(|i| x[i]),
                beta: x[8..].to_vec(),
                force,
                eta,
                plastic,
                strain,
                stress,
                stored_nmm,
                hardening_nmm,
                dissipation_nmm: LP_AREA * FY * (0..8).map(|i| (x[i] - epc[i]).abs()).sum::<f64>(),
                branches,
            };
        }
        panic!("reference active-set limit");
    }

    fn branch_tangent(&self, branches: &[i8; 8]) -> [[f64; 12]; 12] {
        let free: Vec<_> = (0..self.columns.len())
            .filter(|&i| i >= 8 || branches[i] != 0)
            .collect();
        let matrix: Vec<Vec<_>> = free
            .iter()
            .map(|&i| free.iter().map(|&j| self.h[i][j]).collect())
            .collect();
        let mut tangent = [[0.0; 12]; 12];
        for j in 0..12 {
            let rhs: Vec<_> = free
                .iter()
                .map(|&i| {
                    -(0..12)
                        .map(|a| self.columns[i][a] * self.k[a][j])
                        .sum::<f64>()
                })
                .collect();
            let derivative = spd_solve(&matrix, &rhs);
            let mut dv = [0.0; 12];
            dv[j] = 1.0;
            for (&index, dx) in free.iter().zip(derivative) {
                for (a, value) in dv.iter_mut().enumerate() {
                    *value += self.columns[index][a] * dx;
                }
            }
            for i in 0..12 {
                tangent[i][j] = dot(&self.k[i], &dv);
            }
        }
        tangent
    }
}

#[test]
fn coupled_wall_partial_yield_unload_history_matches_uncondensed_primal_state_tangent_and_energy() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let normal = Some([0.0, 0.6, 0.8]);
    let reference = PrimalReference::with_original(normal, Some(1e7));
    let target: [f64; 12] = std::array::from_fn(|i| match i {
        6 => 2.95,
        4 => 0.012,
        10 => -0.007,
        5 => 0.009,
        11 => -0.004,
        _ => 0.0,
    });
    for ms in [false, true] {
        let mut member = axial_member(normal, Some(1e7), 0.01);
        let mut previous = [0.0; 12];
        let mut previous_force = [0.0; 12];
        let mut previous_expected_force = [0.0; 12];
        let mut epc = [0.0; 8];
        let mut observed_epc = [0.0; 8];
        let mut observed_work = 0.0;
        let mut expected_work = 0.0;
        let mut observed_dissipation = 0.0;
        let mut expected_dissipation = 0.0;
        for factor in [0.25, 1.0, 0.75, 1.01] {
            let u = target.map(|value| value * factor);
            let expected = reference.solve_displacement(&u, &epc);
            println!(
                "coupled factor={factor} ms={ms} branches={:?} max_ep_increment={:e}",
                expected.branches,
                (0..8)
                    .map(|i| (expected.ep[i] - epc[i]).abs())
                    .fold(0.0_f64, f64::max)
            );
            let tangent = reference.branch_tangent(&expected.branches);
            for j in 0..12 {
                for sign in [-1.0, 1.0] {
                    let mut perturbed = u;
                    perturbed[j] += sign * if j % 6 < 3 { 1e-6 } else { 1e-9 };
                    assert_eq!(
                        reference.solve_displacement(&perturbed, &epc).branches,
                        expected.branches
                    );
                }
            }
            if factor >= 1.0 {
                let count = expected
                    .branches
                    .iter()
                    .filter(|&&branch| branch != 0)
                    .count();
                assert!(count > 0 && count < 8);
            }

            let behavior = axial_behavior(&mut member, ms);
            let snapshot = behavior.snapshot_state();
            let du = LocalVec {
                data: (0..12).map(|i| u[i] - previous[i]).collect(),
            };
            behavior.update_state(&du, false, &ctx).unwrap();
            let force = behavior.internal_force(&ctx);
            let actual_tangent = behavior.tangent_stiffness(&ctx);
            let states = behavior.fiber_section_states().unwrap();
            for i in 0..12 {
                assert_relative_eq!(force.data[i], expected.force[i], epsilon = 1e-4);
                for j in 0..12 {
                    assert_relative_eq!(
                        actual_tangent.get(i, j),
                        tangent[i][j],
                        epsilon = 1e-4,
                        max_relative = 1e-9
                    );
                }
                observed_work += 0.5 * (force.data[i] + previous_force[i]) * (u[i] - previous[i]);
                expected_work +=
                    0.5 * (expected.force[i] + previous_expected_force[i]) * (u[i] - previous[i]);
            }
            let hinge = member.inner.hinge.as_ref().unwrap();
            for i in 0..6 {
                assert_relative_eq!(hinge.trial_eta[i], expected.eta[i], epsilon = 1e-11);
                assert_relative_eq!(hinge.trial_plastic[i], expected.plastic[i], epsilon = 1e-9);
            }
            let mut observed_ep = [0.0; 8];
            for (end, state) in states.iter().enumerate() {
                for (i, fiber) in state.fibers.iter().enumerate() {
                    let index = end * 4 + i;
                    let stress = member.inner.gauss_points[end].trial_stress[i];
                    assert_relative_eq!(fiber.strain, expected.strain[index], epsilon = 1e-11);
                    assert_relative_eq!(stress, expected.stress[index], epsilon = 1e-8);
                    observed_ep[index] = fiber.strain - stress / E;
                    assert_relative_eq!(observed_ep[index], expected.ep[index], epsilon = 1e-11);
                    observed_dissipation +=
                        LP_AREA * FY * (observed_ep[index] - observed_epc[index]).abs();
                }
            }
            let mut x = observed_ep.to_vec();
            for (i, beta) in expected.beta.iter().enumerate() {
                let column = &reference.columns[8 + i];
                let observed_beta = member.inner.trial_int[i] - dot(column, &u);
                assert_relative_eq!(observed_beta, *beta, epsilon = 1e-11);
                assert!(
                    (dot(column, &force.data) + reference.springs[8 + i] * observed_beta).abs()
                        < 1e-4
                );
                x.push(observed_beta);
            }
            if factor == 1.0 {
                assert!(expected.beta.iter().any(|beta| beta.abs() > 1e-6));
            }
            let mut elastic = u;
            for (column, value) in reference.columns.iter().zip(&x) {
                for i in 0..12 {
                    elastic[i] += column[i] * value;
                }
            }
            let mut stored = (0..12)
                .map(|i| elastic[i] * dot(&reference.k[i], &elastic))
                .sum::<f64>()
                / 2.0
                + LP_AREA * HP * dot(&observed_ep, &observed_ep) / 2.0;
            for (i, beta) in x.iter().enumerate().skip(8) {
                stored += reference.springs[i] * beta * beta / 2.0;
            }
            let r = self_stress_projector();
            for end in 0..2 {
                let ep = &observed_ep[end * 4..end * 4 + 4];
                stored += LP_AREA * E / 2.0 * (0..4).map(|i| ep[i] * dot(&r[i], ep)).sum::<f64>();
            }
            assert_relative_eq!(stored, expected.stored_nmm, epsilon = 1e-4);
            expected_dissipation += expected.dissipation_nmm;
            assert_relative_eq!(observed_dissipation, expected_dissipation, epsilon = 1e-4);
            assert_relative_eq!(observed_work, expected_work, epsilon = 1e-4);
            let behavior = axial_behavior(&mut member, ms);
            let trial = behavior.serialize_checkpoint();
            behavior.restore_state(snapshot.as_ref());
            behavior.update_state(&du, true, &ctx).unwrap();
            let committed_force = behavior.internal_force(&ctx);
            assert_eq!(committed_force.data, force.data);
            assert!(!trial.is_empty());
            previous = u;
            previous_force = std::array::from_fn(|i| force.data[i]);
            previous_expected_force = expected.force;
            observed_epc = observed_ep;
            epc = expected.ep;
        }
        assert!(observed_dissipation > 0.0);
    }
}

#[test]
fn ordinary_fixed_fiber_and_ms_biaxial_history_matches_uncondensed_primal() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let reference = PrimalReference::with_original(None, None);
    let target = std::array::from_fn::<_, 12, _>(|i| match i {
        6 => 2.95,
        4 => 0.012,
        10 => -0.007,
        5 => 0.009,
        11 => -0.004,
        _ => 0.0,
    });
    for ms in [false, true] {
        let mut member = axial_member(None, None, 0.01);
        let mut previous = [0.0; 12];
        let mut epc = [0.0; 8];
        for factor in [0.25, 1.0, 0.75, 1.01] {
            let u = target.map(|value| value * factor);
            let expected = reference.solve_displacement(&u, &epc);
            let tangent = reference.branch_tangent(&expected.branches);
            let behavior = axial_behavior(&mut member, ms);
            behavior
                .update_state(
                    &LocalVec {
                        data: (0..12).map(|i| u[i] - previous[i]).collect(),
                    },
                    true,
                    &ctx,
                )
                .unwrap();
            let force = behavior.internal_force(&ctx);
            let actual_tangent = behavior.tangent_stiffness(&ctx);
            for i in 0..12 {
                assert_relative_eq!(force.data[i], expected.force[i], epsilon = 1e-4);
                for j in 0..12 {
                    assert_relative_eq!(
                        actual_tangent.get(i, j),
                        tangent[i][j],
                        epsilon = 1e-4,
                        max_relative = 1e-9
                    );
                }
            }
            let hinge = member.inner.hinge.as_ref().unwrap();
            for i in 0..6 {
                assert_relative_eq!(hinge.trial_eta[i], expected.eta[i], epsilon = 1e-11);
                assert_relative_eq!(hinge.trial_plastic[i], expected.plastic[i], epsilon = 1e-9);
            }
            println!(
                "ordinary fixed ms={ms}, factor={factor}, primal_branches={:?}",
                expected.branches
            );
            previous = u;
            epc = expected.ep;
        }
        assert!(epc.iter().any(|strain| strain.abs() > 1e-6));
    }
}
