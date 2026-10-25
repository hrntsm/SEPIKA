use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::*;
use sepika_core::model::*;
use sepika_element::behavior::{Ctx, LocalVec};
use sepika_element::factory::{build_nonlinear_behavior, StrengthBasis};
use sepika_solver::common::constraint::Reducer;
use sepika_solver::dynamic::damping::{Damping, DampingAccumulation, StiffnessKind};
use sepika_solver::dynamic::timehistory::{
    nonlinear_time_history_analysis, GroundMotion, NewmarkCfg, NonlinearThCfg, ResponseResult,
};
use sepika_solver::statics::linear::linear_static_once;

fn model(kind: ElementKind, cantilever: bool) -> Model {
    Model {
        nodes: (0..2)
            .map(|i| Node {
                id: NodeId(i),
                coord: [6000. * i as f64, 0., 0.],
                restraint: if i == 0 || !cantilever {
                    Dof6Mask::FIXED
                } else {
                    Dof6Mask(0b011101)
                },
                mass: Some([1.; 6]),
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0., 1., 0.],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.),
            plastic_zone: Some(600.),
            spring: None,
        }],
        sections: vec![Section {
            area: 80000.,
            width: 200.,
            depth: 400.,
            iy: 80000. * 400. * 400. / 12.,
            iz: 80000. * 200. * 200. / 12.,
            j: 1e8,
            material: Some(MaterialId(0)),
            ..Section::zero(SectionId(0), "部材荷重独立梁".into())
        }],
        materials: vec![Material {
            id: MaterialId(0),
            name: "弾性範囲鋼".into(),
            category: MaterialCategory::Steel,
            young: 205000.,
            poisson: 0.3,
            density: 0.,
            shear: Some(80000.),
            fc: None,
            fy: Some(1e10),
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        load_cases: vec![LoadCase {
            id: LoadCaseId(0),
            name: "元長期等分布".into(),
            kind: LoadCaseKind::Dead,
            nodal: vec![],
            member: vec![MemberLoad::manual(
                ElemId(0),
                [0., 1., 0.],
                MemberLoadKind::Distributed {
                    a: 0.,
                    b: 6000.,
                    w1: 1.,
                    w2: 1.,
                },
            )],
        }],
        ..Default::default()
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-7 * expected.abs().max(1.),
        "{actual} != {expected}"
    );
}

fn run(model: &Model) -> ResponseResult {
    run_with_long_term(model, true).unwrap()
}

fn run_with_long_term(
    model: &Model,
    apply_long_term: bool,
) -> Result<ResponseResult, sepika_math::solver::SolveError> {
    let map = DofMap::build(model);
    let reducer = Reducer::build(model, &map);
    nonlinear_time_history_analysis(
        model,
        &map,
        &reducer,
        &GroundMotion {
            dt: 0.001,
            accel_x: vec![0.; 4],
            accel_y: None,
            accel_theta: None,
        },
        &NewmarkCfg {
            beta: 0.25,
            gamma: 0.5,
            dt: 0.001,
        },
        &Damping::StiffnessProportional {
            h: 0.,
            omega: 1.,
            basis: StiffnessKind::Initial,
        },
        DampingAccumulation::NonCumulative,
        &[],
        &[],
        NonlinearThCfg {
            apply_long_term,
            record_every: Some(1),
            ..NonlinearThCfg::new(30, 1e-9)
        },
    )
}

#[test]
fn all_fixed_static_keeps_udl_end_and_midspan_forces_for_each_input_kind() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let model = model(kind, false);
        let result = linear_static_once(&model, LoadCaseId(0)).unwrap();
        assert_eq!(result.member_forces.len(), 1);
        for &(x, f) in &result.member_forces[0].1.at {
            close(f[1], -3000. + 6000. * x);
            close(f[5], 3e6 - 18e6 * x + 18e6 * x * x);
        }
    }
}

#[test]
fn cantilever_long_term_material_response_and_records_match_independent_elastic_values() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let model = model(kind, true);
        let result = run(&model);
        let record = result.recording.unwrap();
        for frame in &record.member_forces {
            let at = &frame[0].as_ref().unwrap().at;
            close(at[0].1[1], -6000.);
            close(at[0].1[5], 18e6);
            close(at[1].1[1], 0.);
            close(at[1].1[5], 0.);
        }
        for &(x, f) in &record.peak_member_forces[0].as_ref().unwrap().at {
            close(f[5], 18e6 * (1. - x).powi(2));
        }
    }
}

#[test]
fn zero_reduced_gravity_retains_loaded_fixed_beam_at_initial_and_following_records() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = model(kind, false);
        let mut node = model.nodes[1].clone();
        node.id = NodeId(2);
        node.coord[0] = 7000.;
        node.restraint = Dof6Mask(0b111110);
        model.nodes.push(node);
        let mut extra = model.elements[0].clone();
        extra.id = ElemId(1);
        extra.nodes = smallvec::smallvec![NodeId(1), NodeId(2)];
        extra.plastic_zone = Some(100.);
        model.elements.push(extra);
        let map = DofMap::build(&model);
        let reducer = Reducer::build(&model, &map);
        assert_eq!(reducer.n_indep, 1);
        let result = run(&model);
        assert!(result.applied_long_term);
        let record = result.recording.unwrap();
        for frame in &record.member_forces {
            let at = &frame[0].as_ref().unwrap().at;
            close(at[0].1[1], -3000.);
            close(at[1].1[1], 3000.);
            close(at[0].1[5], 3e6);
            close(at[1].1[5], 3e6);
        }
        for &(x, f) in &record.peak_member_forces[0].as_ref().unwrap().at {
            close(f[5], 3e6 - 18e6 * x + 18e6 * x * x);
        }
    }
}

#[test]
fn load_only_updates_material_end_equilibrium_work_and_checkpoint_without_accumulation() {
    for (kind, fy) in [ElementKind::Fiber, ElementKind::MultiSpring]
        .into_iter()
        .flat_map(|kind| [235., 1e10].into_iter().map(move |fy| (kind, fy)))
    {
        let mut model = model(kind, false);
        model.materials[0].fy = Some(fy);
        let ctx = Ctx { model: &model };
        let build = || {
            build_nonlinear_behavior(
                &model.elements[0],
                &model,
                StrengthBasis::Nominal,
                AnalysisKind::TimeHistory,
                None,
            )
        };
        let mut member = build();
        let zero = LocalVec {
            data: smallvec::smallvec![0.; 12],
        };
        for factor in [1., 0.5, 1., 0.] {
            let loads = if factor == 0. {
                vec![]
            } else {
                vec![MemberLoad::manual(
                    ElemId(0),
                    [0., 1., 0.],
                    MemberLoadKind::Distributed {
                        a: 0.,
                        b: 6000.,
                        w1: factor,
                        w2: factor,
                    },
                )]
            };
            member
                .update_state_with_member_loads(&zero, &loads, true, &ctx)
                .unwrap();
            let response = member.state_frame_load_response().unwrap().unwrap();
            let f = response.physical_frame_end_force_global;
            close(f[1], -3000. * factor);
            close(f[7], -3000. * factor);
            close(f[5], -3e6 * factor);
            close(f[11], 3e6 * factor);
            close(f[1] + f[7] + 6000. * factor, 0.);
            close(f[5] + f[11] + 6000. * f[7] + 18e6 * factor, 0.);
            let virtual_work = f[1] * 0.1 + f[5] * 0.0001 - f[7] * 0.3 - f[11] * 0.0002;
            close(virtual_work, -300. * factor);
            for section in member.fiber_section_states().unwrap() {
                let material_moment: f64 = section
                    .fibers
                    .iter()
                    .map(|fiber| -205000. * fiber.strain * fiber.area * fiber.y)
                    .sum();
                close(material_moment, 3e6 * factor);
            }
            let checkpoint = member.serialize_checkpoint();
            let mut resumed = build();
            resumed.deserialize_checkpoint(&checkpoint).unwrap();
            assert_eq!(resumed.serialize_checkpoint(), checkpoint);
            assert_eq!(
                resumed.internal_force(&ctx).data,
                member.internal_force(&ctx).data
            );
            assert_eq!(
                resumed.tangent_stiffness(&ctx).data,
                member.tangent_stiffness(&ctx).data
            );
            assert_eq!(
                resumed.state_member_forces(&ctx).unwrap().at,
                member.state_member_forces(&ctx).unwrap().at
            );
            let mut invalid = model.load_cases[0].member[0].clone();
            invalid.elem = ElemId(999);
            assert!(member
                .update_state_with_member_loads(&zero, &[invalid], true, &ctx)
                .is_err());
            assert_eq!(member.serialize_checkpoint(), checkpoint);
        }
    }
}

#[test]
fn plastic_fixed_mp_member_load_clear_retains_history_and_support_moments() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let mut model = model(kind, false);
        model.materials[0].fy = Some(235.);
        let ctx = Ctx { model: &model };
        let mut member = build_nonlinear_behavior(
            &model.elements[0],
            &model,
            StrengthBasis::Nominal,
            AnalysisKind::TimeHistory,
            None,
        );
        let zero = LocalVec {
            data: smallvec::smallvec![0.; 12],
        };
        let loads = [MemberLoad::manual(
            ElemId(0),
            [0., 1., 0.],
            MemberLoadKind::Distributed {
                a: 0.,
                b: 6000.,
                w1: 1000.,
                w2: 1000.,
            },
        )];
        member
            .update_state_with_member_loads(&zero, &loads, true, &ctx)
            .unwrap();
        assert!(member
            .fiber_section_states()
            .unwrap()
            .iter()
            .flat_map(|s| &s.fibers)
            .any(|f| f.yield_ratio > 1.));
        member
            .update_state_with_member_loads(&zero, &[], false, &ctx)
            .unwrap();
        let response = member.state_frame_load_response().unwrap().unwrap();
        assert!(response.physical_frame_end_force_global[5].abs() > 1e6);
        close(response.physical_frame_end_force_global[1], 0.);
        close(response.physical_frame_end_force_global[7], 0.);
        assert!(
            (response.physical_frame_end_force_global[5]
                + response.physical_frame_end_force_global[11])
                .abs()
                < 1e-5
        );
    }
}

#[test]
fn mixed_time_history_member_rhs_keeps_legacy_axial_displacement_and_owner_physical_records() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        let mut model = model(kind, false);
        let mut node = model.nodes[1].clone();
        node.id = NodeId(2);
        node.coord[0] = 7000.;
        node.restraint = Dof6Mask(0b111110);
        model.nodes.push(node);
        let mut legacy = model.elements[0].clone();
        legacy.id = ElemId(1);
        legacy.kind = ElementKind::Beam;
        legacy.force_regime = ForceRegime::UniaxialBendingShear;
        legacy.nodes = smallvec::smallvec![NodeId(1), NodeId(2)];
        legacy.plastic_zone = Some(100.);
        model.elements.push(legacy);
        model.load_cases[0].member.push(MemberLoad::manual(
            ElemId(1),
            [1., 0., 0.],
            MemberLoadKind::Distributed {
                a: 0.,
                b: 1000.,
                w1: 2.,
                w2: 2.,
            },
        ));
        for (i, expected) in [true, false].into_iter().enumerate() {
            let behavior = build_nonlinear_behavior(
                &model.elements[i],
                &model,
                StrengthBasis::Nominal,
                AnalysisKind::TimeHistory,
                None,
            );
            assert_eq!(behavior.owns_frame_member_loads(), expected);
        }
        let record = run(&model).recording.unwrap();
        for (u, frame) in record.node_disp.iter().zip(&record.member_forces) {
            close(u[2][0], 1e6 / (205000. * 80000.));
            let owner = &frame[0].as_ref().unwrap().at;
            close(owner[0].1[1], -3000.);
            close(owner[1].1[1], 3000.);
            close(owner[0].1[5], 3e6);
            close(owner[1].1[5], 3e6);
            let legacy = &frame[1].as_ref().unwrap().at;
            close(legacy[0].1[0], 1000.);
            close(legacy[1].1[0], 1000.);
            let support_reaction_n = -legacy[0].1[0] - 1000.;
            close(support_reaction_n, -2000.);
            close(support_reaction_n + 2000., 0.);
            let nodal_virtual_work_nmm = 1000. * 0.1 + legacy[1].1[0] * -0.3;
            close(nodal_virtual_work_nmm, -200.);
        }
        for &(x, f) in &record.peak_member_forces[0].as_ref().unwrap().at {
            close(f[5], 3e6 - 18e6 * x + 18e6 * x * x);
        }
    }
}

#[test]
fn all_fixed_time_history_retains_loaded_end_forces_at_every_recorded_step() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let model = model(kind, false);
        let map = DofMap::build(&model);
        let reducer = Reducer::build(&model, &map);
        assert_eq!(reducer.n_indep, 0);
        for enabled in [true, false] {
            let result = run_with_long_term(&model, enabled).unwrap();
            assert_eq!(result.time, vec![0.0, 0.001, 0.002, 0.003]);
            let record = result.recording.unwrap();
            assert_eq!(record.member_forces.len(), 4);
            let factor = if enabled { 1.0 } else { 0.0 };
            for frame in &record.member_forces {
                let at = &frame[0].as_ref().unwrap().at;
                close(at[0].1[1], factor * -3000.0);
                close(at[1].1[1], factor * 3000.0);
                close(at[0].1[5], factor * 3e6);
                close(at[1].1[5], factor * 3e6);
            }
            for &(x, f) in &record.peak_member_forces[0].as_ref().unwrap().at {
                close(f[1], factor * (-3000.0 + 6000.0 * x));
                close(f[5], factor * (3e6 - 18e6 * x + 18e6 * x * x));
            }
            assert!(record
                .node_disp
                .iter()
                .flatten()
                .flatten()
                .all(|v| *v == 0.0));
        }
    }
}

#[test]
fn all_fixed_time_history_reports_member_load_failure_and_keeps_empty_model_uncomputed() {
    let mut model = model(ElementKind::Fiber, false);
    model.load_cases[0].member[0].kind = MemberLoadKind::Distributed {
        a: 0.0,
        b: 6000.0,
        w1: f64::MAX,
        w2: f64::MAX,
    };
    let error = run_with_long_term(&model, true).unwrap_err().to_string();
    assert!(
        error.contains("0") && (error.contains("要素") || error.contains("部材")),
        "{error}"
    );
    let empty = run(&Model::default());
    assert!(empty.time.is_empty());
    assert!(empty.recording.is_none());
}
