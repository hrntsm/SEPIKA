use super::*;
use approx::assert_relative_eq;
use sepika_core::model::{EndCondition, Model};
use sepika_math::lu::LuSolver;
use sepika_math::solver::LinearSolver;
use sepika_math::sparse::{assemble_csc, Triplet};

fn raw_member(torsion: bool) -> FiberBeam {
    let mut member = super::constitutive_tests::bilinear_member();
    if torsion {
        member.g = 80000.0;
        member.torsion_j = 1e5;
        let kt = member.g * member.torsion_j / member.length;
        for (i, j, sign) in [(3, 3, 1.0), (9, 9, 1.0), (3, 9, -1.0), (9, 3, -1.0)] {
            member.initial_elastic_stiffness.set(i, j, sign * kt);
            member.hinge.as_mut().unwrap().k_el.set(i, j, sign * kt);
        }
    }
    member
}

fn configure_original(member: &mut FiberBeam, conditions: [EndCondition; 2]) {
    member.releases = resolve_end_releases(&conditions, [false; 2], member.g > 0.0);
    member.trial_int = SmallVec::from_elem(0.0, member.releases.len());
    member.committed_int = member.trial_int.clone();
}

fn update(
    member: &mut dyn ElementBehavior,
    u: [f64; 12],
    previous: &mut [f64; 12],
    commit: bool,
    ctx: &Ctx,
) {
    member
        .update_state(
            &LocalVec {
                data: (0..12).map(|i| u[i] - previous[i]).collect(),
            },
            commit,
            ctx,
        )
        .unwrap();
    *previous = u;
}

// 未縮約の6回転と拘束反力を未知数とし、固定条件をLagrange乗数で課す。
pub(super) fn expanded_elastic_state(
    k: &LocalMat,
    u: &[f64; 12],
    normal: [f64; 2],
    conditions: [EndCondition; 2],
) -> ([f64; 12], [f64; 12]) {
    let norm = normal[0].hypot(normal[1]);
    let [ny, nz] = normal.map(|x| x / norm);
    let directions = [[1.0, 0.0, 0.0], [0.0, -nz, ny]];
    let mut constraints = Vec::new();
    let mut springs = [[0.0; 6]; 6];
    for (end, condition) in conditions.into_iter().enumerate() {
        for direction in directions {
            let mut v = [0.0; 6];
            v[end * 3..end * 3 + 3].copy_from_slice(&direction);
            match condition {
                EndCondition::Fixed => constraints.push(v),
                EndCondition::Pinned => {}
                EndCondition::SemiRigid { k_theta } => {
                    for i in 0..6 {
                        for j in 0..6 {
                            springs[i][j] += k_theta * v[i] * v[j];
                        }
                    }
                }
            }
        }
    }
    if conditions.iter().all(|c| {
        matches!(
            c,
            EndCondition::Pinned | EndCondition::SemiRigid { k_theta: 0.0 }
        )
    }) {
        constraints.push([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }
    let rotation_slots = [3, 4, 5, 9, 10, 11];
    let translation_slots = [0, 1, 2, 6, 7, 8];
    let size = 6 + constraints.len();
    let mut matrix = vec![0.0; size * size];
    let mut rhs = vec![0.0; size];
    for i in 0..6 {
        for j in 0..6 {
            matrix[i * size + j] =
                (k.get(rotation_slots[i], rotation_slots[j]) + springs[i][j]) / 1e8;
            rhs[i] += springs[i][j] * u[rotation_slots[j]] / 1e8;
        }
        for j in translation_slots {
            rhs[i] -= k.get(rotation_slots[i], j) * u[j] / 1e8;
        }
    }
    for (c, v) in constraints.iter().enumerate() {
        for i in 0..6 {
            matrix[(6 + c) * size + i] = v[i];
            matrix[i * size + 6 + c] = v[i];
            rhs[6 + c] += v[i] * u[rotation_slots[i]];
        }
    }
    let triplets = matrix
        .iter()
        .enumerate()
        .filter_map(|(i, &val)| {
            (val != 0.0).then_some(Triplet {
                row: i / size,
                col: i % size,
                val,
            })
        })
        .collect();
    let mut solver = LuSolver::default();
    solver.factorize(&assemble_csc(size, triplets)).unwrap();
    let solution = solver.solve(&rhs).unwrap();
    for i in 0..size {
        let row = &matrix[i * size..(i + 1) * size];
        let actual: f64 = row.iter().zip(&solution).map(|(a, x)| a * x).sum();
        assert!((actual - rhs[i]).abs() < 1e-12 * rhs.iter().map(|v| v.abs()).fold(1.0, f64::max));
    }
    let mut e = *u;
    for (i, slot) in rotation_slots.into_iter().enumerate() {
        e[slot] = solution[i];
    }
    (
        e,
        std::array::from_fn(|i| (0..12).map(|j| k.get(i, j) * e[j]).sum()),
    )
}

fn expanded_elastic_force(
    k: &LocalMat,
    u: &[f64; 12],
    normal: [f64; 2],
    conditions: [EndCondition; 2],
) -> [f64; 12] {
    expanded_elastic_state(k, u, normal, conditions).1
}

#[test]
fn elastic_arbitrary_normals_original_connections_rigid_arms_and_local_axes_match_expanded_system()
{
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for normal in [[1.0, 0.0], [0.0, 1.0], [0.6, 0.8], [-0.6, -0.8]] {
        for conditions in [
            [EndCondition::Fixed; 2],
            [EndCondition::Pinned; 2],
            [
                EndCondition::SemiRigid { k_theta: 1e7 },
                EndCondition::Fixed,
            ],
            [
                EndCondition::Pinned,
                EndCondition::SemiRigid { k_theta: 1e7 },
            ],
        ] {
            for rotated in [false, true] {
                let mut fiber = raw_member(true);
                configure_original(&mut fiber, conditions);
                fiber.length = 3250.0;
                fiber.rigid_i = 100.0;
                fiber.rigid_j = 150.0;
                if rotated {
                    fiber.axis = crate::transform::LocalFrame::from_nodes(
                        [0.0; 3],
                        [0.0, 0.0, 3000.0],
                        [0.6, 0.8, 0.0],
                    );
                }
                let normal_global = std::array::from_fn(|i| {
                    fiber.axis.rot[1][i] * normal[0] + fiber.axis.rot[2][i] * normal[1]
                });
                fiber.configure_wall_release(normal_global).unwrap();
                let u = [
                    0.0, 0.02, -0.01, 1e-5, 2e-5, -1e-5, 0.1, -0.03, 0.04, -2e-5, -1e-5, 3e-5,
                ];
                let flexible = crate::frame::rigid_arm::to_flex_disp(&u, 100.0, 150.0);
                let expected = expanded_elastic_force(
                    &fiber.initial_elastic_stiffness,
                    &flexible,
                    normal,
                    conditions,
                );
                let expected_node = crate::frame::rigid_arm::to_node_force(&expected, 100.0, 150.0);
                let expected_global = fiber.axis.rotate_to_global(&expected_node);
                let ug = fiber.axis.rotate_to_global(&u);
                update(&mut fiber, ug, &mut [0.0; 12], false, &ctx);
                let actual = fiber.internal_force(&ctx);
                for i in 0..12 {
                    assert_relative_eq!(
                        actual.data[i],
                        expected_global[i],
                        epsilon = 1e-5,
                        max_relative = 1e-8
                    );
                }
                let raw = fiber.elem_internal_force(&fiber.elem_disp(&fiber.flex_disp()));
                for offset in [0, 6] {
                    assert!(
                        (normal[0] * raw[offset + 4] + normal[1] * raw[offset + 5]).abs() < 1e-5
                    );
                }
                let kt = fiber.tangent_stiffness(&ctx);
                for j in 0..12 {
                    let mut e = [0.0; 12];
                    e[j] = 1.0;
                    let el = fiber.axis.rotate_to_local(&e);
                    let ef = crate::frame::rigid_arm::to_flex_disp(&el, 100.0, 150.0);
                    let ff = expanded_elastic_force(
                        &fiber.initial_elastic_stiffness,
                        &ef,
                        normal,
                        conditions,
                    );
                    let fnod = crate::frame::rigid_arm::to_node_force(&ff, 100.0, 150.0);
                    let fg = fiber.axis.rotate_to_global(&fnod);
                    for (i, expected) in fg.iter().enumerate() {
                        assert_relative_eq!(
                            kt.get(i, j),
                            *expected,
                            epsilon = 2e-6,
                            max_relative = 1e-8
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn released_fiber_and_ms_keep_axial_yield_unload_reload_work_and_checkpoints() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for ms in [false, true] {
        for sign in [-1.0, 1.0] {
            let mut fiber = raw_member(false);
            fiber.configure_wall_release([0.0, 0.6, 0.8]).unwrap();
            let mut member: Box<dyn ElementBehavior> = if ms {
                Box::new(crate::frame::multi_spring::MultiSpringElement {
                    springs: Vec::new(),
                    inner: fiber,
                })
            } else {
                Box::new(fiber)
            };
            let mut previous = [0.0; 12];
            let mut u = [0.0; 12];
            u[6] = sign * 3.0;
            update(member.as_mut(), u, &mut previous, false, &ctx);
            assert_relative_eq!(
                member.internal_force(&ctx).data[6],
                sign * 200000.0,
                epsilon = 1e-5
            );
            u[6] = sign * 6.0;
            update(member.as_mut(), u, &mut previous, true, &ctx);
            let peak = member.internal_force(&ctx).data[6];
            assert_relative_eq!(peak, sign * 209615.3846153846, epsilon = 1e-5);
            let work = 0.5 * 200000.0 * 3.0 + 0.5 * (200000.0 + peak.abs()) * 3.0;
            assert_relative_eq!(work, 914423.0769230769, epsilon = 1e-5);
            let checkpoint = member.serialize_checkpoint();
            let force = member.internal_force(&ctx).data;
            let tangent = member.tangent_stiffness(&ctx).data;
            let snapshot = member.snapshot_state();
            u[6] = sign * 2.85576923076923;
            update(member.as_mut(), u, &mut previous, false, &ctx);
            assert!(member.internal_force(&ctx).data[6].abs() < 1e-5);
            member.revert_state();
            previous[6] = sign * 6.0;
            assert_eq!(member.serialize_checkpoint(), checkpoint);
            member.restore_state(snapshot.as_ref());
            let bad = LocalVec {
                data: smallvec::smallvec![f64::MAX;12],
            };
            assert!(member.update_state(&bad, true, &ctx).is_err());
            assert_eq!(member.serialize_checkpoint(), checkpoint);
            assert_eq!(member.internal_force(&ctx).data, force);
            assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
            member.deserialize_checkpoint(&checkpoint).unwrap();
            u[6] = sign * 4.0;
            update(member.as_mut(), u, &mut previous, true, &ctx);
            assert_relative_eq!(
                member.internal_force(&ctx).data[6],
                sign * 76282.0512820513,
                epsilon = 1e-5
            );
            u[6] = sign * 6.0;
            update(member.as_mut(), u, &mut previous, false, &ctx);
            assert_relative_eq!(member.internal_force(&ctx).data[6], peak, epsilon = 1e-5);
        }
    }
}

#[test]
fn nonlinear_trial_tangent_matches_reconverged_finite_differences_and_fresh_checkpoint_resume() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for ms in [false, true] {
        for normal in [[0.0, 0.0, 1.0], [0.0, 0.6, 0.8]] {
            let build = || -> Box<dyn ElementBehavior> {
                let mut f = raw_member(false);
                if ms {
                    let mut m = crate::frame::multi_spring::MultiSpringElement {
                        springs: Vec::new(),
                        inner: f,
                    };
                    m.configure_wall_release(normal).unwrap();
                    Box::new(m)
                } else {
                    f.configure_wall_release(normal).unwrap();
                    Box::new(f)
                }
            };
            let mut fiber = build();
            let mut committed = [0.0; 12];
            committed[6] = 1.5;
            update(fiber.as_mut(), committed, &mut [0.0; 12], true, &ctx);
            let checkpoint = fiber.serialize_checkpoint();
            let mut trial = committed;
            trial[6] = 3.1;
            trial[2] = 0.2;
            trial[4] = 0.002;
            trial[7] = 0.3;
            trial[11] = 0.001;
            update(fiber.as_mut(), trial, &mut committed.clone(), false, &ctx);
            let force = fiber.internal_force(&ctx).data;
            let tangent = fiber.tangent_stiffness(&ctx);
            assert!((force[6] - tangent.get(6, 6) * trial[6]).abs() > 1000.0);
            let trial_cp = fiber.serialize_checkpoint();
            let mut resumed = build();
            resumed.deserialize_checkpoint(&trial_cp).unwrap();
            assert_eq!(resumed.internal_force(&ctx).data, force);
            assert_eq!(resumed.tangent_stiffness(&ctx).data, tangent.data);
            for j in 0..12 {
                let h = if j % 6 < 3 { 1e-5 } else { 1e-8 };
                let mut forces = Vec::new();
                for sign in [-1.0, 1.0] {
                    let mut f = build();
                    f.deserialize_checkpoint(&checkpoint).unwrap();
                    let mut perturbed = trial;
                    perturbed[j] += sign * h;
                    update(f.as_mut(), perturbed, &mut committed.clone(), false, &ctx);
                    forces.push(f.internal_force(&ctx).data);
                }
                let scale = (0..12).map(|i| tangent.get(i, j).abs()).fold(1.0, f64::max);
                for i in 0..12 {
                    let fd = (forces[1][i] - forces[0][i]) / (2.0 * h);
                    assert!(
                        (fd - tangent.get(i, j)).abs() < 2e-5 * scale,
                        "normal={normal:?}, ({i},{j}), fd={fd}, K={}",
                        tangent.get(i, j)
                    );
                }
            }
            let increment = LocalVec {
                data: smallvec::smallvec![0.001;12],
            };
            fiber.update_state(&increment, true, &ctx).unwrap();
            resumed.update_state(&increment, true, &ctx).unwrap();
            assert_eq!(resumed.serialize_checkpoint(), fiber.serialize_checkpoint());
        }
    }
}

#[test]
fn real_fiber_and_ms_constructors_apply_release_without_rotating_material_coordinates() {
    let model = super::tests::build_release_model([
        EndCondition::SemiRigid { k_theta: 1e9 },
        EndCondition::Fixed,
    ]);
    let ctx = Ctx { model: &model };
    for ms in [false, true] {
        let mut member: Box<dyn ElementBehavior> = if ms {
            let mut f = crate::frame::multi_spring::MultiSpringElement::new(
                &model.elements[0],
                &model,
                crate::factory::StrengthBasis::Nominal,
                AnalysisKind::Incremental,
            );
            f.configure_wall_release([0.0, 0.0, 1.0]).unwrap();
            Box::new(f)
        } else {
            let mut f = FiberBeam::build_plastic_zone(
                &model.elements[0],
                &model,
                300.0,
                12,
                20,
                crate::factory::StrengthBasis::Nominal,
                AnalysisKind::Incremental,
            );
            f.configure_wall_release([0.0, 0.0, 1.0]).unwrap();
            Box::new(f)
        };
        let before = member.fiber_section_states().unwrap();
        let mut u = [0.0; 12];
        u[6] = 0.1;
        u[7] = 1.0;
        u[11] = 0.002;
        u[4] = 0.00001;
        update(member.as_mut(), u, &mut [0.0; 12], true, &ctx);
        let force = member.internal_force(&ctx);
        assert!(force.data[6].abs() > 1.0);
        assert!(force.data[4].abs() > 1.0);
        assert!(force.data[5].abs() < 1e-3);
        assert!(force.data[11].abs() < 1e-3);
        assert!(force.data[1].abs() < 1e-3);
        let after = member.fiber_section_states().unwrap();
        for (a, b) in before.iter().zip(after) {
            for (a, b) in a.fibers.iter().zip(b.fibers) {
                assert_eq!(
                    (a.y, a.z, a.area, a.material),
                    (b.y, b.z, b.area, b.material)
                );
            }
        }
    }
}

#[test]
fn incompatible_checkpoint_and_configuration_after_trial_are_rejected_atomically() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut f = raw_member(false);
    f.configure_wall_release([0.0, 0.6, 0.8]).unwrap();
    let cp = f.serialize_checkpoint();
    let mut other = raw_member(false);
    other.configure_wall_release([0.0, 1.0, 0.0]).unwrap();
    let other_cp = other.serialize_checkpoint();
    assert!(other.deserialize_checkpoint(&cp).is_err());
    assert_eq!(other.serialize_checkpoint(), other_cp);
    assert_eq!(f.serialize_checkpoint(), cp);
    update(&mut f, [0.0; 12], &mut [0.0; 12], false, &ctx);
    assert!(f.configure_wall_release([0.0, 1.0, 0.0]).is_err());
    assert_eq!(f.serialize_checkpoint(), cp);
}

#[test]
fn singular_inner_trial_after_material_evaluation_rolls_back_and_allows_retry() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for ms in [false, true] {
        let mut fiber = raw_member(false);
        for gp in &mut fiber.gauss_points {
            for material in &mut gp.mats {
                *material = Box::new(sepika_material::uniaxial::Bilinear::new(
                    200000.0, 200.0, 0.0,
                ));
            }
        }
        fiber.configure_wall_release([0.0, 0.6, 0.8]).unwrap();
        let mut member: Box<dyn ElementBehavior> = if ms {
            Box::new(crate::frame::multi_spring::MultiSpringElement {
                springs: Vec::new(),
                inner: fiber,
            })
        } else {
            Box::new(fiber)
        };
        let mut u = [0.0; 12];
        u[6] = 1.5;
        update(member.as_mut(), u, &mut [0.0; 12], true, &ctx);
        let checkpoint = member.serialize_checkpoint();
        let force = member.internal_force(&ctx).data;
        let tangent = member.tangent_stiffness(&ctx).data;
        let mut du = LocalVec {
            data: smallvec::smallvec![0.0;12],
        };
        du.data[6] = 4.5;
        let error = member.update_state(&du, true, &ctx).unwrap_err();
        assert!(
            error.to_string().contains("内側断面釣合いのJacobian"),
            "{error}"
        );
        assert_eq!(member.serialize_checkpoint(), checkpoint);
        assert_eq!(member.internal_force(&ctx).data, force);
        assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
        member.revert_state();
        du.data[6] = 0.5;
        member.update_state(&du, true, &ctx).unwrap();
        assert_relative_eq!(
            member.internal_force(&ctx).data[6],
            133333.3333333333,
            epsilon = 1e-5
        );
    }
}

fn test_behavior(
    member: &mut crate::frame::multi_spring::MultiSpringElement,
    ms: bool,
) -> &mut dyn ElementBehavior {
    if ms {
        member
    } else {
        &mut member.inner
    }
}

fn test_configure(
    member: &mut crate::frame::multi_spring::MultiSpringElement,
    ms: bool,
    normal: [f64; 3],
) -> Result<(), String> {
    if ms {
        member.configure_wall_release(normal)
    } else {
        member.inner.configure_wall_release(normal)
    }
}

fn wrapped_raw() -> crate::frame::multi_spring::MultiSpringElement {
    crate::frame::multi_spring::MultiSpringElement {
        springs: Vec::new(),
        inner: raw_member(true),
    }
}

#[test]
fn snapshots_restore_connection_configuration_and_initial_configuration_permission_for_fiber_and_ms(
) {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for ms in [false, true] {
        let mut member = wrapped_raw();
        let original = test_behavior(&mut member, ms).snapshot_state();
        let original_cp = test_behavior(&mut member, ms).serialize_checkpoint();
        let original_force = test_behavior(&mut member, ms).internal_force(&ctx).data;
        let original_tangent = test_behavior(&mut member, ms).tangent_stiffness(&ctx).data;
        test_configure(&mut member, ms, [0.0, 0.6, 0.8]).unwrap();
        let first = test_behavior(&mut member, ms).snapshot_state();
        let first_cp = test_behavior(&mut member, ms).serialize_checkpoint();
        test_configure(&mut member, ms, [0.0, 1.0, 0.0]).unwrap();
        let second_cp = test_behavior(&mut member, ms).serialize_checkpoint();
        assert_ne!(first_cp, second_cp);
        test_behavior(&mut member, ms).restore_state(first.as_ref());
        assert_eq!(
            test_behavior(&mut member, ms).serialize_checkpoint(),
            first_cp
        );
        let mut u = [0.0; 12];
        u[6] = 3.5;
        u[2] = 0.01;
        u[4] = 0.00001;
        update(
            test_behavior(&mut member, ms),
            u,
            &mut [0.0; 12],
            true,
            &ctx,
        );
        assert!(test_configure(&mut member, ms, [0.0, 1.0, 0.0]).is_err());
        test_behavior(&mut member, ms).restore_state(first.as_ref());
        test_configure(&mut member, ms, [0.0, 1.0, 0.0]).unwrap();
        assert_eq!(
            test_behavior(&mut member, ms).serialize_checkpoint(),
            second_cp
        );
        test_behavior(&mut member, ms).restore_state(original.as_ref());
        assert_eq!(
            test_behavior(&mut member, ms).serialize_checkpoint(),
            original_cp
        );
        assert_eq!(
            test_behavior(&mut member, ms).internal_force(&ctx).data,
            original_force
        );
        assert_eq!(
            test_behavior(&mut member, ms).tangent_stiffness(&ctx).data,
            original_tangent
        );
        test_configure(&mut member, ms, [0.0, 0.6, 0.8]).unwrap();
        assert_eq!(
            test_behavior(&mut member, ms).serialize_checkpoint(),
            first_cp
        );
    }
}

#[test]
fn checkpoints_reject_different_original_springs_and_torsion_releases_atomically_for_fiber_and_ms()
{
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let semi = |k_theta| EndCondition::SemiRigid { k_theta };
    for ms in [false, true] {
        for (source_condition, target_condition, source_torsion, target_torsion) in [
            ([semi(1e7); 2], [semi(2e7); 2], [false; 2], [false; 2]),
            (
                [EndCondition::Pinned; 2],
                [semi(1e7); 2],
                [false; 2],
                [false; 2],
            ),
            (
                [EndCondition::Fixed; 2],
                [EndCondition::Fixed; 2],
                [true, false],
                [false, true],
            ),
        ] {
            let build = |conditions, torsion| {
                let mut member = wrapped_raw();
                member.inner.releases = resolve_end_releases(&conditions, torsion, true);
                member.inner.trial_int = SmallVec::from_elem(0.0, member.inner.releases.len());
                member.inner.committed_int = member.inner.trial_int.clone();
                member
            };
            let mut source = build(source_condition, source_torsion);
            let mut u = [0.0; 12];
            u[6] = 3.5;
            u[2] = 0.01;
            u[4] = 0.00001;
            update(
                test_behavior(&mut source, ms),
                u,
                &mut [0.0; 12],
                true,
                &ctx,
            );
            let cp = test_behavior(&mut source, ms).serialize_checkpoint();
            let mut target = build(target_condition, target_torsion);
            assert_eq!(source.inner.releases.len(), target.inner.releases.len());
            let mut target_u = [0.0; 12];
            target_u[6] = 0.1;
            update(
                test_behavior(&mut target, ms),
                target_u,
                &mut [0.0; 12],
                false,
                &ctx,
            );
            let before = test_behavior(&mut target, ms).serialize_checkpoint();
            let force = test_behavior(&mut target, ms).internal_force(&ctx).data;
            let tangent = test_behavior(&mut target, ms).tangent_stiffness(&ctx).data;
            assert!(test_behavior(&mut target, ms)
                .deserialize_checkpoint(&cp)
                .is_err());
            assert_eq!(
                test_behavior(&mut target, ms).serialize_checkpoint(),
                before
            );
            assert_eq!(
                test_behavior(&mut target, ms).internal_force(&ctx).data,
                force
            );
            assert_eq!(
                test_behavior(&mut target, ms).tangent_stiffness(&ctx).data,
                tangent
            );
            let mut matching = build(source_condition, source_torsion);
            test_behavior(&mut matching, ms)
                .deserialize_checkpoint(&cp)
                .unwrap();
            assert_eq!(test_behavior(&mut matching, ms).serialize_checkpoint(), cp);
            assert_eq!(
                test_behavior(&mut matching, ms).internal_force(&ctx).data,
                test_behavior(&mut source, ms).internal_force(&ctx).data
            );
            assert_eq!(
                test_behavior(&mut matching, ms)
                    .tangent_stiffness(&ctx)
                    .data,
                test_behavior(&mut source, ms).tangent_stiffness(&ctx).data
            );
            let du = LocalVec {
                data: smallvec::smallvec![0.001;12],
            };
            test_behavior(&mut matching, ms)
                .update_state(&du, true, &ctx)
                .unwrap();
            test_behavior(&mut source, ms)
                .update_state(&du, true, &ctx)
                .unwrap();
            assert_eq!(
                test_behavior(&mut matching, ms).serialize_checkpoint(),
                test_behavior(&mut source, ms).serialize_checkpoint()
            );
        }
    }
}
