use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
};
use sepika_element::factory::build_behavior;
use sepika_solver::statics::analysis::Analysis;

fn rc_frame() -> Model {
    scaled_rc_frame(1.0)
}

fn scaled_rc_frame(length_scale: f64) -> Model {
    let sections = [(600.0, 600.0), (400.0, 700.0)]
        .into_iter()
        .enumerate()
        .map(|(i, (width, depth))| {
            let shape = if i == 0 {
                SectionShape::RcColumnRect {
                    b: width * length_scale,
                    d: depth * length_scale,
                    rebar: RcRectColumnRebar {
                        main_dia: 25.0 * length_scale,
                        x: vec![],
                        y: vec![],
                        cover: 40.0 * length_scale,
                        hoop: RectColumnHoop {
                            dia: 10.0 * length_scale,
                            pitch: 100.0 * length_scale,
                            legs_x: 2,
                            legs_y: 2,
                        },
                    },
                }
            } else {
                SectionShape::RcBeamRect {
                    b: width * length_scale,
                    d: depth * length_scale,
                    rebar: RcBeamRebar {
                        main_dia: 25.0 * length_scale,
                        top: vec![],
                        bottom: vec![],
                        cover: 40.0 * length_scale,
                        stirrup: BeamStirrup {
                            dia: 10.0 * length_scale,
                            pitch: 100.0 * length_scale,
                            legs: 2,
                        },
                    },
                }
            };
            let mut section = shape
                .input_section(SectionId(i as u32), format!("RC{i}"))
                .unwrap();
            section.frame_use = Some(if i == 0 {
                FrameSectionUse::Column
            } else {
                FrameSectionUse::Girder
            });
            section.material = Some(MaterialId(0));
            section.rebar_material = Some(MaterialId(1));
            section.shear_rebar_material = Some(MaterialId(1));
            section
        })
        .collect();
    let mut model = Model {
        nodes: [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0], [6000.0, 0.0, 3000.0]]
            .into_iter()
            .enumerate()
            .map(|(i, coord)| Node {
                id: NodeId(i as u32),
                coord: coord.map(|value| value * length_scale),
                restraint: if i == 0 {
                    Dof6Mask::FIXED
                } else {
                    Dof6Mask::FREE
                },
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        sections,
        materials: vec![
            Material {
                id: MaterialId(0),
                name: "RC".into(),
                category: MaterialCategory::Concrete,
                young: 23000.0,
                poisson: 0.2,
                density: 0.0,
                shear: None,
                fc: Some(24.0),
                fy: None,
                concrete_class: Default::default(),
                strength_factor: None,
            },
            Material {
                id: MaterialId(1),
                name: "SD345".into(),
                category: MaterialCategory::Rebar,
                young: 205000.0,
                poisson: 0.3,
                density: 0.0,
                shear: None,
                fc: None,
                fy: Some(345.0),
                concrete_class: Default::default(),
                strength_factor: None,
            },
        ],
        elements: (0..2)
            .map(|i| ElementData {
                id: ElemId(i),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![NodeId(i), NodeId(i + 1)],
                section: Some(SectionId(i)),
                local_axis: LocalAxis {
                    ref_vector: if i == 0 {
                        [1.0, 0.0, 0.0]
                    } else {
                        [0.0, 0.0, 1.0]
                    },
                },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                vertical_eccentricity: Some(0.0),
                plastic_zone: None,
                spring: None,
            })
            .collect(),
        load_cases: vec![LoadCase {
            id: LoadCaseId(0),
            name: "先端荷重".into(),
            kind: Default::default(),
            nodal: vec![NodalLoad::manual(
                NodeId(2),
                [0.0, 500.0, -1000.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        }],
        ..Default::default()
    };
    sepika_element::frame::beam::apply_auto_rigid_zones(&mut model, &Default::default());
    model
}

#[test]
fn rc_torsion_release_has_independent_zero_energy_motion() {
    let model = rc_frame();
    assert_eq!(model.elements[0].rigid_zone.length_j, 200.0);
    assert_eq!(model.elements[1].rigid_zone.length_i, 125.0);
    let mut displacement = [0.0; 18];
    let theta = 0.001;
    for (node, local, value) in [(1, 5, theta), (2, 1, 6000.0 * theta), (2, 5, theta)] {
        displacement[node * 6 + local] = value;
    }
    for element in &model.elements {
        let behavior = build_behavior(element, &model);
        let local: Vec<f64> = element
            .nodes
            .iter()
            .flat_map(|node| {
                displacement[node.index() * 6..node.index() * 6 + 6]
                    .iter()
                    .copied()
            })
            .collect();
        let stiffness =
            behavior.tangent_stiffness(&sepika_element::behavior::Ctx { model: &model });
        for i in 0..stiffness.n {
            let force: f64 = local
                .iter()
                .enumerate()
                .map(|(j, value)| stiffness.get(i, j) * value)
                .sum();
            assert!(force.abs() < 1e-7, "{} {i}: {force}", element.id.0);
        }
    }
}

#[test]
fn rc_torsion_release_is_rejected() {
    for load in [
        [0.0; 6],
        [0.0, 0.0, -1000.0, 0.0, 0.0, 0.0],
        [0.0, 500.0, -1000.0, 0.0, 0.0, 0.0],
    ] {
        let mut model = rc_frame();
        model.load_cases[0].nodal = vec![NodalLoad::manual(NodeId(2), load)];
        match Analysis::prepare(&model) {
            Ok(_) => panic!("機構が正常終了"),
            Err(sepika_math::solver::SolveError::InvalidInput(message)) => {
                assert!(message.contains("数値精度") && message.contains("機構"))
            }
            Err(error) => panic!("不安定以外のエラー: {error}"),
        }
        assert!(sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).is_err());
    }
}

#[test]
fn retained_column_torsion_matches_independent_flexibility() {
    for length_scale in [0.001, 1.0, 1000.0] {
        for stiffness_scale in [1e-6, 1.0, 1e6] {
            let mut model = scaled_rc_frame(length_scale);
            model.materials[0].young *= stiffness_scale;
            model.girder_torsion = GirderTorsionMode::Keep;
            let result = Analysis::prepare(&model)
                .unwrap()
                .linear_static(LoadCaseId(0))
                .unwrap();
            // 正方形600mmの採用J=18,252,000,000mm⁴、ねじりは全長3000mmに分布。
            let expected = 500.0 * 6000.0 * 3000.0
                / ((23000.0 / 2.4) * 18_252_000_000.0)
                / (stiffness_scale * length_scale * length_scale);
            approx::assert_relative_eq!(result.disp[1][5], expected, max_relative = 1e-9);
            for node in result.disp {
                assert!(node.into_iter().all(f64::is_finite));
            }
        }
    }
}

#[test]
fn standalone_eigen_entry_rejects_the_same_mechanism_before_iteration() {
    let mut model = rc_frame();
    model.materials[0].density = 2.4e-9;
    let dofmap = sepika_core::dof::DofMap::build(&model);
    let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dofmap);
    let error =
        sepika_solver::dynamic::eigen::solve_eigen(&model, &dofmap, &reducer, 1).unwrap_err();
    assert!(
        matches!(error, sepika_math::solver::SolveError::InvalidInput(message) if message.contains("数値精度") && message.contains("機構"))
    );
}
