use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_design_jp::ultimate::{collect_cft_ultimate_checks, CftColumnClass};
use sepika_design_jp::{
    run_member_design_checks, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};

#[path = "fixtures/cft_column_buckling_model.rs"]
mod cft_column_buckling_input;
#[path = "fixtures/cft_ultimate_strength_model.rs"]
mod strength_input;

use cft_column_buckling_input as input;

const COLUMN: ElemId = ElemId(0);
const COMPRESSION_N: f64 = 7_450_000.0;

#[test]
fn cft_ultimate_rejects_concrete_strength_below_statutory_minimum() {
    let check = |model| collect_cft_ultimate_checks(model, &[(COLUMN, 100_000.0)]);
    let below = strength_input::circular_column(23.999);
    let reason = check(&below).unwrap_err();
    for message in [
        "CFT 部材 0",
        "Fc=23.999 N/mm²",
        "告示464号第三2",
        "下限24 N/mm²",
    ] {
        assert!(reason.contains(message), "{reason}");
    }
    assert_eq!(below.materials[0].fc, Some(23.999));

    let at_minimum = strength_input::circular_column(24.0);
    let above = strength_input::circular_column(24.001);
    let at_minimum_rows = check(&at_minimum).unwrap();
    let above_rows = check(&above).unwrap();
    for rows in [&at_minimum_rows, &above_rows] {
        let [row] = rows.as_slice() else {
            panic!("{rows:?}");
        };
        assert_eq!(row.elem, COLUMN);
        assert!(row.ok && row.ncu > 100_000.0 && row.ntu > 0.0);
        assert!(*row.mu_nm.as_ref().unwrap() > 0.0);
    }
    assert!(above_rows[0].ncu > at_minimum_rows[0].ncu);
    assert_eq!(at_minimum.materials[0].fc, Some(24.0));
    assert_eq!(above.materials[0].fc, Some(24.001));
}

#[test]
fn circular_cft_ultimate_checks_compression_and_tension_at_each_class_limit() {
    use sepika_core::{ids::SectionId, section_shape::SectionShape};
    use sepika_design_jp::ultimate::cft_mu_nm;

    for (d, t, lk, class, expected_ncu, expected_ntu, compression_connects) in [
        (
            600.0,
            12.0,
            2400.0,
            CftColumnClass::Short,
            16_966_749.178_760,
            7_204_300.273_212,
            false,
        ),
        (
            480.0,
            12.0,
            1920.0,
            CftColumnClass::Short,
            12_181_600.912_518,
            5_734_034.911_332,
            true,
        ),
        (
            400.0,
            16.0,
            1600.0,
            CftColumnClass::Short,
            11_157_730.733_972,
            6_273_132.210_688,
            true,
        ),
        (
            400.0,
            16.0,
            3600.0,
            CftColumnClass::Medium,
            9_630_388.896_738,
            6_273_132.210_688,
            true,
        ),
        (
            400.0,
            0.001,
            3600.0,
            CftColumnClass::Medium,
            3_635_176.456_625,
            408.406_024,
            false,
        ),
        (
            400.0,
            16.0,
            7200.0,
            CftColumnClass::Long,
            7_342_847.611_110,
            6_273_132.210_688,
            false,
        ),
    ] {
        let shape = SectionShape::CftPipe {
            outer_dia: d,
            thick: t,
        };
        let mut model = input::cantilever(true, 100_000.0);
        model.sections[0] = shape.to_section(SectionId(0), "CFT".into());
        model.sections[0].material = Some(sepika_core::ids::MaterialId(0));
        model.sections[0].steel_material = Some(sepika_core::ids::MaterialId(1));
        model.materials[0].fc = Some(30.0);
        model.materials[1].name = "SN490B".into();
        model.materials[1].fy = Some(325.0);
        model.steel_design_attrs = vec![input::direct_lengths(COLUMN, Some(lk), Some(lk))];

        let check = |n| {
            collect_cft_ultimate_checks(&model, &[(COLUMN, n)])
                .unwrap()
                .remove(0)
        };
        let zero = check(0.0);
        assert_eq!(zero.class, class);
        assert!((zero.ncu - expected_ncu).abs() < 1e-5, "{}", zero.ncu);
        assert!((zero.ntu - expected_ntu).abs() < 1e-5, "{}", zero.ntu);
        for (limit, sign, side, connects) in [
            (zero.ncu, 1.0, "圧縮", compression_connects),
            (-zero.ntu, -1.0, "引張", class != CftColumnClass::Short),
        ] {
            let just_inside = if sign > 0.0 {
                limit.next_down()
            } else {
                limit.next_up()
            };
            let just_outside = if sign > 0.0 {
                limit.next_up()
            } else {
                limit.next_down()
            };
            let inside = check(just_inside);
            assert!(inside.ok && inside.mu_nm.unwrap().is_finite());
            let outer = check(just_outside);
            assert!(!outer.ok);
            let reason = outer.mu_nm.unwrap_err();
            assert!(reason.contains(side) && reason.contains("軸NG"), "{reason}");
            assert!(reason.contains(&format!("座屈長さ {lk} mm、N={just_outside} N")));
            assert!(reason.contains(&format!("={limit} N")));
            assert!(outer.detail.contains(&reason));

            let at = check(limit);
            assert!(at.ok);
            if connects {
                assert_eq!(at.mu_nm.unwrap(), 0.0);
            } else {
                let reason = at.mu_nm.unwrap_err();
                assert!(
                    reason.contains(&format!("純{side}限界")) && reason.contains("接続"),
                    "{reason}"
                );
                assert!(at.detail.contains(&reason));
            }

            for n in [just_inside, limit, just_outside] {
                assert_eq!(cft_mu_nm(&shape, 30.0, 325.0, n, lk, true), check(n).mu_nm);
            }
        }

        if d == 600.0 {
            assert!((check(zero.ncu - 1.0).mu_nm.unwrap() / 1e6 - 4.322_866).abs() < 1e-6);
        } else if class == CftColumnClass::Long {
            assert!((check(zero.ncu - 1.0).mu_nm.unwrap() / 1e6 - 201.813_114).abs() < 1e-6);
        } else if t == 16.0 && class == CftColumnClass::Short {
            assert!((check(-zero.ntu + 1.0).mu_nm.unwrap() / 1e6 - 96.076_754).abs() < 1e-6);
        }
    }
}

#[test]
fn circular_cft_ultimate_bending_does_not_adopt_the_weak_axis_limit() {
    let mut model = input::cantilever(true, 100_000.0);
    let demand = 6_500_000.0;
    for (strong, weak, mu_available) in [(1200.0, 7200.0, true), (7200.0, 1200.0, false)] {
        model.steel_design_attrs = vec![input::direct_lengths(COLUMN, Some(strong), Some(weak))];
        let row = collect_cft_ultimate_checks(&model, &[(COLUMN, demand)])
            .unwrap()
            .remove(0);
        assert_eq!(row.class, CftColumnClass::Long);
        assert!(row.ncu < demand && !row.ok);
        if mu_available {
            assert!(row.mu_nm.unwrap() > 0.0);
        } else {
            assert!(row.mu_nm.unwrap_err().contains("軸圧縮終局耐力"));
        }
    }
}

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
