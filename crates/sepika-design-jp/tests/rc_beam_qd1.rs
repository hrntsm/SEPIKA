use std::collections::HashMap;

use sepika_core::dof::Dof6Mask;
use sepika_core::ids::ElemId;
use sepika_core::model::{EndCondition, Model};
use sepika_core::section_shape::{RebarSize, SectionShape, StandardRebar};
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions, QdMethod,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod input;

fn model() -> Model {
    let mut model = input::model(false);
    model.nodes[1].coord[0] = 4500.0;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    let beam = &mut model.elements[0];
    beam.local_axis.ref_vector = [0.0, 0.0, 1.0];
    beam.rigid_zone.face_i = Some(250.0);
    beam.rigid_zone.face_j = Some(250.0);
    let mut section = input::section(350.0, 600.0, 19.0, &[2], &[4]);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut section.shape else {
        unreachable!();
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D19);
    rebar.cover = 30.5;
    model.sections[0] = section;
    model
}

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
    use sepika_core::dof::Dof;
    use sepika_core::ids::{NodeId, SectionId};
    use sepika_core::model::FrameSectionUse;
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};

    let mut model = model();
    let column_shape = SectionShape::RcColumnRect {
        b: 500.0,
        d: 500.0,
        rebar: RcRectColumnRebar {
            main_dia: RebarSize::Standard(StandardRebar::D19),
            x: vec![3],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: RebarSize::Standard(StandardRebar::D10),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let mut section = column_shape.to_section(SectionId(1), "C1".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = model.sections[0].material;
    model.sections.push(section);
    let beam = model.elements[0].clone();
    for end in 0..2 {
        model.nodes[end].restraint = Dof6Mask::FREE;
        let mut base = model.nodes[end].clone();
        let base_id = NodeId(2 + end as u32);
        base.id = base_id;
        base.coord[2] = -3000.0;
        base.restraint = Dof6Mask::FIXED;
        model.nodes.push(base);
        let mut column = beam.clone();
        column.id = ElemId(1 + end as u32);
        column.section = Some(SectionId(1));
        column.nodes = [base_id, beam.nodes[end]].into_iter().collect();
        column.local_axis.ref_vector = [1.0, 1.0, 0.0];
        model.elements.push(column);
    }
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

    let mut free_branch = model.clone();
    free_branch.nodes[3].restraint = Dof6Mask::FREE;
    for (fixed, spring, qd1_available) in [
        (None, None, false),
        (Some(Dof::Uz), None, false),
        (None, Some(Dof::Uz), false),
        (Some(Dof::Ux), None, true),
        (None, Some(Dof::Ry), true),
    ] {
        let far = &mut free_branch.nodes[3];
        far.restraint = Dof6Mask::FREE;
        if let Some(dof) = fixed {
            far.restraint.set_fixed(dof);
        }
        far.support_spring = spring.map(|dof| {
            let mut spring = [0.0; 6];
            spring[dof as usize] = 1e6;
            spring
        });
        for method in [QdMethod::Qd1, QdMethod::Min] {
            let outcomes = check(&free_branch, method, Some(0.0), &rows, -10_000.0);
            if qd1_available {
                let expected = if method == QdMethod::Qd1 {
                    77_398.0
                } else {
                    70_000.0
                };
                assert_shear_n(&outcomes[0].2, expected);
            } else {
                assert!(
                    matches!(&outcomes[0].2, CheckOutcome::Skipped { reason } if reason.contains("伝達先")),
                    "{outcomes:?}"
                );
            }
        }
        let qd2 = check(&free_branch, QdMethod::Qd2, None, &rows, -10_000.0);
        assert_shear_n(&qd2[0].2, 70_000.0);
    }

    free_branch.nodes[3].support_spring = None;
    let mut unrestrained_tip = free_branch.nodes[3].clone();
    unrestrained_tip.id = NodeId(4);
    unrestrained_tip.coord[2] = -6000.0;
    free_branch.nodes.push(unrestrained_tip);
    let mut extension = free_branch.elements[2].clone();
    extension.id = ElemId(3);
    extension.nodes = [NodeId(4), NodeId(3)].into_iter().collect();
    free_branch.elements.push(extension);
    for method in [QdMethod::Qd1, QdMethod::Min] {
        let outcomes = check(&free_branch, method, Some(0.0), &rows, -10_000.0);
        assert!(
            matches!(&outcomes[0].2, CheckOutcome::Skipped { reason } if reason.contains("伝達先")),
            "{outcomes:?}"
        );
    }
    let qd2 = check(&free_branch, QdMethod::Qd2, None, &rows, -10_000.0);
    assert_shear_n(&qd2[0].2, 70_000.0);

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
fn a_slab_assigned_to_part_of_the_beam_blocks_only_qd1_dependent_demand() {
    use sepika_core::ids::{NodeId, SectionId, SlabId};
    use sepika_core::model::{PlateAssignment, Slab, SlabPlate, SlabShape, SupportMemberId};

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
    for assignment in [PlateAssignment::Unset, PlateAssignment::NoPlate] {
        model.floor_assignment_regions.regions[0].assignment = assignment;
        let qd1 = check(&model, QdMethod::Qd1, Some(0.0), &rows, -10_000.0);
        assert_shear_n(&qd1[0].2, 73_390.556_25);
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
}
