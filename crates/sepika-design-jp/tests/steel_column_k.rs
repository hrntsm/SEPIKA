use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
#[path = "fixtures/steel_column_k_model.rs"]
mod input;

#[test]
fn column_default_length_comes_from_the_nodes_even_with_material_end_offsets() {
    let mut model = input::portal(1.0);
    model.elements[0].end_offsets_mm[1][2] = 500.0;
    let element = &model.elements[0];
    let default = sepika_design_jp::steel::frame_buckling_lengths(
        &model,
        element,
        sepika_design_jp::MemberKind::Column,
        None,
    );
    assert_eq!(default, (3600.0, 3600.0));
    let direct = sepika_design_jp::steel::frame_buckling_lengths(
        &model,
        element,
        sepika_design_jp::MemberKind::Column,
        Some(&model.steel_design_attrs[0]),
    );
    assert_eq!(direct, (3600.0, 3000.0));
}

#[test]
fn ideal_pin_portal_exposes_the_limit_of_the_default_length() {
    for (direct, adopted_mm, load_scale, ratio, ok) in [
        (None, "3600", 1.0, 0.9340281677153761, true),
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
