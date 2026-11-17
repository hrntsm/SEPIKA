use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
#[path = "fixtures/steel_column_k_model.rs"]
mod input;

#[test]
fn column_k_public_analysis_requires_both_direct_lengths() {
    for (ly, lz, missing) in [
        (None, None, "強軸・弱軸"),
        (Some(3000.0), None, "弱軸"),
        (None, Some(3000.0), "強軸"),
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
        for (id, _, outcome) in columns {
            assert!(
                matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains(&format!("柱 ID {}", id.0)) && reason.contains(missing) && reason.contains("直接指定")),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn column_k_direct_lengths_preserve_independent_boundary_ng_and_finite_ok() {
    for (load_scale, ok) in [(1.0, false), (0.5, true)] {
        let mut model = input::portal(1.0);
        for attr in &mut model.steel_design_attrs {
            attr.lk_y_direct = Some(6983.630277482522);
        }
        for load in &mut model.load_cases[0].nodal {
            load.values[2] *= load_scale;
        }
        let solved = sepika_solver::statics::linear::linear_static_once(
            &model,
            sepika_core::ids::LoadCaseId(0),
        )
        .unwrap();
        for (id, forces) in &solved.member_forces {
            if id.0 < 2 {
                for (_, force) in &forces.at {
                    assert!((force[0] + 2215200.0 * load_scale).abs() < 1e-7);
                    assert!(force[1..].iter().all(|v| v.abs() < 1e-7));
                }
            }
        }
        let report = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions {
                term: LoadTerm::Long,
                ..Default::default()
            },
        )
        .unwrap();
        let columns: Vec<_> = report
            .member_checks
            .iter()
            .filter(|(id, _, _)| id.0 < 2)
            .collect();
        assert_eq!(columns.len(), 6);
        for (_, _, outcome) in columns {
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}")
            };
            let axial = result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::AxialBending)
                .unwrap();
            assert!(
                (axial.ratio - 1.014743328291276 * load_scale).abs() < 1e-12,
                "{axial:?}"
            );
            assert!(axial.ratio.is_finite());
            assert_eq!(result.ok(), ok, "{result:?}");
        }
    }
}

#[test]
fn column_k_cft_keeps_existing_auto_length_even_with_primary_steel_material() {
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
    let check = |model: &sepika_core::model::Model| {
        let report = run_member_design_checks(
            model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions::default(),
        )
        .unwrap();
        report
            .member_checks
            .into_iter()
            .filter(|(id, _, _)| id.0 < 2)
            .map(|(_, _, outcome)| {
                let CheckOutcome::Checked(result) = outcome else {
                    panic!("{outcome:?}")
                };
                result.ratio()
            })
            .collect::<Vec<_>>()
    };
    let automatic = check(&model);
    assert_eq!(automatic.len(), 6);
    for attr in &mut model.steel_design_attrs {
        attr.lk_y_direct = Some(5708.907121538976);
    }
    let direct = check(&model);
    for (a, b) in automatic.iter().zip(direct) {
        assert!((a - b).abs() < 1e-12, "{a} {b}");
    }
}

#[test]
fn column_k_invalid_direct_axis_never_falls_back_and_zero_remains_explicit() {
    let mut model = input::portal(1.0);
    let solved =
        sepika_solver::statics::linear::linear_static_once(&model, sepika_core::ids::LoadCaseId(0))
            .unwrap();
    for invalid in [None, Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
        for axis in [0, 1] {
            for attr in &mut model.steel_design_attrs {
                attr.lk_y_direct = if axis == 0 {
                    invalid
                } else {
                    Some(6983.630277482522)
                };
                attr.lk_z_direct = if axis == 1 { invalid } else { Some(3000.0) };
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
            assert!(columns
                .iter()
                .all(|(_, _, outcome)| matches!(outcome, CheckOutcome::Skipped { .. })));
        }
    }
    for attr in &mut model.steel_design_attrs {
        attr.lk_y_direct = Some(0.0);
        attr.lk_z_direct = Some(0.0);
    }
    let report = run_member_design_checks(
        &model,
        &solved.member_forces,
        &solved.panel_moments,
        &MemberDesignCheckOptions::default(),
    )
    .unwrap();
    assert!(report
        .member_checks
        .iter()
        .filter(|(id, _, _)| id.0 < 2)
        .all(|(_, _, outcome)| matches!(outcome, CheckOutcome::Checked(_))));
}

#[test]
fn column_k_fixed_support_and_rigid_beams_do_not_prove_story_buckling_conditions() {
    let mut model = input::portal(1.0);
    for node in &mut model.nodes[..2] {
        node.restraint = sepika_core::dof::Dof6Mask::FIXED;
    }
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
    let columns: Vec<_> = report
        .member_checks
        .iter()
        .filter(|(id, _, _)| id.0 < 2)
        .collect();
    assert_eq!(columns.len(), 6);
    assert!(columns.iter().all(|(_, _, outcome)| matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("層内軸力分布・同時座屈"))));
}

#[test]
fn column_k_eccentric_column_reports_the_missing_axis_after_partial_direct_input() {
    let mut model = input::portal(1.0);
    model.elements[0].end_offsets_mm = [[1.0, 0.0, 0.0]; 2];
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
            matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains(&format!("柱 ID {}", id.0)) && reason.contains("強軸の座屈長さ") && reason.contains("直接指定")),
            "{outcome:?}"
        );
    }
}
