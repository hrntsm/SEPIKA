use sepika_core::ids::LoadCaseId;
use sepika_design_jp::{run_member_design_checks, CheckOutcome, MemberDesignCheckOptions};

#[path = "../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_model.rs"]
mod input;
#[path = "../../sepika-design-jp/tests/fixtures/rc_column_axial_boundary_reference.rs"]
mod reference;

#[test]
fn prepared_column_with_self_weight_reaches_axial_limit_at_loaded_end() {
    for circle in [false, true] {
        let na_n = reference::axial_limit_n(circle);
        let mut model = input::column_model(circle, na_n);
        let area_mm2 = if circle {
            std::f64::consts::PI * 90_000.0
        } else {
            240_000.0
        };
        let upper_self_weight_n = area_mm2 * 3000.0 * 2.4e-9 * 9806.65 / 2.0;
        model.load_cases[0].nodal[0].values[2] += upper_self_weight_n;
        sepika_job::prepare_model_for_analysis(&mut model, &Default::default(), None).unwrap();
        let solved =
            sepika_job::compute::compute_linear_static(model.clone(), LoadCaseId(0)).unwrap();
        assert_eq!(solved.member_forces.len(), 1);
        let forces = &solved.member_forces[0].1.at;
        assert!(!forces.is_empty());
        assert!(forces
            .iter()
            .all(|(p, f)| p.is_finite() && f.iter().all(|v| v.is_finite())));
        let end = forces.iter().find(|(p, _)| *p == 1.0).unwrap().1;
        assert!(model.load_cases[0].nodal.len() > 1);
        assert!((-end[0] - na_n).abs() < 1e-8);
        assert!((end[4].abs() - 1_000_000.0).abs() < 1e-8);
        assert!((end[5].abs() - 2_000_000.0).abs() < 1e-8);
        let report = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions::default(),
        )
        .unwrap();
        assert_eq!(report.member_checks.len(), 3);
        for (_, _, outcome) in report.member_checks {
            match outcome {
                CheckOutcome::Skipped { reason } => assert!(reason.contains("検定不能")),
                CheckOutcome::Checked(result) => {
                    assert!(circle && !result.ok(), "{result:?}");
                    assert!(result.ratio().is_finite());
                }
            }
        }
    }
}
