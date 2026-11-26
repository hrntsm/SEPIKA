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
use input::{
    add_columns_at_outer_nodes, beam_with_recorded_continuous_slab_rebar, extend_j_column, model,
};

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
fn a_slab_assigned_to_part_of_the_beam_warns_for_qd1_and_blocks_rectangular_ultimate_shear() {
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
    for (method, q0, expected_n) in [
        (QdMethod::Qd1, Some(0.0), 73_390.556_25),
        (QdMethod::Qd1, Some(1_000_000.0), 1_073_390.556_25),
        (QdMethod::Min, Some(0.0), 70_000.0),
        (QdMethod::Qd2, None, 70_000.0),
    ] {
        let outcomes = check(&model, method, q0, &rows, -10_000.0);
        assert_shear_n(&outcomes[0].2, expected_n);
        let CheckOutcome::Checked(result) = &outcomes[0].2 else {
            unreachable!();
        };
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Shear)
            .unwrap();
        assert_eq!(shear.ratio <= 1.0, q0 != Some(1_000_000.0));
        assert_eq!(
            shear.detail.contains("スラブ筋の寄与を確認できず"),
            method != QdMethod::Qd2
        );
        assert_eq!(shear.detail.contains("過小評価"), method != QdMethod::Qd2);
    }
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

#[test]
fn recorded_slab_reinforcement_raises_mechanism_demand_in_both_bending_directions() {
    let model = beam_with_recorded_continuous_slab_rebar();
    let base_n = 48_927.037_5;
    // 協力幅573.69 mmに入るのはy=300 mmの筋だけ。D13・d=570 mmで
    // 上端引張時のMyは22.424 kN·m増える。
    for mz in [-10e6, 10e6] {
        let rows = [(0.5, [0., 60_000., 0., 0., 0., mz])];
        let qd1 = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
        assert_shear_n(&qd1[0].2, base_n + 3_737.333_25);
        let qd2 = check(&model, QdMethod::Qd2, None, &rows, -10_000.0);
        assert_shear_n(&qd2[0].2, 115_000.0);
    }
}

#[test]
fn only_confirmed_empty_layout_avoids_a_slab_qd1_fallback_warning() {
    use sepika_core::model::PlateAssignment;
    use sepika_core::section_shape::StandardRebar;

    let model = beam_with_recorded_continuous_slab_rebar();
    let rows = [(0.5, [0., 60_000., 0., 0., 0., 10e6])];
    let owner = model
        .slabs
        .iter()
        .position(|slab| {
            !slab
                .plate
                .top_reinforcement
                .as_ref()
                .unwrap()
                .straight_bars
                .is_empty()
        })
        .unwrap();
    let anchor_j = model
        .slabs
        .iter()
        .position(|slab| {
            model
                .resolved_slab_placement(slab.id)
                .unwrap()
                .top_boundary
                .iter()
                .any(|p| p[0] == 10_500.0)
        })
        .unwrap();
    let farther_i = model
        .slabs
        .iter()
        .position(|slab| {
            model
                .resolved_slab_placement(slab.id)
                .unwrap()
                .top_boundary
                .iter()
                .any(|p| p[0] == -8_000.0)
        })
        .unwrap();
    for case in 0..20 {
        let mut case_model = model.clone();
        let layout = case_model.slabs[owner]
            .plate
            .top_reinforcement
            .as_mut()
            .unwrap();
        match case {
            0 => layout.straight_bars.clear(),
            1 => case_model.slabs[owner].plate.top_reinforcement = None,
            2 => {
                case_model.slabs[anchor_j]
                    .plate
                    .top_reinforcement
                    .as_mut()
                    .unwrap()
                    .monolithic_pour = "2F-B".into()
            }
            3 => layout.straight_bars[0].ends_mm[1][0] = 6_700.0,
            4 => layout.straight_bars[0].yield_strength_n_mm2 = 390.0,
            5 => layout.straight_bars[0].size = StandardRebar::D41,
            6 => {
                case_model.floor_assignment_regions.regions[anchor_j].assignment =
                    PlateAssignment::NoPlate
            }
            7 => layout.straight_bars.push(layout.straight_bars[0].clone()),
            8 => {
                let mut outside = std::mem::take(&mut layout.straight_bars);
                outside
                    .iter_mut()
                    .for_each(|bar| bar.ends_mm[0][0] = -6_400.0);
                case_model.slabs[farther_i]
                    .plate
                    .top_reinforcement
                    .as_mut()
                    .unwrap()
                    .straight_bars = outside;
            }
            9 => {
                layout.straight_bars[1].ends_mm[0][0] = 1_000.0;
                layout.straight_bars[1].ends_mm[1][0] = 3_000.0;
            }
            10 | 13 => {
                let mut concrete = case_model.materials[0].clone();
                concrete.id = sepika_core::ids::MaterialId(case_model.materials.len() as u32);
                concrete.fc = Some(if case == 10 { 61.0 } else { 17.0 });
                case_model.sections.last_mut().unwrap().material = Some(concrete.id);
                case_model.materials.push(concrete);
            }
            11 => {
                for end in &mut layout.straight_bars[0].ends_mm {
                    end[2] = 293.0;
                }
            }
            12 => {
                let mut upper = layout.straight_bars[0].clone();
                upper.size = StandardRebar::D10;
                upper.ends_mm = [[-1_000.0, 100.0, 285.0], [-1_000.0, 1_800.0, 285.0]];
                layout.straight_bars.push(upper);
            }
            14 => layout.straight_bars[0].bond_yield_strength_n_mm2 = None,
            15 => {
                for end in &mut layout.straight_bars[1].ends_mm {
                    end[2] = 260.0;
                }
            }
            16 => layout.straight_bars[0].bond_yield_strength_n_mm2 = Some(330.0),
            17 => layout.straight_bars[0].bond_yield_strength_n_mm2 = Some(345.0),
            18 | 19 => {
                let name = if case == 18 { "任意材種" } else { "SR295" };
                layout.straight_bars[0].grade = name.into();
                let mut shear = case_model.materials[1].clone();
                shear.id = sepika_core::ids::MaterialId(case_model.materials.len() as u32);
                case_model.sections[0].shear_rebar_material = Some(shear.id);
                case_model.materials.push(shear);
                case_model.materials[1].name = name.into();
            }
            _ => unreachable!(),
        }
        let result = check(&case_model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
        if case == 19 {
            assert_eq!(
                sepika_design_jp::rc::beam_qd1_slab_end_moments(
                    &case_model,
                    &sepika_core::adjacency::NodeAdjacency::primary_frame(&case_model),
                    &case_model.elements[0],
                ),
                None,
            );
            assert!(
                matches!(&result[0].2, CheckOutcome::Skipped { reason } if reason.contains("丸鋼材種"))
            );
            continue;
        }
        assert_shear_n(&result[0].2, 48_927.037_5);
        let CheckOutcome::Checked(result) = &result[0].2 else {
            unreachable!()
        };
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Shear)
            .unwrap();
        assert_eq!(
            shear.detail.contains("過小評価"),
            case != 0,
            "case {case}: {shear:?}"
        );
    }
}

#[test]
fn attached_slab_sharing_a_support_line_warns_even_without_a_full_member_edge() {
    use sepika_core::model::{LoadTransfer, PlateAssignment, RegionAnchor, SlabShape};

    let mut model = beam_with_recorded_continuous_slab_rebar();
    let mut slab = model.slabs.remove(0);
    model.slabs.clear();
    for region in &mut model.floor_assignment_regions.regions {
        region.assignment = PlateAssignment::NoPlate;
    }
    let left = model
        .nodes
        .iter()
        .find(|node| node.coord == [-4_000.0, 0.0, 0.0])
        .unwrap()
        .id;
    slab.shape = SlabShape::Attached {
        anchor: RegionAnchor::Line {
            nodes: [left, model.elements[0].nodes[1]],
            span: [0.0, 0.75],
            transfer: LoadTransfer::Anchor,
        },
        extent: [700.0; 2],
    };
    slab.plate.top_reinforcement = None;
    assert!(model
        .attached_slab_supports(&slab)
        .unwrap()
        .iter()
        .any(|part| part.elem == ElemId(0)));
    model.slabs.push(slab);
    assert!(!model.beam_has_attached_slab(&model.elements[0]));
    let forces = [(0.5, [0., 60_000., 0., 0., 0., 10e6])];
    let checks = check(&model, QdMethod::Qd1, Some(0.0), &forces, -10_000.0);
    assert_shear_n(&checks[0].2, 48_927.037_5);
    let CheckOutcome::Checked(result) = &checks[0].2 else {
        unreachable!()
    };
    assert!(result
        .components
        .iter()
        .any(|part| part.rc_beam_qd1_slab_warning().is_some()));
}

#[test]
fn unequal_opposing_girders_do_not_select_a_flange_width_from_boundary_order() {
    use sepika_core::ids::{NodeId, SectionId};
    use sepika_core::model::{SupportBoundary, SupportMemberId};

    let mut model = beam_with_recorded_continuous_slab_rebar();
    let id_at = |coord| {
        model
            .nodes
            .iter()
            .find(|node| node.coord == coord)
            .unwrap()
            .id
    };
    let i = id_at([0.0, 2_000.0, 0.0]);
    let j = id_at([6_500.0, 2_000.0, 0.0]);
    let e = model
        .elements
        .iter()
        .position(|e| e.nodes.as_slice() == [i, j])
        .unwrap();
    let old_id = model.elements[e].id;
    let mut middle = model.nodes[i.index()].clone();
    middle.id = NodeId(model.nodes.len() as u32);
    middle.coord = [3_250.0, 2_000.0, 0.0];
    let m = middle.id;
    model.nodes.push(middle);
    let mut second = model.elements[e].clone();
    second.id = ElemId(model.elements.len() as u32);
    second.nodes[0] = m;
    let mut wide = model.sections[0].clone();
    wide.id = SectionId(model.sections.len() as u32);
    wide.width = 600.0;
    if let Some(SectionShape::RcBeamRect { b, .. }) = &mut wide.shape {
        *b = 600.0;
    }
    second.section = Some(wide.id);
    model.sections.push(wide);
    model.elements[e].nodes[1] = m;
    let second_id = second.id;
    model.elements.push(second);
    let region = model
        .floor_assignment_regions
        .regions
        .iter_mut()
        .find(|r| {
            r.boundary
                .iter()
                .any(|edge| edge.support == SupportMemberId::Primary(old_id))
        })
        .unwrap();
    let pos = region
        .boundary
        .iter()
        .position(|edge| edge.support == SupportMemberId::Primary(old_id))
        .unwrap();
    let original = region.boundary[pos];
    let new = SupportBoundary {
        support: SupportMemberId::Primary(second_id),
        span: original.span,
    };
    let pair = if original.span == [0.0, 1.0] {
        [original, new]
    } else {
        [new, original]
    };
    region.boundary.splice(pos..=pos, pair);
    let forces = [(0.5, [0., 60_000., 0., 0., 0., 10e6])];
    let region_index = model
        .floor_assignment_regions
        .regions
        .iter()
        .position(|r| {
            r.boundary
                .iter()
                .any(|edge| edge.support == SupportMemberId::Primary(old_id))
        })
        .unwrap();
    for rotate in [0, 3] {
        model.floor_assignment_regions.regions[region_index]
            .boundary
            .rotate_left(rotate);
        let check = check(&model, QdMethod::Qd1, Some(0.0), &forces, -10_000.0);
        assert_shear_n(&check[0].2, 48_927.037_5);
        let CheckOutcome::Checked(result) = &check[0].2 else {
            unreachable!()
        };
        assert!(result
            .components
            .iter()
            .any(|part| part.rc_beam_qd1_slab_warning().is_some()));
    }
}

#[test]
fn supports_outside_the_actual_floor_boundary_cannot_confirm_flange_or_anchorage() {
    let model = beam_with_recorded_continuous_slab_rebar();
    let support_ids = [
        [[0.0, 2_000.0, 0.0], [6_500.0, 2_000.0, 0.0]],
        [[0.0, 0.0, 0.0], [0.0, 2_000.0, 0.0]],
        [[6_500.0, 0.0, 0.0], [6_500.0, 2_000.0, 0.0]],
    ]
    .map(|coords| {
        model
            .elements
            .iter()
            .find(|e| {
                e.nodes
                    .iter()
                    .map(|&n| model.node(n).unwrap().coord)
                    .eq(coords)
            })
            .unwrap()
            .id
    });
    let forces = [(0.5, [0., 60_000., 0., 0., 0., 10e6])];
    for id in support_ids {
        for kind in 0..6 {
            let mut changed = model.clone();
            let elem = &mut changed.elements[id.index()];
            match kind {
                0 => elem.end_offsets_mm = [[0.0, 0.0, -1_000.0]; 2],
                1 => elem.end_offsets_mm[1][2] = 300.0,
                2 => {
                    elem.local_axis.ref_vector = if id == support_ids[0] {
                        [0.0, 1.0, 0.0]
                    } else {
                        [1.0, 0.0, 0.0]
                    }
                }
                3 => elem.end_offsets_mm = [[200.0, 0.0, 0.0]; 2],
                4 => elem.end_offsets_mm[1][0] = 200.0,
                5 => elem.end_offsets_mm = [[0.0, 200.0, 0.0]; 2],
                _ => unreachable!(),
            }
            let check = check(&changed, QdMethod::Qd1, Some(0.0), &forces, -10_000.0);
            assert_shear_n(&check[0].2, 48_927.037_5);
            let CheckOutcome::Checked(result) = &check[0].2 else {
                unreachable!()
            };
            assert!(result
                .components
                .iter()
                .any(|part| part.rc_beam_qd1_slab_warning().is_some()));
        }
    }
}

#[test]
fn eccentric_rc_beam_uses_the_end_force_at_a_missed_column_and_the_real_face_at_the_other_end() {
    use sepika_core::adjacency::NodeAdjacency;
    let mut model = model();
    add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    for (j_offset, j_xi, length_mm) in [
        (1_000.0, 1.0, 4_500.0),
        (
            -500.0,
            4_250.0 / 4_500.0,
            f64::hypot(4_500.0, 1_500.0) * 4_250.0 / 4_500.0,
        ),
    ] {
        model.elements[0].end_offsets_mm = [[0.0, 0.0, 1_000.0], [0.0, 0.0, j_offset]];
        let geometry = sepika_design_jp::rc::beam_qd1_geometry(
            &model,
            &NodeAdjacency::primary_frame(&model),
            &model.elements[0],
        )
        .unwrap();
        assert_eq!(geometry.uses_material_end, [true, j_offset > 0.0]);
        assert!((geometry.end_positions[1] - j_xi).abs() < 1e-12);
        assert!((geometry.clear_span_mm - length_mm).abs() < 1e-9);
        let rows = [
            (0.0, 10e6),
            (1e-8, 9e8),
            (0.055, 9e8),
            (0.5, 10e6),
            (j_xi, 20e6),
        ]
        .map(|(xi, mz)| (xi, [0., 60_000., 0., 0., 0., mz]));
        let outcomes = check(&model, QdMethod::Qd1, Some(8_000.0), &rows, -10_000.0);
        assert_eq!(
            outcomes.iter().map(|(_, xi, _)| *xi).collect::<Vec<_>>(),
            [0.0, 0.5, j_xi]
        );
        // 上下端の降伏曲げの和は293.562225 kN·m。重力反力8 kNを加える。
        for (_, _, result) in &outcomes {
            assert_shear_n(result, 8_000.0 + 293_562_225.0 / length_mm);
        }
        let CheckOutcome::Checked(result) = &outcomes[0].2 else {
            unreachable!()
        };
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == CheckKind::Bending && c.detail.contains("|mz|=10000000.0")));
        let outcomes = check(&model, QdMethod::Qd1, Some(8_000.0), &rows[1..], -10_000.0);
        assert!(outcomes.iter().any(|(_, x, result)| *x == 0.0 &&
            matches!(result, CheckOutcome::Skipped { reason } if reason.contains("保存解析断面力がありません"))));
    }
}

#[test]
fn eccentric_end_cannot_bypass_unresolved_column_candidates_or_a_pinned_connection() {
    use sepika_core::adjacency::NodeAdjacency;
    let mut model = model();
    add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 1_000.0]; 2];
    let mut invalid = model.clone();
    invalid.sections[1].shape = None;
    let mut ambiguous = model.clone();
    let wide_id = sepika_core::ids::SectionId(2);
    let mut wide = ambiguous.sections[1].clone();
    wide.id = wide_id;
    if let Some(SectionShape::RcColumnRect { b, d, .. }) = &mut wide.shape {
        *b = 750.0;
        *d = 750.0;
    }
    ambiguous.sections.push(wide);
    let mut second_column = ambiguous.elements[1].clone();
    second_column.id = ElemId(3);
    second_column.section = Some(wide_id);
    ambiguous.elements.push(second_column);
    let mut nonfinite = model.clone();
    nonfinite.elements[0].end_offsets_mm[0][2] = f64::NAN;
    model.elements[0].end_cond[0] = EndCondition::Pinned;
    for (model, expected) in [
        (invalid, "実材軸が接続柱"),
        (ambiguous, "実材軸が接続柱"),
        (nonfinite, "材軸を確定できません"),
        (model, "強軸曲げが解放"),
    ] {
        let result = sepika_design_jp::rc::beam_qd1_clear_span(
            &model,
            &NodeAdjacency::primary_frame(&model),
            &model.elements[0],
        );
        let reason = result.unwrap_err();
        assert!(reason.contains(expected), "{reason}");
    }
}

#[test]
fn material_end_remains_exact_beside_joint_positions_and_does_not_borrow_their_gravity_force() {
    use sepika_core::model::{MemberDetailAttr, MemberJoint};
    let mut model = model();
    add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    model.elements[0].end_offsets_mm = [[0.0, 0.0, 1000.0]; 2];
    let near = 1.0 - 5e-7;
    let almost_same = 1.0 - 5e-10;
    let mut detail = MemberDetailAttr::new(ElemId(0));
    detail.joints = [near, almost_same]
        .map(|xi| MemberJoint {
            distance: xi * 4500.0,
            kind: Default::default(),
        })
        .to_vec();
    model.member_detail_attrs.push(detail);
    let rows = [
        (0.0, [0., 60_000., 0., 0., 0., 10e6]),
        (0.5, [0., 60_000., 0., 0., 0., 10e6]),
        (near, [0., 60_000., 0., 0., 0., 10e6]),
        (almost_same, [0., 60_000., 0., 0., 0., 10e6]),
        (1.0, [0., 60_000., 0., 0., 0., 10e6]),
    ];
    let response = [(ElemId(0), MemberForces { at: rows.to_vec() })];
    let gravity = [(
        ElemId(0),
        MemberForces {
            at: rows[..4]
                .iter()
                .map(|(xi, _)| (*xi, [0., -10_000., 0., 0., 0., 0.]))
                .collect(),
        },
    )];
    let checks = run_member_design_checks(
        &model,
        &response,
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            qd_method: QdMethod::Qd1,
            long_member_forces: Some(&gravity),
            q_simple_by_elem: Some(&HashMap::from([(ElemId(0), 0.0)])),
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks;
    let end = checks
        .iter()
        .find(|(_, xi, _)| *xi == 1.0)
        .expect("厳密な実材端");
    assert!(
        matches!(&end.2, CheckOutcome::Skipped { reason } if reason.contains("同位置の重力参照内力"))
    );
    assert!(checks.iter().any(|(_, xi, _)| *xi == near));
    assert!(!checks.iter().any(|(_, xi, _)| *xi == almost_same));
}

#[test]
fn rounding_tolerance_does_not_substitute_a_material_end_for_an_internal_face() {
    use sepika_design_jp::design_position::{qd1_saved_station_issue, same_qd1_station};
    let inner = 1.0 - 4.0 * f64::EPSILON;
    assert!(!same_qd1_station(inner, 1.0));
    assert!(!same_qd1_station(1.0, inner));
    assert!(same_qd1_station(inner, 1.0 - 5.0 * f64::EPSILON));
    assert!(qd1_saved_station_issue(inner, &[(1.0, [0.0; 6])]).is_some());
}

#[test]
fn material_end_replaces_the_automatic_face_but_keeps_an_explicit_joint_check() {
    use sepika_core::model::{MemberDetailAttr, MemberJoint};
    let mut model = model();
    add_columns_at_outer_nodes(&mut model, [0.0, 1.0, 0.0]);
    let old = 250.0 / 4500.0;
    let other_face = 1.0 - old;
    let rows = [
        (0.0, 10e6),
        (old, 1e10),
        (0.5, 10e6),
        (other_face, 10e6),
        (1.0, 10e6),
    ]
    .map(|(xi, mz)| (xi, [0.0, 60_000.0, 0.0, 0.0, 0.0, mz]));
    let bending_fails = |outcome: &CheckOutcome| {
        matches!(outcome,
        CheckOutcome::Checked(check) if check.components.iter().any(|c| c.kind == CheckKind::Bending && c.ratio > 1.0))
    };
    let baseline = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert!(baseline
        .iter()
        .any(|(_, xi, result)| *xi == old && bending_fails(result)));

    model.elements[0].end_offsets_mm = [[0.0, 0.0, 1000.0]; 2];
    let replaced = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert!(replaced
        .iter()
        .all(|(_, xi, result)| *xi != old && !bending_fails(result)));
    let end = &replaced.iter().find(|(_, xi, _)| *xi == 0.0).unwrap().2;
    assert_shear_n(end, 293_562_225.0 / 4500.0);
    let CheckOutcome::Checked(end) = end else {
        panic!("{end:?}");
    };
    assert!(end
        .components
        .iter()
        .any(|c| c.kind == CheckKind::Shear && c.detail.contains("α=1.000")));

    let mut detail = MemberDetailAttr::new(ElemId(0));
    detail.joints.push(MemberJoint {
        distance: 250.0,
        kind: Default::default(),
    });
    model.member_detail_attrs.push(detail);
    let with_joint = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
    assert!(with_joint
        .iter()
        .any(|(_, xi, result)| *xi == old && bending_fails(result)));
}
