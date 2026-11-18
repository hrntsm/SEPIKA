use sepika_design_jp::{run_member_design_checks, CheckOutcome, MemberDesignCheckOptions};
#[path = "fixtures/steel_column_k_model.rs"]
mod input;

#[test]
fn column_uses_defaults_only_for_unspecified_axes() {
    for (ly, lz, expected) in [
        (None, None, "強軸 3600 ／ 弱軸 3600"),
        (Some(3000.0), None, "強軸 3000 ／ 弱軸 3600"),
        (None, Some(3000.0), "強軸 3600 ／ 弱軸 3000"),
        (Some(4700.0), Some(6200.0), "強軸 4700 ／ 弱軸 6200"),
        (
            Some(1e-10),
            Some(1e-10),
            "強軸 0.0000000001 ／ 弱軸 0.0000000001",
        ),
    ] {
        let mut model = input::portal(1.0);
        for attr in &mut model.steel_design_attrs {
            attr.lk_y_direct = ly;
            attr.lk_z_direct = lz;
        }
        let solved = sepika_solver::statics::linear::linear_static_once(
            &model,
            sepika_core::ids::LoadCaseId(0),
        )
        .unwrap();
        let report = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions::default(),
        )
        .unwrap();
        let columns: Vec<_> = report
            .member_checks
            .iter()
            .filter(|(id, _, _)| id.0 < 2)
            .collect();
        assert_eq!(columns.len(), 6);
        for (_, _, outcome) in columns {
            assert!(
                matches!(outcome, CheckOutcome::Checked(result) if result.detail.contains(expected) && !result.basis.contains("座屈長さ0")),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn column_k_cft_does_not_use_the_available_automatic_length_with_primary_steel_material() {
    let mut model = input::portal(1.0);
    model.materials[0].fc = Some(24.0);
    model.sections[0].steel_material = Some(sepika_core::ids::MaterialId(0));
    model.sections[0].shape = Some(sepika_core::section_shape::SectionShape::CftBox {
        height: 400.0,
        width: 400.0,
        thick: 10.0,
        horizontal_thick: 10.0,
        corner_r: Some(0.0),
    });
    let solved =
        sepika_solver::statics::linear::linear_static_once(&model, sepika_core::ids::LoadCaseId(0))
            .unwrap();
    let adjacency = sepika_core::adjacency::NodeAdjacency::build(&model);
    assert!(
        sepika_design_jp::steel::buckling::steel_column_k_axes_with_index(
            &model,
            &adjacency,
            &model.elements[0],
        )
        .is_some()
    );
    let report = run_member_design_checks(
        &model,
        &solved.member_forces,
        &solved.panel_moments,
        &MemberDesignCheckOptions::default(),
    )
    .unwrap();
    let columns: Vec<_> = report
        .member_checks
        .iter()
        .filter(|(id, _, _)| id.0 < 2)
        .collect();
    assert_eq!(columns.len(), 6);
    assert!(columns.iter().all(|(_, _, outcome)| matches!(outcome,
        CheckOutcome::Skipped { reason } if reason.contains("強軸の座屈長さを有限な正値で直接指定")
    )));
}

#[test]
fn column_invalid_direct_axis_never_falls_back() {
    let mut model = input::portal(1.0);
    let solved =
        sepika_solver::statics::linear::linear_static_once(&model, sepika_core::ids::LoadCaseId(0))
            .unwrap();
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        for axis in ["強軸", "弱軸"] {
            for attr in &mut model.steel_design_attrs {
                attr.lk_y_direct = (axis == "強軸").then_some(invalid);
                attr.lk_z_direct = (axis == "弱軸").then_some(invalid);
            }
            let report = run_member_design_checks(
                &model,
                &solved.member_forces,
                &solved.panel_moments,
                &MemberDesignCheckOptions::default(),
            )
            .unwrap();
            let columns: Vec<_> = report
                .member_checks
                .iter()
                .filter(|(id, _, _)| id.0 < 2)
                .collect();
            assert_eq!(columns.len(), 6);
            for (id, _, outcome) in columns {
                assert!(
                    matches!(outcome, CheckOutcome::Skipped { reason }
                        if reason.contains(&format!("部材 ID {}", id.0))
                        && reason.contains(&format!("{axis}の座屈長さの直接指定"))
                        && reason.contains("有限な正値")),
                    "{outcome:?}"
                );
            }
        }
    }
}

#[test]
fn eccentric_column_uses_the_physical_axis_for_the_missing_length() {
    let mut model = input::portal(1.0);
    model.elements[0].end_offsets_mm = [[0.0; 3], [0.0, 0.0, 500.0]];
    let solved =
        sepika_solver::statics::linear::linear_static_once(&model, sepika_core::ids::LoadCaseId(0))
            .unwrap();
    let report = run_member_design_checks(
        &model,
        &solved.member_forces,
        &solved.panel_moments,
        &MemberDesignCheckOptions::default(),
    )
    .unwrap();
    for (id, _, outcome) in report.member_checks.iter().filter(|(id, _, _)| id.0 < 2) {
        assert!(
            matches!(outcome, CheckOutcome::Checked(result) if result.detail.contains(if id.0 == 0 { "強軸 4200 ／ 弱軸 3000" } else { "強軸 3600 ／ 弱軸 3000" })),
            "{outcome:?}"
        );
    }
}

#[test]
fn column_k_unchecked_and_direct_lengths_use_identical_face_positions() {
    let mut model = input::portal(1.0);
    for column in &mut model.elements[..2] {
        column.rigid_zone.face_i = Some(150.0);
        column.rigid_zone.face_j = Some(300.0);
    }
    let solved =
        sepika_solver::statics::linear::linear_static_once(&model, sepika_core::ids::LoadCaseId(0))
            .unwrap();
    for (id, forces) in &solved.member_forces {
        if id.0 < 2 {
            assert!(forces.at.iter().any(|(pos, _)| *pos == 0.0));
            assert!(forces.at.iter().any(|(pos, _)| *pos == 1.0));
        }
    }
    for (direct, checked) in [
        (None, true),
        (Some(6983.630277482522), true),
        (Some(0.0), false),
    ] {
        for attr in &mut model.steel_design_attrs {
            attr.lk_y_direct = direct;
        }
        let report = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions::default(),
        )
        .unwrap();
        for id in [0, 1] {
            let positions: Vec<_> = report
                .member_checks
                .iter()
                .filter(|(elem, _, _)| elem.0 == id)
                .map(|(_, pos, outcome)| {
                    assert_eq!(matches!(outcome, CheckOutcome::Checked(_)), checked);
                    *pos
                })
                .collect();
            assert_eq!(positions, vec![0.05, 0.5, 0.9]);
        }
    }
}
