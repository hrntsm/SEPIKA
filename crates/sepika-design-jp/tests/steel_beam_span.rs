#[path = "fixtures/steel_h_column_combined_model.rs"]
mod combined_column;

#[path = "fixtures/steel_beam_span_model.rs"]
mod input;

use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::{MemberLoad, MemberLoadKind, Model, SteelDesignAttr};
use sepika_design_jp::{
    girder_group_overrides, run_member_design_checks, CheckKind, CheckOutcome, CheckResult,
    LoadTerm, MemberDesignCheckOptions,
};
use sepika_solver::statics::{analysis::Analysis, linear::StaticOnce};

fn check(
    model: &Model,
    solved: &StaticOnce,
    term: LoadTerm,
    terms: Option<&[(LoadCaseId, f64)]>,
) -> Vec<(f64, CheckOutcome)> {
    check_member(model, solved, term, terms, input::BEAM)
}

fn check_member(
    model: &Model,
    solved: &StaticOnce,
    term: LoadTerm,
    terms: Option<&[(LoadCaseId, f64)]>,
    member: ElemId,
) -> Vec<(f64, CheckOutcome)> {
    let groups = girder_group_overrides(model, &solved.member_forces);
    checked_member(
        model,
        solved,
        &MemberDesignCheckOptions {
            term,
            steel_beam_span_load_terms: terms,
            girder_group_overrides: Some(&groups),
            ..Default::default()
        },
        member,
    )
}

fn checked_member(
    model: &Model,
    solved: &StaticOnce,
    options: &MemberDesignCheckOptions<'_>,
    member: ElemId,
) -> Vec<(f64, CheckOutcome)> {
    run_member_design_checks(model, &solved.member_forces, &solved.panel_moments, options)
        .unwrap()
        .member_checks
        .into_iter()
        .filter(|(id, _, _)| *id == member)
        .map(|(_, pos, result)| (pos, result))
        .collect()
}

fn at(outcomes: &[(f64, CheckOutcome)], xi: f64) -> &CheckResult {
    outcomes
        .iter()
        .find_map(|(pos, outcome)| match outcome {
            CheckOutcome::Checked(result) if (pos - xi).abs() < 1e-12 => Some(result),
            _ => None,
        })
        .unwrap_or_else(|| panic!("ξ={xi}: {outcomes:?}"))
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

fn check_bending(result: &CheckResult, ratio: f64, detail: &str) {
    let bend = result
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Bending)
        .unwrap();
    near(bend.ratio, ratio);
    assert!(bend.detail.contains(detail), "{}", bend.detail);
}

#[test]
fn middle_brace_checks_both_intervals_against_their_own_long_and_short_allowables() {
    let model = input::simply_supported_with_middle_brace(2_000.0, 150_000.0);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let raw = &solved.member_forces[0].1.at;
    assert_eq!(raw.len(), 3);
    assert!(raw.iter().all(|(xi, _)| *xi != 0.25));
    near(raw[1].1[5].abs() / 1e6, 150.0);

    // M(L/4)=3PL/16=225 kN·m、M(L/2)=PL/8=150 kN·m。
    // 左は区間内最大、右は単調で端部比0。Z=1,148,243.4133 mm³。
    for (term, left_fb, right_fb, left_peak, left_end, right_end) in [
        (
            LoadTerm::Long,
            "fb=112.3707",
            "fb=144.5830",
            1.743_795_729,
            1.162_530_486,
            0.903_524_681,
        ),
        (
            LoadTerm::Short,
            "fb=168.5560",
            "fb=216.8745",
            1.162_530_486,
            0.775_020_324,
            0.602_349_787,
        ),
    ] {
        let rows = check(&model, &solved, term, Some(&terms));
        assert!(
            rows.iter()
                .all(|(_, row)| matches!(row, CheckOutcome::Checked(_))),
            "{rows:?}"
        );
        for (pos, interval, count, ratio, fb) in [
            (0.0, "[0.000, 4000.000]", 1, 0.0, left_fb),
            (0.25, "[0.000, 4000.000]", 2, left_peak, left_fb),
            (0.5, "[0.000, 4000.000]", 1, left_end, left_fb),
            (0.5, "[4000.000, 8000.000]", 1, right_end, right_fb),
            (1.0, "[4000.000, 8000.000]", 1, 0.0, right_fb),
        ] {
            let matching = rows
                .iter()
                .filter_map(|(xi, row)| match row {
                    CheckOutcome::Checked(result)
                        if *xi == pos && result.detail.contains(interval) =>
                    {
                        Some(result)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), count, "{rows:?}");
            for result in matching {
                check_bending(result, ratio, fb);
                check_bending(result, ratio, "lb=4000 mm");
            }
        }
    }
}

#[test]
fn unverified_middle_bracing_retains_only_independently_proven_bending_failure() {
    let terms = [(input::DEAD, 1.0)];
    for (point_load, expect_ng) in [(150_000.0, false), (320_000.0, true)] {
        let model = input::simply_supported_with_middle_brace(2_000.0, point_load);
        let solved = Analysis::prepare(&model)
            .unwrap()
            .linear_static(input::DEAD)
            .unwrap();
        let mut direct = model.clone();
        direct.steel_design_attrs[0].lateral_brace_count = None;
        direct.steel_design_attrs[0].lb_direct = Some((4_000.0, 4_000.0, 4_000.0));
        let mut conflicting = model.clone();
        conflicting.steel_design_attrs[0].lb_direct = Some((3_000.0, 3_000.0, 3_000.0));
        for (input, load_terms, reason) in [
            (&model, None, "荷重組合せの由来が未確認"),
            (&direct, Some(&terms[..]), "補剛端の位置を確認できません"),
            (
                &conflicting,
                Some(&terms[..]),
                "補剛端の位置を確認できません",
            ),
        ] {
            let rows = check(input, &solved, LoadTerm::Short, load_terms);
            assert!(
                rows.iter().any(|(_, row)| matches!(row,
                CheckOutcome::Skipped { reason: detail } if detail.contains(reason))),
                "{rows:?}"
            );
            assert!(!rows.iter().any(|(xi, _)| *xi == 0.25), "{rows:?}");
            let bending_ng = rows
                .iter()
                .filter_map(|(xi, row)| match row {
                    CheckOutcome::Checked(result) if *xi == 0.5 => Some(result),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(bending_ng.len(), usize::from(expect_ng), "{rows:?}");
            if let Some(result) = bending_ng.first() {
                check_bending(result, 1.185_900_230, "fb上限=235.0000");
            }
        }
    }
}

#[test]
fn brace_count_limit_applies_even_to_very_short_calculated_lengths() {
    for length in [input::LENGTH_MM, 1.0] {
        let mut model = input::simply_supported_with_middle_brace(length / 4.0, 150.0);
        model.nodes[1].coord[0] = length;
        model.steel_design_attrs[0].lateral_brace_count = Some(u32::MAX);
        let solved = Analysis::prepare(&model)
            .unwrap()
            .linear_static(input::DEAD)
            .unwrap();
        let terms = [(input::DEAD, 1.0)];
        let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
        assert!(
            rows.iter()
                .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains("等間隔横補剛本数が通常検定の上限"))),
            "{rows:?}"
        );
        assert!(
            !rows.iter().any(|(xi, row)| *xi == 0.5
                && matches!(row, CheckOutcome::Checked(result) if result.ok())),
            "{rows:?}"
        );

        model.steel_design_attrs[0].lb_direct = Some((0.0, 0.0, 0.0));
        let direct = check(&model, &solved, LoadTerm::Short, None);
        assert!(direct
            .iter()
            .all(|(_, row)| matches!(row, CheckOutcome::Checked(_))));
        assert!(at(&direct, 0.5)
            .components
            .iter()
            .any(|c| c.kind == CheckKind::Bending && c.detail.contains("lb=0 mm")));
    }
}

#[test]
fn uniform_load_checks_both_intervals_at_the_zero_shear_brace() {
    let mut model = input::simply_supported_with_middle_brace(4_000.0, 1.0);
    model.load_cases[0].member[0].kind = MemberLoadKind::Distributed {
        a: 0.0,
        b: input::LENGTH_MM,
        w1: -8.0,
        w2: -8.0,
    };
    let terms = [(input::DEAD, 1.0)];
    let report = |shear_residual_n: f64, saved_center_qy_n: Option<f64>| {
        use sepika_element::frame::beam::MemberForces;

        // R=wL/2=32 kN、Q(L/2)=0、M(L/2)=wL²/8=64 kN·m。
        let forces = [
            (0.0, 32_000.0, 0.0),
            (0.5, 0.0, 64e6),
            (1.0, -32_000.0, 0.0),
        ];
        let raw = MemberForces {
            at: forces
                .into_iter()
                .filter(|(xi, _, _)| *xi != 0.5 || saved_center_qy_n.is_some())
                .map(|(xi, q, m)| {
                    (
                        xi,
                        [
                            0.0,
                            if xi == 0.5 {
                                saved_center_qy_n.unwrap()
                            } else {
                                q + shear_residual_n
                            },
                            0.0,
                            0.0,
                            0.0,
                            m + shear_residual_n * xi * input::LENGTH_MM,
                        ],
                    )
                })
                .collect(),
        };
        run_member_design_checks(
            &model,
            &[(input::BEAM, raw)],
            &[],
            &MemberDesignCheckOptions {
                term: LoadTerm::Long,
                steel_beam_span_load_terms: Some(&terms),
                ..Default::default()
            },
        )
        .unwrap()
        .member_checks
        .into_iter()
        .map(|(_, xi, row)| (xi, row))
        .collect::<Vec<_>>()
    };
    let results = report(0.0, Some(0.0));
    assert!(
        results
            .iter()
            .all(|(_, row)| matches!(row, CheckOutcome::Checked(_))),
        "{results:?}"
    );
    let brace = results
        .iter()
        .filter_map(|(pos, row)| match row {
            CheckOutcome::Checked(result) if *pos == 0.5 => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(brace.len(), 2, "{results:?}");
    for (row, interval) in brace
        .iter()
        .zip(["[0.000, 4000.000]", "[4000.000, 8000.000]"])
    {
        assert!(row.detail.contains(interval));
        check_bending(row, 0.385_503_863_825, "fb=144.5830");
    }

    let ambiguous = report(-1e-11, Some(-1e-11));
    assert!(
        ambiguous
            .iter()
            .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
        if reason.contains("[0.000, 4000.000]") && reason.contains("強軸曲げ分布を確認できず"))),
        "{ambiguous:?}"
    );

    let exact_without_saved_center = report(0.0, None);
    assert_eq!(
        exact_without_saved_center
            .iter()
            .filter(|(xi, row)| *xi == 0.5 && matches!(row, CheckOutcome::Checked(_)))
            .count(),
        2
    );

    let contradictory = report(0.0, Some(-1e-11));
    for interval in ["[0.000, 4000.000]", "[4000.000, 8000.000]"] {
        assert!(
            contradictory.iter().any(|(xi, row)| *xi == 0.5
                && matches!(row, CheckOutcome::Skipped { reason } if reason.contains(interval))),
            "{contradictory:?}"
        );
    }
}

#[test]
fn point_load_at_the_brace_uses_each_intervals_inner_cut() {
    let model = input::simply_supported_with_middle_brace(4_000.0, 100_000.0);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
    let brace = rows.iter().filter(|(xi, _)| *xi == 0.5).collect::<Vec<_>>();
    assert_eq!(brace.len(), 2, "{rows:?}");
    assert!(matches!(&brace[0].1, CheckOutcome::Checked(result)
        if result.detail.contains("[0.000, 4000.000]") && !result.detail.contains("集中荷重の右側")));
    assert!(matches!(&brace[1].1, CheckOutcome::Checked(result)
        if result.detail.contains("[4000.000, 8000.000]") && result.detail.contains("集中荷重の右側")));
}

#[test]
fn point_load_between_design_sections_makes_the_saved_short_combination_fail_bending() {
    let model = input::short_point_combination();
    let combo = &model.combinations[0];
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_combination(combo)
        .unwrap();
    let sampled = &solved.member_forces[0].1.at;
    for (&(pos, forces), (xi, mz_knm)) in
        sampled.iter().zip([(0.0, 100.0), (0.5, 90.0), (1.0, 0.0)])
    {
        assert_eq!(pos, xi);
        near(forces[5] / 1e6, mz_knm);
    }
    assert_eq!(sampled.len(), 3);
    for (term, ratio, fb) in [
        (LoadTerm::Long, 1.793_841_894, "fb=65.5414"),
        (LoadTerm::Short, 1.195_894_596, "fb=98.3121"),
    ] {
        let result = check(&model, &solved, term, Some(&combo.terms));
        let peak = at(&result, 0.25);
        assert!(!peak.ok());
        check_bending(peak, ratio, fb);
        assert!(
            result
                .iter()
                .all(|(_, row)| matches!(row, CheckOutcome::Checked(_))),
            "{result:?}"
        );
    }
}

#[test]
fn asymmetric_distributed_load_finds_the_shear_zero_without_sampling() {
    let model = input::cantilever(
        20_000.0,
        0.0,
        Some(MemberLoadKind::Distributed {
            a: 0.0,
            b: input::LENGTH_MM,
            w1: -12.5,
            w2: 0.0,
        }),
    );
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let xi = 1.0 - 0.4_f64.sqrt();
    for (term, expected) in [
        (LoadTerm::Long, 0.896_414_996),
        (LoadTerm::Short, 0.597_609_997),
    ] {
        let results = check(&model, &solved, term, Some(&terms));
        let peak = at(&results, xi);
        check_bending(peak, expected, "C=1.0000, 塑性限界細長比=0.3000");
        assert!(peak.ok());
    }
}

#[test]
fn monotonic_reverse_curvature_uses_the_positive_end_ratio() {
    let model = input::cantilever(12_500.0, 25e6, None);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    for (term, expected, fb) in [
        (LoadTerm::Long, 0.572_558_725, "fb=114.0794"),
        (LoadTerm::Short, 0.381_705_817, "fb=171.1191"),
    ] {
        let results = check(&model, &solved, term, Some(&terms));
        let start = at(&results, 0.0);
        check_bending(start, expected, fb);
        check_bending(start, expected, "C=2.1333, 塑性限界細長比=0.7000");
        assert!(start.ok());
    }
}

#[test]
fn nonmonotonic_with_largest_absolute_moment_at_end_has_no_verified_coefficient() {
    let model = input::cantilever(
        20_000.0,
        0.0,
        Some(MemberLoadKind::Distributed {
            a: 0.0,
            b: input::LENGTH_MM,
            w1: 0.0,
            w2: -12.5,
        }),
    );
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let results = check(&model, &solved, LoadTerm::Short, Some(&terms));
    assert!(results.iter().any(|(_, result)| matches!(result,
        CheckOutcome::Skipped { reason } if reason.contains("単調性または区間内部の絶対値最大を確認できません"))), "{results:?}");
    assert!(results.iter().all(|(_, result)| !matches!(result, CheckOutcome::Checked(row) if row.components.iter().any(|c| c.kind == CheckKind::Bending))));
}

#[test]
fn unverified_missing_and_inconsistent_responses_do_not_erase_independent_failure() {
    let model = input::cantilever(50_000.0, 0.0, None);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let unknown = check(&model, &solved, LoadTerm::Short, None);
    let start = at(&unknown, 0.0);
    assert!(!start.ok());
    check_bending(start, 1.482_375_287, "fb上限=235.0000");
    assert!(unknown.iter().any(|(_, result)| matches!(result, CheckOutcome::Skipped { reason } if reason.contains("荷重組合せの由来が未確認"))));

    let mut missing = solved.clone();
    missing.member_forces[0].1.at.retain(|(xi, _)| *xi == 0.5);
    let results = check(&model, &missing, LoadTerm::Short, Some(&terms));
    assert!(results.iter().any(|(_, result)| matches!(result, CheckOutcome::Skipped { reason } if reason.contains("i端の解析断面力がありません"))));

    let mut mismatch = solved;
    mismatch.member_forces[0].1.at[1].1[5] += 1e6;
    let results = check(&model, &mismatch, LoadTerm::Short, Some(&terms));
    assert!(results.iter().any(|(_, result)| matches!(result, CheckOutcome::Skipped { reason } if reason.contains("釣合いを確認できません"))));
    assert!(!at(&results, 0.0).ok());
}

#[test]
fn interior_plateau_equal_to_an_endpoint_uses_the_interior_maximum_coefficients() {
    let model = input::cantilever(
        0.0,
        -75e6,
        Some(MemberLoadKind::Point {
            a: input::LENGTH_MM / 4.0,
            p: -10_000.0,
        }),
    );
    let mut solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    // 釣合いから得た厳密な切片力で、せん断0の区間を確認する。
    solved.member_forces[0].1.at = vec![
        (0.0, [0.0, 10_000.0, 0.0, 0.0, 0.0, 55e6]),
        (0.5, [0.0, 0.0, 0.0, 0.0, 0.0, 75e6]),
        (1.0, [0.0, 0.0, 0.0, 0.0, 0.0, 75e6]),
    ];
    let terms = [(input::DEAD, 1.0)];
    let results = check(&model, &solved, LoadTerm::Short, Some(&terms));
    check_bending(
        at(&results, 0.5),
        0.664_385_887,
        "C=1.0000, 塑性限界細長比=0.3000",
    );
}

#[test]
fn moment_diagram_uses_the_full_span_but_demand_does_not_add_points_inside_rigid_ends() {
    let mut model = input::cantilever(
        22_500.0,
        0.0,
        Some(MemberLoadKind::Point {
            a: input::LENGTH_MM / 4.0,
            p: -40_000.0,
        }),
    );
    model.elements[0].rigid_zone.face_i = Some(3000.0);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let results = check(&model, &solved, LoadTerm::Short, Some(&terms));
    let face = at(&results, 0.375);
    check_bending(face, 0.996_578_830, "C=1.0000, 塑性限界細長比=0.3000");
    assert!(face.ok());
    assert!(results.iter().all(|(xi, _)| *xi >= 0.375), "{results:?}");
}

#[test]
fn missing_and_duplicate_member_rows_have_distinct_unavailable_outcomes() {
    let model = input::short_point_combination();
    let combo = &model.combinations[0];
    let mut solved = Analysis::prepare(&model)
        .unwrap()
        .linear_combination(combo)
        .unwrap();
    let mut duplicate = solved.clone();
    duplicate
        .member_forces
        .push(solved.member_forces[0].clone());
    solved.member_forces.clear();
    for (response, expected) in [
        (solved.clone(), "鋼梁の解析断面力がありません"),
        (duplicate, "鋼梁の解析断面力が重複"),
    ] {
        let rows = check(&model, &response, LoadTerm::Short, Some(&combo.terms));
        assert!(
            matches!(&rows[..], [(_, CheckOutcome::Skipped { reason })]
            if reason.contains(expected)),
            "{rows:?}"
        );
    }

    let mut partial = model;
    partial
        .steel_design_attrs
        .push(sepika_core::model::SteelDesignAttr {
            elem: input::BEAM,
            lateral_brace_count: Some(1),
            column_lateral_buckling_end_supports: None,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lk_y_direct: None,
            lk_z_direct: None,
            c_direct: None,
        });
    assert!(matches!(
        &check(&partial, &solved, LoadTerm::Short, None)[..],
        [(_, CheckOutcome::Skipped { reason })] if reason.contains("鋼梁の解析断面力がありません")
    ));
}

#[test]
fn conflicting_same_position_does_not_erase_an_independent_failure_at_a_unique_section() {
    let model = input::cantilever(100_000.0, 0.0, None);
    let mut solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    solved.member_forces[0]
        .1
        .at
        .insert(1, (0.0, [0.0, 0.0, 0.0, 0.0, 0.0, -500e6]));
    let terms = [(input::DEAD, 1.0)];
    let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
    assert!(
        rows.iter().any(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("同一の評価位置で解析断面力が重複"))
        ),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .all(|(xi, outcome)| *xi != 0.0 || !matches!(outcome, CheckOutcome::Checked(_))),
        "{rows:?}"
    );
    let center = at(&rows, 0.5);
    check_bending(center, 1.482_375_287, "fb上限=235.0000");
    assert!(!center.ok());
}

#[test]
fn strong_bending_at_a_point_load_checks_both_simultaneous_cut_sides() {
    let mut model = input::cantilever(
        12_000.0,
        0.0,
        Some(MemberLoadKind::Point {
            a: input::LENGTH_MM / 4.0,
            p: -20_000.0,
        }),
    );
    model.load_cases[0].nodal[0].values[0] = -150_000.0;
    model.load_cases[0].member.push(MemberLoad::manual(
        input::BEAM,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Point {
            a: input::LENGTH_MM / 4.0,
            p: 150_000.0,
        },
    ));
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
    let sides: Vec<_> = rows
        .iter()
        .filter(|(xi, _)| *xi == 0.25)
        .map(|(_, row)| row)
        .collect();
    let [CheckOutcome::Checked(left), CheckOutcome::Checked(right)] = sides.as_slice() else {
        panic!("{rows:?}");
    };
    assert!(left.detail.contains("集中荷重の左側"));
    assert!(right.detail.contains("集中荷重の右側"));
    check_bending(left, 0.637_810_451, "σax=0.0000");
    check_bending(right, 1.032_736_814, "σax=18.3105");
    assert!(left.ok() && !right.ok());
    assert!(rows
        .iter()
        .filter(|(xi, _)| *xi != 0.25)
        .all(|(_, row)| matches!(row, CheckOutcome::Checked(result) if result.ok())));

    model.elements[0].rigid_zone.face_i = Some(2_000.0);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
    let face_rows: Vec<_> = rows
        .iter()
        .filter(|(xi, _)| *xi == 0.25)
        .map(|(_, row)| row)
        .collect();
    let [CheckOutcome::Checked(inside)] = face_rows.as_slice() else {
        panic!("{rows:?}");
    };
    assert!(inside.detail.contains("集中荷重の右側"), "{inside:?}");
    check_bending(inside, 1.032_736_814, "σax=18.3105");
    assert!(!inside.ok());
}

#[test]
fn all_equal_moment_peak_candidates_use_forces_at_their_own_location() {
    let mut model = input::cantilever(0.0, 0.0, None);
    let loads = &mut model.load_cases[0].member;
    for (a, p) in [
        (1_000.0, -144_000.0),
        (2_000.0, 144_000.0),
        (3_000.0, -144_000.0),
        (4_000.0, 72_000.0),
    ] {
        loads.push(MemberLoad::manual(
            input::BEAM,
            [0.0, 0.0, 1.0],
            MemberLoadKind::Point { a, p },
        ));
    }
    for (a, p) in [(2_500.0, 150_000.0), (3_500.0, -150_000.0)] {
        loads.push(MemberLoad::manual(
            input::BEAM,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Point { a, p },
        ));
    }
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
    let first = at(&rows, 0.125);
    let second = at(&rows, 0.375);
    check_bending(first, 0.637_810_451, "σax=0.0000");
    check_bending(second, 1.032_736_814, "σax=18.3105");
    assert!(first.ok() && !second.ok());
}

#[test]
fn the_recovered_demand_range_uses_unclamped_faces_and_rejects_unconfirmed_faces() {
    let mut model = input::cantilever(
        22_500.0,
        0.0,
        Some(MemberLoadKind::Point {
            a: 4_500.0,
            p: -40_000.0,
        }),
    );
    model.elements[0].rigid_zone.face_i = Some(5_000.0);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
    let face = at(&rows, 0.625);
    check_bending(face, 0.597_947_298, "C=1.0000, 塑性限界細長比=0.3000");
    assert!(rows.iter().all(|(xi, _)| *xi != 0.5625), "{rows:?}");

    for (face, expected) in [
        (None, "材端フェース位置が未確認"),
        (Some(8_000.0), "材端フェース間に正の区間を確認できません"),
    ] {
        model.elements[0].rigid_zone.face_i = face;
        let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
        assert!(rows.iter().any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason } if reason.contains(expected))), "{rows:?}");
    }
}

#[test]
fn the_j_face_uses_the_physical_left_cut_even_when_xi_rounds_to_the_right() {
    for face_j_mm in [498.3, 499.1] {
        assert_j_face_inside_cut(face_j_mm);
    }
}

fn assert_j_face_inside_cut(face_j_mm: f64) {
    const LENGTH: f64 = 8_395.6;
    let point_mm = LENGTH - face_j_mm;
    let tip_moment_nmm = 72e6 - face_j_mm * 20_000.0;
    let mut model = input::cantilever(
        20_000.0,
        -tip_moment_nmm,
        Some(MemberLoadKind::Point {
            a: point_mm,
            p: -30_000.0,
        }),
    );
    model.nodes[1].coord[0] = LENGTH;
    model.elements[0].rigid_zone.face_j = Some(face_j_mm);
    model.load_cases[0].member.push(MemberLoad::manual(
        input::BEAM,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Point {
            a: point_mm,
            p: -150_000.0,
        },
    ));
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let xi = point_mm / LENGTH;
    let sampled_xi = 1.0 - face_j_mm / LENGTH;
    assert!(sampled_xi * LENGTH > point_mm);
    let sample = solved.member_forces[0]
        .1
        .at
        .iter()
        .find(|(pos, _)| *pos == sampled_xi)
        .unwrap();
    near(sample.1[0], 0.0);
    let terms = [(input::DEAD, 1.0)];
    let rows = check(&model, &solved, LoadTerm::Short, Some(&terms));
    let at_face = at(&rows, xi);
    assert_eq!(
        rows.iter()
            .filter(|(pos, _)| (pos - xi).abs() <= f64::EPSILON)
            .count(),
        1
    );
    check_bending(at_face, 1.117_006_539, "σax=18.3105");
    assert!(!at_face.ok());
}

const SECOND_BEAM: ElemId = ElemId(1);
const SPLIT_MM: f64 = 4_800.0;

fn split_cantilever(mut model: Model, split_mm: f64) -> Model {
    let mut joint = model.nodes[1].clone();
    joint.id = NodeId(2);
    joint.coord[0] = split_mm;
    model.nodes.push(joint);
    let mut second = model.elements[0].clone();
    second.id = SECOND_BEAM;
    second.nodes[0] = NodeId(2);
    model.elements[0].nodes[1] = NodeId(2);
    model.elements.push(second);
    model.girder_groups = vec![vec![input::BEAM, SECOND_BEAM]];
    let length = input::LENGTH_MM;
    model.steel_design_attrs = [input::BEAM, SECOND_BEAM]
        .into_iter()
        .map(|elem| SteelDesignAttr {
            elem,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: Some((length, length, length)),
            lateral_brace_count: None,
            lk_y_direct: None,
            lk_z_direct: None,
            c_direct: None,
            column_lateral_buckling_end_supports: None,
        })
        .collect();
    model
}

#[test]
fn split_and_unsplit_girders_have_the_same_verified_interior_maximum_and_allowable_stress() {
    let whole = input::short_point_combination();
    let divided = split_cantilever(whole.clone(), SPLIT_MM);
    let solve = |model: &Model| {
        Analysis::prepare(model)
            .unwrap()
            .linear_combination(&model.combinations[0])
            .unwrap()
    };
    let divided_response = solve(&divided);
    let divided_i = &divided_response.member_forces[0].1;
    let divided_j = &divided_response.member_forces[1].1;
    near(divided_i.at[0].1[5] / 1e6, 100.0);
    near(divided_i.at.last().unwrap().1[5] / 1e6, 72.0);
    near(divided_j.at[0].1[5] / 1e6, 72.0);
    near(divided_j.at.last().unwrap().1[5] / 1e6, 0.0);
    let whole_response = solve(&whole);
    for (term, ratio, fb) in [
        (LoadTerm::Long, 1.793_841_894, "fb=65.5414"),
        (LoadTerm::Short, 1.195_894_596, "fb=98.3121"),
    ] {
        for (model, response, xi) in [
            (&whole, &whole_response, 0.25),
            (&divided, &divided_response, 2_000.0 / SPLIT_MM),
        ] {
            let result = check(model, response, term, Some(&model.combinations[0].terms));
            let peak = at(&result, xi);
            assert!(!peak.ok());
            check_bending(peak, ratio, fb);
            check_bending(peak, ratio, "C=1.0000, 塑性限界細長比=0.3000");
        }
    }
}

#[test]
fn split_girder_supports_monotonic_reverse_curvature_and_the_outer_end_ratio() {
    let model = split_cantilever(input::cantilever(12_500.0, 25e6, None), SPLIT_MM);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let results = check(
        &model,
        &solved,
        LoadTerm::Short,
        Some(&[(input::DEAD, 1.0)]),
    );
    check_bending(
        at(&results, 0.0),
        0.381_705_817,
        "C=2.1333, 塑性限界細長比=0.7000",
    );
}

#[test]
fn unverified_split_girder_keeps_independent_bending_failure() {
    let model = split_cantilever(input::cantilever(50_000.0, 0.0, None), SPLIT_MM);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let mut disconnected = model.clone();
    let mut duplicate_joint = disconnected.nodes[2].clone();
    duplicate_joint.id = NodeId(3);
    disconnected.nodes.push(duplicate_joint);
    disconnected.elements[1].nodes[0] = NodeId(3);
    let mut bent = model.clone();
    bent.nodes[2].coord[1] = 100.0;
    let mut reversed = model.clone();
    reversed.elements[1].local_axis.ref_vector = [0.0, 0.0, -1.0];
    let mut missing = solved.clone();
    missing.member_forces.retain(|(id, _)| *id != SECOND_BEAM);
    let mut discontinuous = solved.clone();
    discontinuous.member_forces[1].1.at[0].1[5] += 1e6;
    let mut unconfirmed_supports = model.clone();
    unconfirmed_supports.steel_design_attrs.pop();

    for (model, response, reason) in [
        (
            &disconnected,
            &solved,
            "接続節点・実材軸または断面が連続していません",
        ),
        (&bent, &solved, "実材軸が共線かつ同方向ではありません"),
        (&reversed, &solved, "実材軸が共線かつ同方向ではありません"),
        (&model, &missing, "一意な解析断面力がありません"),
        (
            &model,
            &discontinuous,
            "要素境界で解析断面力が連続していません",
        ),
        (
            &unconfirmed_supports,
            &solved,
            "有効支点間距離が指定されていません",
        ),
    ] {
        let results = check(
            model,
            response,
            LoadTerm::Short,
            Some(&[(input::DEAD, 1.0)]),
        );
        assert!(
            results.iter().any(|(_, outcome)| matches!(outcome,
            CheckOutcome::Skipped { reason: actual } if actual.contains(reason))),
            "{results:?}"
        );
        let start = at(&results, 0.0);
        assert!(!start.ok());
        check_bending(start, 1.482_375_287, "fb上限=235.0000");
    }
}

#[test]
fn split_distributed_load_finds_the_maximum_on_the_later_element() {
    let split_mm = 1_600.0;
    let model = input::cantilever(
        20_000.0,
        0.0,
        Some(MemberLoadKind::Distributed {
            a: 0.0,
            b: split_mm,
            w1: -12.5,
            w2: -10.0,
        }),
    );
    let mut model = split_cantilever(model, split_mm);
    model.load_cases[0].member.push(MemberLoad::manual(
        SECOND_BEAM,
        [0.0, 0.0, 1.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 6_400.0,
            w1: -10.0,
            w2: 0.0,
        },
    ));
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let terms = [(input::DEAD, 1.0)];
    let later = check_member(&model, &solved, LoadTerm::Short, Some(&terms), SECOND_BEAM);
    // 自由端側の荷重合力が釣り合う位置は全長比 1−√0.4。
    let xi_local = ((1.0 - 0.4_f64.sqrt()) * input::LENGTH_MM - split_mm) / 6_400.0;
    check_bending(at(&later, xi_local), 0.597_609_997, "fb=98.3121");
}

#[test]
fn verified_full_span_uses_its_own_length_with_or_without_caller_group_override() {
    let model = split_cantilever(input::cantilever(12_500.0, 25e6, None), SPLIT_MM);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let mut groups = girder_group_overrides(&model, &solved.member_forces);
    for group in groups.values_mut() {
        group.length = 9_000.0;
    }
    for override_length in [None, Some(&groups)] {
        let results = checked_member(
            &model,
            &solved,
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                steel_beam_span_load_terms: Some(&[(input::DEAD, 1.0)]),
                girder_group_overrides: override_length,
                ..Default::default()
            },
            input::BEAM,
        );
        let start = at(&results, 0.0);
        check_bending(start, 0.381_705_817, "採用横座屈長さ lb=8000 mm");
        check_bending(start, 0.381_705_817, "C=2.1333, 塑性限界細長比=0.7000");
    }
}

#[test]
fn split_girder_preserves_zero_section_checks_but_positive_lengths_do_not_prove_brace_positions() {
    let model = split_cantilever(input::cantilever(12_500.0, 25e6, None), SPLIT_MM);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    for (direct, count) in [
        (Some((0.0, 0.0, 0.0)), None),
        (Some((4_000.0, 4_000.0, 4_000.0)), None),
        (None, Some(1)),
    ] {
        let mut model = model.clone();
        for attr in &mut model.steel_design_attrs {
            attr.lb_direct = direct;
            attr.lateral_brace_count = count;
        }
        let results = check(&model, &solved, LoadTerm::Short, None);
        if direct == Some((0.0, 0.0, 0.0)) {
            let start = at(&results, 0.0);
            check_bending(start, 0.277_945_366, "lb=0 mm");
            assert!(results
                .iter()
                .all(|(_, row)| matches!(row, CheckOutcome::Checked(_))));
        } else {
            assert!(
                results
                    .iter()
                    .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
                if reason.contains("補剛端の位置を確認できません"))),
                "{results:?}"
            );
            assert!(results
                .iter()
                .all(|(_, row)| matches!(row, CheckOutcome::Skipped { .. })));
        }
    }
}

#[test]
fn a_single_elements_partial_bracing_does_not_verify_either_part_of_the_girder() {
    let mut model = split_cantilever(input::cantilever(12_500.0, 25e6, None), SPLIT_MM);
    model.steel_design_attrs[0].lb_direct = None;
    model.steel_design_attrs[0].lateral_brace_count = Some(1);
    model.steel_design_attrs.pop();
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let first = check(&model, &solved, LoadTerm::Short, None);
    assert!(
        first
            .iter()
            .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
        if reason.contains("補剛端の位置を確認できません"))),
        "{first:?}"
    );
    let second = check_member(&model, &solved, LoadTerm::Short, None, SECOND_BEAM);
    assert!(second.iter().any(|(_, outcome)| matches!(outcome,
        CheckOutcome::Skipped { reason } if reason.contains("共通の単一補剛区間が指定されていません"))));
    assert!(first
        .iter()
        .chain(&second)
        .all(|(_, row)| matches!(row, CheckOutcome::Skipped { .. })));
}

#[test]
fn split_girder_preserves_the_left_cut_at_a_saved_point_load_station() {
    let load_mm = 2_000.5;
    let whole = input::cantilever(
        22_500.0,
        0.0,
        Some(MemberLoadKind::Point {
            a: load_mm,
            p: -40_000.0,
        }),
    );
    let split = split_cantilever(whole.clone(), 4_001.0);
    let terms = [(input::DEAD, 1.0)];
    for (model, xi) in [(&whole, load_mm / input::LENGTH_MM), (&split, 0.5)] {
        let solved = Analysis::prepare(model)
            .unwrap()
            .linear_static(input::DEAD)
            .unwrap();
        for (term, expected, fb) in [
            (LoadTerm::Long, 1.793_692_407, "fb=65.5414"),
            (LoadTerm::Short, 1.195_794_938, "fb=98.3121"),
        ] {
            let results = check(model, &solved, term, Some(&terms));
            check_bending(at(&results, xi), expected, fb);
            let cuts: Vec<_> = results
                .iter()
                .filter_map(|(pos, row)| match row {
                    CheckOutcome::Checked(row) if (pos - xi).abs() < 1e-12 => {
                        Some(row.detail.as_str())
                    }
                    _ => None,
                })
                .collect();
            assert!(
                cuts.iter().any(|detail| detail.contains("集中荷重の左側")),
                "{results:?}"
            );
            assert!(
                cuts.iter().any(|detail| detail.contains("集中荷重の右側")),
                "{results:?}"
            );
        }
    }
}

#[test]
fn split_girder_keeps_zero_and_positive_partial_bracing_in_separate_positions() {
    let mut model = split_cantilever(input::cantilever(12_500.0, 25e6, None), SPLIT_MM);
    for attr in &mut model.steel_design_attrs {
        attr.lb_direct = Some((0.0, 4_000.0, 4_000.0));
    }
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    let results = check(&model, &solved, LoadTerm::Short, None);
    let start = at(&results, 0.0);
    check_bending(start, 0.277_945_366, "fb=235.0000");
    check_bending(start, 0.277_945_366, "lb=0 mm");
    for target in [0.5, 1.0] {
        assert!(results.iter().any(|(xi, row)| *xi == target
            && matches!(row, CheckOutcome::Skipped { reason } if reason.contains("横座屈"))));
        assert!(!results
            .iter()
            .any(|(xi, row)| *xi == target && matches!(row, CheckOutcome::Checked(_))));
    }
}

#[test]
fn later_element_verifies_saved_cut_before_rounding_into_global_coordinates() {
    let split_mm = 4_096.4;
    let local_load_mm = 1_951.8;
    let load_mm = split_mm + local_load_mm;
    let whole = input::cantilever(
        60_000.0,
        0.0,
        Some(MemberLoadKind::Point {
            a: load_mm,
            p: -80_000.0,
        }),
    );
    let mut split = split_cantilever(input::cantilever(60_000.0, 0.0, None), split_mm);
    split.load_cases[0].member.push(MemberLoad::manual(
        SECOND_BEAM,
        [0.0, 0.0, 1.0],
        MemberLoadKind::Point {
            a: local_load_mm,
            p: -80_000.0,
        },
    ));
    let solve = |model: &Model| {
        Analysis::prepare(model)
            .unwrap()
            .linear_static(input::DEAD)
            .unwrap()
    };
    let whole_response = solve(&whole);
    let split_response = solve(&split);
    near(split_response.member_forces[1].1.at[1].1[1], -60_000.0);
    let terms = [(input::DEAD, 1.0)];
    for (term, ratio, fb) in [
        (LoadTerm::Long, 1.556_098_048, "fb=65.5414"),
        (LoadTerm::Short, 1.037_398_699, "fb=98.3121"),
    ] {
        for (model, response, id, xi) in [
            (
                &whole,
                &whole_response,
                input::BEAM,
                load_mm / input::LENGTH_MM,
            ),
            (&split, &split_response, SECOND_BEAM, 0.5),
        ] {
            let results = check_member(model, response, term, Some(&terms), id);
            let peak = at(&results, xi);
            assert!(!peak.ok());
            check_bending(peak, ratio, fb);
        }
    }
    let mut inconsistent = split_response;
    inconsistent.member_forces[1].1.at[1].1[1] = 20_000.0;
    let later = check_member(
        &split,
        &inconsistent,
        LoadTerm::Short,
        Some(&terms),
        SECOND_BEAM,
    );
    assert!(later
        .iter()
        .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
        if reason.contains("釣合いを確認できません"))));
}

#[test]
fn invalid_group_and_local_lengths_do_not_confirm_brace_positions() {
    let mut model = split_cantilever(input::cantilever(12_500.0, 25e6, None), SPLIT_MM);
    let solved = Analysis::prepare(&model)
        .unwrap()
        .linear_static(input::DEAD)
        .unwrap();
    model.girder_groups[0].reverse();
    for attr in &mut model.steel_design_attrs {
        attr.lb_direct = Some((2_000.0, 2_000.0, 2_000.0));
    }
    let independent = check(&model, &solved, LoadTerm::Short, None);
    assert!(
        independent
            .iter()
            .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
        if reason.contains("補剛端の位置を確認できません"))),
        "{independent:?}"
    );
    assert!(independent
        .iter()
        .all(|(_, row)| matches!(row, CheckOutcome::Skipped { .. })));

    for (direct, count) in [
        (None, None),
        (None, Some(1)),
        (Some((SPLIT_MM, SPLIT_MM, SPLIT_MM)), None),
    ] {
        for attr in &mut model.steel_design_attrs {
            attr.lb_direct = direct;
            attr.lateral_brace_count = count;
        }
        let unverified = check(&model, &solved, LoadTerm::Short, None);
        assert!(unverified.iter().any(|(_, row)| matches!(row,
            CheckOutcome::Skipped { reason } if reason.contains("連続していません"))));
        assert!(unverified
            .iter()
            .all(|(_, row)| !matches!(row, CheckOutcome::Checked(known)
            if known.components.iter().any(|c| c.kind == CheckKind::Bending))));
    }
}
