use super::*;
use approx::assert_relative_eq;
use sepika_core::model::Model;
use sepika_material::uniaxial::Bilinear;

fn distributed_member() -> FiberBeam {
    let mut model = super::tests::build_test_model(Some(80000.0));
    model.nodes[1].coord = [1000.0, 0.0, 0.0];
    model.sections[0].as_y = 0.0;
    model.sections[0].as_z = 0.0;
    let member = FiberBeam::try_new(
        &model.elements[0],
        &model,
        crate::factory::StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    )
    .unwrap();
    assert!(member.hinge.is_none());
    assert_eq!(member.gauss_points.len(), 2);
    member
}

fn uniform_strain_displacement(length_mm: f64, eta: [f64; 3]) -> [f64; 12] {
    let mut u = [0.0; 12];
    u[6] = length_mm * eta[0];
    u[7] = eta[2] * length_mm * length_mm / 2.0;
    u[8] = -eta[1] * length_mm * length_mm / 2.0;
    u[10] = eta[1] * length_mm;
    u[11] = eta[2] * length_mm;
    u
}

fn increment(u: [f64; 12]) -> LocalVec {
    LocalVec {
        data: u.into_iter().collect(),
    }
}

#[test]
fn distributed_pure_bending_has_physical_fiber_strain_at_every_gauss_point_and_public_output() {
    for plane in [1, 2] {
        for curvature in [-1e-7, 1e-7] {
            for reverse in [false, true] {
                for rotate in [false, true] {
                    let mut model = super::tests::build_test_model(Some(80000.0));
                    model.sections[0].as_y = 5000.0;
                    model.sections[0].as_z = 5000.0;
                    model.elements[0].local_axis.ref_vector = if rotate {
                        [-1.0, 0.0, 0.0]
                    } else {
                        [0.0, 1.0, 0.0]
                    };
                    let mut u = [0.0; 12];
                    for end in 0..2 {
                        let x = if (end == 1) != reverse { 3000.0 } else { 0.0 };
                        model.nodes[end].coord = if rotate { [0.0, x, 0.0] } else { [x, 0.0, 0.0] };
                        if plane == 2 {
                            u[6 * end + if rotate { 0 } else { 1 }] =
                                (if rotate { -1.0 } else { 1.0 }) * curvature * x * x / 2.0;
                            u[6 * end + 5] = curvature * x;
                        } else {
                            u[6 * end + 2] = -curvature * x * x / 2.0;
                            u[6 * end + if rotate { 3 } else { 4 }] =
                                (if rotate { -1.0 } else { 1.0 }) * curvature * x;
                        }
                    }
                    let ctx = Ctx { model: &model };
                    let mut member = FiberBeam::try_new(
                        &model.elements[0],
                        &model,
                        crate::factory::StrengthBasis::Nominal,
                        AnalysisKind::Incremental,
                    )
                    .unwrap();
                    assert!(member.hinge.is_none());
                    member.update_state(&increment(u), false, &ctx).unwrap();
                    let states = member.fiber_section_states().unwrap();
                    assert_eq!(states.len(), member.gauss_points.len());
                    let local_curvature =
                        curvature * if plane == 1 && reverse { -1.0 } else { 1.0 };
                    let mut max_tension = 0.0_f64;
                    let mut max_compression = 0.0_f64;
                    for (gp, state) in member.gauss_points.iter().zip(states) {
                        assert_eq!(state.xi, gp.xi);
                        for (index, fiber) in state.fibers.iter().enumerate() {
                            let expected = if plane == 2 {
                                -local_curvature * fiber.y
                            } else {
                                local_curvature * fiber.z
                            };
                            assert_relative_eq!(fiber.strain, expected, epsilon = 1e-14);
                            assert_relative_eq!(
                                gp.trial_stress[index],
                                205000.0 * expected,
                                epsilon = 1e-8
                            );
                            max_tension = max_tension.max(expected);
                            max_compression = max_compression.max(-expected);
                        }
                    }
                    let probe = member.ductility_probe().unwrap();
                    assert_relative_eq!(probe.curvature, curvature.abs(), epsilon = 1e-16);
                    assert_relative_eq!(probe.max_tension_strain, max_tension, epsilon = 1e-14);
                    assert_relative_eq!(
                        probe.max_compression_strain,
                        max_compression,
                        epsilon = 1e-14
                    );
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
struct CompressionYield;

impl UniaxialMaterial for CompressionYield {
    fn trial(&mut self, strain: f64) -> (f64, f64) {
        self.probe(strain)
    }
    fn probe(&self, strain: f64) -> (f64, f64) {
        if strain < -0.00005 {
            (-10.0 + 20000.0 * (strain + 0.00005), 20000.0)
        } else {
            (200000.0 * strain, 200000.0)
        }
    }
    fn commit(&mut self) {}
    fn revert(&mut self) {}
    fn clone_box(&self) -> Box<dyn UniaxialMaterial> {
        Box::new(self.clone())
    }
    fn serialize_state(&self) -> Vec<u8> {
        vec![]
    }
    fn deserialize_state(
        &mut self,
        data: &[u8],
    ) -> Result<(), sepika_material::uniaxial::MaterialStateError> {
        if data.is_empty() {
            Ok(())
        } else {
            Err(sepika_material::uniaxial::MaterialStateError::Decode(
                "人工材料の状態は空です".into(),
            ))
        }
    }
}

fn asymmetric_member(compression_yield: bool) -> FiberBeam {
    let mut member = distributed_member();
    for gp in &mut member.gauss_points {
        gp.section.fibers = [
            (100.0, -10.0, 0.5),
            (100.0, 10.0, 0.5),
            (-50.0, -10.0, 1.0),
            (-50.0, 10.0, 1.0),
        ]
        .into_iter()
        .map(|(y, z, area)| Fiber {
            y,
            z,
            area,
            material: 0,
        })
        .collect();
        gp.mats = (0..4)
            .map(|index| {
                if compression_yield && index < 2 {
                    Box::new(CompressionYield) as Box<dyn UniaxialMaterial>
                } else {
                    Box::new(Bilinear::new(200000.0, 20.0, 0.1)) as Box<dyn UniaxialMaterial>
                }
            })
            .collect();
        gp.trial_stress = vec![0.0; 4];
        gp.trial_et = vec![200000.0; 4];
        gp.refresh_response();
        gp.committed_response = gp.response();
    }
    member
}

#[test]
fn asymmetric_compression_and_geometry_reproduce_independent_boundary_tractions_and_work_contraction(
) {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for (curvature, n, mz, contraction) in [(1e-6, 9.0, 2100.0, 2.1), (-1e-6, 0.0, -3000.0, 3.0)] {
        let mut member = asymmetric_member(true);
        let u = uniform_strain_displacement(member.length, [0.0, 0.0, curvature]);
        member.update_state(&increment(u), false, &ctx).unwrap();
        let force = member.internal_force(&ctx);
        for i in 0..12 {
            let expected = match i {
                0 => -n,
                6 => n,
                5 => -mz,
                11 => mz,
                _ => 0.0,
            };
            assert_relative_eq!(force.data[i], expected, epsilon = 1e-8);
        }
        for gp in &member.gauss_points {
            assert_relative_eq!(
                gp.trial_stress[0],
                if curvature > 0.0 { -11.0 } else { 20.0 },
                epsilon = 1e-10
            );
            assert_relative_eq!(
                gp.trial_stress[2],
                if curvature > 0.0 { 10.0 } else { -10.0 },
                epsilon = 1e-10
            );
        }
        // 終点の仕事共役積であり、非線形経路の累積仕事・保存エネルギーではない。
        assert_relative_eq!(
            (0..12).map(|i| force.data[i] * u[i]).sum::<f64>(),
            contraction,
            epsilon = 1e-10
        );
        let probe = member.ductility_probe().unwrap();
        assert_relative_eq!(
            probe.max_tension_strain,
            if curvature > 0.0 { 0.00005 } else { 0.0001 },
            epsilon = 1e-14
        );
        assert_relative_eq!(
            probe.max_compression_strain,
            if curvature > 0.0 { 0.0001 } else { 0.00005 },
            epsilon = 1e-14
        );
    }
}

#[test]
fn distributed_history_work_tangent_and_rollback_match_independent_uniaxial_histories() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    let mut member = asymmetric_member(false);
    let mut references = std::array::from_fn::<_, 4, _>(|_| Bilinear::new(200000.0, 20.0, 0.1));
    let mut previous_u = [0.0; 12];
    let mut previous_force = [0.0; 12];
    let mut previous_strains = [0.0; 4];
    let mut previous_stress = [0.0; 4];
    let mut nodal_work = 0.0;
    let mut fiber_work = 0.0;
    for factor in [0.2, 1.0, 0.4, -0.8, 0.6] {
        let eta = [factor * 0.00004, factor * 0.000002, factor * 0.000003];
        let strains = [
            eta[0] - 10.0 * eta[1] - 100.0 * eta[2],
            eta[0] + 10.0 * eta[1] - 100.0 * eta[2],
            eta[0] - 10.0 * eta[1] + 50.0 * eta[2],
            eta[0] + 10.0 * eta[1] + 50.0 * eta[2],
        ];
        let stress: [f64; 4] = std::array::from_fn(|i| references[i].trial(strains[i]).0);
        let n = 0.5 * stress[0] + 0.5 * stress[1] + stress[2] + stress[3];
        let my = -5.0 * stress[0] + 5.0 * stress[1] - 10.0 * stress[2] + 10.0 * stress[3];
        let mz = -50.0 * stress[0] - 50.0 * stress[1] + 50.0 * stress[2] + 50.0 * stress[3];
        let committed = member.snapshot_state();
        let u = uniform_strain_displacement(member.length, eta);
        let du = std::array::from_fn(|i| u[i] - previous_u[i]);
        member.update_state(&increment(du), false, &ctx).unwrap();
        let force = member.internal_force(&ctx);
        for (slot, expected) in [(0, -n), (6, n), (4, -my), (10, my), (5, -mz), (11, mz)] {
            assert_relative_eq!(force.data[slot], expected, epsilon = 1e-8);
        }
        for state in member.fiber_section_states().unwrap() {
            for (index, fiber) in state.fibers.iter().enumerate() {
                assert_relative_eq!(fiber.strain, strains[index], epsilon = 1e-14);
            }
        }
        nodal_work += (0..12)
            .map(|i| 0.5 * (force.data[i] + previous_force[i]) * (u[i] - previous_u[i]))
            .sum::<f64>();
        fiber_work += member.length
            * (0..4)
                .map(|i| {
                    let area = if i < 2 { 0.5 } else { 1.0 };
                    0.5 * area
                        * (stress[i] + previous_stress[i])
                        * (strains[i] - previous_strains[i])
                })
                .sum::<f64>();
        assert_relative_eq!(nodal_work, fiber_work, epsilon = 1e-10);
        let trial = member.snapshot_state();
        let tangent = member.tangent_stiffness(&ctx);
        for col in [0, 1, 2, 4, 5, 6, 7, 8, 10, 11] {
            let h = if col % 6 < 3 { 1e-6 } else { 1e-9 };
            let mut forces = [[0.0; 12]; 2];
            for (side, sign) in [-1.0, 1.0].into_iter().enumerate() {
                member.restore_state(committed.as_ref());
                let mut perturbed = du;
                perturbed[col] += sign * h;
                member
                    .update_state(&increment(perturbed), false, &ctx)
                    .unwrap();
                forces[side] = std::array::from_fn(|i| member.internal_force(&ctx).data[i]);
            }
            for row in 0..12 {
                let fd = (forces[1][row] - forces[0][row]) / (2.0 * h);
                assert_relative_eq!(
                    tangent.get(row, col),
                    fd,
                    epsilon = 1e-3,
                    max_relative = 1e-7
                );
            }
        }
        member.restore_state(trial.as_ref());
        member.commit_state();
        for reference in &mut references {
            reference.commit();
        }
        let checkpoint = member.serialize_checkpoint();
        assert!(member
            .update_state(&increment([f64::NAN; 12]), true, &ctx)
            .is_err());
        assert_eq!(member.serialize_checkpoint(), checkpoint);
        member
            .update_state(
                &increment(std::array::from_fn(|i| -0.01 * u[i])),
                false,
                &ctx,
            )
            .unwrap();
        member.revert_state();
        assert_eq!(member.serialize_checkpoint(), checkpoint);
        member
            .update_state(&increment([0.0; 12]), false, &ctx)
            .unwrap();
        for i in 0..12 {
            assert_relative_eq!(
                member.internal_force(&ctx).data[i],
                force.data[i],
                epsilon = 1e-8
            );
        }
        previous_u = u;
        previous_force = std::array::from_fn(|i| force.data[i]);
        previous_strains = strains;
        previous_stress = stress;
    }
}
