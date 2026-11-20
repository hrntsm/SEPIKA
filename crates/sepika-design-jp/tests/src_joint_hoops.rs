#[path = "fixtures/src_joint_hoops_model.rs"]
mod fixture;

use sepika_core::adjacency::NodeAdjacency;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{Model, SrcJointHoopAssignment};
use sepika_design_jp::srrc::joint_hoops::{
    resolve_src_short_term_joint_hoops, src_joint_hoop_input_statuses,
};

#[test]
fn short_term_src_joint_hoops_are_resolved_for_each_plane() {
    let mut model = fixture::frame();
    let adjacency = NodeAdjacency::primary_frame(&model);
    let resolve = |model: &Model, axis| {
        resolve_src_short_term_joint_hoops(model, &adjacency, fixture::NODE, axis)
    };
    let missing = resolve(&model, fixture::X).unwrap_err();
    assert!(
        missing.contains("節点 1")
            && missing.contains("[1.0, 0.0, 0.0]")
            && missing.contains("未入力")
    );

    model.src_joint_hoops.push(SrcJointHoopAssignment {
        node: fixture::NODE,
        axis: fixture::X,
        columns: vec![ElemId(0)],
        beams: vec![ElemId(3), ElemId(1)],
        hoops: fixture::example_hoops(),
    });
    let input = resolve(&model, fixture::X).unwrap();
    assert!((input.ratio - 0.0007452941176470588).abs() < 1e-15);
    assert_eq!(input.allowable_tension_n_per_mm2, 295.0);
    assert!(resolve(&model, fixture::Y).unwrap_err().contains("未入力"));
    assert!(resolve(&model, [0.6, 0.8, 0.0])
        .unwrap_err()
        .contains("一意に確認"));
    assert!(
        resolve_src_short_term_joint_hoops(&model, &adjacency, NodeId(0), fixture::X)
            .unwrap_err()
            .contains("節点 0")
    );

    for (grade, stress) in [
        ("SD295A", 295.0),
        ("SD295B", 295.0),
        ("SD345", 345.0),
        ("SD390", 390.0),
        ("SD490", 490.0),
    ] {
        model.src_joint_hoops[0].hoops.grade = grade.into();
        assert_eq!(
            resolve(&model, fixture::X)
                .unwrap()
                .allowable_tension_n_per_mm2,
            stress
        );
    }

    for grade in ["", "SD295X", "SR235", "SR295", "溶接金網", "USD685"] {
        model.src_joint_hoops[0].hoops.grade = grade.into();
        assert!(resolve(&model, fixture::X).unwrap_err().contains("材種"));
    }
    model.src_joint_hoops[0].hoops = fixture::example_hoops();
    for spacing in [
        0.0,
        -425.0,
        f64::INFINITY,
        f64::NAN,
        f64::MAX,
        f64::MIN_POSITIVE,
    ] {
        model.src_joint_hoops[0].hoops.spacing_mm = spacing;
        if spacing == f64::MIN_POSITIVE {
            model.src_joint_hoops[0].hoops.width_mm = f64::MIN_POSITIVE;
        }
        assert!(
            resolve(&model, fixture::X).unwrap_err().contains("間隔")
                || resolve(&model, fixture::X).unwrap_err().contains("帯筋比")
        );
    }
    for width_mm in [253.4, 126.7] {
        model.src_joint_hoops[0].hoops = sepika_core::model::SrcJointHoops {
            spacing_mm: 1.0,
            width_mm,
            ..fixture::example_hoops()
        };
        let reason = resolve(&model, fixture::X).unwrap_err();
        assert!(reason.contains("節点 1") && reason.contains("[1.0, 0.0, 0.0]"));
        assert!(reason.contains("帯筋比には0より大きく1未満"));
    }
    model.src_joint_hoops[0].hoops = fixture::example_hoops();
    model.src_joint_hoops[0].beams = vec![ElemId(1), ElemId(1)];
    assert!(resolve(&model, fixture::X)
        .unwrap_err()
        .contains("構面内の梁"));
    model.src_joint_hoops[0].beams = vec![ElemId(1), ElemId(3)];
    model.src_joint_hoops.push(model.src_joint_hoops[0].clone());
    assert!(resolve(&model, fixture::X).unwrap_err().contains("重複"));
    let rows = src_joint_hoop_input_statuses(&model);
    let row = rows.iter().find(|entry| entry.axis == fixture::X).unwrap();
    assert_eq!(row.assignments.len(), 2);
    assert!(row.reason.as_ref().unwrap().contains("重複"));
    assert!(row.prepared.is_none());
}

#[test]
fn src_joint_hoops_follow_canonical_axis_at_panel_component_tolerance() {
    let mut model = fixture::frame();
    model.nodes[4].coord = [1e-12, -1.0, 3_500.0];
    let adjacency = NodeAdjacency::primary_frame(&model);
    let plane = sepika_core::panel_zone::panel_joint_planes(
        &model,
        fixture::NODE,
        adjacency.elements_at(&model, fixture::NODE),
    )
    .into_iter()
    .find(|plane| plane.beams.len() == 1)
    .unwrap();
    assert_eq!(plane.axis, [-1e-12, 1.0, 0.0]);
    model.src_joint_hoops.push(SrcJointHoopAssignment {
        node: fixture::NODE,
        axis: plane.axis,
        columns: vec![ElemId(0)],
        beams: vec![ElemId(2)],
        hoops: fixture::example_hoops(),
    });
    assert_eq!(
        resolve_src_short_term_joint_hoops(&model, &adjacency, fixture::NODE, plane.axis)
            .unwrap()
            .allowable_tension_n_per_mm2,
        295.0,
    );
    let statuses = src_joint_hoop_input_statuses(&model);
    assert!(statuses
        .iter()
        .any(|row| row.axis == plane.axis && row.prepared.is_some()));
}

#[test]
fn src_joint_hoops_do_not_follow_a_rotated_beam_to_its_new_plane() {
    let mut model = fixture::frame();
    model.src_joint_hoops.push(SrcJointHoopAssignment {
        node: fixture::NODE,
        axis: fixture::Y,
        columns: vec![ElemId(0)],
        beams: vec![ElemId(2)],
        hoops: fixture::example_hoops(),
    });
    model.nodes[4].coord = [3_600.0, 4_800.0, 3_500.0];
    let adjacency = NodeAdjacency::primary_frame(&model);
    let old = resolve_src_short_term_joint_hoops(&model, &adjacency, fixture::NODE, fixture::Y);
    assert!(old.unwrap_err().contains("一意に確認"));
    let new_axis = [0.6, 0.8, 0.0];
    let rotated = resolve_src_short_term_joint_hoops(&model, &adjacency, fixture::NODE, new_axis);
    assert!(rotated.unwrap_err().contains("未入力"));
    let statuses = src_joint_hoop_input_statuses(&model);
    assert!(statuses.iter().any(|row| row.axis == fixture::Y
        && row.beam_ids.is_empty()
        && row
            .reason
            .as_ref()
            .is_some_and(|reason| reason.contains("構面がありません"))));
}
