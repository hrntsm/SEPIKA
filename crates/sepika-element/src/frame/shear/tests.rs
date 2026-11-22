use super::*;
use crate::factory::{build_nonlinear_behavior, StrengthBasis};
use sepika_core::{
    ids::{ElemId, MaterialId, NodeId, SectionId},
    model::{
        AnalysisKind, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
        MaterialCategory, MemberLoadKind, Node,
    },
    section_shape::SectionShape,
};
use smallvec::smallvec;

fn member_model(shape: SectionShape, kind: ElementKind) -> Model {
    let nodes = [0.0, 400.0]
        .into_iter()
        .enumerate()
        .map(|(index, x)| Node {
            id: NodeId(index as u32),
            coord: [x, 0.0, 0.0],
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        })
        .collect();
    let mut section = shape.to_section(SectionId(0), "test shear".into());
    section.material = Some(MaterialId(0));
    Model {
        nodes,
        elements: vec![ElementData {
            id: ElemId(0),
            kind,
            nodes: smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::UniaxialBendingShear,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![Material {
            id: MaterialId(0),
            name: "SS400".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fy: Some(235.0),
            fc: None,
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        ..Default::default()
    }
}

fn h_shape() -> SectionShape {
    SectionShape::SteelH {
        height: 300.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 16.0,
        root_r: Some(0.0),
    }
}

fn build(model: &Model, kind: AnalysisKind) -> Box<dyn ElementBehavior> {
    build_nonlinear_behavior(
        &model.elements[0],
        model,
        StrengthBasis::Nominal,
        kind,
        None,
    )
}

fn transverse_mm(y: f64, z: f64) -> LocalVec {
    let mut data = smallvec![0.0; 12];
    data[7] = y;
    data[8] = z;
    LocalVec { data }
}

#[test]
fn elastic_shear_and_loaded_face_forces_remain_identical_through_both_element_routes() {
    for el in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = member_model(h_shape(), el);
        model.sections[0].frame_use = Some(FrameSectionUse::Girder);
        model.elements[0].end_offsets_mm = [[0.0, 5.0, 0.0]; 2];
        let ctx = Ctx { model: &model };
        let mut new = build(&model, AnalysisKind::Incremental);
        let mut previous = build(&model, AnalysisKind::TimeHistory);
        let loads = [MemberLoad {
            elem: ElemId(0),
            dir: [0.0, 1.0, 0.0],
            kind: MemberLoadKind::Distributed {
                a: 0.0,
                b: 400.0,
                w1: 10.0,
                w2: 20.0,
            },
            extent: Default::default(),
            name: "台形分布荷重".into(),
            source: Default::default(),
        }];
        for element in [&mut new, &mut previous] {
            element
                .update_state_with_member_loads(&transverse_mm(0.0002, 0.0), &loads, false, &ctx)
                .unwrap();
        }
        let a = new.state_frame_load_response().unwrap().unwrap();
        let b = previous.state_frame_load_response().unwrap().unwrap();
        assert_eq!(a.nodal_residual.data, b.nodal_residual.data);
        assert_eq!(a.flex_face_force_local, b.flex_face_force_local);
        assert_eq!(
            new.tangent_stiffness(&ctx).data,
            previous.tangent_stiffness(&ctx).data
        );
        let state = new.member_shear_status().unwrap();
        assert!(state.span_issue.is_none());
        for i in 0..2 {
            let face = state.ends[i].as_ref().unwrap().as_ref().unwrap();
            assert!((face.shear_n[0] - a.section.at(i as f64).unwrap()[1]).abs() < 1e-8);
        }
        assert!(previous.member_shear_status().is_none());
    }
}

#[test]
fn uniform_web_shear_post_yield_adds_ninety_nine_times_its_elastic_compliance() {
    for el in [ElementKind::Beam, ElementKind::Fiber] {
        let model = member_model(h_shape(), el);
        let ctx = Ctx { model: &model };
        let elastic = BeamElement::new(&model.elements[0], &model);
        let mut behavior = build(&model, AnalysisKind::Incremental);
        let initial = behavior.tangent_stiffness(&ctx).get(7, 7);
        let qy = (300.0 - 2.0 * 16.0) * 8.0 * 235.0 / 3.0_f64.sqrt();
        let yield_disp = qy / initial;
        behavior
            .update_state(&transverse_mm(1.8 * yield_disp, 0.0), false, &ctx)
            .unwrap();
        let tangent = behavior.tangent_stiffness(&ctx).get(7, 7);
        let compliance = 1.0 / tangent - 1.0 / initial;
        let expected = 99.0 * elastic.length / (elastic.g * elastic.as_y);
        assert!(
            (compliance / expected - 1.0).abs() < 2e-5,
            "{compliance} vs {expected}"
        );
        let event = behavior.member_shear_status().unwrap().ends[0]
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert!(event.reached);
        assert_eq!(event.governing_axis, Some(0));
        let force = behavior.internal_force(&ctx).data[7];
        let expected_force = qy + tangent * 0.8 * yield_disp;
        assert!((force / expected_force - 1.0).abs() < 1e-6);
        let du = 1e-3 * yield_disp;
        behavior
            .update_state(&transverse_mm(du, 0.0), false, &ctx)
            .unwrap();
        let difference_tangent = (behavior.internal_force(&ctx).data[7] - force) / du;
        assert!(
            (difference_tangent / tangent - 1.0).abs() < 1e-5,
            "{el:?}: {difference_tangent} vs {tangent}"
        );
        let snapshot = behavior.snapshot_state();
        let yielded_force = behavior.internal_force(&ctx).data[7];
        behavior.commit_state();
        let unload = -0.02 * yield_disp;
        behavior
            .update_state(&transverse_mm(unload, 0.0), false, &ctx)
            .unwrap();
        assert!((behavior.tangent_stiffness(&ctx).get(7, 7) / initial - 1.0).abs() < 1e-6);
        assert!(
            (behavior.internal_force(&ctx).data[7] - yielded_force - initial * unload).abs() < 1.0
        );
        behavior.revert_state();
        assert_eq!(behavior.internal_force(&ctx).data[7], yielded_force);
        behavior.restore_state(snapshot.as_ref());
        behavior.revert_state();
        assert_eq!(behavior.internal_force(&ctx).data[7], 0.0);
        assert_eq!(behavior.tangent_stiffness(&ctx).get(7, 7), initial);
        assert_eq!(behavior.member_shear_status().unwrap().ends, [None, None]);
    }
}

#[test]
fn web_yield_and_unsupported_weak_axis_are_recorded_together() {
    let model = member_model(h_shape(), ElementKind::Fiber);
    let ctx = Ctx { model: &model };
    let mut behavior = build(&model, AnalysisKind::Incremental);
    let initial = behavior.tangent_stiffness(&ctx);
    let qy = (300.0 - 2.0 * 16.0) * 8.0 * 235.0 / 3.0_f64.sqrt();
    let dy = 1.8 * qy / initial.get(7, 7);
    behavior
        .update_state(&transverse_mm(dy, 0.01), false, &ctx)
        .unwrap();
    let status = behavior.member_shear_status().unwrap();
    for (end, issue) in status.ends.iter().zip(&status.end_issues) {
        let evaluated = end.as_ref().unwrap().as_ref().unwrap();
        assert!(evaluated.reached);
        assert_eq!(evaluated.governing_axis, Some(0));
        assert_eq!(evaluated.shear_n[1], 0.0);
        let issue = issue.as_ref().unwrap();
        assert!(issue.contains("弱軸せん断"), "{issue}");
    }
    let tangent = behavior.tangent_stiffness(&ctx);
    assert!(tangent.get(7, 7) < initial.get(7, 7) * 0.1);
    assert!((tangent.get(8, 8) / initial.get(8, 8) - 1.0).abs() < 1e-6);
}

#[test]
fn point_loads_at_flexible_faces_use_the_inward_cut_and_remain_in_global_equilibrium() {
    use sepika_core::model::RigidZone;
    let mut model = member_model(h_shape(), ElementKind::Beam);
    model.elements[0].rigid_zone = RigidZone {
        length_i: 50.0,
        length_j: 30.0,
        face_i: Some(50.0),
        face_j: Some(30.0),
        ..Default::default()
    };
    let ctx = Ctx { model: &model };
    let mut behavior = build(&model, AnalysisKind::Incremental);
    let loads = [[50.0, 20_000.0], [370.0, -8000.0]].map(|[a, p]| MemberLoad {
        elem: ElemId(0),
        dir: [0.0, 1.0, 0.0],
        kind: MemberLoadKind::Point { a, p },
        extent: Default::default(),
        name: String::new(),
        source: Default::default(),
    });
    behavior
        .update_state_with_member_loads(&transverse_mm(0.005, 0.0), &loads, false, &ctx)
        .unwrap();
    let response = behavior.state_frame_load_response().unwrap().unwrap();
    let status = behavior.member_shear_status().unwrap();
    let [i, j] = status
        .ends
        .each_ref()
        .map(|end| end.as_ref().unwrap().as_ref().unwrap());
    assert!(
        (i.shear_n[0] - (response.section.at(50.0 / 400.0).unwrap()[1] + 20_000.0)).abs() < 1e-8
    );
    assert!((j.shear_n[0] - response.section.at(370.0 / 400.0).unwrap()[1]).abs() < 1e-8);
    assert!((i.shear_n[0] - j.shear_n[0]).abs() < 1e-8);
    let f = response.nodal_residual.data;
    assert!((f[1] + f[7] + 12_000.0).abs() < 1e-8);
    assert!((f[5] + f[11] + 400.0 * f[7] + 50.0 * 20_000.0 - 370.0 * 8000.0).abs() < 1e-6);
}

#[test]
fn distributed_load_can_yield_one_face_with_half_the_member_plastic_compliance() {
    let model = member_model(h_shape(), ElementKind::Fiber);
    let ctx = Ctx { model: &model };
    let elastic = BeamElement::new(&model.elements[0], &model);
    let mut behavior = build(&model, AnalysisKind::Incremental);
    let stiffness = behavior.tangent_stiffness(&ctx).get(7, 7);
    let qy = (300.0 - 2.0 * 16.0) * 8.0 * 235.0 / 3.0_f64.sqrt();
    let load = MemberLoad {
        elem: ElemId(0),
        dir: [0.0, 1.0, 0.0],
        kind: MemberLoadKind::Distributed {
            a: 0.0,
            b: 400.0,
            w1: 200.0,
            w2: 200.0,
        },
        extent: Default::default(),
        name: "等分布荷重".into(),
        source: Default::default(),
    };
    let displacement = 1.1 * (qy - 40_000.0) / stiffness;
    behavior
        .update_state_with_member_loads(&transverse_mm(displacement, 0.0), &[load], false, &ctx)
        .unwrap();
    let status = behavior.member_shear_status().unwrap();
    let yielded = status
        .ends
        .each_ref()
        .map(|end| end.as_ref().unwrap().as_ref().unwrap().reached);
    assert_eq!(yielded, [true, false]);
    let tangent = behavior.tangent_stiffness(&ctx).get(7, 7);
    let compliance = 1.0 / tangent - 1.0 / stiffness;
    let expected = 99.0 * elastic.length / (2.0 * elastic.g * elastic.as_y);
    assert!((compliance / expected - 1.0).abs() < 2e-5);
}

#[test]
fn concentrated_member_loads_between_flexible_ends_can_govern_with_zero_end_shear() {
    use crate::frame::member_load::{FrameDeadLoadMap, LoadedFrameSection};
    let model = member_model(h_shape(), ElementKind::Beam);
    let beam = BeamElement::new(&model.elements[0], &model);
    let capacity = ShearCapacity::from_model(&model, &model.elements[0])
        .unwrap()
        .unwrap();
    let mechanics = Mechanics {
        capacity,
        axis: beam.axis,
        length_mm: 400.0,
        rigid_mm: [0.0, 0.0],
        hardening: [100.0, 100.0],
    };
    let q = (300.0 - 2.0 * 16.0) * 8.0 * 235.0 / 3.0_f64.sqrt();
    let loads = [[100.0, q], [200.0, -2.0 * q], [300.0, q]].map(|[a, p]| MemberLoad {
        elem: ElemId(0),
        dir: [0.0, 1.0, 0.0],
        kind: MemberLoadKind::Point { a, p },
        extent: Default::default(),
        name: String::new(),
        source: Default::default(),
    });
    let map = FrameDeadLoadMap::new(&loads, &beam.axis, 400.0, 0.0, 0.0).unwrap();
    let response = LoadedFrameResponse {
        nodal_residual: transverse_mm(0.0, 0.0),
        physical_frame_end_force_global: [0.0; 12],
        flex_face_force_local: [0.0; 12],
        section: LoadedFrameSection::new(map, [0.0; 12]),
    };
    assert_eq!(response.section.at(0.0).unwrap()[1], 0.0);
    assert_eq!(response.section.at(1.0).unwrap()[1], 0.0);
    assert_eq!(response.section.at(0.4).unwrap()[1], q);
    assert!(mechanics
        .span_issue(&response, &State::new(false).status)
        .is_some());
}

#[test]
fn box_associated_slip_preserves_the_elastic_direction_tangent_to_initial_yield() {
    let mut model = member_model(
        SectionShape::SteelBox {
            height: 210.0,
            width: 210.0,
            thick: 10.0,
            horizontal_thick: 10.0,
            corner_r: Some(0.0),
        },
        ElementKind::Fiber,
    );
    model.nodes[1].coord[0] = 250.0;
    let ctx = Ctx { model: &model };
    let mut behavior = build(&model, AnalysisKind::Incremental);
    let initial = behavior.tangent_stiffness(&ctx);
    let mut reference = build(&model, AnalysisKind::TimeHistory);
    reference
        .update_state(&transverse_mm(0.001, 0.001), false, &ctx)
        .unwrap();
    let dy = reference.internal_force(&ctx).data[7] / 0.001;
    let dz = reference.internal_force(&ctx).data[8] / 0.001;
    let axis_limit_n = 2.0 * 200.0 * 10.0 * 235.0 / 3.0_f64.sqrt();
    let onset_n = axis_limit_n / 1.5;
    behavior
        .update_state(
            &transverse_mm(1.00001 * onset_n / dy, 1.00001 * onset_n / dz),
            false,
            &ctx,
        )
        .unwrap();
    let actual = behavior.tangent_stiffness(&ctx);
    let event = behavior.member_shear_status().unwrap().ends[0]
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert!(event.reached, "{event:?}");
    let stiff =
        |k: &LocalMat, sign| (k.get(7, 7) + k.get(8, 8) + sign * (k.get(8, 7) + k.get(7, 8))) / 2.0;
    assert!(stiff(&actual, 1.0) < 0.2 * stiff(&initial, 1.0));
    let neutral = stiff(&actual, -1.0) / stiff(&initial, -1.0);
    assert!((neutral - 1.0).abs() < 1e-3, "{neutral}");
}

#[test]
fn rc_column_uses_each_trial_compression_at_the_same_face() {
    use sepika_core::section_shape::{RcRectColumnRebar, RebarSize, RectColumnHoop};
    let shape = SectionShape::RcColumnRect {
        b: 400.0,
        d: 400.0,
        rebar: RcRectColumnRebar {
            main_dia: RebarSize::DiameterMm(25.0),
            x: vec![4],
            y: vec![4],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let mut model = member_model(shape, ElementKind::Fiber);
    model.materials[0].name = "Fc30".into();
    model.materials[0].category = MaterialCategory::Concrete;
    model.materials[0].young = 25000.0;
    model.materials[0].fc = Some(30.0);
    model.materials[0].fy = None;
    for (id, name, fy) in [(1, "SD345", 345.0), (2, "SD295", 295.0)] {
        model.materials.push(Material {
            id: MaterialId(id),
            name: name.into(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fy: Some(fy),
            fc: None,
            strength_factor: None,
            concrete_class: Default::default(),
        });
    }
    model.sections[0].rebar_material = Some(MaterialId(1));
    model.sections[0].shear_rebar_material = Some(MaterialId(2));
    let ctx = Ctx { model: &model };
    let mut behavior = build(&model, AnalysisKind::Incremental);
    let mut first = transverse_mm(0.012, 0.008);
    first.data[6] = -0.02;
    let loads = [MemberLoad {
        elem: ElemId(0),
        dir: [0.0, 1.0, 0.0],
        kind: MemberLoadKind::Distributed {
            a: 0.0,
            b: 400.0,
            w1: 40.0,
            w2: 60.0,
        },
        extent: Default::default(),
        name: "台形分布荷重".into(),
        source: Default::default(),
    }];
    behavior
        .update_state_with_member_loads(&first, &loads, false, &ctx)
        .unwrap();
    assert!(behavior.member_shear_status().unwrap().span_issue.is_none());
    let low = behavior.member_shear_status().unwrap().ends[0]
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .limit_n;
    let mut second = transverse_mm(0.0, 0.0);
    second.data[6] = -0.05;
    behavior.update_state(&second, false, &ctx).unwrap();
    assert!(behavior.member_shear_status().unwrap().span_issue.is_none());
    let high = behavior.member_shear_status().unwrap().ends[0]
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .limit_n;
    assert!(high > low);
    let observed = behavior.member_shear_status().unwrap().ends[0]
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert!(
        (observed.limit_n * observed.utilization - observed.shear_n[0].hypot(observed.shear_n[1]))
            .abs()
            < 1e-8
    );
    behavior.revert_state();
    assert_eq!(behavior.member_shear_status().unwrap().ends, [None, None]);
    let mut tension = transverse_mm(0.012, 0.008);
    tension.data[6] = 0.02;
    behavior.update_state(&tension, false, &ctx).unwrap();
    let status = behavior.member_shear_status().unwrap();
    assert!(status.span_issue.is_none());
    assert!(status.ends.iter().all(|end| end
        .as_ref()
        .is_some_and(|e| { e.as_ref().is_err_and(|reason| reason.contains("引張軸力")) })));
}
