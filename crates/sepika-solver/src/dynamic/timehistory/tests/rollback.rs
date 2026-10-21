use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::*;
use sepika_core::model::*;

fn model(kind: ElementKind) -> Model {
    Model {
        nodes: (0..2)
            .map(|i| Node {
                id: NodeId(i),
                coord: [1000. * i as f64, 0., 0.],
                restraint: if i == 0 {
                    Dof6Mask::FIXED
                } else {
                    Dof6Mask(0b111110)
                },
                mass: if i == 0 {
                    None
                } else {
                    Some([1., 0., 0., 0., 0., 0.])
                },
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(10),
            kind,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0., 1., 0.],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: Some(100.),
            spring: None,
        }],
        sections: vec![Section {
            material: Some(MaterialId(0)),
            area: 20000.,
            iy: 66666666.66666667,
            iz: 16666666.66666667,
            j: 1e6,
            depth: 200.,
            width: 100.,
            ..Section::zero(SectionId(0), "長期荷重試験".into())
        }],
        materials: vec![Material {
            id: MaterialId(0),
            name: "長期荷重試験".into(),
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
        ..Default::default()
    }
}

fn udl() -> MemberLoad {
    MemberLoad::manual(
        ElemId(10),
        [0., 1., 0.],
        MemberLoadKind::Distributed {
            a: 0.,
            b: 1000.,
            w1: 2.,
            w2: 2.,
        },
    )
}

use sepika_element::behavior::{Ctx, LocalVec};

fn loaded_behaviors(model: &Model, kind: ElementKind) -> Vec<Box<dyn ElementBehavior>> {
    use sepika_element::factory::AnalysisKind;
    let data = &model.elements[0];
    let member: Box<dyn ElementBehavior> = match kind {
        ElementKind::Fiber => {
            let mut member = sepika_element::frame::fiber::FiberBeam::with_plastic_zone(
                data,
                model,
                100.,
                StrengthBasis::Nominal,
                AnalysisKind::TimeHistory,
            );
            member.configure_wall_release([0., 0., 1.]).unwrap();
            Box::new(member)
        }
        ElementKind::MultiSpring => {
            let mut member = sepika_element::frame::multi_spring::MultiSpringElement::new(
                data,
                model,
                StrengthBasis::Nominal,
                AnalysisKind::TimeHistory,
            );
            member.configure_wall_release([0., 0., 1.]).unwrap();
            Box::new(member)
        }
        _ => unreachable!(),
    };
    vec![member]
}

fn loaded_trial(
    model: &Model,
    behaviors: &mut [Box<dyn ElementBehavior>],
    axial_mm: f64,
    load: f64,
) {
    let mut du = LocalVec {
        data: smallvec::smallvec![0.; 12],
    };
    du.data[6] = axial_mm;
    let mut loads = udl();
    if let MemberLoadKind::Distributed { w1, w2, .. } = &mut loads.kind {
        *w1 *= load;
        *w2 *= load;
    }
    behaviors[0]
        .update_state_with_member_loads(&du, &[loads], false, &Ctx { model })
        .unwrap();
}

fn assert_same_state(
    model: &Model,
    actual: &[Box<dyn ElementBehavior>],
    expected: &[Box<dyn ElementBehavior>],
) {
    let ctx = Ctx { model };
    assert_eq!(
        actual[0].serialize_checkpoint(),
        expected[0].serialize_checkpoint()
    );
    assert_eq!(
        actual[0].internal_force(&ctx).data,
        expected[0].internal_force(&ctx).data
    );
    assert_eq!(
        actual[0].tangent_stiffness(&ctx).data,
        expected[0].tangent_stiffness(&ctx).data
    );
    let a = actual[0].state_frame_load_response().unwrap().unwrap();
    let e = expected[0].state_frame_load_response().unwrap().unwrap();
    assert_eq!(a.nodal_residual.data, e.nodal_residual.data);
    assert_eq!(
        a.physical_frame_end_force_global,
        e.physical_frame_end_force_global
    );
    assert_eq!(a.flex_face_force_local, e.flex_face_force_local);
    assert_eq!(
        a.section.recover(&[0., 0.5, 1.]).unwrap().at,
        e.section.recover(&[0., 0.5, 1.]).unwrap().at
    );
}

#[test]
fn failed_step_restores_loaded_plastic_snapshot_and_retry_matches_control() {
    for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
        for reason in [
            "全体釣合いが反復上限 1 回までに収束しません",
            "応答に非有限値があります",
        ] {
            let mut model = model(kind);
            model.materials[0].fy = Some(235.);
            let ctx = Ctx { model: &model };
            let mut actual = loaded_behaviors(&model, kind);
            let mut control = loaded_behaviors(&model, kind);
            let elastic = actual[0].tangent_stiffness(&ctx).get(6, 6);
            for batch in [&mut actual, &mut control] {
                loaded_trial(&model, batch, 1.5, 0.5);
                assert!(batch[0].tangent_stiffness(&ctx).get(6, 6) < elastic);
                batch[0].commit_state();
                loaded_trial(&model, batch, -0.2, 0.75);
            }
            let snapshot = StateSnapshot::capture(&actual);
            loaded_trial(&model, &mut actual, 0.7, 2.);
            assert_ne!(
                actual[0].serialize_checkpoint(),
                control[0].serialize_checkpoint()
            );
            let error = rollback_failed_step(&mut actual, &snapshot, 2, 0.002, reason).to_string();
            assert!(
                error.contains("step 2")
                    && error.contains("時刻 0.002 s")
                    && error.contains(reason),
                "{error}"
            );
            assert_same_state(&model, &actual, &control);
            for batch in [&mut actual, &mut control] {
                loaded_trial(&model, batch, -0.1, 0.8);
                batch[0].commit_state();
            }
            assert_same_state(&model, &actual, &control);
        }
    }
}

#[test]
fn public_entry_stops_at_nonfinite_step_after_a_converged_step() {
    let model = model(ElementKind::Fiber);
    let map = DofMap::build(&model);
    let reducer = Reducer::build(&model, &map);
    for accumulation in [
        DampingAccumulation::NonCumulative,
        DampingAccumulation::Cumulative,
    ] {
        let error = nonlinear_time_history_analysis(
            &model,
            &map,
            &reducer,
            &GroundMotion {
                dt: 0.001,
                accel_x: vec![0., 0., f64::NAN, 1.],
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
                basis: crate::dynamic::damping::StiffnessKind::Initial,
            },
            accumulation,
            &[0.],
            &[0.],
            NonlinearThCfg::new(20, 1e-9),
        )
        .expect_err("非有限な試行応答を確定してはいけない")
        .to_string();
        assert!(
            error.contains("step 2") && error.contains("時刻 0.002 s") && error.contains("非有限"),
            "{error}"
        );
    }
}
