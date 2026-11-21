use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions, QdMethod,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod input;
#[path = "fixtures/rc_column_axial_boundary_reference.rs"]
mod reference;
use input::column_model;

#[test]
fn common_product_entry_retains_only_known_shear_ng_at_the_unchecked_position() {
    for circle in [false, true] {
        let na_n = reference::axial_limit_n(circle);
        let model = column_model(circle, na_n);
        for (term, gravity, qz_n, ng_positions, missing_axes) in [
            (LoadTerm::Long, None, 0.0, &[0.0][..], &[][..]),
            (
                LoadTerm::Short,
                Some(&[][..]),
                0.0,
                &[][..],
                &[0.0, 0.5, 1.0][..],
            ),
            (
                LoadTerm::Short,
                Some(&[(0.0, f64::NAN), (0.5, 0.0), (1.0, 0.0)][..]),
                0.0,
                &[][..],
                &[0.0][..],
            ),
            (
                LoadTerm::Short,
                Some(&[(0.0, f64::NAN), (0.5, 0.0), (1.0, 0.0)][..]),
                1_000_000.0,
                &[0.0][..],
                &[0.0][..],
            ),
            (
                LoadTerm::Short,
                Some(&[(0.0, 0.0), (1.0, 0.0)][..]),
                0.0,
                &[0.0][..],
                &[0.5][..],
            ),
        ] {
            let n_n = na_n * if term == LoadTerm::Long { 1.0 } else { 2.0 };
            let forces = vec![(
                ElemId(0),
                MemberForces {
                    at: vec![
                        (
                            0.0,
                            [-n_n, 1_000_000.0, qz_n, 0.0, 1_000_000.0, 2_000_000.0],
                        ),
                        (0.5, [-n_n, 0.0, 0.0, 0.0, 1_000_000.0, 2_000_000.0]),
                        (1.0, [-n_n, 10_000.0, 0.0, 0.0, 1_000_000.0, 2_000_000.0]),
                    ],
                },
            )];
            let long = gravity.map(|input| {
                if input.is_empty() {
                    return vec![];
                }
                vec![(
                    ElemId(0),
                    MemberForces {
                        at: input
                            .iter()
                            .map(|&(pos, qy)| (pos, [-na_n, qy, 0.0, 0.0, 0.0, 0.0]))
                            .collect(),
                    },
                )]
            });
            let report = run_member_design_checks(
                &model,
                &forces,
                &[],
                &MemberDesignCheckOptions {
                    term,
                    qd_method: QdMethod::Qd2,
                    long_member_forces: long.as_deref(),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut ng = Vec::new();
            let mut skipped = Vec::new();
            for (id, pos, outcome) in report.member_checks {
                assert_eq!(id, ElemId(0));
                match outcome {
                    CheckOutcome::Checked(result) => {
                        assert_eq!(result.components.len(), 1, "{result:?}");
                        assert_eq!(result.components[0].kind, CheckKind::Shear);
                        assert!(result.ratio().is_finite() && result.ratio() > 1.0);
                        ng.push(pos);
                    }
                    CheckOutcome::Skipped { reason } => {
                        assert!(
                            reason.contains("MA_y=0") && reason.contains("検定不能"),
                            "{reason}"
                        );
                        assert_eq!(
                            reason.contains("重力参照内力"),
                            missing_axes.contains(&pos),
                            "{reason}"
                        );
                        skipped.push(pos);
                    }
                }
            }
            assert_eq!(ng, ng_positions, "{circle} {term:?} {gravity:?}");
            assert_eq!(skipped, [0.0, 0.5, 1.0]);
        }
    }
}

#[test]
fn common_product_entry_uses_required_gravity_components_for_qd1_and_minimum() {
    for circle in [false, true] {
        let na_n = reference::axial_limit_n(circle);
        let mut model = column_model(circle, na_n);
        model.nodes[1].coord[2] = 1_000.0;
        let response = [(
            ElemId(0),
            MemberForces {
                at: vec![
                    (
                        0.0,
                        [-2.0 * na_n, 1_000_000.0, 0.0, 0.0, 1_000_000.0, 2_000_000.0],
                    ),
                    (
                        0.5,
                        [-2.0 * na_n, 1_000_000.0, 0.0, 0.0, 1_000_000.0, 2_000_000.0],
                    ),
                    (
                        1.0,
                        [-2.0 * na_n, 1_000_000.0, 0.0, 0.0, 1_000_000.0, 2_000_000.0],
                    ),
                ],
            },
        )];
        for (qd_method, end_n, expected_ng, missing_reason) in [
            (QdMethod::Qd1, -na_n, true, None),
            (QdMethod::Min, -na_n, false, Some("同位置の重力参照内力")),
            (QdMethod::Qd1, f64::NAN, false, Some("柱端の重力参照軸力")),
        ] {
            let gravity = [(
                ElemId(0),
                MemberForces {
                    at: vec![
                        (0.0, [-na_n, 0.0, 0.0, 0.0, 0.0, 0.0]),
                        (1.0, [end_n, 0.0, 0.0, 0.0, 0.0, 0.0]),
                    ],
                },
            )];
            let report = run_member_design_checks(
                &model,
                &response,
                &[],
                &MemberDesignCheckOptions {
                    term: LoadTerm::Short,
                    qd_method,
                    long_member_forces: Some(&gravity),
                    ..Default::default()
                },
            )
            .unwrap();
            let middle: Vec<_> = report
                .member_checks
                .iter()
                .filter_map(|(_, pos, outcome)| (*pos == 0.5).then_some(outcome))
                .collect();
            assert_eq!(
                middle.len(),
                if expected_ng { 2 } else { 1 },
                "{circle} {qd_method:?} {middle:?}"
            );
            let Some(CheckOutcome::Skipped { reason }) = middle.last() else {
                panic!("{middle:?}")
            };
            assert!(reason.contains("許容曲げが非正"), "{reason}");
            if let Some(missing_reason) = missing_reason {
                assert!(reason.contains(missing_reason), "{reason}");
            } else {
                let CheckOutcome::Checked(result) = middle[0] else {
                    panic!("{middle:?}")
                };
                assert_eq!(result.components.len(), 1);
                assert_eq!(result.components[0].kind, CheckKind::Shear);
                assert!(result.ratio().is_finite() && result.ratio() > 1.0);
            }
        }
    }
}

#[test]
fn common_product_entry_rejects_nonzero_bending_at_zero_capacity() {
    for circle in [false, true] {
        let na_n = reference::axial_limit_n(circle);
        let model = column_model(circle, na_n);
        for (factor, moments, expected_ok) in [
            (0.99, [0.0, 0.0], true),
            (0.99, [1.0, 0.0], true),
            (0.99, [0.0, 1.0], true),
            (0.99, [1.0, 1.0], true),
            (1.0, [0.0, 0.0], true),
            (1.0, [1_000_000.0, 0.0], false),
            (1.0, [0.0, 2_000_000.0], false),
            (1.0, [1_000_000.0, 2_000_000.0], false),
            (1.01, [1_000_000.0, 0.0], false),
            (1.01, [0.0, 2_000_000.0], false),
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
                if factor >= 1.0 && moments != [0.0, 0.0] {
                    let CheckOutcome::Skipped { reason } = outcome else {
                        panic!("{outcome:?}")
                    };
                    for field in ["NA=", "N=", "MA_z=0", "MA_y=0", "mz=", "my=", "検定不能"] {
                        assert!(reason.contains(field), "{reason}");
                    }
                    continue;
                }
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

#[path = "fixtures/rc_column_tension_model.rs"]
mod tension_input;
#[path = "fixtures/rc_column_tension_reference.rs"]
mod tension_reference;

#[test]
fn common_product_entry_checks_tension_against_all_main_bars_for_both_load_terms() {
    for circle in [false, true] {
        let model = tension_input::tension_model(input::column_model(circle, 0.0), 0.0, [0.0, 0.0]);
        for term in [LoadTerm::Long, LoadTerm::Short] {
            let nt_n = tension_reference::tension_limit_n(term == LoadTerm::Long);
            for factor in [-0.1, 0.0, 0.25, 0.99, 1.0, 1.01] {
                for moments in [
                    [0.0, 0.0],
                    [1_000_000.0, 0.0],
                    [0.0, 2_000_000.0],
                    [1_000_000.0, 2_000_000.0],
                ] {
                    let forces = vec![(
                        ElemId(0),
                        MemberForces {
                            at: [0.0, 0.5, 1.0]
                                .into_iter()
                                .map(|pos| {
                                    (pos, [factor * nt_n, 0.0, 0.0, 0.0, moments[0], moments[1]])
                                })
                                .collect(),
                        },
                    )];
                    let report = run_member_design_checks(
                        &model,
                        &forces,
                        &[],
                        &MemberDesignCheckOptions {
                            term,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    assert_eq!(report.member_checks.len(), 3);
                    for (_, _, outcome) in report.member_checks {
                        if factor >= 0.99 && moments != [0.0, 0.0] {
                            let CheckOutcome::Skipped { reason } = outcome else {
                                panic!("{circle} {term:?} {factor} {outcome:?}")
                            };
                            assert!(
                                reason.contains("検定不能")
                                    && reason.contains("MA_z=0")
                                    && reason.contains("MA_y=0"),
                                "{reason}"
                            );
                        } else {
                            let CheckOutcome::Checked(result) = outcome else {
                                panic!("{circle} {term:?} {factor} {outcome:?}")
                            };
                            let nm = result
                                .components
                                .iter()
                                .find(|c| c.kind == CheckKind::AxialBending)
                                .unwrap();
                            if moments == [0.0, 0.0] {
                                let expected = if factor >= 0.0 {
                                    factor
                                } else {
                                    let na_long_n = if circle {
                                        2_515_988.458_924_536
                                    } else {
                                        2_254_041.748_339_885
                                    };
                                    let na_n =
                                        na_long_n * if term == LoadTerm::Long { 1.0 } else { 2.0 };
                                    -factor * nt_n / na_n
                                };
                                assert!((nm.ratio - expected).abs() < 1e-12, "{nm:?}");
                            }
                            assert_eq!(
                                result.ok(),
                                factor <= 1.0,
                                "{circle} {term:?} {factor} {result:?}"
                            );
                            assert!(nm.detail.contains("NT="));
                        }
                    }
                }
            }
            let forces = vec![(
                ElemId(0),
                MemberForces {
                    at: [0.0, 0.5, 1.0]
                        .into_iter()
                        .map(|pos| (pos, [1_000_000.0, 0.0, 0.0, 0.0, 0.0, 0.0]))
                        .collect(),
                },
            )];
            let report = run_member_design_checks(
                &model,
                &forces,
                &[],
                &MemberDesignCheckOptions {
                    term,
                    ..Default::default()
                },
            )
            .unwrap();
            for (_, _, outcome) in report.member_checks {
                let CheckOutcome::Checked(result) = outcome else {
                    panic!("{outcome:?}")
                };
                assert!(!result.ok());
                let expected = if term == LoadTerm::Long {
                    2.0505693885446803
                } else {
                    1.2778910682234963
                };
                assert!((result.ratio() - expected).abs() < 1e-9, "{result:?}");
            }
        }
    }
}
