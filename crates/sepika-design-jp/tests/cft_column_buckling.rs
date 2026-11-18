use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_design_jp::ultimate::{collect_cft_ultimate_checks, CftColumnClass};
use sepika_design_jp::{
    run_member_design_checks, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};

#[path = "fixtures/cft_column_buckling_model.rs"]
mod input;

const COLUMN: ElemId = ElemId(0);
const COMPRESSION_N: f64 = 7_450_000.0;

#[test]
fn cft_requires_two_finite_positive_direct_lengths_in_both_entries() {
    for circular in [false, true] {
        let mut model = input::cantilever(circular, COMPRESSION_N);
        let solved =
            sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).unwrap();
        let mut cases = vec![(None, None, "強軸・弱軸")];
        for invalid in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            cases.push((invalid, Some(6000.0), "強軸"));
            cases.push((Some(6000.0), invalid, "弱軸"));
        }
        for (strong, weak, axis) in cases {
            model.steel_design_attrs = vec![input::direct_lengths(COLUMN, strong, weak)];
            for term in [LoadTerm::Long, LoadTerm::Short] {
                let report = run_member_design_checks(
                    &model,
                    &solved.member_forces,
                    &solved.panel_moments,
                    &MemberDesignCheckOptions {
                        term,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(report.member_checks.len(), 3);
                for (id, _, outcome) in report.member_checks {
                    assert_eq!(id, COLUMN);
                    let CheckOutcome::Skipped { reason } = outcome else {
                        panic!("{outcome:?}");
                    };
                    assert!(reason.contains("CFT柱 ID 0"), "{reason}");
                    assert!(reason.contains(&format!(": {axis}の座屈長さ")), "{reason}");
                    assert!(reason.contains("有限な正値で直接指定"), "{reason}");
                }
            }
            let reason =
                collect_cft_ultimate_checks(&model, &[(COLUMN, COMPRESSION_N)]).unwrap_err();
            assert!(reason.contains("CFT柱 ID 0"), "{reason}");
            assert!(reason.contains(&format!(": {axis}の座屈長さ")), "{reason}");
        }
    }
}

#[test]
fn cft_fixed_free_column_uses_independent_euler_length_at_both_entries() {
    let mut model = input::cantilever(false, COMPRESSION_N);
    let solved = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).unwrap();
    let (_, forces) = &solved.member_forces[0];
    assert!((forces.at[0].1[0] + COMPRESSION_N).abs() < 0.01);
    for (strong, weak) in [(6000.0, 6000.0), (1000.0, 6000.0), (6000.0, 1000.0)] {
        model.steel_design_attrs = vec![input::direct_lengths(COLUMN, Some(strong), Some(weak))];
        // □400×400×12・Fc24・F235、固定自由の座屈長さ 2L。手計算値はV&V参照。
        for (term, allowable_n) in [
            (LoadTerm::Long, 3_813_102.812_619_05),
            (LoadTerm::Short, 6_285_158.218_928_575),
        ] {
            let report = run_member_design_checks(
                &model,
                &solved.member_forces,
                &solved.panel_moments,
                &MemberDesignCheckOptions {
                    term,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(report.member_checks.len(), 3);
            for (_, _, outcome) in report.member_checks {
                let CheckOutcome::Checked(result) = outcome else {
                    panic!("{outcome:?}");
                };
                assert!((result.ratio() - COMPRESSION_N / allowable_n).abs() < 1e-9);
            }
        }
        let rows = collect_cft_ultimate_checks(&model, &[(COLUMN, COMPRESSION_N)]).unwrap();
        assert_eq!(rows[0].class, CftColumnClass::Long);
        assert!((rows[0].ncu - 7_223_541.809_976_947).abs() < 0.01);
        assert!(!rows[0].ok);
    }
}

#[test]
fn cft_ultimate_bending_uses_strong_axis_length_and_its_class() {
    for circular in [false, true] {
        let mut model = input::cantilever(circular, 100_000.0);
        let measure = |model: &mut sepika_core::model::Model, strong, weak| {
            model.steel_design_attrs =
                vec![input::direct_lengths(COLUMN, Some(strong), Some(weak))];
            collect_cft_ultimate_checks(model, &[(COLUMN, 100_000.0)])
                .unwrap()
                .remove(0)
        };
        let short = measure(&mut model, 1000.0, 1000.0);
        let just_long = measure(&mut model, 1000.0, 4800.1);
        let weak_long = measure(&mut model, 1000.0, 6000.0);
        let strong_long = measure(&mut model, 6000.0, 1000.0);
        assert_eq!(short.class, CftColumnClass::Short);
        assert_eq!(just_long.class, CftColumnClass::Long);
        assert!(just_long.detail.contains("1000・4800.1 mm"));
        assert_eq!(weak_long.class, CftColumnClass::Long);
        assert_eq!(strong_long.class, CftColumnClass::Long);
        assert!(short.ncu > weak_long.ncu);
        assert_eq!(weak_long.ncu, strong_long.ncu);
        let short_mu = short.mu_nm.unwrap();
        assert_eq!(weak_long.mu_nm.unwrap(), short_mu);
        assert!(strong_long.mu_nm.unwrap() < short_mu);
    }
}

#[test]
fn cft_ultimate_uses_the_lower_axial_capacity_across_the_curve_boundary() {
    let mut model = input::cantilever(false, 100_000.0);
    // □400×400×12の鋼管の規準化細長比1.3はlk=19116.1282 mm。
    let shorter = 19_116.127;
    let longer = 19_116.129;
    let capacity = |model: &mut sepika_core::model::Model, strong, weak| {
        model.steel_design_attrs = vec![input::direct_lengths(COLUMN, Some(strong), Some(weak))];
        collect_cft_ultimate_checks(model, &[(COLUMN, 100_000.0)]).unwrap()[0].ncu
    };
    let shorter_capacity = capacity(&mut model, shorter, shorter);
    let longer_capacity = capacity(&mut model, longer, longer);
    assert!(shorter_capacity < longer_capacity);
    assert_eq!(capacity(&mut model, shorter, longer), shorter_capacity);
    assert_eq!(capacity(&mut model, longer, shorter), shorter_capacity);
}
