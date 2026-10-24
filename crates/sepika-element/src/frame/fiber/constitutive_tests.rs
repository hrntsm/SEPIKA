use super::*;
use crate::factory::StrengthBasis;
use approx::assert_relative_eq;
use sepika_core::model::{AnalysisKind, ElementKind, EndCondition, Model};
use sepika_material::uniaxial::Bilinear;

pub(super) fn bilinear_member() -> FiberBeam {
    bilinear_member_at(0.0, 0.0)
}

fn bilinear_member_at(y_offset: f64, z_offset: f64) -> FiberBeam {
    similar_bilinear_member(1.0, y_offset, z_offset)
}

fn similar_bilinear_member(scale: f64, y_offset: f64, z_offset: f64) -> FiberBeam {
    let sections = std::array::from_fn(|_| {
        let fibers = [(-10.0, -20.0), (10.0, -20.0), (-10.0, 20.0), (10.0, 20.0)]
            .into_iter()
            .map(|(y, z)| Fiber {
                y: (y + y_offset) * scale,
                z: (z + z_offset) * scale,
                area: 250.0 * scale.powi(2),
                material: 2,
            })
            .collect();
        let materials = (0..4)
            .map(|_| Box::new(Bilinear::new(200000.0, 200.0, 0.01)) as Box<dyn UniaxialMaterial>)
            .collect();
        (FiberSection { fibers }, materials)
    });
    FiberBeam::from_raw_parts(
        [NodeId(0), NodeId(1)],
        3000.0 * scale,
        crate::transform::LocalFrame::from_nodes(
            [0.0; 3],
            [3000.0 * scale, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        )
        .unwrap(),
        SectionMassProperties::default(),
        200000.0,
        0.0,
        1000.0 * scale.powi(2),
        400000.0 * scale.powi(4),
        100000.0 * scale.powi(4),
        0.0,
        0.0,
        0.0,
        300.0 * scale,
        sections,
    )
}

fn set_displacement(
    member: &mut dyn ElementBehavior,
    u: [f64; 12],
    previous: &mut [f64; 12],
    commit: bool,
    ctx: &Ctx,
) {
    let du = LocalVec {
        data: (0..12).map(|i| u[i] - previous[i]).collect(),
    };
    member.update_state(&du, commit, ctx).unwrap();
    *previous = u;
}

fn axial(
    member: &mut dyn ElementBehavior,
    delta: f64,
    previous: &mut [f64; 12],
    commit: bool,
    ctx: &Ctx,
) -> f64 {
    let mut u = [0.0; 12];
    u[6] = delta;
    set_displacement(member, u, previous, commit, ctx);
    member.internal_force(ctx).data[6]
}

#[test]
fn bilinear_axial_yield_unload_reverse_reload_and_work_match_independent_series_solution() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for sign in [-1.0, 1.0] {
        let mut member = bilinear_member();
        let mut previous = [0.0; 12];
        assert_relative_eq!(
            axial(&mut member, sign * 1.5, &mut previous, false, &ctx),
            sign * 100000.0,
            epsilon = 1e-6
        );
        let yield_force = axial(&mut member, sign * 3.0, &mut previous, false, &ctx);
        assert_relative_eq!(yield_force, sign * 200000.0, epsilon = 1e-6);
        let peak = axial(&mut member, sign * 6.0, &mut previous, false, &ctx);
        assert_relative_eq!(peak, sign * 209615.3846153846, epsilon = 1e-5);
        assert_relative_eq!(
            member.tangent_stiffness(&ctx).get(6, 6),
            3205.1282051282,
            epsilon = 1e-5
        );
        let work = 0.5 * yield_force * (sign * 3.0) + 0.5 * (yield_force + peak) * (sign * 3.0);
        assert_relative_eq!(work, 914423.0769230769, epsilon = 1e-4);
        let sections = member.fiber_section_states().unwrap();
        let expected_strain = sign * 0.005807692307692307;
        for section in sections {
            for fiber in section.fibers {
                assert_relative_eq!(fiber.strain, expected_strain, epsilon = 1e-12);
            }
        }
        assert_relative_eq!(
            member.ductility_probe().unwrap().max_yield_ratio,
            5.807692307692307,
            epsilon = 1e-9
        );
        let elastic_energy = 0.5 * peak * peak / 66666.66666666667;
        let material_work = 600.0
            * 1000.0
            * (0.5 * 200.0 * 0.001
                + 0.5 * (200.0 + peak.abs() / 1000.0) * (expected_strain.abs() - 0.001));
        let elastic_subtraction = 600.0 * 1000.0 * 0.5 * (peak / 1000.0).powi(2) / 200000.0;
        assert_relative_eq!(elastic_energy, 329539.5710059172, epsilon = 1e-5);
        assert_relative_eq!(material_work, 650791.4201183432, epsilon = 1e-5);
        assert_relative_eq!(elastic_subtraction, 65907.91420118344, epsilon = 1e-5);
        assert_relative_eq!(
            elastic_energy + material_work - elastic_subtraction,
            work,
            epsilon = 1e-5
        );
        member.commit_state();
        let zero = axial(
            &mut member,
            sign * 2.85576923076923,
            &mut previous,
            true,
            &ctx,
        );
        assert!(zero.abs() < 1e-5, "N={zero}");
        assert_relative_eq!(
            member.tangent_stiffness(&ctx).get(6, 6),
            66666.66666666667,
            epsilon = 1e-5
        );
        assert_relative_eq!(
            axial(&mut member, 0.0, &mut previous, true, &ctx),
            -sign * 190384.6153846154,
            epsilon = 1e-5
        );
        assert_relative_eq!(
            axial(&mut member, -sign * 3.0, &mut previous, true, &ctx),
            -sign * 200000.0,
            epsilon = 1e-5
        );
        assert_relative_eq!(
            axial(&mut member, sign * 6.0, &mut previous, false, &ctx),
            sign * 209615.3846153846,
            epsilon = 1e-5
        );
    }
}

#[test]
fn axial_biaxial_fiber_forces_and_plastic_work_are_conjugate() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut member = bilinear_member();
    // 4点の手計算応力 [80, 0, 201.2, 200.4] N/mm²。
    let q = [120400.0, 1608000.0, 202000.0];
    let eta = [0.0008, 0.00003, 0.00002];
    let d0 = [200000000.0, 80000000000.0, 20000000000.0];
    let plastic: [f64; 3] = std::array::from_fn(|a| 300.0 * (eta[a] - q[a] / d0[a]));
    let mut u = [0.0; 12];
    u[0] = -plastic[0];
    u[6] = 3000.0 * q[0] / d0[0] + plastic[0];
    u[4] = -(1500.0 * q[1] / d0[1] + plastic[1]);
    u[10] = -u[4];
    u[5] = -(1500.0 * q[2] / d0[2] + plastic[2]);
    u[11] = -u[5];
    let mut previous = [0.0; 12];
    set_displacement(&mut member, u, &mut previous, false, &ctx);
    let force = member.internal_force(&ctx);
    for (slot, expected) in [
        (0, -q[0]),
        (6, q[0]),
        (4, -q[1]),
        (10, q[1]),
        (5, -q[2]),
        (11, q[2]),
    ] {
        assert_relative_eq!(force.data[slot], expected, epsilon = 1e-5);
    }
    for end in 0..2 {
        for a in 0..3 {
            assert_relative_eq!(
                member.hinge.as_ref().unwrap().trial_eta[end * 3 + a],
                eta[a],
                epsilon = 1e-12
            );
            assert_relative_eq!(
                member.hinge.as_ref().unwrap().trial_plastic[end * 3 + a],
                plastic[a],
                epsilon = 1e-12
            );
        }
    }
    let nodal_work: f64 = (0..12).map(|i| force.data[i] * u[i]).sum();
    let elastic_work = 3000.0 * (0..3).map(|a| q[a] * q[a] / d0[a]).sum::<f64>();
    let plastic_work = 2.0 * (0..3).map(|a| q[a] * plastic[a]).sum::<f64>();
    assert_relative_eq!(nodal_work - elastic_work, plastic_work, epsilon = 1e-6);
    assert!(member.tangent_stiffness(&ctx).get(6, 4).abs() > 1.0);
}

#[test]
fn same_commit_finite_difference_includes_all_axial_biaxial_couplings() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut member = bilinear_member();
    let mut previous = [0.0; 12];
    axial(&mut member, 1.0, &mut previous, true, &ctx);
    let committed = member.snapshot_state();
    let mut u = [0.0; 12];
    u[6] = 3.7;
    u[4] = 0.012;
    u[10] = -0.007;
    u[5] = 0.009;
    u[11] = -0.004;
    set_displacement(&mut member, u, &mut previous, false, &ctx);
    let tangent = member.tangent_stiffness(&ctx);
    for j in 0..12 {
        let step = if j % 6 < 3 { 1e-5 } else { 1e-7 };
        let mut forces = Vec::new();
        for sign in [-1.0, 1.0] {
            member.restore_state(committed.as_ref());
            previous = [0.0; 12];
            previous[6] = 1.0;
            let mut target = u;
            target[j] += sign * step;
            set_displacement(&mut member, target, &mut previous, false, &ctx);
            forces.push(member.internal_force(&ctx));
        }
        for i in 0..12 {
            let fd = (forces[1].data[i] - forces[0].data[i]) / (2.0 * step);
            assert_relative_eq!(tangent.get(i, j), fd, epsilon = 0.02, max_relative = 2e-6);
        }
    }
}

#[test]
fn failed_trial_snapshot_checkpoint_revert_and_retry_preserve_axial_history() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut member = bilinear_member();
    let mut previous = [0.0; 12];
    axial(&mut member, 6.0, &mut previous, true, &ctx);
    axial(&mut member, 5.0, &mut previous, false, &ctx);
    let snapshot = member.snapshot_state();
    let bytes = member.serialize_checkpoint();
    let force = member.internal_force(&ctx).data.to_vec();
    let tangent = member.tangent_stiffness(&ctx).data;
    let bad = LocalVec {
        data: smallvec::smallvec![f64::NAN;12],
    };
    let failure = member.update_state(&bad, false, &ctx);
    assert!(failure.is_err());
    assert_eq!(
        member.internal_force(&ctx).data.as_slice(),
        force.as_slice()
    );
    assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
    axial(&mut member, -3.0, &mut previous, false, &ctx);
    member.restore_state(snapshot.as_ref());
    assert_eq!(
        member.internal_force(&ctx).data.as_slice(),
        force.as_slice()
    );
    member.deserialize_checkpoint(&bytes).unwrap();
    assert_eq!(
        member.internal_force(&ctx).data.as_slice(),
        force.as_slice()
    );
    assert_relative_eq!(
        member.fiber_section_states().unwrap()[0].fibers[0].strain,
        0.005474358974358974,
        epsilon = 1e-12
    );
    member.revert_state();
    previous = [0.0; 12];
    previous[6] = 6.0;
    assert_relative_eq!(
        axial(&mut member, 2.85576923076923, &mut previous, false, &ctx),
        0.0,
        epsilon = 1e-5
    );
}

#[test]
fn public_fiber_and_ms_factories_keep_axial_plasticity_for_fixed_pinned_and_semirigid_ends() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        for end in [
            EndCondition::Fixed,
            EndCondition::Pinned,
            EndCondition::SemiRigid { k_theta: 1e9 },
        ] {
            let mut model = super::tests::build_test_model(Some(80000.0));
            model.elements[0].kind = kind;
            model.elements[0].end_cond = [end, end];
            model.elements[0].plastic_zone = Some(300.0);
            model.sections[0].j = 1e6;
            model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
            model.materials[0].category = sepika_core::model::MaterialCategory::Steel;
            model.materials[0].young = 200000.0;
            model.materials[0].fy = Some(200.0);
            model.materials[0].fc = None;
            let ctx = Ctx { model: &model };
            let mut member = crate::factory::build_nonlinear_behavior(
                &model.elements[0],
                &model,
                StrengthBasis::Nominal,
                AnalysisKind::Incremental,
                None,
            );
            let mut previous = [0.0; 12];
            let initial = member.tangent_stiffness(&ctx).get(6, 6);
            let mut reference = MenegottoPinto::new(200000.0, 200.0);
            let (stress, et) = reference.trial(0.005);
            let delta = 3000.0 * stress / 200000.0 + 600.0 * (0.005 - stress / 200000.0);
            assert_relative_eq!(
                axial(member.as_mut(), delta, &mut previous, false, &ctx),
                20000.0 * stress,
                epsilon = 1e-4
            );
            let serial_tangent =
                20000.0 / (3000.0 / 200000.0 + 600.0 * (1.0 / et - 1.0 / 200000.0));
            assert_relative_eq!(
                member.tangent_stiffness(&ctx).get(6, 6),
                serial_tangent,
                epsilon = 1e-3,
                max_relative = 1e-8
            );
            let force = axial(member.as_mut(), 6.0, &mut previous, true, &ctx);
            assert!(
                force > 0.0 && force < 0.7 * initial * 6.0,
                "{kind:?} {end:?}: N={force}, Kel={initial}"
            );
            assert!(member.tangent_stiffness(&ctx).get(6, 6) < 0.2 * initial);
            let unloaded = axial(member.as_mut(), 5.5, &mut previous, false, &ctx);
            assert!(unloaded < force);
            member.revert_state();
            assert_relative_eq!(member.internal_force(&ctx).data[6], force, epsilon = 1e-5);
        }
    }
}

#[test]
fn noncentroidal_initial_section_retains_full_axial_biaxial_coupling_without_extra_elastic_compliance(
) {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut member = bilinear_member_at(3.0, 7.0);
    let mut previous = [0.0; 12];
    assert_relative_eq!(
        axial(&mut member, 1.5, &mut previous, false, &ctx),
        100000.0,
        epsilon = 1e-6
    );
    let h = member.hinge.as_ref().unwrap();
    assert!(h.d0[0][0][1].abs() > 1.0 && h.d0[0][0][2].abs() > 1.0 && h.d0[0][1][2].abs() > 1.0);
    for end in 0..2 {
        for (a, expected) in [0.00060625, -8.75e-6, 15e-6].into_iter().enumerate() {
            assert_relative_eq!(h.trial_eta[end * 3 + a], expected, epsilon = 1e-12);
            assert_relative_eq!(h.trial_plastic[end * 3 + a], 0.0, epsilon = 1e-12);
        }
    }
    assert_relative_eq!(
        member.tangent_stiffness(&ctx).get(6, 6),
        66666.66666666667,
        epsilon = 1e-5
    );
}

#[test]
fn checkpoint_material_decode_failure_is_atomic() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut member = bilinear_member();
    let mut previous = [0.0; 12];
    axial(&mut member, 6.0, &mut previous, true, &ctx);
    let checkpoint = member.serialize_checkpoint();
    let mut invalid: FiberBeamCheckpoint = bincode::deserialize(&checkpoint).unwrap();
    invalid.trial_disp[6] = 123.0;
    invalid.gauss_points[1][2] = vec![255];
    assert!(member
        .deserialize_checkpoint(&bincode::serialize(&invalid).unwrap())
        .is_err());
    assert_eq!(member.serialize_checkpoint(), checkpoint);
}

#[test]
fn overflow_in_uncancelled_residual_scale_is_rejected_without_committing() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut member = bilinear_member();
    let h = member.hinge.as_mut().unwrap();
    for (i, j, sign) in [(0, 0, 1.0), (0, 6, -1.0), (6, 0, -1.0), (6, 6, 1.0)] {
        h.k_el.set(i, j, sign * f64::MAX);
    }
    let mut u = [0.0; 12];
    u[0] = 1.0;
    u[6] = 1.0;
    let failed = member
        .update_state(
            &LocalVec {
                data: u.into_iter().collect(),
            },
            true,
            &ctx,
        )
        .unwrap_err();
    assert!(failed.to_string().contains("非有限"), "{failed}");
    assert_eq!(member.trial_disp, [0.0; 12]);
    assert_eq!(member.committed_disp, [0.0; 12]);
}

fn small_member(material: Box<dyn UniaxialMaterial>, e: f64, torsion: bool) -> FiberBeam {
    let sections = std::array::from_fn(|_| {
        let fibers = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
            .into_iter()
            .map(|(y, z)| Fiber {
                y,
                z,
                area: 1.0,
                material: 0,
            })
            .collect();
        (
            FiberSection { fibers },
            (0..4).map(|_| material.clone_box()).collect(),
        )
    });
    FiberBeam::from_raw_parts(
        [NodeId(0), NodeId(1)],
        8.0,
        crate::transform::LocalFrame::from_nodes([0.0; 3], [8.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            .unwrap(),
        SectionMassProperties::default(),
        e,
        if torsion { 1e9 } else { 0.0 },
        4.0,
        4.0,
        4.0,
        0.0,
        0.0,
        if torsion { 8.0 } else { 0.0 },
        1.0,
        sections,
    )
}

#[test]
fn committed_bilinear_and_concrete_extrema_preserve_exact_trial_caches_on_all_restore_paths() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for (mut material, e, strain) in [
        (
            Box::new(Bilinear::new(128.0, 1.0, 0.5)) as Box<dyn UniaxialMaterial>,
            128.0,
            1.0 / 32.0,
        ),
        (
            Box::new(sepika_material::ConcreteCyclic::newrc_with_initial_tangent(
                30.0, 0.01, 2.0, 2500.0, 25000.0,
            )) as Box<dyn UniaxialMaterial>,
            25000.0,
            -0.001,
        ),
    ] {
        let (stress, et) = material.trial(strain);
        let delta = 8.0 * stress / e + 2.0 * (strain - stress / e);
        let mut member = small_member(material.clone_box(), e, false);
        let mut previous = [0.0; 12];
        axial(&mut member, delta, &mut previous, true, &ctx);
        let checkpoint = member.serialize_checkpoint();
        let snapshot = member.snapshot_state();
        let force = member.internal_force(&ctx).data;
        let tangent = member.tangent_stiffness(&ctx).data;
        assert_relative_eq!(force[6], 4.0 * stress, epsilon = 1e-8);
        let expected = 4.0 / (8.0 / e + 2.0 * (1.0 / et - 1.0 / e));
        assert_relative_eq!(tangent[6 * 12 + 6], expected, epsilon = 1e-8);
        if e == 128.0 {
            assert_eq!(delta, 23.0 / 128.0);
            assert_relative_eq!(expected, 51.2, epsilon = 1e-12);
        }
        let bad = LocalVec {
            data: smallvec::smallvec![f64::NAN;12],
        };
        assert!(member.update_state(&bad, true, &ctx).is_err());
        assert_eq!(member.serialize_checkpoint(), checkpoint);
        assert_eq!(member.internal_force(&ctx).data, force);
        assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
        axial(&mut member, delta * 0.9, &mut previous, false, &ctx);
        member.restore_state(snapshot.as_ref());
        assert_eq!(member.serialize_checkpoint(), checkpoint);
        assert_eq!(member.internal_force(&ctx).data, force);
        assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
        member.deserialize_checkpoint(&checkpoint).unwrap();
        assert_eq!(member.serialize_checkpoint(), checkpoint);
        assert_eq!(member.internal_force(&ctx).data, force);
        assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
        member.revert_state();
        assert_eq!(member.serialize_checkpoint(), checkpoint);
        assert_eq!(member.internal_force(&ctx).data, force);
        assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
    }
}

#[test]
fn finite_torsion_rotation_overflow_is_rejected_before_commit_with_exact_rollback() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut member = small_member(Box::new(Bilinear::new(128.0, 1.0, 0.5)), 128.0, true);
    let mut previous = [0.0; 12];
    axial(&mut member, 23.0 / 128.0, &mut previous, true, &ctx);
    assert_eq!(member.torsion_stiffness(), Some(1e9));
    let checkpoint = member.serialize_checkpoint();
    let force = member.internal_force(&ctx).data;
    let tangent = member.tangent_stiffness(&ctx).data;
    let mut du = LocalVec {
        data: smallvec::smallvec![0.0;12],
    };
    du.data[3] = 1e300;
    assert!(member.update_state(&du, true, &ctx).is_err());
    assert_eq!(member.serialize_checkpoint(), checkpoint);
    assert_eq!(member.internal_force(&ctx).data, force);
    assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
}

#[test]
fn original_rc_factory_combined_point_two_mm_trial_reaches_material_equilibrium() {
    let mut model = super::tests::rc_fiber_model();
    model.elements[0].plastic_zone = Some(250.0);
    let ctx = Ctx { model: &model };
    let mut member = crate::factory::build_nonlinear_behavior(
        &model.elements[0],
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
        None,
    );
    let committed = member.snapshot_state();
    let mut target = [0.0; 12];
    target[6] = 0.2;
    target[7] = 0.2;
    target[8] = 0.2;
    let mut previous = [0.0; 12];
    set_displacement(member.as_mut(), target, &mut previous, false, &ctx);
    let force = member.internal_force(&ctx);
    let tangent = member.tangent_stiffness(&ctx);
    assert!(force.data.iter().all(|value| value.is_finite()));
    assert!(tangent.data.iter().all(|value| value.is_finite()));
    let sections = member.fiber_section_states().unwrap();
    assert!(sections
        .iter()
        .flat_map(|section| &section.fibers)
        .any(|fiber| fiber.material == 0 && fiber.strain > 8e-5));
    let snapshot = member.snapshot_state();
    let state = snapshot.downcast_ref::<FiberBeamSnapshot>().unwrap();
    let axis = crate::transform::LocalFrame::from_nodes(
        model.nodes[0].coord,
        model.nodes[1].coord,
        model.elements[0].local_axis.ref_vector,
    )
    .unwrap();
    let local_force = axis.rotate_to_local(&std::array::from_fn(|i| force.data[i]));
    let mut max_residual_n = 0.0_f64;
    let mut max_residual_nmm = 0.0_f64;
    for (end, section) in sections.iter().enumerate() {
        let mut section_force = [0.0; 3];
        for (i, fiber) in section.fibers.iter().enumerate() {
            let stress = state.2[end][i].probe(fiber.strain).0;
            section_force[0] += stress * fiber.area;
            section_force[1] += stress * fiber.area * fiber.z;
            section_force[2] -= stress * fiber.area * fiber.y;
        }
        for component in 0..3 {
            let q =
                if end == 0 { -1.0 } else { 1.0 } * local_force[HINGE_SLOTS[end * 3 + component]];
            assert_relative_eq!(
                q,
                section_force[component],
                epsilon = 1e-5,
                max_relative = 1e-10
            );
            let residual = (q - section_force[component]).abs();
            if component == 0 {
                max_residual_n = max_residual_n.max(residual)
            } else {
                max_residual_nmm = max_residual_nmm.max(residual)
            }
        }
    }
    println!("RC original .2mm: eta={:?}, max_R_N={max_residual_n:.16e}, max_R_Nmm={max_residual_nmm:.16e}",&state.5[..6]);
    for j in [6, 7, 8] {
        let mut forces = Vec::new();
        let step = 1e-6;
        for sign in [-1.0, 1.0] {
            member.restore_state(committed.as_ref());
            previous = [0.0; 12];
            let mut u = target;
            u[j] += sign * step;
            set_displacement(member.as_mut(), u, &mut previous, false, &ctx);
            forces.push(member.internal_force(&ctx));
        }
        for i in 0..12 {
            let fd = (forces[1].data[i] - forces[0].data[i]) / (2.0 * step);
            assert_relative_eq!(tangent.get(i, j), fd, epsilon = 0.05, max_relative = 2e-6);
        }
    }
}

#[test]
fn usual_factory_fiber_and_ms_failed_trials_keep_exact_committed_and_trial_states() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let mut model = super::tests::rc_fiber_model();
        model.elements[0].kind = kind;
        model.elements[0].plastic_zone = Some(250.0);
        let ctx = Ctx { model: &model };
        let mut member = crate::factory::build_nonlinear_behavior(
            &model.elements[0],
            &model,
            StrengthBasis::Nominal,
            AnalysisKind::Incremental,
            None,
        );
        let mut previous = [0.0; 12];
        axial(member.as_mut(), 0.001, &mut previous, true, &ctx);
        let committed = member.serialize_checkpoint();
        axial(member.as_mut(), 0.002, &mut previous, false, &ctx);
        let trial = member.serialize_checkpoint();
        let force = member.internal_force(&ctx).data;
        let tangent = member.tangent_stiffness(&ctx).data;
        for increment in [
            vec![0.0; 11],
            vec![f64::NAN; 12],
            vec![f64::INFINITY; 12],
            vec![1e300; 12],
        ] {
            let error = member
                .update_state(
                    &LocalVec {
                        data: increment.into(),
                    },
                    true,
                    &ctx,
                )
                .unwrap_err();
            assert!(error.to_string().contains("Fiber/MS"), "{error}");
            assert_eq!(member.serialize_checkpoint(), trial);
            assert_eq!(member.internal_force(&ctx).data, force);
            assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
        }
        member.revert_state();
        assert_eq!(member.serialize_checkpoint(), committed);
        previous = [0.0; 12];
        previous[6] = 0.001;
        axial(member.as_mut(), 0.002, &mut previous, false, &ctx);
        assert_eq!(member.serialize_checkpoint(), trial);
    }
}

#[test]
fn distributed_double_pin_removes_torsion_gauge_and_condenses_all_bending_releases() {
    let mut model = super::tests::build_test_model(Some(78846.15));
    model.sections[0].j = 1e6;
    model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
    model.elements[0].end_cond = [EndCondition::Pinned; 2];
    let ctx = Ctx { model: &model };
    let mut member = FiberBeam::new(
        &model.elements[0],
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    );
    assert!(member.hinge.is_none());
    assert!(member.torsion_stiffness().unwrap() > 0.0);
    let zero = LocalVec {
        data: smallvec::smallvec![0.0;12],
    };
    member.update_state(&zero, false, &ctx).unwrap();
    let k = member.tangent_stiffness(&ctx);
    for slot in [1, 2, 3, 4, 5, 7, 8, 9, 10, 11] {
        assert!(
            k.get(slot, slot).abs() < 1e-6,
            "slot={slot}, K={}",
            k.get(slot, slot)
        );
    }
    let mut du = zero;
    du.data[7] = 1.0;
    member.update_state(&du, false, &ctx).unwrap();
    assert!(member
        .internal_force(&ctx)
        .data
        .iter()
        .all(|force| force.abs() < 1e-5));
}

#[test]
fn distributed_zero_release_residual_with_singular_tangent_is_error_and_restores_trial_and_cache() {
    let mut model = super::tests::build_test_model(Some(78846.15));
    model.sections[0].j = 1e6;
    model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
    model.elements[0].end_cond = [EndCondition::Pinned; 2];
    let ctx = Ctx { model: &model };
    let mut member = FiberBeam::new(
        &model.elements[0],
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    );
    for gp in &mut member.gauss_points {
        gp.section.fibers = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
            .into_iter()
            .map(|(y, z)| Fiber {
                y,
                z,
                area: 1.0,
                material: 2,
            })
            .collect();
        gp.mats = (0..4)
            .map(|_| Box::new(Bilinear::new(205000.0, 200.0, 0.0)) as Box<dyn UniaxialMaterial>)
            .collect();
        gp.trial_stress = vec![0.0; 4];
        gp.trial_et = vec![205000.0; 4];
    }
    let mut previous = [0.0; 12];
    axial(&mut member, 0.1, &mut previous, true, &ctx);
    let committed = member.serialize_checkpoint();
    axial(&mut member, 0.2, &mut previous, false, &ctx);
    let trial = member.serialize_checkpoint();
    let snapshot = member.snapshot_state();
    let force = member.internal_force(&ctx).data;
    let tangent = member.tangent_stiffness(&ctx).data;
    member.trial_disp[6] = 6.0;
    let u = member.elem_disp(&member.flex_disp());
    member.update_section_trial(&u);
    let plastic_force = member.elem_internal_force(&u);
    for release in &member.releases {
        assert_eq!(plastic_force[release.dof], 0.0);
    }
    member.restore_state(snapshot.as_ref());
    let mut du = LocalVec {
        data: smallvec::smallvec![0.0;12],
    };
    du.data[6] = 5.8;
    let error = member
        .update_state(&du, true, &ctx)
        .unwrap_err()
        .to_string();
    assert!(error.contains("分布断面の外側端部接続釣合い"), "{error}");
    assert_eq!(member.serialize_checkpoint(), trial);
    assert_eq!(member.internal_force(&ctx).data, force);
    assert_eq!(member.tangent_stiffness(&ctx).data, tangent);
    member.revert_state();
    assert_eq!(member.serialize_checkpoint(), committed);
    previous = [0.0; 12];
    previous[6] = 0.1;
    axial(&mut member, 0.2, &mut previous, false, &ctx);
    assert_eq!(member.serialize_checkpoint(), trial);
}

#[test]
fn similar_sections_retain_independent_coupled_elastic_solution_and_failed_trial_state() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for scale in [1.0, 1e-4, 1e-8, 1e4] {
        let mut member = similar_bilinear_member(scale, 3.0, 7.0);
        let mut previous = [0.0; 12];
        assert_relative_eq!(
            axial(&mut member, 1.5 * scale, &mut previous, true, &ctx),
            100000.0 * scale.powi(2),
            max_relative = 1e-9
        );
        let h = member.hinge.as_ref().unwrap();
        for end in 0..2 {
            for (a, expected) in [0.00060625, -8.75e-6 / scale, 15e-6 / scale]
                .into_iter()
                .enumerate()
            {
                assert_relative_eq!(h.trial_eta[end * 3 + a], expected, max_relative = 1e-9);
            }
        }
        assert_relative_eq!(
            member.tangent_stiffness(&ctx).get(6, 6),
            200000.0 * 1000.0 / 3000.0 * scale,
            max_relative = 1e-9
        );
        let before = member.serialize_checkpoint();
        for invalid in [0.0, f64::NAN] {
            member.hinge.as_mut().unwrap().d0[0] = [[invalid; 3]; 3];
            let error = member
                .update_state(
                    &LocalVec {
                        data: smallvec::smallvec![0.0; 12],
                    },
                    false,
                    &ctx,
                )
                .unwrap_err();
            assert!(error.to_string().contains("初期断面剛性"), "{error}");
            assert!(error.to_string().contains("NodeId(0)"), "{error}");
            assert_eq!(member.serialize_checkpoint(), before);
        }
    }
}

#[test]
fn similar_h_shape_fibers_match_independent_midpoint_area_and_biaxial_inertia() {
    use sepika_core::section_shape::SectionShape;
    for scale in [1.0_f64, 1e-4, 1e-8, 1e4] {
        let mut model = super::tests::build_test_model(Some(80000.0));
        model.nodes[1].coord[0] *= scale;
        let section = &mut model.sections[0];
        section.shape = Some(SectionShape::SteelH {
            height: 256.0 * scale,
            width: 128.0 * scale,
            web_thick: 16.0 * scale,
            flange_thick: 16.0 * scale,
            root_r: Some(0.0),
        });
        section.width = 128.0 * scale;
        section.depth = 256.0 * scale;
        section.area = 7680.0 * scale.powi(2);
        section.iy *= scale.powi(4);
        section.iz *= scale.powi(4);
        let beam = FiberBeam::try_new(
            &model.elements[0],
            &model,
            StrengthBasis::Nominal,
            AnalysisKind::TimeHistory,
        )
        .unwrap();
        for point in &beam.gauss_points {
            // 各矩形の中点則：I_disc = I_exact − Σ A_cell·寸法_cell²/12。
            let expected = [7680.0, 5505024.0, 73891840.0];
            for (a, reference) in expected.into_iter().enumerate() {
                let power = if a == 0 { 2 } else { 4 };
                assert_relative_eq!(
                    point.cached_stiff[a][a] / (205000.0 * scale.powi(power)),
                    reference,
                    max_relative = 1e-12
                );
            }
        }
    }
}

#[test]
fn similar_members_biaxial_elastic_moments_match_independent_pure_bending_solution() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for scale in [1.0_f64, 1e-4, 1e-8, 1e4] {
        let mut member = similar_bilinear_member(scale, 0.0, 0.0);
        let mut u = [0.0; 12];
        u[4] = 1e-5;
        u[10] = -1e-5;
        u[5] = 2e-5;
        u[11] = -2e-5;
        set_displacement(&mut member, u, &mut [0.0; 12], false, &ctx);
        let force = member.internal_force(&ctx);
        for (slot, inertia, rotation) in [(4, 400000.0, 1e-5), (5, 100000.0, 2e-5)] {
            let expected = 2.0 * 200000.0 * inertia / 3000.0 * rotation * scale.powi(3);
            assert_relative_eq!(force.data[slot], expected, max_relative = 1e-9);
            assert_relative_eq!(force.data[slot + 6], -expected, max_relative = 1e-9);
        }
    }
}
