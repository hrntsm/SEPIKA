use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
    Model, Node,
};
use sepika_core::section_shape::SectionShape;
use sepika_element::behavior::{Ctx, MassOption};
use sepika_element::factory::build_behavior;

fn shell_model(coords: [[f64; 3]; 4], order: [usize; 4]) -> Model {
    let mut section = SectionShape::RcWall {
        thickness: 100.0,
        pwh_ratio: None,
        pwv_ratio: None,
        ps: 0.0025,
    }
    .to_section(SectionId(0), "板".into());
    section.material = Some(MaterialId(0));
    Model {
        nodes: coords
            .into_iter()
            .enumerate()
            .map(|(i, coord)| Node {
                id: NodeId(i as u32),
                coord,
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Shell,
            nodes: order.into_iter().map(|i| NodeId(i as u32)).collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: None,
            plastic_zone: None,
            spring: None,
        }],
        materials: vec![Material {
            id: MaterialId(0),
            name: "コンクリート".into(),
            category: MaterialCategory::Concrete,
            young: 23000.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        sections: vec![section],
        ..Default::default()
    }
}

#[test]
fn planar_convex_shell_mass_and_drilling_match_geometry_in_every_node_order_and_orientation() {
    for (xy, expected_area_mm2, expected_mass_t, drilling_diag_n_mm, drilling_off_n_mm) in [
        (
            [[0.0, 0.0], [4000.0, 0.0], [3000.0, 3000.0], [0.0, 2000.0]],
            9_000_000.0,
            2.16,
            6_468_750_000.0,
            -2_156_250_000.0,
        ),
        (
            [[0.0, 0.0], [4000.0, 0.0], [4000.0, 2000.0], [0.0, 2000.0]],
            8_000_000.0,
            1.92,
            5_750_000_000.0,
            -1_916_666_666.666_666_7,
        ),
        (
            [
                [0.0, 0.0],
                [4000.0, 0.0],
                [5000.0, 2000.0],
                [1000.0, 2000.0],
            ],
            8_000_000.0,
            1.92,
            5_750_000_000.0,
            -1_916_666_666.666_666_7,
        ),
    ] {
        for tilted in [false, true] {
            let coords = xy.map(|[x, y]| {
                if tilted {
                    [1000.0 + 0.6 * x, -2000.0 + y, 3000.0 + 0.8 * x]
                } else {
                    [x, y, 0.0]
                }
            });
            let normal = if tilted {
                [-0.8, 0.0, 0.6]
            } else {
                [0.0, 0.0, 1.0]
            };
            for reverse in [false, true] {
                for start in 0..4 {
                    let order =
                        std::array::from_fn(|i| (start + if reverse { 4 - i } else { i }) % 4);
                    let model = shell_model(coords, order);
                    model.validate().unwrap();
                    model.validate_local_frames().unwrap();
                    let behavior = build_behavior(&model.elements[0], &model);
                    for option in [MassOption::Lumped, MassOption::Consistent] {
                        let mass = behavior.try_mass_matrix(option).unwrap();
                        for d in 0..3 {
                            let total_t: f64 = (0..4)
                                .flat_map(|i| (0..4).map(move |j| (i, j)))
                                .map(|(i, j)| mass.get(i * 6 + d, j * 6 + d))
                                .sum();
                            assert!(
                                (total_t - expected_mass_t).abs() < 1e-12,
                                "{order:?}: {total_t}"
                            );
                            assert!((total_t / (2.4e-9 * 100.0) - expected_area_mm2).abs() < 1e-6);
                        }
                        for i in 0..24 {
                            for j in 0..24 {
                                let value = mass.get(i, j);
                                if i % 6 >= 3 || j % 6 >= 3 || i % 6 != j % 6 {
                                    assert_eq!(value, 0.0);
                                } else if matches!(option, MassOption::Lumped) {
                                    let expected = if i == j { expected_mass_t / 4.0 } else { 0.0 };
                                    assert!((value - expected).abs() < 1e-12);
                                }
                            }
                        }
                    }
                    let stiffness = behavior.tangent_stiffness(&Ctx { model: &model });
                    for i in 0..24 {
                        for j in 0..24 {
                            assert!(stiffness.get(i, j).is_finite());
                        }
                    }
                    for i in 0..4 {
                        for j in 0..4 {
                            let mut normal_rotation = 0.0;
                            for (a, na) in normal.into_iter().enumerate() {
                                for (b, nb) in normal.into_iter().enumerate() {
                                    normal_rotation +=
                                        na * stiffness.get(i * 6 + 3 + a, j * 6 + 3 + b) * nb;
                                }
                            }
                            let expected = if i == j {
                                drilling_diag_n_mm
                            } else {
                                drilling_off_n_mm
                            };
                            assert!(
                                (normal_rotation - expected).abs() < 1e-4,
                                "{order:?}: {normal_rotation} != {expected}"
                            );
                        }
                    }
                }
            }
        }
    }
}
