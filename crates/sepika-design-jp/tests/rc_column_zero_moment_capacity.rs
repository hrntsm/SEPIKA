use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod input;
#[path = "fixtures/rc_column_axial_boundary_reference.rs"]
mod reference;
use input::column_model;

#[test]
fn common_product_entry_observes_zero_capacity_boundary_without_rejecting_bending() {
    for circle in [false, true] {
        let na_n = reference::axial_limit_n(circle);
        let model = column_model(circle, na_n);
        for (factor, moments, expected_ok) in [
            (0.99, [1.0, 1.0], true),
            (1.0, [0.0, 0.0], true),
            (1.0, [1_000_000.0, 0.0], true),
            (1.0, [0.0, 2_000_000.0], true),
            (1.0, [1_000_000.0, 2_000_000.0], true),
            (1.01, [1_000_000.0, 2_000_000.0], false),
            (1.01, [0.0, 0.0], false),
        ] {
            let forces = vec![(
                ElemId(0),
                MemberForces {
                    at: [0.0, 0.5, 1.0]
                        .into_iter()
                        .map(|pos| (pos, [-factor * na_n, 0.0, 0.0, 0.0, moments[0], moments[1]]))
                        .collect(),
                },
            )];
            let report = run_member_design_checks(
                &model,
                &forces,
                &[],
                &MemberDesignCheckOptions {
                    term: LoadTerm::Long,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(report.member_checks.len(), 3);
            for (_, pos, outcome) in report.member_checks {
                let CheckOutcome::Checked(result) = outcome else {
                    panic!("{outcome:?}")
                };
                let nm = result
                    .components
                    .iter()
                    .find(|c| c.kind == CheckKind::AxialBending)
                    .unwrap();
                assert!((nm.ratio - factor).abs() < 1e-12);
                assert_eq!(result.ok(), expected_ok);
                assert!(result
                    .components
                    .iter()
                    .filter(|c| c.kind != CheckKind::AxialBending)
                    .all(|c| c.ratio <= 1.0));
                if circle && factor >= 1.0 {
                    assert!(nm.detail.contains("MA=0.0 N·mm"));
                } else if !circle && factor >= 1.0 {
                    assert!(nm.detail.contains("MA_z=0.0 N·mm, MA_y=0.0 N·mm"));
                }
                println!("circle={circle}, pos={pos}, factor={factor}, moments={moments:?}, ok={}, {}, {}", result.ok(), nm.detail, result.detail);
            }
        }
        let solved =
            sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).unwrap();
        assert_eq!(solved.member_forces.len(), 1);
        assert!(solved.member_forces.iter().all(|(_, f)| f
            .at
            .iter()
            .all(|(p, f)| p.is_finite() && f.iter().all(|v| v.is_finite()))));
        for (_, forces) in &solved.member_forces {
            assert!(!forces.at.is_empty());
            for (_, f) in &forces.at {
                assert!((-f[0] - na_n).abs() < 1e-8);
                assert!((f[4].abs() - 1_000_000.0).abs() < 1e-8);
                assert!((f[5].abs() - 2_000_000.0).abs() < 1e-8);
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
        assert_eq!(report.member_checks.len(), 3);
        for (_, pos, outcome) in report.member_checks {
            let CheckOutcome::Checked(result) = outcome else {
                panic!("{outcome:?}")
            };
            let nm = result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::AxialBending)
                .unwrap();
            if !circle {
                assert_eq!(nm.ratio, 1.0);
                assert!(nm.detail.contains("MA_z=0.0 N·mm, MA_y=0.0 N·mm"));
                assert!(result.ok());
            }
            println!(
                "solved circle={circle}, pos={pos}, ratio={}, ok={}, components={:?}",
                result.ratio(),
                result.ok(),
                result.components
            );
        }
    }
}
