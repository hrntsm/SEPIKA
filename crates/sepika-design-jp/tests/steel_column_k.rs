use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
#[path = "fixtures/steel_column_k_model.rs"]
mod input;

#[test]
fn ideal_pin_portal_requires_direct_length_for_all_beam_stiffnesses() {
    for beam_i_ratio in [10.0, 1.0, 0.1, 0.01] {
        let model = input::portal(beam_i_ratio);
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
                matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains(&format!("柱 ID {}", id.0)) && reason.contains("強軸の座屈長さ") && reason.contains("直接指定")),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn ideal_pin_portal_direct_independent_length_gives_six_ng_and_low_load_ok() {
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
