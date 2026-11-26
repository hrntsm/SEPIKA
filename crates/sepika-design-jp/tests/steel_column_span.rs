#[path = "fixtures/steel_h_column_combined_model.rs"]
mod combined_column;
#[path = "fixtures/steel_column_span_model.rs"]
mod input;
#[path = "fixtures/steel_column_lateral_buckling_model.rs"]
mod support_column;

use sepika_core::ids::LoadCaseId;
use sepika_core::model::{MemberLoad, MemberLoadKind, Model};
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, CheckResult, LoadTerm,
    MemberDesignCheckOptions,
};
use sepika_element::frame::beam::MemberForces;
use sepika_solver::statics::linear::linear_static_once;

fn rows(
    model: &Model,
    forces: MemberForces,
    terms: Option<&[(LoadCaseId, f64)]>,
) -> Vec<(f64, CheckOutcome)> {
    run_member_design_checks(
        model,
        &[(model.elements[0].id, forces)],
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            steel_beam_span_load_terms: terms,
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
    .into_iter()
    .map(|(_, pos, outcome)| (pos, outcome))
    .collect()
}

fn ratio(result: &CheckResult) -> f64 {
    result
        .components
        .iter()
        .find(|component| component.kind == CheckKind::AxialBending)
        .unwrap()
        .ratio
}

fn checked(rows: &[(f64, CheckOutcome)], xi: f64) -> impl Iterator<Item = &CheckResult> {
    rows.iter().filter_map(move |(pos, outcome)| match outcome {
        CheckOutcome::Checked(result) if (*pos - xi).abs() < 1e-12 => Some(result),
        _ => None,
    })
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn reported_off_center_maximum_uses_both_cuts_and_ignores_a_favorable_direct_c() {
    let terms = [(input::CASE, 1.0)];
    for direct in [None, Some(2.3)] {
        let mut model = input::column(125_000.0, -135_000.0);
        model.steel_design_attrs[0].c_direct = direct;
        let force = linear_static_once(&model, input::CASE)
            .unwrap()
            .member_forces
            .remove(0)
            .1;
        assert!(!force.at.iter().any(|(xi, _)| *xi == 1.0 / 3.0));
        close(force.at[0].1[5] / 1e6, 240.0);
        close(force.at[1].1[5] / 1e6, 187.5);

        let results = rows(&model, force, Some(&terms));
        assert!(
            results
                .iter()
                .all(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(_))),
            "{results:?}"
        );
        let governed = checked(&results, 1.0 / 3.0).collect::<Vec<_>>();
        assert_eq!(governed.len(), 2, "{results:?}");
        for (&result, side) in governed.iter().zip(["左側", "右側"]) {
            assert!(!result.ok(), "{result:?}");
            close(ratio(result), 1.131_706_715_914_949_4);
            for detail in [
                "補剛区間 [0, 3000.000] mm",
                "x=1000.000000 mm",
                "Mz=250.000000 kN·m",
                "区間内部に絶対値最大",
                "C=1.0000",
                "pλb=0.3000",
                side,
            ] {
                assert!(result.detail.contains(detail), "{result:?}");
            }
            assert!(
                result.components[0].detail.contains("fbX=192.3854"),
                "{result:?}"
            );
            if direct.is_some() {
                assert!(result.detail.contains("C直接入力は不使用"));
            }
        }
    }
}

#[test]
fn missing_shear_at_another_cut_does_not_hide_the_reported_strong_bending_ng() {
    let model = input::column(125_000.0, -135_000.0);
    let response = linear_static_once(&model, input::CASE)
        .unwrap()
        .member_forces
        .remove(0)
        .1;
    for (station, missing, expect_bending) in [
        (1, 2, true),
        (0, 2, false),
        (1, 1, false),
        (1, 5, false),
        (0, 1, false),
        (0, 5, false),
    ] {
        let mut response = response.clone();
        response.at[station].1[missing] = f64::NAN;
        let results = rows(&model, response, Some(&[(input::CASE, 1.0)]));
        let governed: Vec<_> = checked(&results, 1.0 / 3.0)
            .filter(|row| {
                row.components
                    .iter()
                    .any(|c| c.kind == CheckKind::AxialBending)
            })
            .collect();
        assert_eq!(
            governed.len(),
            if expect_bending { 2 } else { 0 },
            "{results:?}"
        );
        for (&row, side) in governed.iter().zip(["左側", "右側"]) {
            close(ratio(row), 1.131_706_715_914_949_4);
            assert!(
                row.detail.contains("C=1.0000") && row.detail.contains(side),
                "{row:?}"
            );
        }
        assert!(
            results.iter().any(
                |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
            if reason.contains("保存断面力に非有限な成分"))
            ),
            "{results:?}"
        );
        assert!(
            results
                .iter()
                .all(|(_, outcome)| !matches!(outcome, CheckOutcome::Checked(row) if row.ok())),
            "{results:?}"
        );
    }
}

#[test]
fn compression_disagreement_keeps_only_a_common_same_point_failure() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member.push(MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Point {
            a: 2900.0,
            p: 300_000.0,
        },
    ));
    for (end_nmm, saved_torsion, saved_qz, expect_combined) in [
        (200e6, 0.0, 0.0, true),
        (150e6, 0.0, 0.0, false),
        (200e6, 1e-9, 0.0, false),
        (200e6, 0.0, 1e-9, false),
    ] {
        let mut response = support_column::moments(end_nmm - 30e6);
        response.at[2].1 = [-1e-9, 300_000.0, saved_qz, saved_torsion, 0.0, end_nmm];
        let results = rows(&model, response, Some(&[(input::CASE, 1.0)]));
        let at_j: Vec<_> = checked(&results, 1.0).flat_map(|r| &r.components).collect();
        if expect_combined {
            let [component] = at_j.as_slice() else {
                panic!("{results:?}")
            };
            assert_eq!(component.kind, CheckKind::Shear);
            assert!(component.detail.contains("式(5.24)"));
            close(component.ratio, 1.013_116_271_683_298);
        } else {
            assert!(at_j.is_empty(), "{results:?}");
        }
        assert!(results.iter().any(|(xi, row)| *xi == 1.0 && matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains(if saved_torsion == 0.0 { "圧縮軸力の有無" } else { "材軸ねじりの有無" }))), "{results:?}");
    }
}

#[test]
fn a_web_shear_failure_requires_confirmed_absence_of_torsion_on_the_same_cut() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member = [(1400.0, 500_000.0), (1600.0, -500_000.0)]
        .map(|(a, p)| {
            MemberLoad::manual(
                model.elements[0].id,
                [1.0, 0.0, 0.0],
                MemberLoadKind::Point { a, p },
            )
        })
        .to_vec();
    for (recovered_mx, saved_mx, end_qz, mid_my) in [
        (1e-10, 0.0, 0.0, 0.0),
        (0.0, 1e-10, 0.0, 0.0),
        (0.0, 0.0, 0.0, 0.0),
        (1e-10, 0.0, f64::NAN, 0.0),
        (0.0, 1e-10, f64::NAN, 0.0),
        (0.0, 0.0, f64::NAN, 0.0),
        (1e-10, 0.0, 0.0, f64::NAN),
        (0.0, 0.0, 0.0, f64::NAN),
    ] {
        let response = MemberForces {
            at: vec![
                (0.0, [0.0, 0.0, 0.0, recovered_mx, 0.0, -50e6]),
                (0.5, [0.0, 500_000.0, 0.0, saved_mx, mid_my, 0.0]),
                (1.0, [0.0, 0.0, end_qz, recovered_mx, 0.0, 50e6]),
            ],
        };
        let results = rows(&model, response, Some(&[(input::CASE, 1.0)]));
        let shear = checked(&results, 0.5)
            .flat_map(|r| &r.components)
            .find(|c| c.kind == CheckKind::Shear && c.ratio > 1.0);
        if recovered_mx == saved_mx {
            close(
                shear.unwrap_or_else(|| panic!("{results:?}")).ratio,
                1.231_689_332_950_902_7,
            );
        } else {
            assert!(shear.is_none(), "{results:?}");
            assert!(
                results.iter().any(|(xi, row)| *xi == 0.5
                    && matches!(row, CheckOutcome::Skipped { reason }
                if reason.contains("材軸ねじりの有無"))),
                "{results:?}"
            );
        }
    }
}

#[test]
fn monotonic_distribution_uses_signed_ends_and_the_matching_default_span() {
    let mut model = input::column(80_000.0, 0.0);
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    let force = linear_static_once(&model, input::CASE)
        .unwrap()
        .member_forces
        .remove(0)
        .1;
    let results = rows(&model, force, Some(&[(input::CASE, 1.0)]));
    assert!(
        results
            .iter()
            .all(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(result) if result.ok())),
        "{results:?}"
    );
    let result = checked(&results, 0.0).next().unwrap();
    close(ratio(result), 0.933_797_967_433_463_4);
    for detail in [
        "節点間距離の既定値",
        "符号付き曲げが単調",
        "C=1.7500",
        "pλb=0.6000",
    ] {
        assert!(result.detail.contains(detail), "{result:?}");
    }
    assert!(
        result.components[0].detail.contains("fbX=223.8331"),
        "{result:?}"
    );
}

#[test]
fn missing_provenance_keeps_only_independent_ng_at_a_saved_section() {
    let mut model = input::column(125_000.0, -135_000.0);
    model.load_cases[0].nodal[0].values[2] = -1_500_000.0;
    let mut force = linear_static_once(&model, input::CASE)
        .unwrap()
        .member_forces
        .remove(0)
        .1;
    force
        .at
        .insert(1, (0.1, [-1_500_000.0, 10_000.0, 0.0, 0.0, 0.0, 243e6]));
    let results = rows(&model, force.clone(), None);
    let saved = checked(&results, 0.1).next().unwrap();
    assert!(ratio(saved) > 1.0, "{results:?}");
    assert!(
        saved.components[0].detail.contains("軸曲げ比の下限"),
        "{results:?}"
    );
    assert!(
        results.iter().any(|(_, outcome)| matches!(outcome,
        CheckOutcome::Skipped { reason } if reason.contains("荷重組合せの由来が未確認"))),
        "{results:?}"
    );
    assert!(
        !results.iter().any(|(_, outcome)| matches!(outcome,
        CheckOutcome::Checked(result) if result.ok())),
        "{results:?}"
    );

    let mismatch = rows(&model, force, Some(&[(input::CASE, 2.0)]));
    assert!(
        mismatch.iter().all(|(_, outcome)| matches!(outcome,
        CheckOutcome::Skipped { reason } if reason.contains("釣合いを確認できません"))),
        "{mismatch:?}"
    );
}

#[test]
fn a_duplicate_middle_cut_does_not_erase_independent_end_failures() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].nodal[0].values[4] = 300e6;
    let terms = [(input::CASE, 1.0)];
    let mut saved = support_column::moments(300e6);
    saved.at.insert(1, saved.at[1]);

    let result = rows(&model, saved.clone(), Some(&terms));
    assert!(
        result
            .iter()
            .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
        if reason.contains("重複"))),
        "{result:?}"
    );
    assert!(checked(&result, 0.5).next().is_none(), "{result:?}");
    for xi in [0.0, 1.0] {
        let failures: Vec<_> = checked(&result, xi).collect();
        let [failure] = failures.as_slice() else {
            panic!("{result:?}")
        };
        close(ratio(failure), 1.111_781_465_373_194);
        assert!(failure.components[0].detail.contains("軸曲げ比の下限"));
    }

    let mut mismatch_at_end = saved.clone();
    mismatch_at_end.at[3].1[5] += 20e6;
    let result = rows(&model, mismatch_at_end, Some(&terms));
    assert!(
        result
            .iter()
            .all(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
        if reason.contains("釣合いを確認できません"))),
        "{result:?}"
    );

    saved.at[2].1[5] = 250e6;
    let result = rows(&model, saved.clone(), Some(&terms));
    assert!(
        result
            .iter()
            .all(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
        if reason.contains("釣合いを確認できません"))),
        "{result:?}"
    );

    saved.at[2].0 = f64::NAN;
    let result = rows(&model, saved, Some(&terms));
    assert!(
        result
            .iter()
            .all(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
        if reason.contains("評価位置が非有限"))),
        "{result:?}"
    );
}

#[test]
fn a_nonfinite_middle_component_does_not_hide_verified_end_failures() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].nodal[0].values[2] = -2e6;
    model.load_cases[0].nodal[0].values[4] = 20e6;
    let terms = [(input::CASE, 1.0)];
    let mut force = support_column::moments(20e6);
    for (_, force) in &mut force.at {
        force[0] = -2e6;
    }
    force.at[1].1[2] = f64::NAN;
    let results = rows(&model, force.clone(), Some(&terms));
    for xi in [0.0, 0.5, 1.0] {
        assert!(
            checked(&results, xi).any(|result| !result.ok()),
            "{results:?}"
        );
    }
    assert!(
        results.iter().any(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("保存断面力に非有限な成分がある"))
        ),
        "{results:?}"
    );
    assert!(
        !results
            .iter()
            .any(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(result) if result.ok())),
        "{results:?}"
    );

    force.at[1].1[5] = 200e6;
    let mismatch = rows(&model, force, Some(&terms));
    assert!(
        mismatch.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("釣合いを確認できません"))
        ),
        "{mismatch:?}"
    );
}

#[test]
fn a_missing_weak_shear_series_does_not_hide_confirmed_compression_slenderness() {
    let mut model = input::column(0.0, 0.0);
    model.steel_design_attrs[0].lk_z_direct = Some(10_000.0);
    let terms = [(input::CASE, 1.0)];
    let mut force = support_column::moments(20e6);
    for (_, force) in &mut force.at {
        force[0] = -1000.0;
        force[2] = f64::NAN;
        force[4] = 1e6;
    }
    let results = rows(&model, force.clone(), Some(&terms));
    for xi in [0.0, 0.5, 1.0] {
        let failure = checked(&results, xi)
            .next()
            .unwrap_or_else(|| panic!("{results:?}"));
        let [component] = failure.components.as_slice() else {
            panic!("{failure:?}")
        };
        assert_eq!(component.kind, CheckKind::Provision);
        close(component.ratio, 1.0864859499895392);
    }
    assert!(
        results.iter().any(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("保存断面力に非有限な成分がある"))
        ),
        "{results:?}"
    );

    model.steel_design_attrs[0].lk_z_direct = Some(3500.0);
    let mut compressed = force.clone();
    for (_, cut) in &mut compressed.at {
        cut[0] = -1_500_000.0;
    }
    let results = rows(&model, compressed, Some(&terms));
    let failure = checked(&results, 0.0)
        .next()
        .unwrap_or_else(|| panic!("{results:?}"));
    let [component] = failure.components.as_slice() else {
        panic!("{failure:?}")
    };
    assert_eq!(component.kind, CheckKind::Axial);
    assert!(
        component.detail.contains("fc=") || component.detail.contains("許容圧縮"),
        "{component:?}"
    );
    assert!(component.ratio > 1.0, "{component:?}");

    force.at[1].1[0] = -2000.0;
    let mismatch = rows(&model, force, Some(&terms));
    assert!(
        mismatch.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("釣合いを確認できません"))
        ),
        "{mismatch:?}"
    );
}

#[test]
fn a_zero_crossing_from_a_saved_cut_checks_pure_compression_inside_the_span() {
    use sepika_core::section_shape::SectionShape;

    let mut model = input::column(24_000.0, 0.0);
    let sec = &mut model.sections[0];
    let mut thinner_web = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 6.0,
        flange_thick: 13.0,
        root_r: Some(0.0),
    }
    .to_section(sec.id, "薄いウェブのH".into());
    thinner_web.material = sec.material;
    thinner_web.frame_use = sec.frame_use;
    *sec = thinner_web;
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    for (saved_j_nmm, expect_pass) in [(-0.25, false), (0.25, true)] {
        let forces = MemberForces {
            at: [
                (0.0, 72_000_000.25),
                (0.5, 36_000_000.25),
                (1.0, saved_j_nmm),
            ]
            .map(|(xi, mz)| (xi, [-77_000.0, -24_000.0, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        assert!(!results.is_empty());
        if expect_pass {
            assert!(
                results
                    .iter()
                    .all(|(_, row)| matches!(row, CheckOutcome::Checked(result) if result.ok())),
                "{results:?}"
            );
        } else {
            assert!(
                results
                    .iter()
                    .all(|(_, row)| !matches!(row, CheckOutcome::Checked(result) if result.ok())),
                "{results:?}"
            );
            assert!(
                results
                    .iter()
                    .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
                if reason.contains("強軸曲げがゼロになる断面") && reason.contains("d/t=62.3333"))),
                "{results:?}"
            );
        }
    }
}

#[test]
fn a_recovered_roundoff_moment_cannot_hide_pure_compression_at_the_saved_j_end() {
    use sepika_core::section_shape::SectionShape;

    let mut model = input::column(3_333.333_333_333_333, 0.0);
    let sec = &mut model.sections[0];
    let mut thinner_web = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 6.0,
        flange_thick: 13.0,
        root_r: Some(0.0),
    }
    .to_section(sec.id, "薄いウェブのH".into());
    thinner_web.material = sec.material;
    thinner_web.frame_use = sec.frame_use;
    *sec = thinner_web;
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    model.load_cases[0].nodal[0].values[2] = -77_000.0;
    let forces = MemberForces {
        at: [(0.0, 10e6), (0.5, 5e6), (1.0, 0.0)]
            .map(|(xi, mz)| (xi, [-77_000.0, -3_333.333_333_333_333, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
    };
    let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
    assert!(
        checked(&results, 0.0).any(|result| result.ok()),
        "{results:?}"
    );
    assert!(
        checked(&results, 1.0).all(|result| !result.ok()),
        "{results:?}"
    );
    assert!(results.iter().any(|(xi, outcome)| *xi == 1.0 && matches!(outcome,
        CheckOutcome::Skipped { reason } if reason.contains("保存断面力と回復断面力で局部座屈の適用条件が一致しません")
        && reason.contains("x=3000.000000 mm") && reason.contains("d/t=62.3333"))), "{results:?}");
}

#[test]
fn a_named_h_without_confirmed_shape_does_not_assume_plate_behavior_across_a_zero_crossing() {
    let mut model = input::column(80_000.0, 0.0);
    let sec = &mut model.sections[0];
    sec.shape = None;
    sec.thickness = Some(13.0);
    sec.property_basis = Default::default();
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    let forces = MemberForces {
        at: [(0.0, 100e6), (0.5, -20e6), (1.0, -140e6)]
            .map(|(xi, mz)| (xi, [-100_000.0, -80_000.0, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
    };
    let terms = [(input::CASE, 1.0)];
    let results = rows(&model, forces.clone(), Some(&terms));
    assert!(
        results.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("実断面形状を確認できない"))
        ),
        "{results:?}"
    );

    model.steel_design_attrs[0].lk_z_direct = Some(10_000.0);
    let results = rows(&model, forces, Some(&terms));
    assert!(
        checked(&results, 0.0).any(|result| result
            .components
            .iter()
            .any(|component| component.kind == CheckKind::Provision && component.ratio > 1.0)),
        "{results:?}"
    );
    assert!(
        results.iter().any(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("実断面形状を確認できない"))
        ),
        "{results:?}"
    );
}

#[test]
fn a_saved_left_cut_cannot_hide_reversal_after_an_interior_point_load() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member = [
        MemberLoadKind::Point {
            a: 1500.0,
            p: -160_000.0,
        },
        MemberLoadKind::Distributed {
            a: 1500.0,
            b: 3000.0,
            w1: 1e-6,
            w2: 1e-6,
        },
    ]
    .map(|kind| MemberLoad::manual(model.elements[0].id, [1.0, 0.0, 0.0], kind))
    .to_vec();
    let terms = [(input::CASE, 1.0)];
    for delta_n in [-0.001, 0.0, 0.001] {
        let j_mz_nmm = 240_000_001.125 + 1500.0 * delta_n;
        let forces = MemberForces {
            at: [
                (0.0, 160_000.0, 0.0),
                (0.5, 160_000.0 + delta_n, 240e6),
                (1.0, 0.0015 + delta_n, j_mz_nmm),
            ]
            .map(|(xi, qy, mz)| (xi, [0.0, qy, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let result = rows(&model, forces, Some(&terms));
        assert!(!result.is_empty());
        if delta_n < 0.0 {
            assert!(
                result
                    .iter()
                    .all(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
                if reason.contains("符号付き強軸曲げの単調変化"))),
                "{result:?}"
            );
        } else {
            assert!(
                result
                    .iter()
                    .all(|(_, row)| matches!(row, CheckOutcome::Checked(check)
                if check.ok() && check.detail.contains("C=1.7500"))),
                "{result:?}"
            );
            let at_load: Vec<_> = checked(&result, 0.5).collect();
            assert_eq!(at_load.len(), 2, "{result:?}");
            for row in at_load {
                close(ratio(row), 0.933_797_967_433_463_4);
            }
        }
    }
}

#[test]
fn a_saved_moment_order_conflicting_with_recovery_prevents_monotonic_pass() {
    let terms = [(input::CASE, 1.0)];
    for (point, delta_nmm, expected_monotonic) in [
        (true, 1.0, false),
        (true, 0.0, true),
        (false, -0.65, false),
        (false, 0.0, true),
    ] {
        let mut model = input::column(0.0, 0.0);
        let (qy_i, qy_j, mi, mid, mj) = if point {
            let after_n = 1.0 / 16_384.0;
            model.load_cases[0].member.push(MemberLoad::manual(
                model.elements[0].id,
                [1.0, 0.0, 0.0],
                MemberLoadKind::Point {
                    a: 1500.0,
                    p: -160_000.0 + after_n,
                },
            ));
            (160_000.0, after_n, 0.0, 240e6, 240e6 + after_n * 1500.0)
        } else {
            (0.0001, 0.0001, 200e6, 200e6 + 0.15, 200e6 + 0.25)
        };
        for sign in [1.0, -1.0] {
            let forces = MemberForces {
                at: [
                    (0.0, qy_i, mi),
                    (0.5, qy_i, mid + delta_nmm),
                    (1.0, qy_j, mj),
                ]
                .map(|(xi, qy, mz)| (xi, [0.0, sign * qy, 0.0, 0.0, 0.0, sign * mz]))
                .to_vec(),
            };
            let signed_terms = terms.map(|(case, _)| (case, sign));
            let result = rows(&model, forces, Some(&signed_terms));
            assert!(!result.is_empty());
            if expected_monotonic {
                assert!(
                    result
                        .iter()
                        .all(|(_, row)| matches!(row, CheckOutcome::Checked(check)
                    if check.ok() && check.detail.contains("符号付き曲げが単調"))),
                    "{result:?}"
                );
            } else {
                assert!(
                    result
                        .iter()
                        .all(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
                    if reason.contains("符号付き強軸曲げの単調変化"))),
                    "{result:?}"
                );
            }
        }
    }
}

#[test]
fn weak_shear_disagreement_does_not_hide_confirmed_axial_bending_ng() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member.push(MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: -100.0,
            w2: -100.0,
        },
    ));
    let terms = [(input::CASE, 1.0)];
    for (mz_nmm, qz_n, axial_bending_ng) in [
        (250e6, 1e-9, true),
        (180e6, 1e-9, false),
        (250e6, 0.0, true),
    ] {
        let end_nmm = mz_nmm - 112.5e6;
        let forces = MemberForces {
            at: [
                (0.0, 150_000.0, 0.0, end_nmm),
                (0.5, 0.0, qz_n, mz_nmm),
                (1.0, -150_000.0, 0.0, end_nmm),
            ]
            .map(|(xi, qy, qz, mz)| (xi, [0.0, qy, qz, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let result = rows(&model, forces, Some(&terms));
        let at_peak: Vec<_> = checked(&result, 0.5).collect();
        assert_eq!(at_peak.len(), usize::from(axial_bending_ng), "{result:?}");
        if let [row] = at_peak.as_slice() {
            close(ratio(row), 1.131_706_715_914_949);
            if qz_n != 0.0 {
                assert!(row
                    .components
                    .iter()
                    .all(|c| !c.detail.contains("式(5.24)")));
            }
        }
        assert_eq!(
            result.iter().any(|(xi, row)| *xi == 0.5
                && matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains("弱軸せん断の有無"))),
            qz_n != 0.0,
            "{result:?}"
        );
    }
}

#[test]
fn saved_shear_offsets_are_checked_at_unsampled_load_boundaries() {
    let terms = [(input::CASE, 1.0)];
    for (point_mm, before_jump, sign) in [
        (1500.0, false, 1.0),
        (2000.0, false, 1.0),
        (2000.0, false, -1.0),
        (2000.0, true, 1.0),
    ] {
        let w_n_per_mm = if before_jump { 1.0 / 1_048_576.0 } else { 1e-6 };
        let i_qy_n = if before_jump {
            w_n_per_mm * point_mm
        } else {
            120_000.0
        };
        let mut model = input::column(0.0, 0.0);
        let (jump_n, a, b, w) = if before_jump {
            (120_000.0, 0.0, point_mm, -w_n_per_mm)
        } else {
            (-120_000.0, point_mm, 3000.0, w_n_per_mm)
        };
        model.load_cases[0].member = [
            MemberLoadKind::Point {
                a: point_mm,
                p: jump_n,
            },
            MemberLoadKind::Distributed { a, b, w1: w, w2: w },
        ]
        .map(|kind| MemberLoad::manual(model.elements[0].id, [sign, 0.0, 0.0], kind))
        .to_vec();
        let (mid_qy_n, mid_mz_nmm, j_qy_n, j_mz_nmm) = if before_jump {
            (
                w_n_per_mm * 500.0,
                w_n_per_mm * 1500.0 * 1250.0,
                120_000.0,
                w_n_per_mm * 2e6 + 120e6,
            )
        } else {
            let length_mm = 3000.0 - point_mm;
            (
                120_000.0,
                180e6,
                w_n_per_mm * length_mm,
                120_000.0 * point_mm + w_n_per_mm * length_mm * length_mm / 2.0,
            )
        };
        let delta_n = if before_jump {
            0.00025
        } else if point_mm == 1500.0 {
            0.0005
        } else {
            0.00075
        };
        for offset_n in [-delta_n, 0.0, delta_n] {
            let forces = MemberForces {
                at: [
                    (0.0, i_qy_n, 0.0),
                    (0.5, mid_qy_n + offset_n, mid_mz_nmm + 1500.0 * offset_n),
                    (1.0, j_qy_n + offset_n, j_mz_nmm + 3000.0 * offset_n),
                ]
                .map(|(xi, qy, mz)| (xi, [0.0, sign * qy, 0.0, 0.0, 0.0, sign * mz]))
                .to_vec(),
            };
            let result = rows(&model, forces, Some(&terms));
            assert!(!result.is_empty());
            if offset_n < 0.0 {
                assert!(
                    result
                        .iter()
                        .all(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
                    if reason.contains("符号付き強軸曲げの単調変化"))),
                    "{result:?}"
                );
            } else {
                assert!(
                    result
                        .iter()
                        .all(|(_, row)| matches!(row, CheckOutcome::Checked(check)
                    if check.ok() && check.detail.contains("C=1.7500"))),
                    "{result:?}"
                );
            }
        }
    }
}

#[test]
fn verified_saved_point_load_right_cut_keeps_its_unrounded_shear_failure() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member = [(1500.0, 305_946.521_022_61), (1500.01, -405_946.521_022_61)]
        .map(|(a, p)| {
            MemberLoad::manual(
                model.elements[0].id,
                [1.0, 0.0, 0.0],
                MemberLoadKind::Point { a, p },
            )
        })
        .to_vec();
    let terms = [(input::CASE, 1.0)];
    for (saved_xi, offset_n, ng) in [
        (0.5, 0.0005, true),
        (0.5, 0.0, false),
        (0.25, 0.0005, false),
    ] {
        let forces = MemberForces {
            at: [
                (0.0, 100_000.0, -150e6),
                (saved_xi, 100_000.0 + offset_n, -150e6 + 300e6 * saved_xi),
                (1.0, 0.0, 4_059.465_21),
            ]
            .map(|(xi, qy, mz)| (xi, [0.0, qy, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let result = rows(&model, forces, Some(&terms));
        let right = checked(&result, 0.5)
            .find(|row| row.detail.contains("集中荷重の右側"))
            .unwrap_or_else(|| panic!("{result:?}"));
        let shear = right
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Shear)
            .unwrap();
        assert_eq!(shear.ratio > 1.0, ng, "{right:?}");
        close(
            shear.ratio,
            if ng {
                1.000_000_000_62
            } else {
                0.999_999_999_38
            },
        );
        assert_eq!(right.detail.contains("保存断面力による照査"), ng);
        let nearby: Vec<_> = checked(&result, 1500.01 / 3000.0).collect();
        assert_eq!(nearby.len(), 2, "{result:?}");
        for row in nearby {
            assert!(
                row.ok() && !row.detail.contains("保存断面力による照査"),
                "{row:?}"
            );
        }
    }
}

#[test]
fn an_unrepresentable_saved_right_cut_does_not_erase_recovered_ng() {
    let mut model = input::column(0.0, 0.0);
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    model.nodes[1].coord[2] = 1.0;
    let qy_n = f64::MAX / 2.0;
    let point_n = qy_n * (1.0 - 2e-10);
    for (a, p) in [(0.5, point_n), (0.75, -point_n)] {
        model.load_cases[0].member.push(MemberLoad::manual(
            model.elements[0].id,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Point { a, p },
        ));
    }
    let forces = MemberForces {
        at: [
            (0.0, qy_n, -qy_n / 2.0),
            (0.5, qy_n * (1.0 + 4e-10), 0.0),
            (1.0, qy_n, qy_n / 2.0 + point_n / 4.0),
        ]
        .map(|(xi, qy, mz)| (xi, [0.0, qy, 0.0, 0.0, 0.0, mz]))
        .to_vec(),
    };
    let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
    assert!(
        result.iter().any(|(xi, row)| *xi == 0.5
            && matches!(row,
        CheckOutcome::Skipped { reason } if reason.contains("集中荷重の右側")
            && reason.contains("有限な値で確認できません"))),
        "{result:?}"
    );
    let right = checked(&result, 0.5)
        .find(|row| row.detail.contains("集中荷重の右側"))
        .unwrap_or_else(|| panic!("{result:?}"));
    let [shear] = right.components.as_slice() else {
        panic!("{right:?}")
    };
    assert_eq!(shear.kind, CheckKind::Shear);
    assert!(shear.ratio.is_finite() && shear.ratio > 1.0, "{right:?}");
    assert!(!right.detail.contains("保存断面力による照査"));
}

#[test]
fn missing_torsional_constant_keeps_both_material_bending_and_web_shear_ng() {
    let mut model = input::column(-500_000.0, 0.0);
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
    model.sections[0].j = 0.0;
    for (j_mz_nmm, expected_bending, expected_combination) in [
        (1500e6, Some(5.558_907_326_865_969), 5.341_524_063_760_777),
        (100e6, None, 1.279_501_654_055_574_8),
    ] {
        model.load_cases[0].nodal[0].values[4] = j_mz_nmm;
        let forces = MemberForces {
            at: [
                (0.0, j_mz_nmm - 1500e6),
                (0.5, j_mz_nmm - 750e6),
                (1.0, j_mz_nmm),
            ]
            .map(|(xi, mz)| (xi, [0.0, 500_000.0, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let failures: Vec<_> = checked(&result, 1.0)
            .flat_map(|row| &row.components)
            .collect();
        assert_eq!(
            failures.len(),
            2 + usize::from(expected_bending.is_some()),
            "{result:?}"
        );
        close(
            failures
                .iter()
                .find(|c| c.kind == CheckKind::Shear && !c.detail.contains("式(5.24)"))
                .unwrap()
                .ratio,
            1.231_689_332_950_902_7,
        );
        close(
            failures
                .iter()
                .find(|c| c.kind == CheckKind::Shear && c.detail.contains("式(5.24)"))
                .unwrap()
                .ratio,
            expected_combination,
        );
        if let Some(expected) = expected_bending {
            close(
                failures
                    .iter()
                    .find(|c| c.kind == CheckKind::Bending)
                    .unwrap()
                    .ratio,
                expected,
            );
        }
        assert!(
            result.iter().any(|(xi, row)| *xi == 1.0
                && matches!(row,
                CheckOutcome::Skipped { reason } if reason.contains("横座屈に必要な断面性能"))),
            "{result:?}"
        );
    }
}

#[test]
fn missing_torsional_constant_does_not_hide_a_verified_same_point_shear_ng() {
    let mut model = input::column(0.0, 0.0);
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
    model.sections[0].j = 0.0;
    let terms = [(input::CASE, 1.0)];
    for (mz_nmm, qy_n, qz_n, mx_nmm, axial_n, expected) in [
        (200e6, 300_000.0, 0.0, 0.0, 0.0, Some(1.013_116_271_683_298)),
        (196e6, 320_000.0, 0.0, 0.0, 0.0, Some(1.040_496_157_306_278)),
        (150e6, 300_000.0, 0.0, 0.0, 0.0, None),
        (200e6, 300_000.0, 1.0, 0.0, 0.0, None),
        (200e6, 300_000.0, 0.0, 1.0, 0.0, None),
        (200e6, 300_000.0, 0.0, 0.0, f64::NAN, None),
    ] {
        let forces = MemberForces {
            at: [0.0, 0.5, 1.0]
                .map(|xi| {
                    let x_mm = 3000.0 * xi;
                    let n = if xi == 1.0 { axial_n } else { 0.0 };
                    (
                        xi,
                        [
                            n,
                            qy_n,
                            qz_n,
                            mx_nmm,
                            -qz_n * x_mm,
                            mz_nmm - qy_n * (3000.0 - x_mm),
                        ],
                    )
                })
                .to_vec(),
        };
        let result = rows(&model, forces, Some(&terms));
        let failures: Vec<_> = checked(&result, 1.0).flat_map(|r| &r.components).collect();
        if let Some(expected) = expected {
            let [combined] = failures.as_slice() else {
                panic!("{result:?}")
            };
            assert_eq!(combined.kind, CheckKind::Shear);
            assert!(combined.detail.contains("式(5.24)"));
            close(combined.ratio, expected);
            assert!(result.iter().any(|(xi, outcome)| *xi == 1.0
                && matches!(outcome,
                CheckOutcome::Skipped { reason } if reason.contains("横座屈に必要な断面性能"))));
        } else {
            assert!(failures.is_empty(), "{result:?}");
            assert!(
                result
                    .iter()
                    .any(|(xi, outcome)| *xi == 1.0
                        && matches!(outcome, CheckOutcome::Skipped { .. }))
            );
        }
    }
}

#[test]
fn the_confirmed_zero_shear_cut_checks_both_moments_before_rounding() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member.push(MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: -25.0,
            w2: -25.0,
        },
    ));
    // 式(5.24)のτ=0で、内法端（187 mm）の曲げ垂直応力度が235 N/mm²となる点。
    let limit_nmm = 288_595_938.110_516_9;
    for offset_nmm in [-1.0, 1.0] {
        let end_nmm = limit_nmm - 28.125e6 + offset_nmm;
        let forces = MemberForces {
            at: [
                (0.0, 37_500.0, end_nmm),
                (0.5, 0.0, limit_nmm - offset_nmm),
                (1.0, -37_500.0, end_nmm),
            ]
            .map(|(xi, qy, mz)| (xi, [0.0, qy, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let at_center: Vec<_> = checked(&result, 0.5).collect();
        let [row] = at_center.as_slice() else {
            panic!("{result:?}")
        };
        let combined = row
            .components
            .iter()
            .find(|c| c.detail.contains("式(5.24)"))
            .unwrap();
        assert!(
            combined.ratio > 1.0 && combined.ratio < 1.000_000_01,
            "{row:?}"
        );
        assert_eq!(
            row.detail.contains("保存断面力による照査"),
            offset_nmm < 0.0
        );
    }
}

#[test]
fn missing_strong_shear_keeps_a_verified_tension_edge_failure() {
    let model = input::column(0.0, 0.0);
    for (n, my, mx, saved_mx, missing_at, expect_ng) in [
        (650_000.0, 0.0, 0.0, 0.0, 2, true),
        (650_000.0, 0.0, 0.0, 0.0, 0, true),
        (450_000.0, 0.0, 0.0, 0.0, 2, false),
        (650_000.0, 1.0, 0.0, 0.0, 2, false),
        (650_000.0, 0.0, 1.0, 1.0, 2, false),
        (650_000.0, 0.0, 1e-10, 0.0, 2, false),
    ] {
        let mut forces = support_column::moments(200e6);
        for (_, f) in &mut forces.at {
            f[0] = n;
            f[3] = mx;
            f[4] = my;
        }
        forces.at[2].1[3] = saved_mx;
        forces.at[missing_at].1[1] = f64::NAN;
        let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let tension = checked(&results, 1.0)
            .flat_map(|row| &row.components)
            .find(|c| c.kind == CheckKind::AxialBending && c.detail.contains("式(6.3)"));
        assert_eq!(tension.is_some(), expect_ng, "{results:?}");
        if let Some(tension) = tension {
            close(tension.ratio, 1.078_828_933_475_746);
        }
        assert!(
            results.iter().any(|(xi, row)| *xi == 1.0
                && matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains("保存断面力に非有限な成分"))),
            "{results:?}"
        );
        assert!(
            results
                .iter()
                .all(|(_, row)| !matches!(row, CheckOutcome::Checked(result) if result.ok())),
            "{results:?}"
        );
    }
}

#[test]
fn missing_torsional_constant_keeps_the_independent_tension_edge_ng() {
    let mut model = input::column(0.0, 0.0);
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
    model.sections[0].j = 0.0;
    for (n, my, mx, expected) in [
        (650_000.0, 0.0, 0.0, true),
        (450_000.0, 0.0, 0.0, false),
        (650_000.0, 1.0, 0.0, false),
        (650_000.0, 0.0, 1.0, false),
    ] {
        let mut forces = support_column::moments(200e6);
        for (_, f) in &mut forces.at {
            f[0] = n;
            f[3] = mx;
            f[4] = my;
        }
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let at_j: Vec<_> = checked(&result, 1.0)
            .flat_map(|row| &row.components)
            .filter(|component| component.detail.contains("式(6.3)"))
            .collect();
        assert_eq!(at_j.len(), usize::from(expected), "{result:?}");
        if let [edge] = at_j.as_slice() {
            assert!(edge.detail.contains("式(6.3)"));
            close(edge.ratio, 1.078_828_933_475_746);
        }
        assert!(result
            .iter()
            .any(|(xi, row)| *xi == 1.0 && matches!(row, CheckOutcome::Skipped { .. })));
    }
}

#[test]
fn a_zero_saved_end_shear_uses_the_same_point_limit_on_its_own_cut() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member.push(MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: -25.0,
            w2: -25.0,
        },
    ));
    let limit_nmm = 288_595_938.110_516_9;
    let q_j_n = 1.0 / 4096.0;
    for offset in [-1.0, 1.0] {
        let end_nmm = limit_nmm + offset;
        let forces = MemberForces {
            at: [
                (0.0, 75_000.0 + q_j_n, end_nmm - 112.5e6 - 3000.0 * q_j_n),
                (0.5, 37_500.0 + q_j_n, end_nmm - 28.125e6 - 1500.0 * q_j_n),
                (1.0, 0.0, limit_nmm - offset),
            ]
            .map(|(xi, qy, mz)| (xi, [0.0, qy, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let is_saved_branch = offset < 0.0;
        let at_j = checked(&result, 1.0)
            .find(|r| {
                r.detail.contains("保存断面力による照査") == is_saved_branch
                    && r.components
                        .iter()
                        .any(|c| c.ratio > 1.0 && c.detail.contains("式(5.24)"))
            })
            .unwrap_or_else(|| panic!("{result:?}"));
        let combined = at_j
            .components
            .iter()
            .find(|c| c.detail.contains("式(5.24)"))
            .unwrap();
        close(combined.ratio, 1.000_000_003_465_052);
    }
}

#[test]
fn the_zero_shear_cut_keeps_its_local_ng_with_axial_variation_elsewhere() {
    let mut model = input::column(0.0, 0.0);
    let elem = model.elements[0].id;
    model.load_cases[0].member = [
        ([1.0, 0.0, 0.0], 0.0, -25.0),
        ([0.0, 0.0, 1.0], 2000.0, 25.0),
    ]
    .map(|(dir, a, w)| {
        MemberLoad::manual(
            elem,
            dir,
            MemberLoadKind::Distributed {
                a,
                b: 3000.0,
                w1: w,
                w2: w,
            },
        )
    })
    .to_vec();
    for (mid_nmm, expected) in [(300e6, true), (250e6, false)] {
        let forces = MemberForces {
            at: [
                (0.0, 0.0, 37_500.0, mid_nmm - 28.125e6),
                (0.5, 0.0, 0.0, mid_nmm),
                (1.0, -25_000.0, -37_500.0, mid_nmm - 28.125e6),
            ]
            .map(|(xi, n, qy, mz)| (xi, [n, qy, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let combined = checked(&result, 0.5)
            .flat_map(|r| &r.components)
            .find(|c| c.detail.contains("式(5.24)"));
        assert_eq!(combined.is_some(), expected, "{result:?}");
        if let Some(co) = combined {
            close(co.ratio, 1.039_515_670_123_936);
        }
        assert!(
            result.iter().any(|(xi, row)| *xi == 0.5
                && matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains("軸方向の分布荷重"))),
            "{result:?}"
        );
    }
}

#[test]
fn an_unsaved_web_shear_peak_survives_an_unrelated_nonfinite_sample() {
    let mut model = input::column(0.0, 0.0);
    for (a, p) in [(1000.0, 500_000.0), (1100.0, -500_000.0)] {
        model.load_cases[0].member.push(MemberLoad::manual(
            model.elements[0].id,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Point { a, p },
        ));
    }
    for (missing, incompatible) in [(4, false), (3, false), (4, true)] {
        let mut forces = MemberForces {
            at: vec![
                (0.0, [0.0; 6]),
                (0.5, [0.0, 0.0, 0.0, 0.0, 0.0, 50e6]),
                (1.0, [0.0, 0.0, 0.0, 0.0, 0.0, 50e6]),
            ],
        };
        forces.at[1].1[missing] = f64::NAN;
        if incompatible {
            forces.at[2].1[1] = 100_000.0;
        }
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let web_ng = checked(&result, 1.0 / 3.0).find(|r| r.detail.contains("集中荷重の右側"));
        assert_eq!(
            web_ng.is_some(),
            missing == 4 && !incompatible,
            "{result:?}"
        );
        if let Some(row) = web_ng {
            let [shear] = row.components.as_slice() else {
                panic!("{row:?}")
            };
            assert_eq!(shear.kind, CheckKind::Shear);
            close(shear.ratio, 1.231_689_332_950_902_7);
        }
        assert!(
            result
                .iter()
                .all(|(_, r)| !matches!(r, CheckOutcome::Checked(r) if r.ok())),
            "{result:?}"
        );
        assert!(result.iter().any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains(if incompatible { "釣合いを確認できません" } else { "保存断面力に非有限な成分" }))), "{result:?}");
    }
}

#[test]
fn identical_duplicate_i_end_forces_can_verify_the_unique_j_end_ng() {
    let model = input::column(0.0, 0.0);
    for agree in [true, false] {
        let mut saved = support_column::moments(300e6);
        saved.at.insert(0, saved.at[0]);
        if !agree {
            saved.at[1].1[5] += 1.0;
        }
        let result = rows(&model, saved, Some(&[(input::CASE, 1.0)]));
        assert!(checked(&result, 0.0).next().is_none(), "{result:?}");
        let end = checked(&result, 1.0).next();
        assert_eq!(end.is_some(), agree, "{result:?}");
        if let Some(row) = end {
            close(ratio(row), 1.111_781_465_373_194);
        }
        assert!(result.iter().any(
            |(_, row)| matches!(row, CheckOutcome::Skipped { reason } if reason.contains("重複"))
        ));
    }
}

#[test]
fn an_interior_maximum_tied_with_an_end_needs_agreement_with_saved_cuts() {
    let mut model = input::column(0.0, 0.0);
    for (a, p) in [(1000.0, 200_000.0), (2000.0, -200_000.0)] {
        model.load_cases[0].member.push(MemberLoad::manual(
            model.elements[0].id,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Point { a, p },
        ));
    }
    for sampled_peak in [false, true] {
        for delta_n in [-0.00025, 0.0, 0.00025] {
            let mut cuts = vec![
                (0.0, -100_000.0, 100e6),
                (1500.0, 100_000.0, 50e6),
                (3000.0, -100_000.0, 0.0),
            ];
            if sampled_peak {
                cuts.insert(2, (2000.0, 100_000.0, 100e6));
            }
            let forces = MemberForces {
                at: cuts
                    .into_iter()
                    .map(|(x, qy, mz)| {
                        let offset = if x == 0.0 { 0.0 } else { delta_n };
                        (
                            x / 3000.0,
                            [0.0, qy + offset, 0.0, 0.0, 0.0, mz + x * offset],
                        )
                    })
                    .collect(),
            };
            let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
            assert!(!result.is_empty());
            if delta_n < 0.0 {
                assert!(
                    result
                        .iter()
                        .all(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
                    if reason.contains("区間内部の絶対値最大"))),
                    "{result:?}"
                );
            } else {
                assert!(
                    result
                        .iter()
                        .all(|(_, row)| matches!(row, CheckOutcome::Checked(c)
                    if c.ok() && c.detail.contains("C=1.0000"))),
                    "{result:?}"
                );
            }
        }
    }
}

#[test]
fn a_partial_response_evaluates_only_local_formulas_with_confirmed_inputs() {
    let mut model = input::column(0.0, 0.0);
    for (peak_qy_n, initial_mz_nmm, missing_at, missing_component, expected_kind, expected_ratio) in [
        (
            500_000.0,
            0.0,
            0,
            5,
            Some("ウェブ面内せん断"),
            1.231_689_332_950_902_7,
        ),
        (320_000.0, 185e6, 0, 5, None, 0.0),
        (
            500_000.0,
            0.0,
            0,
            4,
            Some("ウェブ面内せん断"),
            1.231_689_332_950_902_7,
        ),
        (
            320_000.0,
            185e6,
            1,
            4,
            Some("式(5.24)"),
            1.089_387_618_008_001,
        ),
        (320_000.0, 185e6, 0, 4, None, 0.0),
    ] {
        model.load_cases[0].member = [(1000.0, peak_qy_n), (1100.0, -peak_qy_n)]
            .map(|(a, p)| {
                MemberLoad::manual(
                    model.elements[0].id,
                    [1.0, 0.0, 0.0],
                    MemberLoadKind::Point { a, p },
                )
            })
            .to_vec();
        let mut forces = support_column::moments(initial_mz_nmm);
        for (_, f) in &mut forces.at[1..] {
            f[5] += 100.0 * peak_qy_n;
        }
        forces.at[missing_at].1[missing_component] = f64::NAN;
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let left = checked(&result, 1100.0 / 3000.0).find(|r| r.detail.contains("集中荷重の左側"));
        assert_eq!(left.is_some(), expected_kind.is_some(), "{result:?}");
        if let (Some(row), Some(basis)) = (left, expected_kind) {
            let [component] = row.components.as_slice() else {
                panic!("{row:?}")
            };
            assert!(component.detail.contains(basis));
            close(component.ratio, expected_ratio);
            if missing_at == 0 {
                assert!(!component.detail.contains("式(5.24)"));
            }
            if missing_component == 5 {
                assert!(!row.detail.contains("C="), "{row:?}");
            }
        }
        assert!(
            result
                .iter()
                .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains("保存断面力に非有限な成分"))),
            "{result:?}"
        );
        assert!(
            result
                .iter()
                .all(|(_, outcome)| !matches!(outcome, CheckOutcome::Checked(row) if row.ok())),
            "{result:?}"
        );
    }
}

#[test]
fn a_missing_area_hides_same_point_shear_only_when_an_axial_stress_is_needed() {
    let mut model = input::column(0.0, 0.0);
    model.sections[0].area = f64::NAN;
    for (n, expect_combined) in [(0.0, true), (-100_000.0, false)] {
        let response = MemberForces {
            at: [(0.0, -700e6), (0.5, -250e6), (1.0, 200e6)]
                .map(|(xi, mz)| (xi, [n, 300_000.0, 0.0, 0.0, 0.0, mz]))
                .to_vec(),
        };
        let results = rows(&model, response, Some(&[(input::CASE, 1.0)]));
        let combined = checked(&results, 1.0)
            .flat_map(|r| &r.components)
            .find(|c| c.detail.contains("式(5.24)"));
        assert_eq!(combined.is_some(), expect_combined, "{results:?}");
        if let Some(c) = combined {
            close(c.ratio, 1.013_116_271_683_298);
        }
        assert!(
            results.iter().any(|(xi, row)| *xi == 1.0
                && matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains(" A:") || reason.contains("の A は"))),
            "{results:?}"
        );
    }
}

#[test]
fn missing_torsional_constant_keeps_independent_compression_after_a_web_shear_ng() {
    let mut model = input::column(0.0, 0.0);
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    model.sections[0].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
    model.sections[0].j = 0.0;
    for (n, compression_ng) in [(-2e6, true), (-500_000.0, false)] {
        let response = MemberForces {
            at: [(0.0, -1400e6), (0.5, -650e6), (1.0, 100e6)]
                .map(|(xi, mz)| (xi, [n, 500_000.0, 0.0, 0.0, 0.0, mz]))
                .to_vec(),
        };
        let results = rows(&model, response, Some(&[(input::CASE, 1.0)]));
        let j: Vec<_> = checked(&results, 1.0).flat_map(|r| &r.components).collect();
        let web = j
            .iter()
            .find(|c| c.kind == CheckKind::Shear && !c.detail.contains("式(5.24)"))
            .unwrap_or_else(|| panic!("{results:?}"));
        close(web.ratio, 1.231_689_332_950_902_7);
        let axial = j.iter().find(|c| c.kind == CheckKind::Axial);
        assert_eq!(axial.is_some(), compression_ng, "{results:?}");
        if let Some(axial) = axial {
            assert!(axial.detail.contains("fc="), "{axial:?}");
            close(axial.ratio, 1.460_510_063_065_396_9);
            assert!(checked(&results, 1.0).any(|r| r.basis.contains("許容圧縮応力度")));
        }
        assert!(
            results.iter().any(|(xi, row)| *xi == 1.0
                && matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains("横座屈に必要な断面性能"))),
            "{results:?}"
        );
    }
}

#[test]
fn partial_station_verification_cancels_the_load_before_rounding_moments() {
    let mut model = input::column(0.0, 0.0);
    let q_i_n = 1.000_000_000_000_002_5e22;
    model.load_cases[0].member.push(MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Point { a: 0.0, p: -q_i_n },
    ));
    for compatible in [false, true] {
        let mut forces = support_column::moments(200e6);
        forces.at[0].1[1] = q_i_n;
        forces.at[2].1[4] = f64::NAN;
        if !compatible {
            forces.at[1].1[5] = 2_147_483_648.0;
            forces.at[2].1[5] = 0.0;
        }
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        assert!(!result.is_empty());
        assert!(result.iter().all(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains(if compatible { "保存断面力に非有限な成分" } else { "釣合いを確認できません" }))), "{result:?}");
    }
}

#[test]
fn weak_shear_elsewhere_does_not_hide_the_local_zero_shear_limit_ng() {
    let mut model = input::column(0.0, 0.0);
    let elem = model.elements[0].id;
    for a in [2000.0, 1500.0] {
        model.load_cases[0].member = [([1.0, 0.0, 0.0], 0.0, -25.0), ([0.0, 1.0, 0.0], a, 25.0)]
            .map(|(dir, a, w)| {
                MemberLoad::manual(
                    elem,
                    dir,
                    MemberLoadKind::Distributed {
                        a,
                        b: 3000.0,
                        w1: w,
                        w2: w,
                    },
                )
            })
            .to_vec();
        let length_mm = 3000.0 - a;
        let forces = MemberForces {
            at: vec![
                (0.0, [0.0, 37_500.0, 0.0, 0.0, 0.0, 271.875e6]),
                (0.5, [0.0, 0.0, 0.0, 0.0, 0.0, 300e6]),
                (
                    1.0,
                    [
                        0.0,
                        -37_500.0,
                        25.0 * length_mm,
                        0.0,
                        -25.0 * length_mm * length_mm / 2.0,
                        271.875e6,
                    ],
                ),
            ],
        };
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let at_peak = checked(&result, 0.5)
            .flat_map(|r| &r.components)
            .find(|c| c.detail.contains("式(5.24)"));
        close(
            at_peak.unwrap_or_else(|| panic!("{result:?}")).ratio,
            1.039_515_670_123_936,
        );
        assert!(
            result
                .iter()
                .any(|(_, row)| matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains("弱軸せん断"))),
            "{result:?}"
        );
    }
}

#[test]
fn a_missing_weak_inertia_does_not_block_formulas_that_do_not_use_it() {
    let mut model = input::column(0.0, 0.0);
    model.sections[0].iz = f64::NAN;
    for (n, qy, my, expected) in [
        (
            0.0,
            300_000.0,
            0.0,
            Some(("式(5.24)", 1.013_116_271_683_298)),
        ),
        (
            650_000.0,
            0.0,
            0.0,
            Some(("式(6.3)", 1.078_828_933_475_746)),
        ),
        (0.0, 300_000.0, 1e6, None),
        (650_000.0, 0.0, 1e6, None),
    ] {
        let forces = MemberForces {
            at: [0.0, 0.5, 1.0]
                .map(|xi| (xi, [n, qy, 0.0, 0.0, my, 200e6 - qy * 3000.0 * (1.0 - xi)]))
                .to_vec(),
        };
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let formulas: Vec<_> = checked(&result, 1.0)
            .flat_map(|r| &r.components)
            .filter(|c| c.detail.contains("式(5.24)") || c.detail.contains("式(6.3)"))
            .collect();
        if let Some((name, ratio)) = expected {
            let [f] = formulas.as_slice() else {
                panic!("{result:?}")
            };
            assert!(f.detail.contains(name));
            close(f.ratio, ratio);
        } else {
            assert!(formulas.is_empty(), "{result:?}");
        }
        assert!(
            result.iter().any(|(xi, row)| *xi == 1.0
                && matches!(row, CheckOutcome::Skipped { reason }
            if reason.contains("Iz") || reason.contains("弱軸断面二次モーメント"))),
            "{result:?}"
        );
    }
}

#[test]
fn a_saved_cut_outside_recovery_candidates_checks_weak_shear_applicability() {
    let model = input::column(0.0, 0.0);
    for qz in [1e-9, 0.0] {
        let forces = MemberForces {
            at: [0.0, 0.5, 1.0]
                .map(|xi| {
                    (
                        xi,
                        [
                            0.0,
                            320_000.0,
                            if xi == 0.5 { 0.0 } else { qz },
                            0.0,
                            -qz * 3000.0 * xi,
                            196e6 + 320_000.0 * 3000.0 * (xi - 0.5),
                        ],
                    )
                })
                .to_vec(),
        };
        let result = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let at_saved = checked(&result, 0.5)
            .flat_map(|r| &r.components)
            .find(|c| c.detail.contains("式(5.24)"));
        if qz == 0.0 {
            close(
                at_saved.unwrap_or_else(|| panic!("{result:?}")).ratio,
                1.040_496_157_306_278,
            );
        } else {
            assert!(at_saved.is_none(), "{result:?}");
            assert!(
                result.iter().any(|(xi, row)| *xi == 0.5
                    && matches!(row, CheckOutcome::Skipped { reason }
                 if reason.contains("弱軸せん断の有無"))),
                "{result:?}"
            );
        }
    }
}

#[test]
fn the_same_point_shear_check_includes_the_continuous_zero_shear_peak() {
    let mut model = input::column(0.0, 0.0);
    let element = model.elements[0].id;
    let terms = [(input::CASE, 1.0)];
    for (loaded, weak_qz_n) in [(true, 0.0), (true, 1000.0), (false, 0.0)] {
        let mut forces = support_column::moments(271.875e6);
        if loaded {
            model.load_cases[0].member = vec![MemberLoad::manual(
                element,
                [1.0, 0.0, 0.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 3000.0,
                    w1: -25.0,
                    w2: -25.0,
                },
            )];
            forces.at[0].1[1] = 37_500.0;
            forces.at[1].1[5] = 300e6;
            forces.at[2].1[1] = -37_500.0;
        } else {
            model.load_cases[0].member.clear();
        }
        for (xi, force) in &mut forces.at {
            force[2] = weak_qz_n;
            force[4] = -weak_qz_n * 3000.0 * *xi;
        }
        let result = rows(&model, forces, Some(&terms));
        let at = |xi| {
            checked(&result, xi)
                .flat_map(|row| row.components.iter())
                .find(|c| c.detail.contains("式(5.24)"))
        };
        if loaded && weak_qz_n == 0.0 {
            let interior = at(0.5).unwrap_or_else(|| panic!("{result:?}"));
            close(interior.ratio, 1.039_515_670_123_936);
            assert!(interior.detail.contains("τ=0.0000"));
            for xi in [0.0, 1.0] {
                assert!(at(xi).unwrap().ratio < 1.0, "{result:?}");
            }
        } else {
            for xi in [0.0, 0.5, 1.0] {
                assert!(at(xi).is_none(), "{result:?}");
            }
        }
    }
}

#[test]
fn the_same_saved_cut_keeps_its_failure_across_the_unrounded_ratio_limit() {
    // C=1.75 の短期許容223.8331232 N/mm²と Z=1148243.4133333 mm³からの独立値。
    let limit_nmm = 257_014_909.402_339_07;
    for recovered_margin_nmm in [-1.0, 1.0] {
        let recovered_mz_nmm = limit_nmm + recovered_margin_nmm;
        let saved_mz_nmm = limit_nmm - recovered_margin_nmm;
        let qy_n = recovered_mz_nmm / 3000.0;
        let mut model = input::column(-qy_n, 0.0);
        model.load_cases[0].nodal[0].values[4] = recovered_mz_nmm;
        let forces = MemberForces {
            at: [
                (0.0, 0.0),
                (0.5, recovered_mz_nmm / 2.0),
                (1.0, saved_mz_nmm),
            ]
            .map(|(xi, mz)| (xi, [0.0, qy_n, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let at_j: Vec<_> = checked(&results, 1.0).collect();
        let [failure] = at_j.as_slice() else {
            panic!("{results:?}");
        };
        assert!(!failure.ok(), "{failure:?}");
        assert!(ratio(failure) > 1.0, "{failure:?}");
        assert!(failure.detail.contains("C=1.7500"), "{failure:?}");
        assert_eq!(
            failure.detail.contains("保存断面力による照査"),
            recovered_margin_nmm < 0.0,
            "{failure:?}"
        );
        assert!(!results
            .iter()
            .any(|(_, result)| matches!(result, CheckOutcome::Skipped { .. })));
    }
}

#[test]
fn same_cut_ng_rows_keep_the_governing_components_without_mixing_forces() {
    let qy_n = 500_000.0;
    let j_mz_nmm = 1_500e6;
    let mut model = input::column(-qy_n, 0.0);
    model.load_cases[0].nodal[0].values[4] = j_mz_nmm;
    let terms = [(input::CASE, 1.0)];
    let forces = MemberForces {
        at: [(0.0, 0.0), (0.5, 750e6), (1.0, j_mz_nmm)]
            .map(|(xi, mz)| (xi, [0.0, qy_n, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
    };
    let components = |result: &CheckResult| {
        let [axial_bending, shear] = result.components.as_slice() else {
            panic!("{result:?}");
        };
        assert_eq!(axial_bending.kind, CheckKind::AxialBending);
        assert_eq!(shear.kind, CheckKind::Shear);
        assert!(axial_bending.ratio > 1.0 && shear.ratio > 1.0, "{result:?}");
        [axial_bending.ratio, shear.ratio]
    };
    let reference = rows(&model, forces.clone(), Some(&terms));
    let recovered_ratios = components(checked(&reference, 1.0).next().unwrap());

    for saved_shear_delta_n in [-0.004, 0.004] {
        let mut observed = forces.clone();
        observed.at[2].1[1] += saved_shear_delta_n;
        observed.at[2].1[5] += 0.25;
        let result = rows(&model, observed, Some(&terms));
        assert!(
            result
                .iter()
                .all(|(_, row)| matches!(row, CheckOutcome::Checked(_))),
            "{result:?}"
        );
        let (saved, recovered): (Vec<_>, Vec<_>) =
            checked(&result, 1.0).partition(|row| row.detail.contains("保存断面力による照査"));
        let [saved] = saved.as_slice() else {
            panic!("{result:?}");
        };
        let [axial_bending, shear] = components(saved);
        assert!(axial_bending > recovered_ratios[0], "{result:?}");
        if saved_shear_delta_n > 0.0 {
            assert!(shear > recovered_ratios[1], "{result:?}");
            assert!(recovered.is_empty(), "{result:?}");
        } else {
            assert!(shear < recovered_ratios[1], "{result:?}");
            let [recovered] = recovered.as_slice() else {
                panic!("{result:?}");
            };
            assert_eq!(components(recovered), recovered_ratios);
        }
    }
}

#[test]
fn a_saved_only_same_point_shear_check_is_visible_even_when_it_passes() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].nodal[0].values[4] = 250e6;
    let terms = [(input::CASE, 1.0)];
    let mut force = support_column::moments(250e6);
    force.at[2].1[1] = 1e-9;
    let result = rows(&model, force, Some(&terms));
    let at_j: Vec<_> = checked(&result, 1.0).collect();
    let [row] = at_j.as_slice() else {
        panic!("{result:?}");
    };
    assert!(!row.ok(), "{row:?}");
    assert!(row.detail.contains("保存断面力による照査"), "{row:?}");
    let [_, shear] = row.components.as_slice() else {
        panic!("{row:?}");
    };
    assert_eq!(shear.kind, CheckKind::Shear);
    close(shear.ratio, 0.866_263_058_436_613);
    assert!(shear.detail.contains("式(5.24)"), "{shear:?}");
}

#[test]
fn passing_same_cut_variations_without_a_new_formula_are_not_duplicated() {
    let qy_n = 60_000.0;
    let mut model = input::column(-qy_n, 0.0);
    model.load_cases[0].nodal[0].values[4] = 180e6;
    let terms = [(input::CASE, 1.0)];
    for (delta_qy_n, expect_saved) in [(-0.0005, false), (0.0, true)] {
        let forces = MemberForces {
            at: [
                (0.0, [0.0, qy_n, 0.0, 0.0, 0.0, 0.0]),
                (0.5, [0.0, qy_n, 0.0, 0.0, 0.0, 90e6]),
                (1.0, [0.0, qy_n + delta_qy_n, 0.0, 0.0, 0.0, 180e6 + 0.01]),
            ]
            .to_vec(),
        };
        let result = rows(&model, forces, Some(&terms));
        let at_j: Vec<_> = checked(&result, 1.0).collect();
        let [row] = at_j.as_slice() else {
            panic!("{result:?}");
        };
        assert!(row.ok(), "{row:?}");
        assert_eq!(row.detail.contains("保存断面力による照査"), expect_saved);
        assert!(row.components[1].detail.contains("式(5.24)"));
    }
}

#[test]
fn a_nonfinite_ratio_cannot_replace_a_finite_ng_at_the_same_cut() {
    let mut model = input::column(0.0, 0.0);
    model.materials[0].fy = Some(1e-10);
    // Z=1148243.4133333 mm³ と除算分母の下限1e-9 N/mm²で検定比が溢れる境界。
    let moment_limit_nmm = 2.064_189_301_300_206e305;
    for margin in [-2e-9, 2e-9] {
        let constant_mz_nmm = moment_limit_nmm * (1.0 + margin);
        model.load_cases[0].nodal[0].values[4] = constant_mz_nmm;
        let mut force = support_column::moments(constant_mz_nmm);
        force.at[2].1[5] = moment_limit_nmm * (1.0 - margin);
        let results = rows(&model, force, Some(&[(input::CASE, 1.0)]));
        let at_j: Vec<_> = checked(&results, 1.0).collect();
        assert_eq!(at_j.len(), 2, "{at_j:?}");
        assert!(at_j.iter().any(|row| !ratio(row).is_finite()), "{at_j:?}");
        assert!(
            at_j.iter().any(|row| {
                ratio(row).is_finite()
                    && ratio(row) > 1.0
                    && row.detail.contains("保存断面力による照査") == (margin > 0.0)
            }),
            "{at_j:?}"
        );
    }
}

#[test]
fn monotonic_end_ratios_that_split_the_axial_bending_verdict_are_unconfirmed() {
    // H-400×200×8×13 の式(5.8), (5.13), (5.14)に独立代入。
    // j端100,000,000/100,000,001 N·mmに対し、i端曲げの比は0.999999999822/1.000000000824。
    let i_mz_nmm = 253_148_353.743_817_4;
    for (recovered_j_nmm, saved_j_nmm, expected_i_ok, high_shear) in [
        (100e6, 100e6 + 1.0, None, false),
        (100e6 + 1.0, 100e6, None, false),
        (100e6, 100e6, Some(true), false),
        (100e6 - 1.0, 100e6, Some(true), false),
        (100e6 + 1.0, 100e6 + 2.0, Some(false), false),
        (100e6, 100e6 + 1.0, None, true),
    ] {
        let point_mm = if high_shear { 100.0 } else { 0.0 };
        let moment_at_point_nmm = i_mz_nmm - 500_000.0 * point_mm;
        let j_qy_n = (recovered_j_nmm - moment_at_point_nmm) / (3000.0 - point_mm);
        let i_qy_n = if high_shear { -500_000.0 } else { j_qy_n };
        let mut model = input::column(-j_qy_n, 0.0);
        model.load_cases[0].nodal[0].values[4] = recovered_j_nmm;
        if high_shear {
            model.load_cases[0].member.push(MemberLoad::manual(
                model.elements[0].id,
                [1.0, 0.0, 0.0],
                MemberLoadKind::Point {
                    a: point_mm,
                    p: j_qy_n - i_qy_n,
                },
            ));
        }
        let forces = MemberForces {
            at: [
                (0.0, i_qy_n, i_mz_nmm),
                (
                    0.5,
                    j_qy_n,
                    moment_at_point_nmm + j_qy_n * (1500.0 - point_mm),
                ),
                (1.0, j_qy_n, saved_j_nmm),
            ]
            .map(|(xi, qy, mz)| (xi, [0.0, qy, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
        };
        let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        if let Some(expected_ok) = expected_i_ok {
            assert!(
                !results
                    .iter()
                    .any(|(_, outcome)| matches!(outcome, CheckOutcome::Skipped { .. })),
                "{results:?}"
            );
            let i = checked(&results, 0.0)
                .next()
                .unwrap_or_else(|| panic!("{results:?}"));
            assert_eq!(ratio(i) <= 1.0, expected_ok, "{results:?}");
        } else {
            assert!(results.iter().any(|(xi, outcome)| *xi == 0.0 && matches!(outcome,
                CheckOutcome::Skipped { reason } if reason.contains("保存端部比と回復端部比で同じ切断面の軸曲げの適否が一致しない")
                    && reason.contains("x=0.000000 mm"))), "{results:?}");
            let failed: Vec<_> = checked(&results, 0.0)
                .flat_map(|result| &result.components)
                .filter(|component| component.ratio > 1.0)
                .collect();
            assert_eq!(failed.len(), usize::from(high_shear), "{results:?}");
            assert!(
                failed
                    .iter()
                    .all(|component| component.kind == CheckKind::Shear),
                "{results:?}"
            );
        }
    }
}

#[test]
fn floating_load_cancellation_does_not_hide_a_finite_disagreement_in_exact_recovery() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member = [1e22, -30_000.0, -1e22]
        .map(|p| {
            MemberLoad::manual(
                model.elements[0].id,
                [1.0, 0.0, 0.0],
                MemberLoadKind::Point { a: 1000.0, p },
            )
        })
        .to_vec();
    let forces = MemberForces {
        at: [(0.0, 0.0), (0.5, 150e6), (1.0, 300e6)]
            .map(|(xi, mz)| (xi, [0.0, 100_000.0, 0.0, 0.0, 0.0, mz]))
            .to_vec(),
    };
    let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
    assert!(
        results.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("厳密な回復値") && reason.contains("釣合いを確認できません"))
        ),
        "{results:?}"
    );
}

#[test]
fn an_inconsistent_saved_terminal_shear_does_not_prove_the_favorable_monotonic_factor() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].nodal[0].values[4] = -225e6;
    model.load_cases[0].member.push(MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: 50.0,
            w2: 50.0,
        },
    ));
    let terms = [(input::CASE, 1.0)];
    for qy_j_n in [0.0, -1e-9, 1e-9] {
        let forces = MemberForces {
            at: vec![
                (0.0, [0.0, -150_000.0, 0.0, 0.0, 0.0, 0.0]),
                (0.5, [0.0, -75_000.0, 0.0, 0.0, 0.0, -168.75e6]),
                (1.0, [0.0, qy_j_n, 0.0, 0.0, 0.0, -225e6]),
            ],
        };
        let results = rows(&model, forces, Some(&terms));
        if qy_j_n > 0.0 {
            assert!(results.iter().all(|(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
                if reason.contains("符号付き強軸曲げの単調変化または区間内部の絶対値最大を確認できません"))), "{results:?}");
        } else {
            assert!(
                results.iter().all(
                    |(_, outcome)| matches!(outcome, CheckOutcome::Checked(result)
                if result.ok() && result.detail.contains("C=1.7500"))
                ),
                "{results:?}"
            );
        }
    }
}

#[test]
fn an_unrelated_nonfinite_component_does_not_erase_verified_strong_web_shear_failure() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member = [(1400.0, 500_000.0), (1600.0, -500_000.0)]
        .map(|(a, p)| {
            MemberLoad::manual(
                model.elements[0].id,
                [1.0, 0.0, 0.0],
                MemberLoadKind::Point { a, p },
            )
        })
        .to_vec();
    for (missing, web_shear_confirmed) in [(4, true), (0, true), (2, true), (3, false), (1, false)]
    {
        let mut forces = MemberForces {
            at: vec![
                (0.0, [0.0; 6]),
                (0.5, [0.0, 500_000.0, 0.0, 0.0, 0.0, 50e6]),
                (1.0, [0.0, 0.0, 0.0, 0.0, 0.0, 100e6]),
            ],
        };
        forces.at[1].1[missing] = f64::NAN;
        let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
        let failures: Vec<_> = checked(&results, 0.5).collect();
        if web_shear_confirmed {
            let [failure] = failures.as_slice() else {
                panic!("{results:?}");
            };
            let [component] = failure.components.as_slice() else {
                panic!("{failure:?}");
            };
            assert_eq!(component.kind, CheckKind::Shear);
            close(component.ratio, 1.2316893329509027);
            assert!(component.detail.contains("τ=167.1123"), "{component:?}");
        } else {
            assert!(failures.is_empty(), "{results:?}");
        }
        assert!(results.iter().any(|(xi, outcome)| *xi == 0.5 &&
            matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("保存断面力に非有限な成分がある"))), "{results:?}");
    }
}

#[test]
fn an_unassigned_girders_invalid_section_still_yields_a_diagnostic() {
    let mut model = input::column(0.0, 0.0);
    model.nodes[1].coord = [3000.0, 0.0, 0.0];
    model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
    model.sections[0].frame_use = None;
    model.sections[0].as_z = f64::NAN;
    let results = rows(
        &model,
        support_column::moments(0.0),
        Some(&[(input::CASE, 1.0)]),
    );
    assert_eq!(results.len(), 3, "{results:?}");
    assert!(
        results.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("Asz は有限な非負値が必要"))
        ),
        "{results:?}"
    );
}

#[test]
fn a_discarded_component_at_the_saved_cut_does_not_clear_its_applicability_issue() {
    let mut model = input::column(0.0, 0.0);
    model.steel_design_attrs[0].lk_z_direct = Some(10_000.0);
    let terms = [(input::CASE, 1.0)];
    for (index, condition) in [(2, "弱軸せん断の有無"), (3, "材軸ねじりの有無")] {
        let mut forces = support_column::moments(20e6);
        for (_, force) in &mut forces.at {
            force[0] = -1000.0;
        }
        forces.at[2].1[index] = 1e-10;
        let results = rows(&model, forces, Some(&terms));
        assert!(results.iter().any(|(xi, outcome)| *xi == 1.0 && matches!(outcome,
            CheckOutcome::Skipped { reason } if reason.contains(condition) && reason.contains("x=3000.000000 mm"))), "{results:?}");
        assert!(
            checked(&results, 1.0).any(|result| result
                .components
                .iter()
                .any(|component| component.kind == CheckKind::Provision && component.ratio > 1.0)),
            "{results:?}"
        );
    }
}

#[test]
fn an_inclined_columns_exactly_transverse_load_is_distinguished_from_a_rounded_zero() {
    let mut model = input::column(0.0, 0.0);
    model.nodes[1].coord = [3000.0, 0.0, 3000.0];
    model.elements[0].local_axis.ref_vector = [1.0, 0.0, -1.0];
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    let length_mm = 3000.0 * 2.0_f64.sqrt();
    let forces = MemberForces {
        at: vec![
            (0.0, [0.0, -30_000.0, 0.0, 0.0, 0.0, 150e6]),
            (
                0.5,
                [
                    0.0,
                    -40_000.0,
                    0.0,
                    0.0,
                    0.0,
                    120e6 - 40_000.0 * (length_mm / 2.0 - 1000.0),
                ],
            ),
            (
                1.0,
                [
                    0.0,
                    -40_000.0,
                    0.0,
                    0.0,
                    0.0,
                    120e6 - 40_000.0 * (length_mm - 1000.0),
                ],
            ),
        ],
    };

    for (dir, exactly_transverse) in [
        ([1.0, 0.0, -1.0], true),
        ([1.0, 0.0, -1.0 + f64::EPSILON], false),
    ] {
        model.load_cases[0].member = vec![MemberLoad::manual(
            model.elements[0].id,
            dir,
            MemberLoadKind::Point {
                a: 1000.0,
                p: -10_000.0,
            },
        )];
        let results = rows(&model, forces.clone(), Some(&[(input::CASE, 1.0)]));
        assert!(!results.is_empty());
        if exactly_transverse {
            assert!(
                results.iter().all(
                    |(_, outcome)| matches!(outcome, CheckOutcome::Checked(result)
                if result.ok() && result.detail.contains("符号付き曲げが単調"))
                ),
                "{results:?}"
            );
        } else {
            assert!(
                results.iter().all(
                    |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
                if reason.contains("部材荷重の局所軸成分が回復時に保持されることを確認できません"))
                ),
                "{results:?}"
            );
        }
    }
}

#[test]
fn zero_at_all_saved_stations_does_not_hide_a_nonzero_internal_moment() {
    let mut model = input::column(0.0, 0.0);
    model.load_cases[0].member = [
        (250.0, 600_000.0),
        (750.0, -1_200_000.0),
        (1250.0, 600_000.0),
    ]
    .map(|(a, p)| {
        MemberLoad::manual(
            model.elements[0].id,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Point { a, p },
        )
    })
    .to_vec();
    let forces = support_column::moments(0.0);
    let results = rows(&model, forces.clone(), Some(&[(input::CASE, 1.0)]));
    let result = checked(&results, 0.25).next().unwrap();
    assert!(ratio(result) > 1.35, "{result:?}");
    assert!(result.detail.contains("Mz=300.000000 kN·m"), "{result:?}");
    let unknown = rows(&model, forces, None);
    assert!(
        unknown.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("荷重組合せの由来が未確認"))
        ),
        "{unknown:?}"
    );
}

#[test]
fn a_nonmonotonic_distribution_with_only_an_end_maximum_has_no_favorable_end_factor() {
    let model = input::column(0.0, -40_000.0);
    let forces = MemberForces {
        at: vec![
            (0.0, [0.0, 10_000.0, 0.0, 0.0, 0.0, -200e6]),
            (0.5, [0.0, -30_000.0, 0.0, 0.0, 0.0, -205e6]),
            (1.0, [0.0, -30_000.0, 0.0, 0.0, 0.0, -250e6]),
        ],
    };
    let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
    assert!(
        results.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("符号付き強軸曲げの単調変化または区間内部の絶対値最大を確認できません"))
        ),
        "{results:?}"
    );
}

#[test]
fn the_off_center_combination_uses_axial_force_on_its_own_side_of_the_point_load() {
    let mut model = input::column(125_000.0, -135_000.0);
    model.load_cases[0].member.push(MemberLoad::manual(
        model.elements[0].id,
        [0.0, 0.0, 1.0],
        MemberLoadKind::Point {
            a: 1000.0,
            p: 100_000.0,
        },
    ));
    let force = linear_static_once(&model, input::CASE)
        .unwrap()
        .member_forces
        .remove(0)
        .1;
    let results = rows(&model, force, Some(&[(input::CASE, 1.0)]));
    let at_point = checked(&results, 1.0 / 3.0).collect::<Vec<_>>();
    assert_eq!(at_point.len(), 2, "{results:?}");
    assert!(
        at_point[0].detail.contains("左側") && at_point[0].detail.contains("N=100.000000 kN"),
        "{at_point:?}"
    );
    assert!(
        at_point[1].detail.contains("右側") && at_point[1].detail.contains("N=0.000000 kN"),
        "{at_point:?}"
    );
    assert!(ratio(at_point[0]) < ratio(at_point[1]), "{at_point:?}");
}

#[test]
fn starting_and_ending_at_a_lower_ratio_does_not_certify_a_varying_axial_combination() {
    let mut model = input::column(166_204.932_293_937_1, 0.0);
    model.load_cases[0].nodal[0].values[2] = -597_051.689_031_022_3;
    model.load_cases[0].member = [
        ([1.0, 0.0, 0.0], -79.145_205_854_255_78),
        ([0.0, 0.0, 1.0], 150.632_306_865_624_88),
    ]
    .map(|(direction, w)| {
        MemberLoad::manual(
            model.elements[0].id,
            direction,
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 3000.0,
                w1: w,
                w2: w,
            },
        )
    })
    .to_vec();
    // 軸比は 0.106+0.00011x、曲げ比は 0.79(1-((x-900)/2100)²)。和は x=900 で0.995、x≈1207で1.01189。
    let force = linear_static_once(&model, input::CASE)
        .unwrap()
        .member_forces
        .remove(0)
        .1;
    let results = rows(&model, force, Some(&[(input::CASE, 1.0)]));
    assert!(
        results.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("軸方向の分布荷重があるため"))
        ),
        "{results:?}"
    );
    assert!(results.iter().any(|(xi, outcome)| (*xi - 0.3).abs() < 1e-12
        && matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("x=900.000000"))), "{results:?}");
}

#[test]
fn compressive_side_with_section_loss_does_not_use_the_gross_allowable_bending_stress() {
    let mut model = input::column(0.0, 0.0);
    model.steel_design_attrs[0].joint_flange_loss = 0.1;
    let force = support_column::moments(215e6);
    let results = rows(&model, force, Some(&[(input::CASE, 1.0)]));
    assert!(
        results.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("断面欠損の位置と断面性能を確定できない"))
        ),
        "{results:?}"
    );

    model.load_cases[0].nodal[0].values[2] = -2e6;
    model.load_cases[0].nodal[0].values[4] = 20e6;
    let mut forces = support_column::moments(20e6);
    for (_, force) in &mut forces.at {
        force[0] = -2e6;
    }
    let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
    let failure = checked(&results, 0.5)
        .next()
        .unwrap_or_else(|| panic!("{results:?}"));
    assert_eq!(failure.components.len(), 1, "{failure:?}");
    assert_eq!(failure.components[0].kind, CheckKind::Axial);
    close(failure.components[0].ratio, 1.0388962765957446);
    assert!(
        results.iter().any(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("断面欠損の位置と断面性能を確定できない"))
        ),
        "{results:?}"
    );
}

#[test]
fn unavailable_weak_inertia_does_not_hide_a_verified_strong_material_failure() {
    let mut model = input::column(150_000.0, -162_000.0);
    let forces = linear_static_once(&model, input::CASE)
        .unwrap()
        .member_forces
        .remove(0)
        .1;
    model.sections[0].iz = f64::NAN;
    let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
    let failure = checked(&results, 1.0 / 3.0)
        .next()
        .unwrap_or_else(|| panic!("{results:?}"));
    let [bending, combined] = failure.components.as_slice() else {
        panic!("{failure:?}")
    };
    assert_eq!(bending.kind, CheckKind::Bending);
    assert!(
        bending.detail.contains("材料許容上限=235.0000"),
        "{bending:?}"
    );
    close(bending.ratio, 1.111_781_465_373_194);
    assert_eq!(combined.kind, CheckKind::Shear);
    assert!(combined.detail.contains("式(5.24)"));
    close(combined.ratio, 1.039_935_889_463_504);
    assert!(failure.detail.contains("Mz=300.000000 kN·m"), "{failure:?}");
    assert!(
        results.iter().any(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("Iz は有限な非負値が必要"))
        ),
        "{results:?}"
    );
}

#[test]
fn missing_weak_inertia_does_not_replace_a_confirmed_strong_buckling_length_with_the_default() {
    let mut model = input::column(0.0, 0.0);
    model.nodes[1].coord[2] = 30_000.0;
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    model.sections[0].iz = f64::NAN;
    let mut forces = support_column::moments(20e6);
    for (_, cut) in &mut forces.at {
        cut[0] = -1000.0;
    }
    for (lk_y_mm, expected_ratio) in [(1000.0, None), (40_000.0, Some(1.1945187970110929))] {
        model.steel_design_attrs[0].lk_y_direct = Some(lk_y_mm);
        let results = rows(&model, forces.clone(), Some(&[(input::CASE, 1.0)]));
        let check = checked(&results, 0.0).find_map(|result| {
            result
                .components
                .iter()
                .find(|component| component.kind == CheckKind::Provision)
        });
        match (check, expected_ratio) {
            (None, None) => assert!(checked(&results, 0.0).next().is_none(), "{results:?}"),
            (Some(component), Some(expected)) => close(component.ratio, expected),
            _ => panic!("{results:?}"),
        }
        assert!(
            results.iter().any(
                |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
            if reason.contains("Iz は有限な非負値が必要"))
            ),
            "{results:?}"
        );
    }
}

#[test]
fn an_independent_axial_failure_does_not_use_the_weaker_plate_as_the_material_upper_bound() {
    use sepika_core::section_shape::SectionShape;

    let mut model = input::column(0.0, 0.0);
    let sec = &mut model.sections[0];
    let mut mixed = SectionShape::SteelH {
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 20.0,
        root_r: Some(0.0),
    }
    .to_section(sec.id, "板厚区分の異なるH".into());
    mixed.material = sec.material;
    mixed.frame_use = sec.frame_use;
    *sec = mixed;
    model.materials[0].name = "SS400".into();
    model.materials[0].fy = None;
    model.steel_design_attrs[0].joint_flange_loss = 0.001;
    model.load_cases[0].nodal[0].values[2] = -2_360_000.0;
    model.load_cases[0].nodal[0].values[4] = 20e6;
    let mut forces = support_column::moments(20e6);
    for (_, force) in &mut forces.at {
        force[0] = -2_360_000.0;
    }
    // 断面積10880 mm² × フランジF215は2.3392 MNだが、ウェブF235を含む断面全体の材料限界ではない。
    let results = rows(&model, forces, Some(&[(input::CASE, 1.0)]));
    assert!(
        results.iter().all(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("断面欠損の位置と断面性能を確定できない"))
        ),
        "{results:?}"
    );
}

#[test]
fn welded_symmetric_h_checks_the_off_center_maximum_of_a_uniform_load() {
    use sepika_core::section_shape::SectionShape;

    let mut model = input::column(40_000.0, 0.0);
    let sec = &mut model.sections[0];
    let mut welded = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 200.0,
        upper_thick: 13.0,
        lower_width: 200.0,
        lower_thick: 13.0,
        web_thick: 8.0,
    }
    .to_section(sec.id, "溶接組立H".into());
    welded.material = sec.material;
    welded.frame_use = sec.frame_use;
    *sec = welded;
    model.steel_design_attrs[0].column_lateral_buckling_end_supports =
        model.column_end_supports(&model.elements[0]);
    model.load_cases[0].member.push(MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: -20.0,
            w2: -20.0,
        },
    ));
    model.load_cases[0].nodal[0].values[4] = 210e6;
    // M'=20000-20x [N]。i端240 kN·mから1000 mmで250 kN·mになる。
    let force = linear_static_once(&model, input::CASE)
        .unwrap()
        .member_forces
        .remove(0)
        .1;
    let results = rows(&model, force, Some(&[(input::CASE, 1.0)]));
    let peak = checked(&results, 1.0 / 3.0)
        .next()
        .unwrap_or_else(|| panic!("{results:?}"));
    // フランジ二枚の弱軸断面性能による組立Hの Iw=648999000000 mm⁶、短期 fb=192.3698302 N/mm²。
    close(ratio(peak), 1.131_798_422_428_444_3);
    assert!(peak.detail.contains("Mz=250.000000 kN·m"), "{peak:?}");
}

#[test]
fn varying_strong_load_keeps_independent_material_limit_ng_at_a_verified_station() {
    let mut model = input::column(70_000.0, 0.0);
    model.load_cases[0].member = vec![MemberLoad::manual(
        model.elements[0].id,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: 0.0,
            w2: 40.0,
        },
    )];
    let force = linear_static_once(&model, input::CASE)
        .unwrap()
        .member_forces
        .remove(0)
        .1;
    let results = rows(&model, force, Some(&[(input::CASE, 1.0)]));
    let failure = checked(&results, 0.0)
        .next()
        .unwrap_or_else(|| panic!("{results:?}"));
    assert!(ratio(failure) > 1.22, "{failure:?}");
    assert!(
        results.iter().any(
            |(_, outcome)| matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("合算分布荷重の強度が区間内で変化"))
        ),
        "{results:?}"
    );
    assert!(
        !results
            .iter()
            .any(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(result) if result.ok())),
        "{results:?}"
    );
}

#[test]
fn constant_weak_moment_and_compression_can_be_checked_when_distributed_loads_cancel() {
    use sepika_core::dof::Dof;
    let mut model = input::column(40_000.0, 0.0);
    model.nodes[1].restraint.set_free(Dof::Uy);
    model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
    model.load_cases[0].nodal[0].values[2] = -100_000.0;
    model.load_cases[0].nodal[0].values[3] = 12e6;
    for (dir, w) in [
        ([0.0, 1.0, 0.0], 40.0),
        ([0.0, 1.0, 0.0], -40.0),
        ([0.0, 0.0, 1.0], 80.0),
        ([0.0, 0.0, 1.0], -80.0),
    ] {
        model.load_cases[0].member.push(MemberLoad::manual(
            model.elements[0].id,
            dir,
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 3000.0,
                w1: 0.0,
                w2: w,
            },
        ));
    }
    let force = MemberForces {
        at: [(0.0, 120e6), (0.5, 60e6), (1.0, 0.0)]
            .map(|(xi, mz)| (xi, [-100_000.0, -40_000.0, 0.0, 0.0, 12e6, mz]))
            .to_vec(),
    };
    let results = rows(&model, force, Some(&[(input::CASE, 1.0)]));
    assert!(
        results
            .iter()
            .all(|(_, outcome)| matches!(outcome, CheckOutcome::Checked(result) if result.ok())),
        "{results:?}"
    );
    let result = checked(&results, 0.0).next().unwrap();
    // 同じ断面で圧縮12.2070、強軸104.5075、弱軸69.1670 N/mm²を照査する。
    assert!((ratio(result) - 0.834_3).abs() < 1e-3, "{result:?}");
}
