use sepika_core::ids::ElemId;
use sepika_core::model::Model;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm,
    MemberDesignCheckOptions, MemberForcesAt, MemberKind, QdMethod, RcDesign, SeismicQd,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod input;

fn check_column(
    model: &Model,
    at: Vec<(f64, [f64; 6])>,
    options: MemberDesignCheckOptions<'_>,
) -> Vec<(ElemId, f64, CheckOutcome)> {
    run_member_design_checks(model, &[(ElemId(0), MemberForces { at })], &[], &options)
        .unwrap()
        .member_checks
}

#[test]
fn ordinary_short_term_distinguishes_finite_zero_from_nonfinite_demands() {
    for circle in [false, true] {
        let model = input::column_model(circle, 0.0);
        for (component, label) in [(0, "軸力N"), (4, "曲げMy"), (5, "曲げMz")] {
            for value in [0.0, f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
                let mut demand = [0.0; 6];
                demand[component] = value;
                let checks = check_column(
                    &model,
                    vec![(0.0, demand), (0.5, demand), (1.0, demand)],
                    MemberDesignCheckOptions {
                        term: LoadTerm::Short,
                        ..Default::default()
                    },
                );
                assert_eq!(checks.len(), 3, "{checks:?}");
                for ((_, pos, outcome), expected_pos) in checks.iter().zip([0.0, 0.5, 1.0]) {
                    assert_eq!(*pos, expected_pos);
                    if value.is_finite() {
                        let CheckOutcome::Checked(result) = outcome else {
                            panic!("{outcome:?}");
                        };
                        assert_eq!(result.ratio(), 0.0);
                        assert!(result
                            .components
                            .iter()
                            .any(|c| c.kind == CheckKind::AxialBending && c.ratio == 0.0));
                    } else {
                        assert!(
                            matches!(outcome, CheckOutcome::Skipped { reason }
                            if reason.contains(label) && reason.contains("非有限")),
                            "{outcome:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn ordinary_short_term_keeps_independent_bending_axial_and_safety_shear_failures() {
    for circle in [false, true] {
        let model = input::column_model(circle, 0.0);
        for (demand, expected, label) in [
            (
                [0.0, 0.0, 0.0, 0.0, f64::NAN, 1.0e9],
                vec![CheckKind::AxialBending],
                "曲げMy",
            ),
            (
                [0.0, 0.0, 0.0, 0.0, 1.0e9, f64::NAN],
                vec![CheckKind::AxialBending],
                "曲げMz",
            ),
            (
                [1.0e6, 1.0e6, 0.0, 0.0, 0.0, f64::NAN],
                vec![CheckKind::Axial, CheckKind::Shear],
                "曲げMz",
            ),
            (
                [f64::NAN, 1.0e6, 0.0, 0.0, 0.0, 0.0],
                vec![CheckKind::Shear],
                "軸力N",
            ),
        ] {
            let checks = check_column(
                &model,
                vec![(0.5, demand)],
                MemberDesignCheckOptions {
                    term: LoadTerm::Short,
                    rc_damage_control: false,
                    ..Default::default()
                },
            );
            let [(_, 0.5, CheckOutcome::Checked(result)), (_, 0.5, CheckOutcome::Skipped { reason })] =
                checks.as_slice()
            else {
                panic!("{checks:?}");
            };
            assert!(
                reason.contains(label) && reason.contains("非有限"),
                "{reason}"
            );
            assert_eq!(
                result.components.iter().map(|c| c.kind).collect::<Vec<_>>(),
                expected
            );
            assert!(
                result
                    .components
                    .iter()
                    .all(|c| c.ratio.is_finite() && c.ratio > 1.0),
                "{result:?}"
            );
        }
    }
}

#[test]
fn damage_control_uses_all_member_span_samples_and_keeps_the_other_direction_failure() {
    for circle in [false, true] {
        let model = input::column_model(circle, 0.0);
        for (moment, dependent_shear, other_shear, axis) in [(5, 1, 2, "Qy"), (4, 2, 1, "Qz")] {
            for position in [0.0, 0.25, 1.0] {
                let mut incomplete_station = [0.0; 6];
                incomplete_station[moment] = f64::NAN;
                for (shear, expected_ng) in [(dependent_shear, false), (other_shear, true)] {
                    let mut center = [0.0; 6];
                    center[moment] = 1.0e6;
                    center[shear] = 1.0e6;
                    let mut at = vec![(0.5, center), (position, incomplete_station)];
                    at.sort_by(|a, b| f64::total_cmp(&a.0, &b.0));
                    let checks = check_column(
                        &model,
                        at,
                        MemberDesignCheckOptions {
                            term: LoadTerm::Short,
                            rc_damage_control: true,
                            ..Default::default()
                        },
                    );
                    let center: Vec<_> = checks.iter().filter(|(_, pos, _)| *pos == 0.5).collect();
                    assert_eq!(center.len(), if expected_ng { 2 } else { 1 }, "{center:?}");
                    assert!(
                        matches!(&center.last().unwrap().2, CheckOutcome::Skipped { reason }
                        if reason.contains(axis) && reason.contains("損傷制御用せん断スパン")),
                        "{center:?}"
                    );
                    if expected_ng {
                        assert!(
                            matches!(&center[0].2, CheckOutcome::Checked(result)
                            if matches!(result.components.as_slice(), [c] if c.kind == CheckKind::Shear && c.ratio.is_finite() && c.ratio > 1.0)),
                            "{center:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn ordinary_qd2_does_not_require_axial_force_but_qd1_and_min_require_end_axial_force() {
    for circle in [false, true] {
        let mut model = input::column_model(circle, 0.0);
        model.nodes[1].coord[2] = 500.0;
        let gravity = [(
            ElemId(0),
            MemberForces {
                at: vec![(0.0, [0.0; 6]), (0.5, [0.0; 6]), (1.0, [0.0; 6])],
            },
        )];
        for (end_n, center_n) in [(f64::NAN, 0.0), (0.0, f64::NAN)] {
            for qd_method in [QdMethod::Qd2, QdMethod::Qd1, QdMethod::Min] {
                let checks = check_column(
                    &model,
                    vec![
                        (0.0, [end_n, 1.0e6, 0.0, 0.0, 0.0, 0.0]),
                        (0.5, [center_n, 1.0e6, 0.0, 0.0, 0.0, 0.0]),
                        (1.0, [0.0; 6]),
                    ],
                    MemberDesignCheckOptions {
                        term: LoadTerm::Short,
                        qd_method,
                        long_member_forces: Some(&gravity),
                        ..Default::default()
                    },
                );
                let center: Vec<_> = checks.iter().filter(|(_, pos, _)| *pos == 0.5).collect();
                if end_n.is_nan() && qd_method != QdMethod::Qd2 {
                    assert!(
                        matches!(center.as_slice(), [(_, _, CheckOutcome::Skipped { reason })]
                        if reason.contains("柱端の応答軸力が非有限")),
                        "{center:?}"
                    );
                } else {
                    assert!(
                        matches!(&center[0].2, CheckOutcome::Checked(result)
                        if result.components.iter().any(|c| c.kind == CheckKind::Shear && c.ratio > 1.0)),
                        "{circle} {qd_method:?} {center:?}"
                    );
                    assert_eq!(
                        center.len(),
                        if center_n.is_nan() { 2 } else { 1 },
                        "{center:?}"
                    );
                    if center_n.is_nan() {
                        assert!(
                            matches!(&center[1].2, CheckOutcome::Skipped { reason } if reason.contains("軸力Nが非有限")),
                            "{center:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn direct_rc_entry_preserves_qd2_failure_without_axial_force_and_rejects_unknown_min_branch() {
    for circle in [false, true] {
        let model = input::column_model(circle, 0.0);
        for method in [QdMethod::Qd2, QdMethod::Qd1, QdMethod::Min] {
            let ctx = DesignCtx {
                kind: MemberKind::Column,
                term: LoadTerm::Short,
                length: 3000.0,
                rebar_material: Some(model.materials[1].clone()),
                shear_rebar_material: Some(model.materials[1].clone()),
                seismic_qd: Some(SeismicQd {
                    method,
                    long_at: vec![(0.5, [0.0; 6])],
                    n_factor: 1.5,
                    n_mechanism: 1.0,
                    q_simple: None,
                    clear_length: 3000.0,
                }),
                ..Default::default()
            };
            let forces = MemberForcesAt::from_local(0.5, [f64::NAN, 1.0e6, 0.0, 0.0, 0.0, 0.0]);
            let section = &model.sections[0];
            let material = &model.materials[0];
            assert!(matches!(RcDesign.check(&forces, section, material, &ctx),
                CheckOutcome::Skipped { reason } if reason.contains("軸力Nが非有限")));
            let outcomes: Vec<_> = RcDesign
                .check_outcomes(&forces, section, material, &ctx, false)
                .collect();
            if method == QdMethod::Qd2 {
                assert!(
                    matches!(outcomes.as_slice(), [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }]
                    if matches!(result.components.as_slice(), [c] if c.kind == CheckKind::Shear && c.ratio > 1.0)
                    && reason.contains("軸力Nが非有限")),
                    "{outcomes:?}"
                );
            } else {
                assert!(
                    matches!(outcomes.as_slice(), [CheckOutcome::Skipped { reason }]
                    if reason.contains("軸力Nが非有限") && reason.contains("QD1の代替算定")),
                    "{outcomes:?}"
                );
            }
        }
    }
}
