use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
#[path = "fixtures/steel_column_k_model.rs"]
mod input;

#[test]
fn ideal_pin_portal_default_is_unsafe_relative_to_independent_length() {
    for (direct, adopted_mm, load_scale, ratio, ok) in [
        (None, "3600", 1.0, 0.934028167715, true),
        (
            Some(6983.630277482522),
            "6983.630277482522",
            1.0,
            1.014743328291276,
            false,
        ),
        (
            Some(6983.630277482522),
            "6983.630277482522",
            0.5,
            0.507371664145638,
            true,
        ),
    ] {
        let mut model = input::portal(1.0);
        for attr in &mut model.steel_design_attrs {
            attr.lk_y_direct = direct;
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
            assert!((axial.ratio - ratio).abs() < 1e-12, "{axial:?}");
            assert!(result
                .detail
                .contains(&format!("強軸 {adopted_mm} ／ 弱軸 3000")));
            assert_eq!(result.ok(), ok, "{result:?}");
        }
    }
}
