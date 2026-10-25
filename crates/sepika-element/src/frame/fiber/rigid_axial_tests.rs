use super::*;
use crate::factory::StrengthBasis;
use approx::assert_relative_eq;
use sepika_core::model::{ElementKind, EndCondition, PropertyBasis, RigidZone};

fn axial_trial(member: &mut dyn ElementBehavior, delta: f64, previous: &mut f64, ctx: &Ctx) -> f64 {
    let mut du = LocalVec {
        data: smallvec::smallvec![0.0; 12],
    };
    du.data[6] = delta - *previous;
    member.update_state(&du, false, ctx).unwrap();
    *previous = delta;
    member.internal_force(ctx).data[6]
}

#[test]
fn usual_factory_rigid_axial_yield_unloading_and_work_use_node_length_and_actual_section() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        for end in [
            EndCondition::Fixed,
            EndCondition::Pinned,
            EndCondition::SemiRigid { k_theta: 1e9 },
        ] {
            for sign in [-1.0, 1.0] {
                let mut model = super::tests::build_test_model(Some(80000.0));
                model.elements[0].kind = kind;
                model.elements[0].end_cond = [end, end];
                model.elements[0].rigid_zone = RigidZone {
                    length_i: 100.0,
                    length_j: 50.0,
                    face_i: Some(100.0),
                    face_j: Some(50.0),
                    ..Default::default()
                };
                model.elements[0].plastic_zone = Some(2000.0);
                model.sections[0].j = 1e6;
                model.sections[0].property_basis.j = PropertyBasis::Supplied;
                model.materials[0].young = 200000.0;
                model.materials[0].fy = Some(200.0);
                model.materials[0].density = 7.85e-9;
                let ctx = Ctx { model: &model };
                let mut member = crate::factory::build_nonlinear_behavior(
                    &model.elements[0],
                    &model,
                    StrengthBasis::Nominal,
                    AnalysisKind::Incremental,
                    None,
                );
                let initial = member.tangent_stiffness(&ctx).get(6, 6);
                assert_relative_eq!(initial, 200000.0 * 20000.0 / 3000.0, epsilon = 1e-6);
                let initial_state = member.snapshot_state();
                let initial_state = initial_state.downcast_ref::<FiberBeamSnapshot>().unwrap();
                for point in &initial_state.7 {
                    assert_relative_eq!(
                        point.0.stiffness[0][0],
                        200000.0 * 20000.0,
                        max_relative = 1e-12
                    );
                }
                let mass_options: &[MassOption] = if matches!(end, EndCondition::Pinned) {
                    &[MassOption::Lumped]
                } else {
                    &[MassOption::Lumped, MassOption::Consistent]
                };
                for &option in mass_options {
                    let mass = member.mass_matrix(option);
                    for direction in 0..3 {
                        let total = mass.get(direction, direction)
                            + mass.get(direction, direction + 6)
                            + mass.get(direction + 6, direction)
                            + mass.get(direction + 6, direction + 6);
                        assert_relative_eq!(total, 7.85e-9 * 20000.0 * 3000.0, epsilon = 1e-12);
                    }
                }
                let mut previous = 0.0;
                let mut reference = MenegottoPinto::new(200000.0, 200.0);
                let lp = 1282.5;
                let strain = sign * 0.005;
                let (stress, et) = reference.trial(strain);
                let delta = 3000.0 * stress / 200000.0 + 2.0 * lp * (strain - stress / 200000.0);
                let peak = axial_trial(member.as_mut(), delta, &mut previous, &ctx);
                assert_relative_eq!(peak, 20000.0 * stress, epsilon = 1e-4);
                assert_relative_eq!(
                    member.tangent_stiffness(&ctx).get(6, 6),
                    20000.0 / (3000.0 / 200000.0 + 2.0 * lp * (1.0 / et - 1.0 / 200000.0)),
                    epsilon = 1e-3,
                    max_relative = 1e-8
                );
                assert!(member.tangent_stiffness(&ctx).get(6, 6) < 0.1 * initial);
                let state = member.snapshot_state();
                let state = state.downcast_ref::<FiberBeamSnapshot>().unwrap();
                for point in &state.7 {
                    assert_relative_eq!(point.0.stiffness[0][0], 20000.0 * et, epsilon = 1e-3);
                }
                for section in member.fiber_section_states().unwrap() {
                    assert_relative_eq!(
                        section.fibers.iter().map(|f| f.area).sum::<f64>(),
                        20000.0,
                        epsilon = 1e-8
                    );
                    for fiber in section.fibers {
                        assert_relative_eq!(fiber.strain, strain, epsilon = 1e-11);
                    }
                }
                let elastic_work = peak * peak * 3000.0 / (200000.0 * 20000.0);
                let local_work = 2.0 * lp * 20000.0 * stress * strain;
                let subtracted_work = 2.0 * lp * 20000.0 * stress * stress / 200000.0;
                assert_relative_eq!(
                    peak * delta,
                    elastic_work + local_work - subtracted_work,
                    epsilon = 1e-4
                );
                member.commit_state();
                reference.commit();
                let checkpoint = member.serialize_checkpoint();
                let committed_delta = delta;
                let unload_strain = sign * 0.0048;
                let (unload_stress, unload_et) = reference.trial(unload_strain);
                let unload_delta = 3000.0 * unload_stress / 200000.0
                    + 2.0 * lp * (unload_strain - unload_stress / 200000.0);
                assert_relative_eq!(
                    axial_trial(member.as_mut(), unload_delta, &mut previous, &ctx),
                    20000.0 * unload_stress,
                    epsilon = 1e-4
                );
                assert_relative_eq!(
                    member.tangent_stiffness(&ctx).get(6, 6),
                    20000.0 / (3000.0 / 200000.0 + 2.0 * lp * (1.0 / unload_et - 1.0 / 200000.0)),
                    epsilon = 1e-3,
                    max_relative = 1e-8
                );
                member.revert_state();
                previous = committed_delta;
                assert_eq!(member.serialize_checkpoint(), checkpoint);
                let mut lower = 0.0;
                let mut upper = 0.005;
                for _ in 0..60 {
                    let middle = (lower + upper) / 2.0;
                    if sign * reference.probe(sign * middle).0 > 0.0 {
                        upper = middle;
                    } else {
                        lower = middle;
                    }
                }
                let residual_strain = sign * (lower + upper) / 2.0;
                let residual_delta = 2.0 * lp * residual_strain;
                assert!(residual_delta.abs() > 5.0);
                assert_relative_eq!(
                    axial_trial(member.as_mut(), residual_delta, &mut previous, &ctx),
                    0.0,
                    epsilon = 1e-3
                );
                println!("{kind:?} {end:?} sign={sign}: Lnode=3000 Lfree=2850 Lp={lp}, Npeak={peak:.12e}, residual_delta={residual_delta:.12e}");
            }
        }
    }
}
