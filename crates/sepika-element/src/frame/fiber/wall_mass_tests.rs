use super::*;
use crate::frame::beam::BeamElement;
use crate::frame::multi_spring::MultiSpringElement;
use crate::wall::side_column::{InPlaneReleasedColumn, ReleaseAxis};
use approx::assert_relative_eq;
use sepika_core::model::{EndCondition, GirderTorsionMode};

fn flex_velocity(e: &[f64; 12], beam: &BeamElement, x: f64) -> [f64; 6] {
    let (li, lj) = beam.rigid_lengths();
    let l = beam.length - li - lj;
    let mut v = [0.0; 6];
    v[0] = (1.0 - x) * e[0] + x * e[6];
    v[3] = (1.0 - x) * e[3] + x * e[9];
    for (translation, rotation, sign, inertia, shear) in [
        (1, 5, 1.0, beam.iz, beam.as_y),
        (2, 4, -1.0, beam.iy, beam.as_z),
    ] {
        let phi = 12.0 * beam.e * inertia / (beam.g * shear * l * l);
        let ri = sign * e[rotation];
        let rj = sign * e[rotation + 6];
        let curvature_gradient =
            6.0 * ((e[translation + 6] - e[translation]) / l - (ri + rj) / 2.0) / (1.0 + phi);
        let gamma = phi * curvature_gradient / 6.0;
        v[translation] = e[translation]
            + l * (ri * x + (rj - ri + curvature_gradient) * x * x / 2.0
                - curvature_gradient * x * x * x / 3.0
                + gamma * x);
        v[rotation] = sign * (ri + (rj - ri) * x + curvature_gradient * x * (1.0 - x));
    }
    v
}

fn rigid_velocity(u: &[f64; 12], end: usize, offset: f64) -> [f64; 6] {
    let mut v = std::array::from_fn(|i| u[end * 6 + i]);
    v[1] += offset * v[5];
    v[2] -= offset * v[4];
    v
}

fn independent_mass(beam: &BeamElement, normal: [f64; 2]) -> LocalMat {
    let raw = beam.local_stiffness_flex_raw();
    let (li, lj) = beam.rigid_lengths();
    let l = beam.length - li - lj;
    let basis: Vec<_> = (0..12)
        .map(|j| {
            let mut global = [0.0; 12];
            global[j] = 1.0;
            let node = beam.axis.rotate_to_local(&global);
            let mut flex = node;
            flex[1] += li * node[5];
            flex[2] -= li * node[4];
            flex[7] -= lj * node[11];
            flex[8] += lj * node[10];
            let e = super::wall_connection_tests::expanded_elastic_state(
                &raw,
                &flex,
                normal,
                beam.end_cond,
            )
            .0;
            (node, e)
        })
        .collect();
    let p = beam.mass_properties;
    let density = [
        p.mass_per_length,
        p.mass_per_length,
        p.mass_per_length,
        p.polar_inertia_per_length(),
        p.rotary_inertia_y_per_length,
        p.rotary_inertia_z_per_length,
    ];
    let gauss = [
        (-0.906179845938664, 0.236926885056189),
        (-0.538469310105683, 0.478628670499366),
        (0.0, 0.568888888888889),
        (0.538469310105683, 0.478628670499366),
        (0.906179845938664, 0.236926885056189),
    ];
    let mut mass = LocalMat::zeros(12);
    for (xi, weight) in gauss {
        let x = (xi + 1.0) / 2.0;
        for part in 0..3 {
            let length = [l, li, lj][part];
            let velocities: Vec<_> = basis
                .iter()
                .map(|(node, flex)| match part {
                    0 => flex_velocity(flex, beam, x),
                    1 => rigid_velocity(node, 0, li * x),
                    _ => rigid_velocity(node, 1, -lj * x),
                })
                .collect();
            for i in 0..12 {
                for j in 0..12 {
                    let kinetic = (0..6)
                        .map(|a| density[a] * velocities[i][a] * velocities[j][a])
                        .sum::<f64>();
                    mass.set(i, j, mass.get(i, j) + kinetic * weight * length / 2.0);
                }
            }
        }
    }
    mass
}

#[test]
fn beam_fiber_ms_wall_connection_matches_expanded_stiffness_forces_work_and_kinetic_energy() {
    for normal in [[1.0, 0.0], [0.0, 1.0], [0.6, 0.8], [-0.6, -0.8]] {
        for conditions in [
            [EndCondition::Fixed; 2],
            [EndCondition::Pinned, EndCondition::Fixed],
            [
                EndCondition::SemiRigid { k_theta: 0.0 },
                EndCondition::Fixed,
            ],
            [
                EndCondition::SemiRigid { k_theta: 1e9 },
                EndCondition::Fixed,
            ],
            [
                EndCondition::Fixed,
                EndCondition::SemiRigid { k_theta: 1e9 },
            ],
        ] {
            for rotated in [false, true] {
                for swapped in [false, true] {
                    for rigid in [false, true] {
                        let mut model = super::tests::build_release_model(conditions);
                        model.girder_torsion = GirderTorsionMode::Keep;
                        model.materials[0].density = 7.85e-9;
                        model.sections[0].j = 1e6;
                        model.sections[0].iy = 3.4e9;
                        model.sections[0].as_y = 150000.0;
                        model.sections[0].property_basis.j =
                            sepika_core::model::PropertyBasis::Supplied;
                        model.elements[0].rigid_zone.length_i = if !rigid {
                            0.0
                        } else if swapped {
                            150.0
                        } else {
                            100.0
                        };
                        model.elements[0].rigid_zone.length_j = if !rigid {
                            0.0
                        } else if swapped {
                            100.0
                        } else {
                            150.0
                        };
                        if swapped {
                            model.elements[0].nodes.reverse();
                        }
                        if rotated {
                            model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
                        }
                        let data = &model.elements[0];
                        let beam = BeamElement::try_new(data, &model).unwrap();
                        assert_eq!(beam.torsion_release, [false; 2]);
                        let normal_global = std::array::from_fn(|i| {
                            beam.axis.rot[1][i] * normal[0] + beam.axis.rot[2][i] * normal[1]
                        });
                        let expected_mass = independent_mass(&beam, normal);
                        let raw = beam.local_stiffness_flex_raw();
                        let (li, lj) = beam.rigid_lengths();
                        let expected_total = beam.mass_properties.total_mass(beam.length);
                        let mut fiber = FiberBeam::build_plastic_zone(
                            data,
                            &model,
                            300.0,
                            12,
                            20,
                            crate::factory::StrengthBasis::Nominal,
                            AnalysisKind::Incremental,
                        );
                        fiber.configure_wall_release(normal_global).unwrap();
                        let mut ms = MultiSpringElement::new(
                            data,
                            &model,
                            crate::factory::StrengthBasis::Nominal,
                            AnalysisKind::Incremental,
                        );
                        ms.configure_wall_release(normal_global).unwrap();
                        let column = InPlaneReleasedColumn::new(
                            beam.clone(),
                            ReleaseAxis::LocalDirection(normal),
                        );
                        let ctx = Ctx { model: &model };
                        let mut members: Vec<Box<dyn ElementBehavior>> =
                            vec![Box::new(column), Box::new(fiber), Box::new(ms)];
                        for member in &mut members {
                            member
                                .update_state(
                                    &LocalVec {
                                        data: smallvec::smallvec![0.0; 12],
                                    },
                                    false,
                                    &ctx,
                                )
                                .unwrap();
                            let k = member.tangent_stiffness(&ctx);
                            let mass = member.mass_matrix(MassOption::Consistent);
                            let lumped = member.mass_matrix(MassOption::Lumped);
                            for j in 0..12 {
                                let mut global = [0.0; 12];
                                global[j] = 1.0;
                                let local = beam.axis.rotate_to_local(&global);
                                let flex = crate::frame::rigid_arm::to_flex_disp(&local, li, lj);
                                let force = super::wall_connection_tests::expanded_elastic_state(
                                    &raw, &flex, normal, conditions,
                                )
                                .1;
                                let node_force =
                                    crate::frame::rigid_arm::to_node_force(&force, li, lj);
                                let expected = beam.axis.rotate_to_global(&node_force);
                                for (i, force) in expected.iter().enumerate() {
                                    assert_relative_eq!(
                                        k.get(i, j),
                                        *force,
                                        epsilon = 1e-4,
                                        max_relative = 1e-8
                                    );
                                    assert_relative_eq!(
                                        mass.get(i, j),
                                        expected_mass.get(i, j),
                                        epsilon = 1e-9,
                                        max_relative = 1e-9
                                    );
                                }
                            }
                            for a in 0..3 {
                                assert_relative_eq!(
                                    lumped.get(a, a) + lumped.get(a + 6, a + 6),
                                    expected_total,
                                    epsilon = 1e-12
                                );
                            }
                            for sign in [-1.0, 1.0] {
                                member.revert_state();
                                let u = [
                                    0.01, -0.03, 0.04, 1e-6, -2e-6, 3e-6, -0.02, 0.05, -0.01,
                                    -2e-6, 3e-6, -1e-6,
                                ]
                                .map(|v| sign * v);
                                member
                                    .update_state(
                                        &LocalVec {
                                            data: u.into_iter().collect(),
                                        },
                                        false,
                                        &ctx,
                                    )
                                    .unwrap();
                                let force = member.internal_force(&ctx);
                                let expected: [f64; 12] = std::array::from_fn(|i| {
                                    (0..12).map(|j| k.get(i, j) * u[j]).sum()
                                });
                                for i in 0..12 {
                                    assert_relative_eq!(
                                        force.data[i],
                                        expected[i],
                                        epsilon = 1e-5,
                                        max_relative = 1e-8
                                    );
                                }
                                let actual_work: f64 =
                                    u.iter().zip(&force.data).map(|(u, f)| u * f).sum();
                                let expected_work: f64 =
                                    u.iter().zip(expected).map(|(u, f)| u * f).sum();
                                assert_relative_eq!(
                                    actual_work,
                                    expected_work,
                                    epsilon = 1e-7,
                                    max_relative = 1e-9
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn both_free_torsion_ends_keep_static_response_but_refuse_inertial_gauge_removal() {
    for condition in [
        EndCondition::Pinned,
        EndCondition::SemiRigid { k_theta: 0.0 },
    ] {
        let mut model = super::tests::build_release_model([condition; 2]);
        model.girder_torsion = GirderTorsionMode::Keep;
        model.materials[0].density = 7.85e-9;
        model.sections[0].j = 1e6;
        model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
        let data = &model.elements[0];
        let beam = BeamElement::try_new(data, &model).unwrap();
        let mut fiber = FiberBeam::build_plastic_zone(
            data,
            &model,
            300.0,
            12,
            20,
            crate::factory::StrengthBasis::Nominal,
            AnalysisKind::Incremental,
        );
        fiber.configure_wall_release([0.0, 0.6, 0.8]).unwrap();
        let mut ms = MultiSpringElement::new(
            data,
            &model,
            crate::factory::StrengthBasis::Nominal,
            AnalysisKind::Incremental,
        );
        ms.configure_wall_release([0.0, 0.6, 0.8]).unwrap();
        let column = InPlaneReleasedColumn::new(beam, ReleaseAxis::LocalDirection([0.6, 0.8]));
        let ctx = Ctx { model: &model };
        let mut members: Vec<Box<dyn ElementBehavior>> =
            vec![Box::new(column), Box::new(fiber), Box::new(ms)];
        for member in &mut members {
            member
                .update_state(
                    &LocalVec {
                        data: smallvec::smallvec![0.0;12],
                    },
                    false,
                    &ctx,
                )
                .unwrap();
            assert!(member
                .tangent_stiffness(&ctx)
                .data
                .iter()
                .all(|v| v.is_finite()));
            assert!(member.mass_matrix(MassOption::Lumped).get(0, 0) > 0.0);
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                member.mass_matrix(MassOption::Consistent);
            }))
            .unwrap_err();
            let reason = error
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| error.downcast_ref::<&str>().copied())
                .unwrap();
            assert!(
                reason.contains("慣性を持つため整合質量ではゲージ除去できません"),
                "{reason}"
            );
        }
    }
}
