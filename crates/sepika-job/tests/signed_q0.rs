use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
use sepika_job::{
    simple_beam_q0_by_elem, simple_beam_q0_by_gravity_cases, simple_beam_q0_by_terms,
};

fn model() -> Model {
    let mut model = Model {
        nodes: [0.0, 6000.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Node {
                id: NodeId(i as u32),
                coord: [x, 0.0, 0.0],
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        ..Default::default()
    };
    model.materials.push(Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 23000.0,
        poisson: 0.2,
        density: 0.0,
        shear: None,
        fc: Some(24.0),
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    });
    let mut section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: vec![3],
            bottom: vec![3],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(0), "RC梁".into());
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    model.elements.push(ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        plastic_zone: None,
        spring: None,
    });
    model.load_cases = [LoadCaseKind::Dead, LoadCaseKind::Live]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| LoadCase {
            id: LoadCaseId(i as u32),
            name: "任意名称".into(),
            kind,
            nodal: vec![],
            member: vec![],
        })
        .collect();
    model
}

fn point(a: f64, p: f64) -> MemberLoad {
    MemberLoad::manual(ElemId(0), [0.0, 0.0, -1.0], MemberLoadKind::Point { a, p })
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-8 * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

fn all_helpers(model: &Model, expected: f64) {
    close(
        simple_beam_q0_by_elem(model, LoadCaseId(0)).unwrap()[&ElemId(0)],
        expected,
    );
    close(
        simple_beam_q0_by_terms(model, &[(LoadCaseId(0), 1.0)]).unwrap()[&ElemId(0)],
        expected,
    );
    close(
        simple_beam_q0_by_gravity_cases(model).unwrap()[&ElemId(0)],
        expected,
    );
}

#[test]
fn rc_q0_envelopes_after_signed_end_superposition_for_all_public_helpers() {
    let mut model = model();
    model.load_cases[0].member = vec![point(1500.0, 1000.0)];
    model.load_cases[1].member = vec![point(4500.0, 1000.0)];
    close(
        simple_beam_q0_by_gravity_cases(&model).unwrap()[&ElemId(0)],
        1000.0,
    );
    close(
        simple_beam_q0_by_terms(&model, &[(LoadCaseId(0), 1.0), (LoadCaseId(1), 0.5)]).unwrap()
            [&ElemId(0)],
        875.0,
    );
    close(
        simple_beam_q0_by_terms(&model, &[(LoadCaseId(1), 0.5), (LoadCaseId(0), 1.0)]).unwrap()
            [&ElemId(0)],
        875.0,
    );
    let live = model.load_cases[1].member.pop().unwrap();
    model.load_cases[0].member.push(live);
    all_helpers(&model, 1000.0);
    model.load_cases[0].member = vec![point(1500.0, 1000.0), point(1500.0, -1000.0)];
    all_helpers(&model, 0.0);
    model.load_cases[0].member[1].kind = MemberLoadKind::Point {
        a: 1500.0,
        p: 1000.0,
    };
    model.load_cases[0].member[1].dir = [0.0, 0.0, 1.0];
    all_helpers(&model, 0.0);
    model.load_cases[0].member[1].dir = [0.0, 1.0, 0.0];
    all_helpers(&model, 750.0);
    model.load_cases[0].member.remove(0);
    all_helpers(&model, 0.0);
    model.load_cases[0].member.clear();
    all_helpers(&model, 0.0);
}

#[test]
fn rc_q0_preserves_zero_resultant_first_moment_and_short_positive_intervals() {
    let mut model = model();
    for (a, b, w1, w2, expected) in [
        (0.0, 6000.0, 1.0, -1.0, 1000.0),
        (1000.0, 4000.0, 1.0, -1.0, 250.0),
        (0.0, 6000.0, 1.0, -1.0 + 1e-10, 1000.0000001),
        (0.0, 6000.0, 1.0, -0.9, 1100.0),
        (0.0, 6000.0, 1.0, 1.0, 3000.0),
        (0.0, 6000.0, 0.0, 1.0, 2000.0),
        (0.0, 1e-10, 1e13, 1e13, 1000.0),
        (-1000.0, 3000.0, 1.0, 1.0, 2250.0),
        (3000.0, 1000.0, 1.0, 1.0, 0.0),
    ] {
        model.load_cases[0].member = vec![MemberLoad::manual(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Distributed { a, b, w1, w2 },
        )];
        all_helpers(&model, expected);
    }
    for (a, expected) in [(-100.0, 1000.0), (7000.0, 1000.0)] {
        model.load_cases[0].member = vec![point(a, 1000.0)];
        all_helpers(&model, expected);
    }
}

#[test]
fn rc_q0_missing_input_never_returns_partial_or_old_values() {
    let mut valid = model();
    valid.load_cases[0].member = vec![point(1500.0, 1000.0)];
    valid.load_cases[1].member = vec![point(4500.0, 1000.0)];
    for invalid in 0..12 {
        let mut model = valid.clone();
        match invalid {
            0 => model.load_cases[0].member[0].dir = [0.0; 3],
            1 => model.load_cases[0].member[0].dir[0] = f64::NAN,
            2 => {
                model.load_cases[0].member[0].kind = MemberLoadKind::Point {
                    a: f64::NAN,
                    p: 1000.0,
                }
            }
            3 => {
                model.load_cases[0].member[0].kind = MemberLoadKind::Point {
                    a: 1500.0,
                    p: f64::INFINITY,
                }
            }
            4 => model.elements[0].nodes[0] = NodeId(99),
            5 => model.nodes[0].coord[0] = f64::NAN,
            6 => model.elements[0].local_axis.ref_vector[0] = f64::NAN,
            7 => model.sections[0].material = None,
            8 => model.load_cases[0].member[0].elem = ElemId(99),
            9 => model.nodes[1].coord = model.nodes[0].coord,
            10 => model.load_cases[0].member[0].dir = [f64::MAX; 3],
            _ => {
                model.load_cases[0].member[0].kind = MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 6000.0,
                    w1: f64::MAX,
                    w2: f64::MAX,
                }
            }
        }
        if matches!(invalid, 5 | 6 | 9) {
            for error in [
                simple_beam_q0_by_elem(&model, LoadCaseId(0)).unwrap_err(),
                simple_beam_q0_by_gravity_cases(&model).unwrap_err(),
                simple_beam_q0_by_terms(&model, &[(LoadCaseId(1), 1.0), (LoadCaseId(0), 1.0)])
                    .unwrap_err(),
            ] {
                assert!(error.contains("部材 ID 0"), "{invalid}: {error}");
            }
            continue;
        }
        assert!(
            !simple_beam_q0_by_elem(&model, LoadCaseId(0))
                .unwrap()
                .contains_key(&ElemId(0)),
            "{invalid}"
        );
        assert!(
            !simple_beam_q0_by_gravity_cases(&model)
                .unwrap()
                .contains_key(&ElemId(0)),
            "{invalid}"
        );
        assert!(
            !simple_beam_q0_by_terms(&model, &[(LoadCaseId(1), 1.0), (LoadCaseId(0), 1.0)])
                .unwrap()
                .contains_key(&ElemId(0)),
            "{invalid}"
        );
        model = valid.clone();
        close(
            simple_beam_q0_by_gravity_cases(&model).unwrap()[&ElemId(0)],
            1000.0,
        );
    }
    for terms in [
        vec![],
        vec![(LoadCaseId(99), 1.0)],
        vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), f64::NAN)],
    ] {
        assert!(!simple_beam_q0_by_terms(&valid, &terms)
            .unwrap()
            .contains_key(&ElemId(0)));
    }
    assert!(simple_beam_q0_by_elem(&valid, LoadCaseId(99))
        .unwrap()
        .is_empty());
}

#[test]
fn q0_public_helpers_reject_nonfinite_local_reference_before_returning_any_demand() {
    let mut valid = model();
    valid.load_cases[0].member = vec![point(1500.0, 1000.0)];
    for component in 0..3 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut invalid = valid.clone();
            invalid.elements[0].local_axis.ref_vector[component] = value;
            for result in [
                simple_beam_q0_by_elem(&invalid, LoadCaseId(0)),
                simple_beam_q0_by_gravity_cases(&invalid),
                simple_beam_q0_by_terms(&invalid, &[(LoadCaseId(0), 1.0)]),
            ] {
                let error = result.unwrap_err();
                assert!(error.contains("部材 ID 0"), "{error}");
                assert!(error.contains("非有限"), "{error}");
            }
        }
    }
    all_helpers(&valid, 750.0);
}

#[test]
fn q0_only_requires_local_frames_for_rc_beams_with_resolvable_terms() {
    let mut model = model();
    model.load_cases[0].member = vec![point(1500.0, 1000.0)];
    model.elements.push(ElementData {
        id: ElemId(1),
        section: None,
        local_axis: LocalAxis {
            ref_vector: [f64::NAN, 0.0, 0.0],
        },
        ..model.elements[0].clone()
    });
    all_helpers(&model, 750.0);

    model.elements[0].local_axis.ref_vector = [f64::NAN, 0.0, 0.0];
    assert!(simple_beam_q0_by_terms(&model, &[]).unwrap().is_empty());
    assert!(simple_beam_q0_by_elem(&model, LoadCaseId(99))
        .unwrap()
        .is_empty());
    model.load_cases[0].member.clear();
    let error = simple_beam_q0_by_elem(&model, LoadCaseId(0)).unwrap_err();
    assert!(error.contains("部材 ID 0") && error.contains("非有限"));

    model.sections[0].shape = None;
    model.load_cases[0].member = vec![point(1500.0, 1000.0)];
    all_helpers(&model, 750.0);
}

#[test]
fn q0_rejects_finite_unresolvable_rc_frame_and_preserves_large_valid_rotation() {
    let mut model = model();
    model.load_cases[0].member = vec![point(1500.0, 1000.0)];
    model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1e308];
    all_helpers(&model, 750.0);

    model.nodes[1].coord = [3000.0; 3];
    model.elements[0].local_axis.ref_vector = [1e308, 1e308, 1e308 - 1e292];
    for error in [
        simple_beam_q0_by_elem(&model, LoadCaseId(0)).unwrap_err(),
        simple_beam_q0_by_terms(&model, &[(LoadCaseId(0), 1.0)]).unwrap_err(),
        simple_beam_q0_by_gravity_cases(&model).unwrap_err(),
    ] {
        assert!(error.contains("部材 ID 0") && error.contains("単位直交基底"));
    }
}

#[test]
fn rc_q0_is_invariant_under_reversed_ends_and_joint_rotation_of_model_and_load() {
    let mut model = model();
    model.load_cases[0].member = vec![point(1500.0, 1000.0)];
    all_helpers(&model, 750.0);
    model.elements[0].nodes.reverse();
    model.load_cases[0].member[0] = point(4500.0, 1000.0);
    all_helpers(&model, 750.0);
    model.nodes[1].coord = [0.0, 0.0, 6000.0];
    model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
    model.load_cases[0].member[0].dir = [0.0, -1.0, 0.0];
    all_helpers(&model, 750.0);
}

#[test]
fn non_rc_values_and_map_presence_keep_existing_behavior() {
    let mut model = model();
    model.sections[0].shape = None;
    model.load_cases[0].member = vec![point(1500.0, 1000.0)];
    model.load_cases[1].member = vec![point(4500.0, 1000.0)];
    close(
        simple_beam_q0_by_gravity_cases(&model).unwrap()[&ElemId(0)],
        1500.0,
    );
    model.load_cases[0].member = vec![point(1500.0, 1000.0), point(1500.0, -1000.0)];
    close(
        simple_beam_q0_by_elem(&model, LoadCaseId(0)).unwrap()[&ElemId(0)],
        1500.0,
    );
    model.load_cases[0].member[0].dir = [1.0, 0.0, 0.0];
    model.load_cases[0].member.pop();
    assert!(simple_beam_q0_by_elem(&model, LoadCaseId(0))
        .unwrap()
        .is_empty());
    model.load_cases[0].member.clear();
    assert!(simple_beam_q0_by_elem(&model, LoadCaseId(0))
        .unwrap()
        .is_empty());
}
