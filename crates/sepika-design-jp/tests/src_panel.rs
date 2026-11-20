use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::Model;
use sepika_design_jp::joint_wiring::collect_joint_checks;
use sepika_design_jp::{
    run_member_design_checks, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_solver::statics::linear::linear_static_once;

#[path = "fixtures/src_panel_model.rs"]
mod fixture;
#[path = "fixtures/src_panel_reference.rs"]
mod reference;

#[test]
fn a_missing_dedicated_column_face_is_unavailable_with_a_matching_regular_sample() {
    let model = fixture::input_model();
    let mut response = linear_static_once(&model, LoadCaseId(0)).unwrap();
    let beam = ElemId(1);
    let (_, face_ends) = response
        .girder_column_face_forces
        .iter()
        .find(|(id, _)| *id == beam)
        .unwrap();
    let face = face_ends[0].as_ref().unwrap();
    let exact_position = face.face.position_mm / face.face.length_mm;
    let values = *face.forces.as_ref().unwrap();
    let (_, regular) = response
        .member_forces
        .iter_mut()
        .find(|(id, _)| *id == beam)
        .unwrap();
    regular
        .at
        .retain(|(position, _)| (position - exact_position).abs() > 1e-9);
    regular.at.push((exact_position, values));
    regular.at.sort_by(|a, b| a.0.total_cmp(&b.0));
    let run = |girder_column_face_forces: &[_]| {
        let report = run_member_design_checks(
            &model,
            &response.member_forces,
            &response.panel_moments,
            &MemberDesignCheckOptions {
                term: LoadTerm::Long,
                girder_column_face_forces,
                ..Default::default()
            },
        )
        .unwrap();
        report
            .joint_checks
            .into_iter()
            .find(|(id, label, _)| *id == fixture::JOINT && label == fixture::PANEL_X)
            .unwrap()
            .2
    };
    for unusable in [
        Vec::new(),
        response.girder_column_face_forces.clone(),
        response.girder_column_face_forces.clone(),
    ]
    .into_iter()
    .enumerate()
    .map(|(case, mut faces)| {
        if case != 0 {
            let (_, ends) = faces.iter_mut().find(|(id, _)| *id == beam).unwrap();
            let face = ends[0].as_mut().unwrap();
            if case == 1 {
                face.forces = Err("不連続荷重位置の一側値が未確定".into());
            } else {
                face.face.position_mm += 1.0;
            }
        }
        faces
    }) {
        assert!(
            matches!(run(&unusable), CheckOutcome::Skipped { reason } if reason.contains("実柱面") || reason.contains("不連続荷重"))
        );
    }
    let CheckOutcome::Checked(result) = run(&response.girder_column_face_forces) else {
        panic!("実柱面の応力を使った長期結果が必要");
    };
    assert!(
        (result.ratio() - reference::DEMAND_KN_M / reference::RC_LONG_ALLOWABLE_KN_M).abs() < 1e-10
    );
}

#[test]
fn a_joint_shape_uses_only_beams_in_its_plane() {
    for (above, left, shape) in [
        (false, false, "L字形"),
        (false, true, "T字形"),
        (true, false, "ト字形"),
        (true, true, "十字形"),
    ] {
        let mut model = fixture::input_model();
        let mut add_member = |coord, template: usize, i, reverse| {
            let node = NodeId(model.nodes.len() as u32);
            model.nodes.push(sepika_core::model::Node {
                id: node,
                coord,
                ..model.nodes[0].clone()
            });
            let id = ElemId(model.elements.len() as u32);
            let nodes = if reverse { [node, i] } else { [i, node] };
            model.elements.push(sepika_core::model::ElementData {
                id,
                nodes: nodes.into_iter().collect(),
                ..model.elements[template].clone()
            });
            id
        };
        if above {
            add_member([0.0, 0.0, 8000.0], 0, fixture::JOINT, false);
            add_member([6000.0, 0.0, 8000.0], 1, NodeId(5), false);
        }
        let left_beam = left.then(|| add_member([-6000.0, 0.0, 4000.0], 1, fixture::JOINT, true));
        let right = [(0.05, [0.0, 0.0, 0.0, 0.0, 0.0, 285e6])];
        let zero_left = [(0.95, [0.0; 6])];
        let noisy_orthogonal = [(0.05, [0.0, 0.0, 0.0, 0.0, 0.0, 999e6])];
        let mut forces: Vec<_> = vec![(ElemId(1), right.as_slice())];
        if let Some(id) = left_beam {
            forces.push((id, zero_left.as_slice()));
        }
        for supply_orthogonal in [false, true] {
            if supply_orthogonal {
                forces.push((ElemId(3), noisy_orthogonal.as_slice()));
            }
            let rows = collect_joint_checks(&model, &forces, LoadTerm::Long);
            assert!(rows.iter().any(|(node, label, outcome)| *node == fixture::JOINT
                && label == fixture::PANEL_Y
                && matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("弱軸"))));
            let (_, _, outcome) = rows
                .iter()
                .find(|(node, label, _)| *node == fixture::JOINT && label == fixture::PANEL_X)
                .unwrap();
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{shape}: {outcome:?}");
            };
            assert!(result.basis.contains(&format!("(20.1) {shape}・長期")));
            assert!(
                (result.ratio() - reference::DEMAND_KN_M / reference::RC_LONG_ALLOWABLE_KN_M).abs()
                    < 1e-10
            );
        }
    }
}

#[test]
fn the_panel_rejects_invalid_or_unavailable_model_hoops() {
    use fixture::ShortBeam;
    for (beam, allowable) in [
        (ShortBeam::Src, reference::SRC_SHORT_ALLOWABLE_KN_M),
        (ShortBeam::Steel, reference::STEEL_SHORT_ALLOWABLE_KN_M),
        (ShortBeam::Rc, f64::NAN),
    ] {
        let mut model = fixture::short_model(beam);
        let right = [(0.05, [0.0, 0.0, 0.0, 0.0, 0.0, 285e6])];
        let check = |model: &Model| {
            collect_joint_checks(model, &[(ElemId(1), &right)], LoadTerm::Short)
                .into_iter()
                .find(|(node, label, _)| *node == fixture::JOINT && label == fixture::PANEL_X)
                .unwrap()
                .2
        };
        let outcome = check(&model);
        if matches!(beam, ShortBeam::Rc) {
            assert!(
                matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("RC梁の短期式(20.2)"))
            );
            continue;
        }
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}")
        };
        assert!((result.ratio() - reference::DEMAND_KN_M / allowable).abs() < 1e-10);
        model.src_joint_hoops[0].hoops.effective_legs = 0;
        assert!(
            matches!(check(&model), CheckOutcome::Skipped { reason } if reason.contains("有効脚本数"))
        );
        model.src_joint_hoops.clear();
        assert!(
            matches!(check(&model), CheckOutcome::Skipped { reason } if reason.contains("帯筋"))
        );
    }
}

#[test]
fn a_load_discontinuity_at_the_column_face_is_reported() {
    let model = fixture::with_unavailable_column_face(fixture::input_model());
    let response = linear_static_once(&model, LoadCaseId(0)).unwrap();
    let report = run_member_design_checks(
        &model,
        &response.member_forces,
        &response.panel_moments,
        &MemberDesignCheckOptions {
            girder_column_face_forces: &response.girder_column_face_forces,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.joint_checks.iter().any(|(node, label, outcome)| *node == fixture::JOINT
        && label == fixture::PANEL_X
        && matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("実柱面") && reason.contains("切断側"))));
}

#[test]
fn invalid_steel_girder_dimensions_cannot_turn_short_overload_into_a_pass() {
    use sepika_core::section_shape::SectionShape;

    let mut model = fixture::short_model(fixture::ShortBeam::Steel);
    let forces = [(0.05, [0.0, 0.0, 0.0, 0.0, 0.0, 594_285_714.285_714_3])];
    let panel = |model: &Model| {
        collect_joint_checks(model, &[(ElemId(1), &forces)], LoadTerm::Short)
            .into_iter()
            .find(|(node, label, _)| *node == fixture::JOINT && label == fixture::PANEL_X)
            .unwrap()
            .2
    };
    let CheckOutcome::Checked(valid) = panel(&model) else {
        panic!("有効なH形鋼断面で短期検定が必要");
    };
    assert!(!valid.ok());
    assert!((valid.ratio() - 520.0 / reference::STEEL_SHORT_ALLOWABLE_KN_M).abs() < 1e-10);

    for thickness_mm in [-13.0, 250.0] {
        let Some(SectionShape::SteelH { flange_thick, .. }) = &mut model.sections[1].shape else {
            panic!("H形鋼断面が必要");
        };
        *flange_thick = thickness_mm;
        assert!(matches!(panel(&model), CheckOutcome::Skipped { reason }
            if reason.contains("梁 ID 1 のH形鋼断面が不正")));
    }
}
