use std::collections::HashMap;

use sepika_core::dof::Dof6Mask;
use sepika_core::ids::ElemId;
use sepika_core::model::{EndCondition, Model};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions, QdMethod,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod asymmetric;
#[path = "fixtures/rc_beam_qd1_model.rs"]
mod input;
use input::{add_columns_at_outer_nodes, extend_j_column, model};

fn check(
    model: &Model,
    method: QdMethod,
    q0: Option<f64>,
    rows: &[(f64, [f64; 6])],
    ql: f64,
) -> Vec<(ElemId, f64, CheckOutcome)> {
    let response = [(ElemId(0), MemberForces { at: rows.to_vec() })];
    let gravity = [(
        ElemId(0),
        MemberForces {
            at: rows
                .iter()
                .map(|(pos, _)| (*pos, [0.0, ql, 0.0, 0.0, 0.0, 0.0]))
                .collect(),
        },
    )];
    let q0_by_elem = q0.map(|value| HashMap::from([(ElemId(0), value)]));
    run_member_design_checks(
        model,
        &response,
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            qd_method: method,
            long_member_forces: Some(&gravity),
            q_simple_by_elem: q0_by_elem.as_ref(),
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
}

fn assert_shear_n(outcome: &CheckOutcome, expected_n: f64) {
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    let shear = result
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Shear)
        .expect("強軸せん断が算定される");
    let qa_displayed: f64 = shear
        .detail
        .strip_prefix("QA=")
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let actual_n = qa_displayed * shear.ratio;
    assert!(
        (actual_n - expected_n).abs() < 0.1,
        "{actual_n} != {expected_n}: {shear:?}"
    );
}

#[test]
fn opposite_end_moment_strengths_govern_qd1_independent_of_response_sign_and_station() {
    let model = model();
    let stations = [250.0 / 4500.0, 0.5, 1.0 - 250.0 / 4500.0];
    // RC規準（解15.7）で2-D19は97.854 kN·m、4-D19は195.708 kN·m。内法4 mで73.391 kN。
    let qd1: f64 = 73_390.556_25;
    for (qy_n, qd2) in [(30_000.0, 70_000.0), (60_000.0, 115_000.0)] {
        for mz in [10e6, -10e6, 0.0] {
            let rows = stations.map(|pos| (pos, [0.0, qy_n, 0.0, 0.0, 0.0, mz]));
            for (method, q0, expected) in [
                (QdMethod::Qd1, Some(0.0), qd1),
                (QdMethod::Qd1, Some(8_000.0), qd1 + 8_000.0),
                (QdMethod::Qd2, None, qd2),
                (QdMethod::Min, Some(0.0), qd1.min(qd2)),
            ] {
                let outcomes = check(&model, method, q0, &rows, -10_000.0);
                assert_eq!(outcomes.len(), 3, "{method:?} {outcomes:?}");
                for ((_, pos, outcome), station) in outcomes.iter().zip(stations) {
                    assert_eq!(*pos, station);
                    assert_shear_n(outcome, expected);
                }
            }
        }
    }
}

#[test]
fn unavailable_qd1_preserves_independent_bending_failure_and_never_switches_to_qd2() {
    type InvalidQd1 = (&'static str, fn(&mut Model));
    let variants: [InvalidQd1; 9] = [
        ("強軸曲げが解放", |m| {
            m.elements[0].end_cond[0] = EndCondition::Pinned
        }),
        ("強軸曲げが解放", |m| {
            m.elements[0].end_cond[1] = EndCondition::SemiRigid { k_theta: 0.0 }
        }),
        ("自由端", |m| m.nodes[1].restraint = Dof6Mask::FREE),
        ("回転自由", |m| m.nodes[1].restraint = Dof6Mask::PINNED),
        ("フェース距離が未算定", |m| {
            m.elements[0].rigid_zone.face_j = None
        }),
        ("正の内法長さ", |m| {
            m.elements[0].rigid_zone.face_j = Some(4250.0)
        }),
        ("有限な非負値", |m| {
            m.elements[0].rigid_zone.face_j = Some(f64::NAN)
        }),
        ("ハンチ付き", |m| {
            use sepika_core::model::{Haunch, MemberDetailAttr};
            let mut detail = MemberDetailAttr::new(ElemId(0));
            detail.haunch_i = Some(Haunch {
                length: 600.0,
                depth_increase: 200.0,
                width_increase: 0.0,
            });
            m.member_detail_attrs.push(detail);
        }),
        ("上端引張降伏曲げ", |m| {
            let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut m.sections[0].shape else {
                unreachable!()
            };
            rebar.top.clear();
        }),
    ];
    let rows = [(0.5, [0.0, 30_000.0, 0.0, 0.0, 0.0, 1e9])];
    for (expected_reason, change) in variants {
        let mut model = model();
        change(&mut model);
        for method in [QdMethod::Qd1, QdMethod::Min] {
            let outcomes = check(&model, method, Some(0.0), &rows, -10_000.0);
            let [(_, _, CheckOutcome::Checked(ng)), (_, _, CheckOutcome::Skipped { reason })] =
                outcomes.as_slice()
            else {
                panic!("{method:?} {outcomes:?}");
            };
            assert!(reason.contains(expected_reason), "{reason}");
            assert!(ng
                .components
                .iter()
                .any(|c| c.kind == CheckKind::Bending && c.ratio > 1.0));
            assert!(ng.components.iter().all(|c| c.kind != CheckKind::Shear));
        }
        let outcomes = check(&model, QdMethod::Qd2, None, &rows, -10_000.0);
        assert_eq!(outcomes.len(), 1, "{outcomes:?}");
        assert_shear_n(&outcomes[0].2, 70_000.0);
    }
}

#[test]
fn connected_columns_define_moment_transfer_and_physical_clear_span() {
    let mut model = model();
    add_columns_at_outer_nodes(&mut model, [1.0, 1.0, 0.0]);
    let rows = [(0.5, [0.0, 30_000.0, 0.0, 0.0, 0.0, 10e6])];
    let outcomes = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    // 45°の500角柱は梁軸上の柱面が両端とも353.553 mm。内法3792.893 mmで77.398 kN。
    assert_shear_n(&outcomes[0].2, 77_398.0);

    let mut missing_shape = model.clone();
    missing_shape.sections[1].shape = None;
    let missing = check(&missing_shape, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert!(
        matches!(&missing[0].2, CheckOutcome::Skipped { reason } if reason.contains("実柱面")),
        "{missing:?}"
    );

    let mut extended = model.clone();
    extend_j_column(&mut extended);
    for restraint in [Dof6Mask::FIXED, Dof6Mask::FREE] {
        extended.nodes[4].restraint = restraint;
        for (method, expected_n) in [
            (QdMethod::Qd1, 77_398.0),
            (QdMethod::Min, 70_000.0),
            (QdMethod::Qd2, 70_000.0),
        ] {
            let outcomes = check(&extended, method, Some(0.0), &rows, -10_000.0);
            assert_eq!(outcomes.len(), 1, "{outcomes:?}");
            assert_shear_n(&outcomes[0].2, expected_n);
        }
    }

    extended.elements[0].end_cond[1] = EndCondition::Pinned;
    let beam_pin = check(&extended, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert!(
        matches!(&beam_pin[0].2, CheckOutcome::Skipped { reason } if reason.contains("強軸曲げが解放")),
        "{beam_pin:?}"
    );

    model.elements[2].end_cond[1] = EndCondition::Pinned;
    let outcomes = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert!(
        matches!(&outcomes[0].2, CheckOutcome::Skipped { reason } if reason.contains("伝達先")),
        "{outcomes:?}"
    );

    let mut off_axis_column = model.elements[2].clone();
    off_axis_column.id = ElemId(3);
    off_axis_column.end_cond[1] = EndCondition::Fixed;
    off_axis_column.end_offsets_mm = [[0.0, 1000.0, 0.0]; 2];
    model.elements.push(off_axis_column);
    let adjacency = sepika_core::adjacency::NodeAdjacency::primary_frame(&model);
    let faces =
        sepika_core::face_distance::girder_column_faces(&model, &model.elements[0], &adjacency)
            .unwrap();
    assert_eq!(faces[1].as_ref().unwrap().columns, [ElemId(2)]);
    let no_matching_column = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert!(
        matches!(&no_matching_column[0].2, CheckOutcome::Skipped { reason } if reason.contains("伝達先")),
        "{no_matching_column:?}"
    );
    model.elements[2].end_cond[1] = EndCondition::Fixed;
    let matching_column = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert_shear_n(&matching_column[0].2, 77_398.0);
}

#[test]
fn oblique_girder_does_not_establish_qd1_support_or_a_physical_face() {
    use sepika_core::ids::NodeId;

    let mut model = model();
    let mut remote = model.nodes[1].clone();
    remote.id = NodeId(2);
    remote.coord = [7000.0, 2000.0, 0.0];
    model.nodes.push(remote);
    let mut oblique = model.elements[0].clone();
    oblique.id = ElemId(1);
    oblique.nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    model.elements.push(oblique);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    let rows = [(0.5, [0.0, 30_000.0, 0.0, 0.0, 0.0, 10e6])];

    for (restraint, expected) in [(Dof6Mask::FREE, "伝達先"), (Dof6Mask::FIXED, "実端面")] {
        model.nodes[1].restraint = restraint;
        for method in [QdMethod::Qd1, QdMethod::Min] {
            let outcomes = check(&model, method, Some(0.0), &rows, -10_000.0);
            assert!(
                matches!(&outcomes[0].2, CheckOutcome::Skipped { reason } if reason.contains(expected)),
                "{outcomes:?}"
            );
        }
        let qd2 = check(&model, QdMethod::Qd2, None, &rows, -10_000.0);
        assert_shear_n(&qd2[0].2, 70_000.0);
    }
}

#[test]
fn a_slab_assigned_to_part_of_the_beam_blocks_qd1_and_rectangular_ultimate_shear() {
    use sepika_core::ids::{NodeId, SectionId, SlabId};
    use sepika_core::model::{PlateAssignment, Slab, SlabPlate, SlabShape, SupportMemberId};
    use sepika_design_jp::ultimate::{
        collect_rc_ultimate_checks, MemberDemand, UltimateShearOptions,
    };

    let mut model = model();
    for (index, coord) in [
        [1125.0, 0.0, 0.0],
        [1125.0, 2000.0, 0.0],
        [3375.0, 2000.0, 0.0],
        [3375.0, 0.0, 0.0],
    ]
    .into_iter()
    .enumerate()
    {
        let mut node = model.nodes[0].clone();
        node.id = NodeId(2 + index as u32);
        node.coord = coord;
        model.nodes.push(node);
    }
    for index in 0..3_u32 {
        let mut boundary = model.elements[0].clone();
        boundary.id = ElemId(1 + index);
        boundary.nodes = [NodeId(2 + index), NodeId(3 + index)].into_iter().collect();
        model.elements.push(boundary);
    }
    let rebuilt = model.rebuild_floor_assignment_regions();
    assert_eq!(rebuilt.rejection, None);
    assert_eq!(rebuilt.regions, 1);
    let region = &model.floor_assignment_regions.regions[0];
    assert!(region.boundary.iter().any(|edge| {
        edge.support == SupportMemberId::Primary(ElemId(0))
            && matches!(edge.span, [0.25, 0.75] | [0.75, 0.25])
    }));
    let rows = [(0.5, [0.0, 30_000.0, 0.0, 0.0, 0.0, 10e6])];
    model.ultimate_gravity_combination = Some(0);
    let ultimate = |model: &Model| {
        let demands: Vec<_> = model
            .elements
            .iter()
            .map(|elem| {
                (
                    elem.id,
                    MemberDemand {
                        min_axial_for_shear_n: Some(0.0),
                        ..MemberDemand::from_pushover(0.0, 10e6, 0.0, 30_000.0, 0.0, 0.0)
                    },
                )
            })
            .collect();
        let opts = UltimateShearOptions {
            include_bond: false,
            ..Default::default()
        };
        collect_rc_ultimate_checks(model, &demands, &opts)
            .unwrap()
            .remove(0)
    };
    for assignment in [PlateAssignment::Unset, PlateAssignment::NoPlate] {
        model.floor_assignment_regions.regions[0].assignment = assignment;
        let qd1 = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
        assert_shear_n(&qd1[0].2, 73_390.556_25);
        let result = ultimate(&model);
        assert!(result.ok && result.qsu.is_finite(), "{result:?}");
    }

    let slab_id = SlabId(0);
    let slab_section = SectionId(1);
    let mut section =
        SectionShape::RcSlab { thickness: 150.0 }.to_section(slab_section, "S1".into());
    section.material = model.sections[0].material;
    model.sections.push(section);
    model.slabs.push(Slab {
        id: slab_id,
        shape: SlabShape::Enclosed,
        plate: SlabPlate {
            section: Some(slab_section),
            ..Default::default()
        },
        reference_story: None,
        vertical_eccentricity_mm: None,
        tip_loads: Vec::new(),
    });
    model.floor_assignment_regions.regions[0].assignment = PlateAssignment::Plate(slab_id);
    for method in [QdMethod::Qd1, QdMethod::Min] {
        let outcomes = check(&model, method, Some(0.0), &rows, -10_000.0);
        assert!(
            matches!(&outcomes[0].2, CheckOutcome::Skipped { reason } if reason.contains("スラブ筋")),
            "{outcomes:?}"
        );
    }
    let qd2 = check(&model, QdMethod::Qd2, None, &rows, -10_000.0);
    assert_shear_n(&qd2[0].2, 70_000.0);
    let result = ultimate(&model);
    assert!(
        !result.ok && result.qsu.is_nan() && result.shear_margin.is_nan(),
        "{result:?}"
    );
    assert!(result.unchecked_reason.unwrap().contains("スラブ付きRC梁"));
}

#[test]
fn damage_control_beam_uses_signed_combined_shear_without_qd_selection() {
    let model = model();
    for (gravity_n, combined_n, expected_n) in [
        (50_000.0, 200_000.0, 200_000.0),
        (-50_000.0, 100_000.0, 100_000.0),
        (50_000.0, -100_000.0, 100_000.0),
    ] {
        let response = [(
            ElemId(0),
            MemberForces {
                at: vec![(0.5, [0.0, combined_n, 0.0, 0.0, 0.0, 30e6])],
            },
        )];
        let gravity = [(
            ElemId(0),
            MemberForces {
                at: vec![(0.5, [0.0, gravity_n, 0.0, 0.0, 0.0, 0.0])],
            },
        )];
        for method in [QdMethod::Qd1, QdMethod::Qd2, QdMethod::Min] {
            for long_member_forces in [Some(gravity.as_slice()), Some(&[][..]), None] {
                let report = run_member_design_checks(
                    &model,
                    &response,
                    &[],
                    &MemberDesignCheckOptions {
                        term: LoadTerm::Short,
                        rc_damage_control: true,
                        qd_method: method,
                        long_member_forces,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(report.member_checks.len(), 1);
                assert_shear_n(&report.member_checks[0].2, expected_n);
            }
        }
    }
}

fn split_beam() -> Model {
    use sepika_core::ids::NodeId;

    let mut model = model();
    let mut midpoint = model.nodes[0].clone();
    midpoint.id = NodeId(2);
    midpoint.coord[0] = 2250.0;
    midpoint.restraint = Dof6Mask::FREE;
    model.nodes.push(midpoint);

    let mut right = model.elements[0].clone();
    right.id = ElemId(1);
    right.nodes[0] = NodeId(2);
    right.rigid_zone.face_i = Some(0.0);
    model.elements[0].nodes[1] = NodeId(2);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    model.elements.push(right);
    model.girder_groups = vec![vec![ElemId(0), ElemId(1)]];
    model
}

fn check_split(
    model: &Model,
    method: QdMethod,
    group_q0: Option<Result<f64, String>>,
) -> Vec<(ElemId, f64, CheckOutcome)> {
    let response = [
        (
            ElemId(0),
            [
                model.elements[0].rigid_zone.face_i_or_zero() / 2250.0,
                0.5,
                1.0,
            ],
        ),
        (
            ElemId(1),
            [
                0.0,
                0.5,
                1.0 - model.elements[1].rigid_zone.face_j_or_zero() / 2250.0,
            ],
        ),
    ]
    .map(|(id, stations)| {
        (
            id,
            MemberForces {
                at: stations
                    .into_iter()
                    .map(|pos| (pos, [0.0, 80_000.0, 0.0, 0.0, 0.0, 10e6]))
                    .collect(),
            },
        )
    });
    let overrides = sepika_design_jp::girder_group_overrides(model, &response);
    let group_q0 = group_q0.map(|q0| HashMap::from([(ElemId(0), q0.clone()), (ElemId(1), q0)]));
    run_member_design_checks(
        model,
        &response,
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            qd_method: method,
            long_member_forces: Some(&response),
            q_simple_by_elem: Some(&HashMap::from([
                (ElemId(0), 10_000.0),
                (ElemId(1), 10_000.0),
            ])),
            rc_grouped_beam_q0: group_q0.as_ref(),
            girder_group_overrides: Some(&overrides),
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
}

#[test]
fn splitting_rc_beam_preserves_qd1_and_checks_both_outer_faces() {
    let stations = [250.0 / 4500.0, 0.5, 1.0 - 250.0 / 4500.0];
    let rows = stations.map(|pos| (pos, [0.0, 80_000.0, 0.0, 0.0, 0.0, 10e6]));
    let single = check(&model(), QdMethod::Qd1, Some(20_000.0), &rows, 80_000.0);
    let split = check_split(&split_beam(), QdMethod::Qd1, Some(Ok(20_000.0)));
    let expected_n = 93_390.556_25;
    assert_eq!(single.len(), 3);
    assert_eq!(split.len(), 6, "{split:?}");
    for (_, _, outcome) in single.iter().chain(&split) {
        assert_shear_n(outcome, expected_n);
    }
    assert_eq!(split[0].1, 250.0 / 2250.0);
    assert_eq!(split[5].1, 1.0 - 250.0 / 2250.0);
}

#[test]
fn grouped_rc_beam_uses_the_larger_of_opposite_yield_combinations_at_outer_sections() {
    use sepika_core::ids::SectionId;

    let mut model = split_beam();
    let mut right = model.sections[0].clone();
    right.id = SectionId(1);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut right.shape else {
        unreachable!();
    };
    rebar.top = vec![6];
    rebar.bottom = vec![3];
    model.sections.push(right);
    model.elements[1].section = Some(SectionId(1));
    let outcomes = check_split(&model, QdMethod::Qd1, Some(Ok(20_000.0)));
    // 同一有効せいの左2/4本・右6/3本。逆符号の組の合計は10本相当と5本相当。
    // 10-D19の0.9Atσydは489.270 kN·mとなり、内法4 mのせん断に20 kNを加える。
    assert_eq!(outcomes.len(), 6, "{outcomes:?}");
    for (_, _, outcome) in outcomes {
        assert_shear_n(&outcome, 142_317.593_75);
    }
}

#[test]
fn grouped_rc_beam_qd1_does_not_accept_an_element_only_q0_or_unverified_span() {
    let mut reversed_axis = split_beam();
    reversed_axis.elements[1].local_axis.ref_vector = [0.0, 0.0, -1.0];
    let mut intermediate_release = split_beam();
    intermediate_release.elements[1].end_cond[0] = EndCondition::Pinned;
    let mut missing_face = split_beam();
    missing_face.elements[1].rigid_zone.face_j = None;
    let variants = [
        (split_beam(), None, "全スパンの重力反力"),
        (
            split_beam(),
            Some(Err("中間節点荷重".into())),
            "中間節点荷重",
        ),
        (reversed_axis, Some(Ok(20_000.0)), "局所軸の向き"),
        (intermediate_release, Some(Ok(20_000.0)), "強軸曲げの連続性"),
        (missing_face, Some(Ok(20_000.0)), "フェース距離が未算定"),
    ];
    for (model, q0, expected) in variants {
        for method in [QdMethod::Qd1, QdMethod::Min] {
            let outcomes = check_split(&model, method, q0.clone());
            assert!(outcomes.len() >= 6, "{outcomes:?}");
            assert!(
                outcomes
                    .iter()
                    .filter(|(_, _, outcome)| matches!(
                        outcome, CheckOutcome::Skipped { reason } if reason.contains(expected)
                    ))
                    .count()
                    >= 6,
                "{expected}: {outcomes:?}"
            );
            assert!(outcomes.iter().all(|(_, _, outcome)| !matches!(
                outcome, CheckOutcome::Checked(result) if result.components.iter().any(|c| c.kind == CheckKind::Shear)
            )));
        }
    }
}

#[test]
fn grouped_rc_qd1_requires_transverse_reaction_support_at_each_outer_end() {
    use sepika_core::dof::Dof;

    let mut directly_supported = split_beam();
    let mut column_supported = split_beam();
    add_columns_at_outer_nodes(&mut column_supported, [1.0, 0.0, 0.0]);
    // 反平行な柱軸も同じ鉛直反力を伝える。
    column_supported.elements[3].nodes.swap(0, 1);

    for (model, support_index) in [(&mut directly_supported, 1), (&mut column_supported, 4)] {
        // 0.5 m角の柱面も、直接支持として入力したフェースも、節点芯から各0.25 m。
        for stage in [0, 1, 2] {
            model.nodes[support_index]
                .restraint
                .set(Dof::Uz, stage == 0);
            model.nodes[support_index].support_spring =
                (stage == 2).then_some([0.0, 0.0, 1.0e6, 0.0, 0.0, 0.0]);
            for method in [QdMethod::Qd1, QdMethod::Min] {
                let outcomes = check_split(model, method, Some(Ok(20_000.0)));
                assert_eq!(outcomes.len(), 6, "{outcomes:?}");
                if stage == 1 {
                    assert!(outcomes.iter().all(|(_, _, outcome)| matches!(
                        outcome, CheckOutcome::Skipped { reason } if reason.contains("局所Qy方向の反力")
                    )), "{outcomes:?}");
                } else {
                    let expected = if method == QdMethod::Qd1 {
                        93_390.556_25
                    } else {
                        80_000.0
                    };
                    for (_, _, outcome) in outcomes {
                        assert_shear_n(&outcome, expected);
                    }
                }
            }
        }
    }
}
