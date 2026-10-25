use super::*;
use sepika_core::ids::LoadCaseId;
use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination, Model};

fn fixture() -> (Model, Vec<Vec<(ElemId, MemberForces)>>) {
    let mut model = super::tests::axial_column(-150000.0, false, [1.0, 0.0, 0.0]);
    model.sections[0].shape = Some(sepika_core::section_shape::SectionShape::CftBox {
        height: 100.0,
        width: 100.0,
        thick: 5.0,
        horizontal_thick: 5.0,
        corner_r: Some(0.0),
    });
    let model = Model {
        load_cases: [
            LoadCaseKind::Dead,
            LoadCaseKind::Dead,
            LoadCaseKind::Live,
            LoadCaseKind::LiveSeismic,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| LoadCase {
            id: LoadCaseId(i as u32),
            name: format!("任意{i}"),
            kind,
            nodal: vec![],
            member: vec![],
        })
        .collect(),
        combinations: vec![LoadCombination {
            name: "任意組合せ".into(),
            terms: vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0),
            ],
        }],
        ultimate_gravity_combination: Some(0),
        ..model
    };
    let responses = [
        [-100000.0, 10000.0, 0.0, 0.0, 2000000.0, 4000000.0],
        [-20000.0, -3000.0, 0.0, 0.0, 500000.0, 1000000.0],
        [-30000.0, 5000.0, 0.0, 0.0, 500000.0, 2000000.0],
        [-10000.0, 1000.0, 0.0, 0.0, 100000.0, 500000.0],
    ]
    .into_iter()
    .map(|f| {
        let mut end = f;
        for v in &mut end[1..] {
            *v = -*v;
        }
        vec![(
            ElemId(0),
            MemberForces {
                at: vec![(0.0, f), (1.0, end)],
            },
        )]
    })
    .collect();
    (model, responses)
}

#[test]
fn saved_frame_terms_combine_signed_components_before_demand_conversion() {
    let (mut model, forces) = fixture();
    let resolve =
        |model: &Model| complete_ultimate_static_demand(model, |id| Ok(forces[id.index()].clone()));
    let d = resolve(&model).unwrap()[0].1;
    assert_eq!(
        (d.n_axial, d.q_long, d.my, d.mz),
        (150000.0, Some(12000.0), 3000000.0, 7000000.0)
    );
    println!(
        "frame_terms observed N={} QL={:?} My={} Mz={}",
        d.n_axial, d.q_long, d.my, d.mz
    );
    model.load_cases.reverse();
    for case in &mut model.load_cases {
        case.name = "改名".into();
    }
    assert_eq!(resolve(&model).unwrap()[0].1.n_axial, 150000.0);
    model.combinations[0].terms = vec![
        (LoadCaseId(0), 0.5),
        (LoadCaseId(0), 0.5),
        (LoadCaseId(1), 1.0),
        (LoadCaseId(2), 1.0),
        (LoadCaseId(3), 0.0),
    ];
    assert_eq!(resolve(&model).unwrap()[0].1.q_long, Some(12000.0));
    model.combinations[0].terms[2].1 = 0.5;
    assert!(resolve(&model)
        .unwrap_err()
        .starts_with("gravity_combination_unsupported:"));
    let arithmetic = crate::complete_design_gravity_forces(
        &[
            (LoadCaseId(0), 1.0),
            (LoadCaseId(1), 0.5),
            (LoadCaseId(2), 1.0),
        ],
        |id| Some(forces[id.index()].clone()),
    )
    .unwrap();
    let ql = q_long_map_from_member_forces(&arithmetic);
    let d = member_demand_from_static_forces(&arithmetic, Some(&ql))[0].1;
    assert_eq!(
        (d.n_axial, d.q_long, d.mz),
        (140000.0, Some(13500.0), 6500000.0)
    );
    println!(
        "arithmetic_only_half_dead observed N={} QL={:?} Mz={}",
        d.n_axial, d.q_long, d.mz
    );
}

#[test]
fn incomplete_cases_positions_and_nonfinite_components_never_form_partial_demand() {
    let (model, forces) = fixture();
    assert!(
        complete_ultimate_static_demand(&model, |id| if id == LoadCaseId(1) {
            Err("未解析".into())
        } else {
            Ok(forces[id.index()].clone())
        })
        .unwrap_err()
        .contains("case=1")
    );
    for corruption in 0..9 {
        let mut broken = forces.clone();
        match corruption {
            0 => broken[1].clear(),
            1 => {
                broken[1][0].1.at.pop();
            }
            2 => broken[1][0].1.at[0].0 = f64::NAN,
            3 => broken[1][0].1.at[0].1[0] = f64::INFINITY,
            4 => {
                let duplicate = broken[1][0].clone();
                broken[1].push(duplicate);
            }
            5 => broken[1][0].1.at.insert(1, (0.5, [0.0; 6])),
            6 => broken[1][0].1.at[1].0 = 0.0,
            7 => broken[1][0].1.at.clear(),
            _ => broken[1][0].1.at[1].0 = 1.5,
        }
        let error = complete_ultimate_static_demand(&model, |id| Ok(broken[id.index()].clone()))
            .unwrap_err();
        let cause = match corruption {
            0 => "gravity_member_missing:",
            4 => "gravity_member_duplicate:",
            5 => "gravity_grid_mismatch:",
            _ => "gravity_positions_invalid:",
        };
        for expected in [
            cause,
            "case=1",
            "member=0",
            "purpose=frame_long",
            "mode=static_hinge",
            "selected_terms=[case=0 coefficient=1",
        ] {
            assert!(
                error.contains(expected),
                "corruption={corruption} missing {expected}: {error}"
            );
        }
    }
    let mut overflow = forces.clone();
    for case in &mut overflow[..3] {
        case[0].1.at[0].1[0] = f64::MAX;
    }
    let error =
        complete_ultimate_static_demand(&model, |id| Ok(overflow[id.index()].clone())).unwrap_err();
    for expected in [
        "gravity_sum_nonfinite:",
        "member=0",
        "purpose=frame_long",
        "selected_terms=[case=0 coefficient=1",
    ] {
        assert!(error.contains(expected), "missing {expected}: {error}");
    }
    let mut mixed = model.clone();
    let mut outside = mixed.elements[0].clone();
    outside.id = ElemId(1);
    outside.section = None;
    mixed.elements.push(outside);
    let mut extra = forces.clone();
    for list in &mut extra {
        list.push((ElemId(1), MemberForces { at: vec![] }));
    }
    assert_eq!(
        complete_ultimate_static_demand(&mixed, |id| Ok(extra[id.index()].clone()))
            .unwrap()
            .len(),
        1
    );
    let mut invalid = model.clone();
    invalid.combinations[0].terms.push((LoadCaseId(99), 0.0));
    assert!(
        complete_ultimate_static_demand(&invalid, |id| Ok(forces[id.index()].clone())).is_err()
    );
    invalid = model.clone();
    invalid.combinations[0].terms[0].1 = f64::NAN;
    assert!(
        complete_ultimate_static_demand(&invalid, |id| Ok(forces[id.index()].clone())).is_err()
    );
    invalid = model.clone();
    invalid.ultimate_gravity_combination = None;
    assert!(complete_ultimate_static_demand(&invalid, |_| panic!("未指定時は求解しない")).is_err());
    invalid = model.clone();
    invalid.combinations[0].terms = vec![(LoadCaseId(3), 1.0)];
    assert!(
        complete_ultimate_static_demand(&invalid, |_| panic!("地震用積載は求解しない")).is_err()
    );
}

#[test]
fn actual_vertical_column_frame_gravity_preserves_compression_and_tension_in_all_axes() {
    for load in [-150000.0, 150000.0] {
        for reversed in [false, true] {
            for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                let mut model = super::tests::axial_column(load, reversed, axis);
                model.sections[0].shape = Some(sepika_core::section_shape::SectionShape::CftBox {
                    height: 100.0,
                    width: 100.0,
                    thick: 5.0,
                    horizontal_thick: 5.0,
                    corner_r: Some(0.0),
                });
                let mut steel = model.materials[0].clone();
                steel.id = sepika_core::ids::MaterialId(1);
                model.sections[0].steel_material = Some(steel.id);
                model.materials[0].category = sepika_core::model::MaterialCategory::Concrete;
                model.materials[0].young = 25000.0;
                model.materials[0].fc = Some(24.0);
                model.materials[0].fy = None;
                model.materials.push(steel);
                model.load_cases.push(LoadCase {
                    id: LoadCaseId(1),
                    name: "明示P0".into(),
                    kind: LoadCaseKind::Live,
                    nodal: vec![],
                    member: vec![],
                });
                model.combinations = vec![LoadCombination {
                    name: "架構".into(),
                    terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
                }];
                model.ultimate_gravity_combination = Some(0);
                let d = complete_ultimate_static_demand(&model, |id| {
                    crate::compute::compute_linear_static(model.clone(), id)
                        .map(|r| {
                            println!(
                                "physical_static case={} observed_raw_N={:?}",
                                id.0,
                                r.member_forces
                                    .first()
                                    .and_then(|(_, forces)| forces.at.first())
                                    .map(|(_, values)| values[0])
                            );
                            r.member_forces
                        })
                        .map_err(|e| e.to_string())
                })
                .unwrap();
                println!(
                    "physical_column input_load_z={} reversed={} axis={:?} observed_demandN={} QL={:?}",
                    load, reversed, axis, d[0].1.n_axial, d[0].1.q_long
                );
                assert!((d[0].1.n_axial + load).abs() < 1e-6);
                assert_eq!(d[0].1.q_long, Some(0.0));
            }
        }
    }
}
