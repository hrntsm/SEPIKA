#[path = "fixtures/steel_h_column_combined_model.rs"]
mod combined_column;

#[path = "fixtures/steel_beam_span_model.rs"]
mod input;

use sepika_core::ids::LoadCaseId;
use sepika_core::model::{MemberLoad, MemberLoadKind, Model};
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, CheckResult, LoadTerm,
    MemberDesignCheckOptions,
};
use sepika_solver::statics::{analysis::Analysis, linear::StaticOnce};

fn check(
    model: &Model,
    solved: &StaticOnce,
    term: LoadTerm,
    terms: Option<&[(LoadCaseId, f64)]>,
) -> Vec<(f64, CheckOutcome)> {
    run_member_design_checks(
        model,
        &solved.member_forces,
        &solved.panel_moments,
        &MemberDesignCheckOptions {
            term,
            steel_beam_span_load_terms: terms,
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
    .into_iter()
    .filter(|(id, _, _)| *id == input::BEAM)
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
    assert!(check(&partial, &solved, LoadTerm::Short, None).is_empty());
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
