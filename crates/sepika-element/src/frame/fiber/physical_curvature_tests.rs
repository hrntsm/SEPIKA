use super::*;
use crate::factory::StrengthBasis;
use approx::assert_relative_eq;
use sepika_core::model::{AnalysisKind, ElementKind, Model};

#[test]
fn pure_bending_displacement_field_reproduces_physical_fiber_strain_at_both_ends() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    for plane in [1, 2] {
        for curvature in [-1e-7, 1e-7] {
            let mut member = super::constitutive_tests::bilinear_member();
            let l = 3000.0;
            let mut u = [0.0; 12];
            if plane == 2 {
                u[7] = curvature * l * l / 2.0;
                u[11] = curvature * l;
            } else {
                u[8] = -curvature * l * l / 2.0;
                u[10] = curvature * l;
            }
            member
                .update_state(
                    &LocalVec {
                        data: u.into_iter().collect(),
                    },
                    false,
                    &ctx,
                )
                .unwrap();
            let states = member.fiber_section_states().unwrap();
            let force = member.internal_force(&ctx);
            let moment = curvature * if plane == 2 { 2e10 } else { 8e10 };
            assert_relative_eq!(force.data[plane + 3], -moment, epsilon = 1e-6);
            assert_relative_eq!(force.data[plane + 9], moment, epsilon = 1e-6);
            for state in states {
                for fiber in state.fibers {
                    let strain = if plane == 2 {
                        -curvature * fiber.y
                    } else {
                        curvature * fiber.z
                    };
                    assert_relative_eq!(fiber.strain, strain, epsilon = 1e-13);
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct CompressionYield;

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

fn asymmetric_member() -> FiberBeam {
    let sections = std::array::from_fn(|_| {
        let fibers = [
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
            material: 2,
        })
        .collect();
        let mats = (0..4)
            .map(|i| {
                if i < 2 {
                    Box::new(CompressionYield) as Box<dyn UniaxialMaterial>
                } else {
                    Box::new(sepika_material::uniaxial::Bilinear::new(
                        200000.0, 100.0, 0.1,
                    )) as Box<dyn UniaxialMaterial>
                }
            })
            .collect();
        (FiberSection { fibers }, mats)
    });
    FiberBeam::from_raw_parts(
        [NodeId(0), NodeId(1)],
        1000.0,
        crate::transform::LocalFrame::from_nodes([0.0; 3], [1000.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            .unwrap(),
        SectionMassProperties::default(),
        200000.0,
        0.0,
        3.0,
        300.0,
        15000.0,
        0.0,
        0.0,
        0.0,
        100.0,
        sections,
    )
}

#[test]
fn asymmetric_material_and_geometry_keep_physical_compression_side_and_boundary_work() {
    let model = Model::default();
    let ctx = Ctx { model: &model };
    // 正曲率の上下応力は -11, +10 N/mm²、ΣAy²=15000 mm⁴。端回転差は L·Mz/EIz+2Lp(κz−Mz/EIz)。
    for (curvature, axial_i, axial_j, rotation_i, rotation_j, n, mz, work) in [
        (1e-6, 0.0015, 0.0135, -0.00038, 0.00038, 9.0, 2100.0, 1.704),
        (-1e-6, 0.0, 0.0, 0.0005, -0.0005, 0.0, -3000.0, 3.0),
    ] {
        let mut member = asymmetric_member();
        let mut u = [0.0; 12];
        u[0] = axial_i;
        u[6] = axial_j;
        u[5] = rotation_i;
        u[11] = rotation_j;
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
        for (slot, expected) in [(0, -n), (6, n), (5, -mz), (11, mz)] {
            assert_relative_eq!(force.data[slot], expected, epsilon = 1e-7);
        }
        for (end, state) in member
            .fiber_section_states()
            .unwrap()
            .into_iter()
            .enumerate()
        {
            for (index, fiber) in state.fibers.into_iter().enumerate() {
                assert_relative_eq!(fiber.strain, -curvature * fiber.y, epsilon = 1e-13);
                let expected = if fiber.y > 0.0 && curvature > 0.0 {
                    -11.0
                } else {
                    -200000.0 * curvature * fiber.y
                };
                assert_relative_eq!(
                    member.gauss_points[end].trial_stress[index],
                    expected,
                    epsilon = 1e-7
                );
            }
        }
        assert_relative_eq!(
            (0..12).map(|i| force.data[i] * u[i]).sum::<f64>(),
            work,
            epsilon = 1e-9
        );
    }
}

#[test]
fn usual_rc_fiber_and_ms_factories_keep_top_bottom_bending_under_end_swap_and_quarter_turn() {
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        for reverse in [false, true] {
            for rotate in [false, true] {
                for curvature in [-1e-8, 1e-8] {
                    let mut model = super::tests::rc_fiber_model();
                    let shape = SectionShape::RcBeamRect {
                        b: 500.0,
                        d: 500.0,
                        rebar: RcBeamRebar {
                            main_dia: 25.0,
                            top: vec![4],
                            bottom: vec![2],
                            cover: 40.0,
                            stirrup: BeamStirrup {
                                dia: 10.0,
                                pitch: 100.0,
                                legs: 2,
                            },
                        },
                    };
                    model.sections[0] = shape.to_section(SectionId(0), "上下非対称RC梁".into());
                    model.sections[0].material = Some(MaterialId(0));
                    model.sections[0].rebar_material = Some(MaterialId(1));
                    model.sections[0].shear_rebar_material = Some(MaterialId(1));
                    model.elements[0].kind = kind;
                    model.elements[0].plastic_zone = Some(250.0);
                    let mut u = [0.0; 12];
                    for end in 0..2 {
                        let x = if (end == 1) != reverse { 3000.0 } else { 0.0 };
                        model.nodes[end].coord = if rotate { [0.0, x, 0.0] } else { [x, 0.0, 0.0] };
                        let uy = curvature * x * x / 2.0;
                        if rotate {
                            u[end * 6] = -uy;
                        } else {
                            u[end * 6 + 1] = uy;
                        }
                        u[end * 6 + 5] = curvature * x;
                    }
                    model.elements[0].local_axis.ref_vector = if rotate {
                        [-1.0, 0.0, 0.0]
                    } else {
                        [0.0, 1.0, 0.0]
                    };
                    let ctx = Ctx { model: &model };
                    let mut member = crate::factory::build_nonlinear_behavior(
                        &model.elements[0],
                        &model,
                        StrengthBasis::Nominal,
                        AnalysisKind::Incremental,
                        None,
                    );
                    member
                        .update_state(
                            &LocalVec {
                                data: u.into_iter().collect(),
                            },
                            false,
                            &ctx,
                        )
                        .unwrap();
                    let states = member.fiber_section_states().unwrap();
                    assert_eq!(states.len(), 2);
                    let snapshot = member.snapshot_state();
                    let state = snapshot.downcast_ref::<FiberBeamSnapshot>().unwrap();
                    let axis = crate::transform::LocalFrame::from_nodes(
                        model.nodes[0].coord,
                        model.nodes[1].coord,
                        model.elements[0].local_axis.ref_vector,
                    )
                    .unwrap();
                    let force = member.internal_force(&ctx);
                    let local = axis.rotate_to_local(&std::array::from_fn(|i| force.data[i]));
                    for (end, section) in states.iter().enumerate() {
                        let mut top = 0;
                        let mut bottom = 0;
                        let mut physical_q = [0.0; 3];
                        for (index, fiber) in section.fibers.iter().enumerate() {
                            if fiber.material == 1 {
                                assert!(fiber.strain*curvature*fiber.y < 0.0,"{kind:?} reverse={reverse} rotate={rotate} κ={curvature} y={} ε={}",fiber.y,fiber.strain);
                                if fiber.y > 0.0 {
                                    top += 1;
                                } else {
                                    bottom += 1;
                                }
                            }
                            let traction = state.2[end][index].probe(fiber.strain).0 * fiber.area;
                            physical_q[0] += traction;
                            physical_q[1] += fiber.z * traction;
                            physical_q[2] -= fiber.y * traction;
                        }
                        assert_eq!((top, bottom), (4, 2));
                        for (component, slot) in [0, 4, 5].into_iter().enumerate() {
                            let boundary = if end == 0 {
                                -local[slot]
                            } else {
                                local[slot + 6]
                            };
                            assert_relative_eq!(
                                physical_q[component],
                                boundary,
                                epsilon = 1e-5,
                                max_relative = 1e-9
                            );
                        }
                        assert!(physical_q[2] * curvature > 0.0);
                    }
                }
            }
        }
    }
}
