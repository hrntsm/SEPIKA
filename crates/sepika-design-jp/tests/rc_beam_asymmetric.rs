use sepika_core::ids::ElemId;
use sepika_core::model::MaterialCategory;
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, BondMethod, CheckComponent, LoadTerm, MemberDesignCheckOptions,
    QdMethod, SeismicQd,
};
use sepika_element::frame::beam::MemberForces;
use std::collections::HashMap;

#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod input;
use input::{material, section};
use sepika_design_jp::{CheckKind, CheckOutcome, DesignCheck, DesignCtx, MemberForcesAt, RcDesign};

struct Case {
    name: &'static str,
    top: &'static [u32],
    bottom: &'static [u32],
    negative: bool,
    main_dia_mm: f64,
    width_mm: f64,
    depth_mm: f64,
    ma_t: f64,
    expected: [[f64; 3]; 3],
}

// 固定値は二次方程式の正根から独立に算定した。試験側は各位置の軸力を二分法で釣り合わせる。
const CASES: &[Case] = &[
    Case {
        name: "two_one_shallow",
        top: &[3, 3],
        bottom: &[3],
        negative: false,
        main_dia_mm: 19.0,
        width_mm: 250.0,
        depth_mm: 350.0,
        ma_t: 46484802.2371241,
        expected: [
            [103.48303967, 584663953.776134, 45198823.354266],
            [112.382567651, 543277485.024749, 38673434.5997672],
            [112.382567651, 556711430.99409, 39629735.6525128],
        ],
    },
    Case {
        name: "two_one_shallow",
        top: &[3, 3],
        bottom: &[3],
        negative: true,
        main_dia_mm: 19.0,
        width_mm: 250.0,
        depth_mm: 350.0,
        ma_t: 85368819.2547529,
        expected: [
            [141.495101422, 676809717.007614, 38266185.0597119],
            [137.58392331, 715368773.152387, 41596067.676612],
            [137.58392331, 729762286.690966, 42432997.6429797],
        ],
    },
    Case {
        name: "single",
        top: &[4],
        bottom: &[4],
        negative: false,
        main_dia_mm: 20.0,
        width_mm: 300.0,
        depth_mm: 600.0,
        ma_t: 127658617.478621,
        expected: [
            [177.944061522, 3279059927.29792, 147419808.191124],
            [177.944061522, 3279059927.29792, 147419808.191124],
            [177.944061522, 3279059927.29792, 147419808.191124],
        ],
    },
    Case {
        name: "single",
        top: &[4],
        bottom: &[4],
        negative: true,
        main_dia_mm: 20.0,
        width_mm: 300.0,
        depth_mm: 600.0,
        ma_t: 127658617.478621,
        expected: [
            [177.944061522, 3279059927.29792, 147419808.191124],
            [177.944061522, 3279059927.29792, 147419808.191124],
            [177.944061522, 3279059927.29792, 147419808.191124],
        ],
    },
    Case {
        name: "two_one",
        top: &[4, 4],
        bottom: &[4],
        negative: false,
        main_dia_mm: 20.0,
        width_mm: 300.0,
        depth_mm: 600.0,
        ma_t: 127658617.478621,
        expected: [
            [158.076081897, 3482957996.51718, 176267425.392532],
            [166.637740659, 3324842082.26811, 159620122.98648],
            [166.637740659, 3346833230.84324, 160675881.351334],
        ],
    },
    Case {
        name: "two_one",
        top: &[4, 4],
        bottom: &[4],
        negative: true,
        main_dia_mm: 20.0,
        width_mm: 300.0,
        depth_mm: 600.0,
        ma_t: 243496992.598111,
        expected: [
            [232.072773136, 4648165690.55175, 160231314.608298],
            [228.53674853, 4786980218.83426, 167569732.207394],
            [228.53674853, 4810542163.73618, 168394525.420862],
        ],
    },
    Case {
        name: "three_two",
        top: &[4, 4, 4],
        bottom: &[4, 4],
        negative: false,
        main_dia_mm: 20.0,
        width_mm: 300.0,
        depth_mm: 600.0,
        ma_t: 243496992.598111,
        expected: [
            [198.706868948, 5238442233.60197, 210901304.473084],
            [207.422147254, 4959830960.85667, 191294170.907986],
            [207.422147254, 5071357500.0591, 195595603.158266],
        ],
    },
    Case {
        name: "three_two",
        top: &[4, 4, 4],
        bottom: &[4, 4],
        negative: true,
        main_dia_mm: 20.0,
        width_mm: 300.0,
        depth_mm: 600.0,
        ma_t: 347515125.358469,
        expected: [
            [245.608407946, 5506146966.73783, 179347181.565587],
            [240.264767522, 5762020004.71412, 191855678.688035],
            [240.264767522, 5878258932.89694, 195726039.852738],
        ],
    },
    Case {
        name: "unequal_counts",
        top: &[4, 2],
        bottom: &[4, 4],
        negative: false,
        main_dia_mm: 20.0,
        width_mm: 300.0,
        depth_mm: 600.0,
        ma_t: 243496992.598111,
        expected: [
            [222.289867461, 4825827108.01521, 173676908.017047],
            [220.605015341, 4887672662.00247, 177246112.177158],
            [220.605015341, 4925895372.62115, 178632217.041893],
        ],
    },
    Case {
        name: "unequal_counts",
        top: &[4, 2],
        bottom: &[4, 4],
        negative: true,
        main_dia_mm: 20.0,
        width_mm: 300.0,
        depth_mm: 600.0,
        ma_t: 185577805.038366,
        expected: [
            [190.183003743, 4279431155.93674, 180013190.315365],
            [192.608697964, 4214593195.64182, 175053078.711212],
            [192.608697964, 4252292307.4849, 176618910.877323],
        ],
    },
];

fn beam_check(case: &Case, pos: f64, slab: bool, demand_nmm: f64, shear_n: f64) -> CheckOutcome {
    let ctx = DesignCtx {
        rebar_material: Some(material("SD345", MaterialCategory::Rebar)),
        shear_rebar_material: Some(material("SD345", MaterialCategory::Rebar)),
        beam_has_slab: slab,
        ..Default::default()
    };
    RcDesign.check(
        &MemberForcesAt {
            pos,
            n: 0.0,
            qy: shear_n,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: if case.negative {
                -demand_nmm
            } else {
                demand_nmm
            },
        },
        &section(
            case.width_mm,
            case.depth_mm,
            case.main_dia_mm,
            case.top,
            case.bottom,
        ),
        &material("FC24", MaterialCategory::Concrete),
        &ctx,
    )
}

fn bending(outcome: CheckOutcome) -> CheckComponent {
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}")
    };
    result
        .components
        .into_iter()
        .find(|c| c.kind == CheckKind::Bending)
        .unwrap()
}

fn layers(counts: &[u32], tension: bool, depth_mm: f64, dia_mm: f64) -> Vec<(f64, f64)> {
    counts
        .iter()
        .enumerate()
        .map(|(i, &count)| {
            let from_edge_mm =
                40.0 + 10.0 + dia_mm / 2.0 + (dia_mm + (1.5 * dia_mm).max(25.0)) * i as f64;
            let y_mm = if tension {
                depth_mm - from_edge_mm
            } else {
                from_edge_mm
            };
            (
                y_mm,
                count as f64 * std::f64::consts::PI * dia_mm.powi(2) / 4.0,
            )
        })
        .collect()
}

fn centroid(layers: &[(f64, f64)]) -> (f64, f64) {
    let area_mm2: f64 = layers.iter().map(|&(_, a)| a).sum();
    (
        layers.iter().map(|&(y, a)| y * a).sum::<f64>() / area_mm2,
        area_mm2,
    )
}

fn equilibrium(layers: &[(f64, f64)], width_mm: f64, depth_mm: f64) -> [f64; 3] {
    let transformed_area = |x_mm: f64, y_mm: f64, a_mm2: f64| {
        if y_mm < x_mm {
            14.0 * a_mm2
        } else {
            15.0 * a_mm2
        }
    };
    let residual = |x_mm: f64| {
        width_mm * x_mm * x_mm / 2.0
            + layers
                .iter()
                .map(|&(y, a)| transformed_area(x_mm, y, a) * (x_mm - y))
                .sum::<f64>()
    };
    let (mut lo, mut hi) = (0.0, depth_mm);
    assert!(residual(lo) < 0.0 && residual(hi) > 0.0);
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if residual(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let x_mm = (lo + hi) / 2.0;
    assert!(residual(x_mm).abs() < 1e-6);
    let icr_mm4 = width_mm * x_mm.powi(3) / 3.0
        + layers
            .iter()
            .map(|&(y, a)| transformed_area(x_mm, y, a) * (x_mm - y).powi(2))
            .sum::<f64>();
    [x_mm, icr_mm4, 8.0 * icr_mm4 / x_mm]
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-8 * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

#[test]
fn asymmetric_beams_compare_virtual_position_real_centroids_and_each_layer() {
    for case in CASES {
        let (tension, compression) = if case.negative {
            (case.top, case.bottom)
        } else {
            (case.bottom, case.top)
        };
        let t_layers = layers(tension, true, case.depth_mm, case.main_dia_mm);
        let c_layers = layers(compression, false, case.depth_mm, case.main_dia_mm);
        let (d_mm, at_mm2) = centroid(&t_layers);
        let (dc_mm, ac_mm2) = centroid(&c_layers);
        let virtual_layers = [(case.depth_mm - d_mm, ac_mm2), (d_mm, at_mm2)];
        let real_centroids = [(dc_mm, ac_mm2), (d_mm, at_mm2)];
        let real_layers: Vec<_> = c_layers.iter().chain(&t_layers).copied().collect();
        let values = [
            equilibrium(&virtual_layers, case.width_mm, case.depth_mm),
            equilibrium(&real_centroids, case.width_mm, case.depth_mm),
            equilibrium(&real_layers, case.width_mm, case.depth_mm),
        ];
        for (actual, expected) in values.iter().zip(case.expected) {
            for (&a, e) in actual.iter().zip(expected) {
                close(a, e);
            }
        }
        let x_mm = values[2][0];
        assert!(c_layers.iter().all(|&(y, _)| y < x_mm));
        assert!(t_layers.iter().all(|&(y, _)| y > x_mm));
        close(values[1][0], values[2][0]);
        let dispersion_mm4 = c_layers
            .iter()
            .map(|&(y, a)| 14.0 * a * (y - dc_mm).powi(2))
            .sum::<f64>()
            + t_layers
                .iter()
                .map(|&(y, a)| 15.0 * a * (y - d_mm).powi(2))
                .sum::<f64>();
        close(values[2][1] - values[1][1], dispersion_mm4);
        close(at_mm2 * 215.0 * 7.0 * d_mm / 8.0, case.ma_t);
        for (pos, slab) in [(0.0, false), (0.5, false), (1.0, true), (0.5, true)] {
            let outcome = beam_check(case, pos, slab, 200e6, 0.0);
            if pos == 0.5 && slab && !case.negative {
                let component = bending(outcome);
                check_capacity(&component, 200e6, case.ma_t, None);
                assert!(component.detail.contains("中央+スラブ正曲げ"));
            } else if case.name == "single" {
                check_capacity(
                    &bending(outcome),
                    200e6,
                    case.ma_t,
                    Some(case.expected[0][2]),
                );
            } else {
                assert!(
                    matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("重心までの距離が一致しません"))
                );
            }
        }
    }
}

#[test]
fn standard_asymmetric_beam_is_unchecked_at_the_previous_passing_moment() {
    let case = CASES
        .iter()
        .find(|c| c.name == "two_one_shallow" && !c.negative)
        .unwrap();
    assert!(sepika_core::section_shape::StandardRebar::ALL
        .iter()
        .any(|size| size.designation_number() == case.main_dia_mm));
    assert!(sepika_core::material_grade::MAIN_REBAR_GRADES.contains(&"SD345"));
    assert!(45e6 / case.ma_t.min(case.expected[0][2]) < 1.0);
    assert!(
        matches!(beam_check(case, 0.0, false, 45e6, 0.0), CheckOutcome::Skipped { reason } if reason.contains("正曲げ") && reason.contains("重心までの距離が一致しません"))
    );
    assert!(45e6 / case.ma_t.min(case.expected[1][2]) > 1.0);
    assert!(45e6 / case.ma_t.min(case.expected[2][2]) > 1.0);
    let reverse = CASES
        .iter()
        .find(|c| c.name == "two_one_shallow" && c.negative)
        .unwrap();
    assert!(reverse.expected[0][2] < reverse.expected[1][2]);
    let masked = CASES
        .iter()
        .find(|c| c.name == "two_one" && !c.negative)
        .unwrap();
    assert!(masked.expected.iter().all(|v| v[2] > masked.ma_t));
}

fn check_capacity(
    component: &CheckComponent,
    demand_nmm: f64,
    ma_t_nmm: f64,
    ma_c_nmm: Option<f64>,
) {
    let ma_nmm = ma_c_nmm.map_or(ma_t_nmm, |ma_c_nmm| ma_t_nmm.min(ma_c_nmm));
    close(component.ratio, demand_nmm / ma_nmm);
    let shown = |key: &str| {
        component.detail.split(key).nth(1).map(|value| {
            value
                .split_whitespace()
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        })
    };
    for (key, expected) in [
        ("MA_t=", Some(ma_t_nmm)),
        ("MA_c=", ma_c_nmm),
        ("MA=", Some(ma_nmm)),
    ] {
        match expected {
            Some(expected) => assert!(
                (shown(key).unwrap() - expected).abs() <= 0.051,
                "{}",
                component.detail
            ),
            None => assert!(shown(key).is_none(), "{}", component.detail),
        }
    }
}

#[test]
fn equal_centroid_beams_keep_tension_and_compression_controlled_capacities() {
    // 圧縮縁を基準にした独立釣合いと、各側の重心から求めた固定期待値。
    for (top, bottom, signed_expected) in [
        (
            &[3, 3][..],
            &[3, 3][..],
            [(165_377_084.723_124_9, 174_596_511.723_861_25); 2],
        ),
        (
            &[2, 2][..],
            &[3, 3][..],
            [
                (165_377_084.723_124_9, 165_022_113.044_584_36),
                (110_251_389.815_416_6, 150_205_067.501_363_5),
            ],
        ),
        (
            &[2, 6][..],
            &[2, 1, 1][..],
            [
                (107_717_794.742_251_49, 145_175_388.758_810_5),
                (215_435_589.484_502_97, 171_366_419.468_029_08),
            ],
        ),
    ] {
        for (negative, (ma_t, ma_c)) in [false, true].into_iter().zip(signed_expected) {
            let case = Case {
                name: "same_centroid",
                top,
                bottom,
                negative,
                main_dia_mm: 19.0,
                width_mm: 400.0,
                depth_mm: 600.0,
                ma_t,
                expected: [[0.0; 3]; 3],
            };
            let outcome = beam_check(&case, 0.0, false, 150e6, 0.0);
            check_capacity(&bending(outcome), 150e6, ma_t, Some(ma_c));
        }
    }
}

#[test]
fn equal_centroids_with_bars_crossing_the_neutral_axis_are_unchecked() {
    // 上下対称でも、重心集約の中立軸102.154 mmを2段目107 mmと3段目154.5 mmが越える。
    for negative in [false, true] {
        let case = Case {
            name: "crosses_neutral_axis",
            top: &[1, 1, 1],
            bottom: &[1, 1, 1],
            negative,
            main_dia_mm: 19.0,
            width_mm: 600.0,
            depth_mm: 450.0,
            ma_t: 54_885_670.111_303_18,
            expected: [[0.0; 3]; 3],
        };
        assert!(
            matches!(beam_check(&case, 0.0, false, 45e6, 0.0), CheckOutcome::Skipped { reason } if reason.contains("中立軸"))
        );
        if !negative {
            check_capacity(
                &bending(beam_check(&case, 0.5, true, 45e6, 0.0)),
                45e6,
                case.ma_t,
                None,
            );
        }
    }
}

#[test]
fn singly_reinforced_beams_require_steel_on_the_tension_side() {
    // 単筋の独立釣合いは中立軸128.564449 mm、コンクリート支配のMA 31.838367 kN·m。
    for negative in [false, true] {
        let (top, bottom): (&[u32], &[u32]) = if negative { (&[3], &[]) } else { (&[], &[3]) };
        let case = Case {
            name: "singly_reinforced",
            top,
            bottom,
            negative,
            main_dia_mm: 19.0,
            width_mm: 250.0,
            depth_mm: 350.0,
            ma_t: 46_484_802.237_124_13,
            expected: [[0.0; 3]; 3],
        };
        check_capacity(
            &bending(beam_check(&case, 0.0, false, 45e6, 0.0)),
            45e6,
            case.ma_t,
            Some(31_838_366.621_351_764),
        );
        let opposite = Case {
            negative: !negative,
            ..case
        };
        for (pos, slab) in [(0.0, false), (0.5, true)] {
            assert!(matches!(beam_check(&opposite, pos, slab, 45e6, 0.0),
                CheckOutcome::Skipped { reason } if reason.contains("引張側の主筋がありません")));
        }
    }
}

#[test]
fn singly_reinforced_beam_with_tension_layers_crossing_the_neutral_axis_is_unchecked() {
    // 引張側最内段が圧縮縁から174 mm、中立軸が176.238156 mm。
    let case = Case {
        name: "tension_layers_cross_neutral_axis",
        top: &[],
        bottom: &[2, 2],
        negative: false,
        main_dia_mm: 41.0,
        width_mm: 250.0,
        depth_mm: 347.0,
        ma_t: 0.0,
        expected: [[0.0; 3]; 3],
    };
    assert!(matches!(beam_check(&case, 0.0, false, 45e6, 0.0),
        CheckOutcome::Skipped { reason } if reason.contains("中立軸")));
}

#[test]
fn without_bending_demand_asymmetric_beam_retains_shear_check_without_capacity() {
    let CheckOutcome::Checked(result) = beam_check(&CASES[0], 0.0, false, 0.0, 20_000.0) else {
        panic!("零曲げはせん断検定を妨げない")
    };
    assert!(result
        .components
        .iter()
        .any(|c| c.kind == CheckKind::Shear && c.ratio > 0.0));
    let bending = result
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Bending)
        .unwrap();
    assert_eq!(bending.ratio, 0.0);
    assert!(!bending.detail.contains("MA"));
}

#[test]
fn common_member_check_runner_preserves_unchecked_stations_with_zero_bending_at_one_end() {
    let model = input::model(true);
    let forces = MemberForces {
        at: vec![
            (0.0, [0.0, 20_000.0, 0.0, 0.0, 0.0, 45e6]),
            (0.5, [0.0, 20_000.0, 0.0, 0.0, 0.0, 45e6]),
            (1.0, [0.0, 20_000.0, 0.0, 0.0, 0.0, 0.0]),
        ],
    };
    let report = run_member_design_checks(
        &model,
        &[(ElemId(0), forces)],
        &[],
        &MemberDesignCheckOptions::default(),
    )
    .unwrap();
    assert_eq!(report.member_checks.len(), 3);
    for (_, pos, outcome) in report.member_checks {
        if pos == 1.0 {
            assert!(
                matches!(outcome, CheckOutcome::Checked(result) if result.components.iter().any(|c| c.kind == CheckKind::Shear && c.ratio > 0.0))
            );
        } else {
            assert!(
                matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("正曲げ") && reason.contains("重心までの距離が一致しません"))
            );
        }
    }
}

#[test]
fn common_member_check_retains_only_independent_failures_when_beam_bending_is_unchecked() {
    let mut model = input::model(true);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        unreachable!()
    };
    rebar.stirrup.pitch = 350.0;
    let at = [0.0, 0.5, 1.0];
    let forces = [(
        ElemId(0),
        MemberForces {
            at: at
                .map(|pos| (pos, [0.0, 150_000.0, 0.0, 0.0, 0.0, 45e6]))
                .to_vec(),
        },
    )];
    let gravity = [(
        ElemId(0),
        MemberForces {
            at: at
                .map(|pos| (pos, [0.0, -20_000.0, 0.0, 0.0, 0.0, 0.0]))
                .to_vec(),
        },
    )];
    let other_member_gravity = [(ElemId(1), gravity[0].1.clone())];
    let other_station_gravity = [(
        ElemId(0),
        MemberForces {
            at: vec![(0.2, [0.0; 6])],
        },
    )];
    let q0 = HashMap::from([(ElemId(0), 20_000.0)]);
    for (term, method, long, q0, expected, reference_reason) in [
        (
            LoadTerm::Long,
            QdMethod::Min,
            None,
            None,
            Some((1.643_190_723, 1.426_593_895)),
            None,
        ),
        (
            LoadTerm::Short,
            QdMethod::Qd2,
            None,
            None,
            Some((1.095_460_482, 0.951_062_597)),
            None,
        ),
        (
            LoadTerm::Short,
            QdMethod::Qd2,
            Some(gravity.as_slice()),
            None,
            Some((2.008_344_217, 1.743_614_760)),
            None,
        ),
        (
            LoadTerm::Short,
            QdMethod::Qd1,
            Some(gravity.as_slice()),
            Some(&q0),
            None,
            None,
        ),
        (
            LoadTerm::Short,
            QdMethod::Min,
            Some(gravity.as_slice()),
            Some(&q0),
            None,
            None,
        ),
        (
            LoadTerm::Short,
            QdMethod::Qd2,
            Some(other_member_gravity.as_slice()),
            None,
            None,
            Some("重力参照"),
        ),
        (
            LoadTerm::Short,
            QdMethod::Qd2,
            Some(other_station_gravity.as_slice()),
            None,
            None,
            Some("重力参照"),
        ),
        (
            LoadTerm::Short,
            QdMethod::Qd1,
            Some(gravity.as_slice()),
            None,
            None,
            Some("Q0"),
        ),
        (
            LoadTerm::Short,
            QdMethod::Min,
            Some(gravity.as_slice()),
            None,
            None,
            Some("Q0"),
        ),
    ] {
        let report = run_member_design_checks(
            &model,
            &forces,
            &[],
            &MemberDesignCheckOptions {
                term,
                qd_method: method,
                long_member_forces: long,
                q_simple_by_elem: q0,
                bond_method: BondMethod::Rc1991,
                rc_damage_control: false,
                ..Default::default()
            },
        )
        .unwrap();
        for pos in at {
            let mut components = vec![];
            let mut reasons = vec![];
            for (_, _, outcome) in report.member_checks.iter().filter(|(_, p, _)| *p == pos) {
                match outcome {
                    CheckOutcome::Checked(result) => {
                        assert!(!result.ok());
                        components.extend(&result.components);
                    }
                    CheckOutcome::Skipped { reason } => reasons.push(reason),
                }
            }
            assert_eq!(reasons.len(), 1);
            assert!(reasons[0].contains("正曲げ") && reasons[0].contains("重心までの距離"));
            if let Some(reference) = reference_reason {
                assert!(reasons[0].contains(reference), "{reasons:?}");
            }
            let mut expected_components = vec![];
            if let Some((shear, bond)) = expected {
                expected_components.push((CheckKind::Shear, shear));
                if bond > 1.0 {
                    expected_components.push((CheckKind::Bond, bond));
                }
            }
            if pos == 0.5 {
                expected_components.push((CheckKind::Provision, 1.01));
            }
            assert_eq!(
                components.len(),
                expected_components.len(),
                "{components:?}"
            );
            for (component, (kind, ratio)) in components.into_iter().zip(expected_components) {
                assert_eq!(component.kind, kind);
                assert!((component.ratio - ratio).abs() < 1e-8, "{component:?}");
                if kind == CheckKind::Provision {
                    assert!(component.detail.contains("あばら間隔 350 mm"));
                }
            }
        }
    }
}

#[test]
fn simplified_beam_check_keeps_1999_bond_failure_with_bending_and_reference_issues() {
    let model = input::model(true);
    let section = &model.sections[0];
    let concrete = &model.materials[0];
    let rebar = &model.materials[1];
    let mut forces = MemberForcesAt::from_local(0.5, [0.0, 150_000.0, 0.0, 10.0, 0.0, 300e6]);
    let mut ctx = DesignCtx {
        length: 3000.0,
        term: LoadTerm::Short,
        rebar_material: Some(rebar.clone()),
        shear_rebar_material: Some(rebar.clone()),
        bond_method: BondMethod::Rc1999,
        rc_damage_control: false,
        seismic_qd: Some(SeismicQd {
            method: QdMethod::Min,
            long_at: vec![(0.5, [0.0; 6])],
            n_factor: 1.5,
            n_mechanism: 1.0,
            clear_length: 3000.0,
            q_simple: None,
        }),
        ..Default::default()
    };
    for (qy, missing_gravity, reference) in [
        (150_000.0, false, "Q0"),
        (150_000.0, true, "重力参照"),
        (f64::NAN, false, "Q0"),
    ] {
        forces.qy = qy;
        let results: Vec<_> = RcDesign
            .check_outcomes(&forces, section, concrete, &ctx, missing_gravity)
            .collect();
        let [CheckOutcome::Checked(ng), CheckOutcome::Skipped { reason }] = results.as_slice()
        else {
            panic!("{results:?}")
        };
        assert_eq!(ng.components.len(), 1);
        assert_eq!(ng.components[0].kind, CheckKind::Bond);
        assert!((ng.components[0].ratio - 2.254_053_998).abs() < 1e-8);
        assert!(ng.components[0].detail.contains("1999"));
        for expected in ["重心までの距離", "ねじり", reference] {
            assert!(reason.contains(expected), "{reason}");
        }
        if !qy.is_finite() {
            assert!(reason.contains("解析せん断力Qyが非有限"), "{reason}");
        }
    }
    forces.qy = 150_000.0;
    assert!(matches!(
        RcDesign.check(&forces, section, concrete, &ctx),
        CheckOutcome::Skipped { reason } if reason.contains("重心までの距離") && reason.contains("Q0")
    ));
    for (mz, length) in [
        (f64::NAN, 3000.0),
        (f64::INFINITY, 3000.0),
        (300e6, f64::INFINITY),
    ] {
        forces.mz = mz;
        ctx.length = length;
        let results: Vec<_> = RcDesign
            .check_outcomes(&forces, section, concrete, &ctx, false)
            .collect();
        assert!(
            matches!(results.as_slice(), [CheckOutcome::Skipped { .. }]),
            "{results:?}"
        );
    }
}

#[test]
fn simplified_beam_check_does_not_infer_shear_failure_without_tension_bars() {
    let model = input::model(true);
    let section = section(250.0, 350.0, 19.0, &[3], &[]);
    let concrete = &model.materials[0];
    let rebar = &model.materials[1];
    let forces = MemberForcesAt::from_local(0.0, [0.0, 1e6, 0.0, 0.0, 0.0, 45e6]);
    let ctx = DesignCtx {
        length: 3000.0,
        rebar_material: Some(rebar.clone()),
        shear_rebar_material: Some(rebar.clone()),
        bond_method: BondMethod::Rc1991,
        ..Default::default()
    };
    let results: Vec<_> = RcDesign
        .check_outcomes(&forces, &section, concrete, &ctx, false)
        .collect();
    assert!(
        matches!(results.as_slice(), [CheckOutcome::Skipped { reason }] if reason.contains("引張側の主筋がありません")),
        "{results:?}"
    );
}

#[test]
fn common_member_check_with_invalid_stirrup_spacing_keeps_only_independent_bond_failures() {
    let mut model = input::model(true);
    let forces = [(
        ElemId(0),
        MemberForces {
            at: vec![(0.5, [0.0, 300_000.0, 0.0, 0.0, 0.0, 300e6])],
        },
    )];
    for pitch in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
            unreachable!()
        };
        rebar.stirrup.pitch = pitch;
        for (term, bond_method, bond_failure) in [
            (LoadTerm::Long, BondMethod::Rc1991, true),
            (LoadTerm::Long, BondMethod::Rc1999, true),
            (LoadTerm::Short, BondMethod::Rc1999, false),
        ] {
            let report = run_member_design_checks(
                &model,
                &forces,
                &[],
                &MemberDesignCheckOptions {
                    term,
                    bond_method,
                    ..Default::default()
                },
            )
            .unwrap();
            let (unrated, checked): (Vec<_>, Vec<_>) = report
                .member_checks
                .iter()
                .map(|(_, _, outcome)| outcome)
                .partition(|outcome| matches!(outcome, CheckOutcome::Skipped { .. }));
            assert!(
                matches!(unrated.as_slice(),
                [CheckOutcome::Skipped { reason }] if reason.contains("重心までの距離")
                    && reason.contains("あばら筋間隔が非有限")),
                "{unrated:?}"
            );
            if bond_failure {
                assert!(
                    matches!(checked.as_slice(),
                    [CheckOutcome::Checked(result)] if result.components.len() == 1
                        && result.components[0].kind == CheckKind::Bond && !result.ok()),
                    "{checked:?}"
                );
            } else {
                assert!(checked.is_empty(), "{checked:?}");
            }
        }
    }
}

const NONFINITE_STIRRUP_PITCHES: [f64; 3] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];

fn symmetric_beam_outcomes(
    pitch_mm: f64,
    legs: u32,
    mz_nmm: f64,
    qy_n: f64,
    options: &MemberDesignCheckOptions<'_>,
) -> Vec<CheckOutcome> {
    let mut model = input::model(false);
    model.nodes[1].restraint = sepika_core::dof::Dof6Mask::FIXED;
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        unreachable!()
    };
    rebar.stirrup.pitch = pitch_mm;
    rebar.stirrup.legs = legs;
    let report = run_member_design_checks(
        &model,
        &[(
            ElemId(0),
            MemberForces {
                at: vec![(0.5, [0.0, qy_n, 0.0, 0.0, 0.0, mz_nmm])],
            },
        )],
        &[],
        options,
    )
    .unwrap();
    report
        .member_checks
        .into_iter()
        .map(|(member, pos, outcome)| {
            assert_eq!(member, ElemId(0));
            assert_eq!(pos, 0.5);
            outcome
        })
        .collect()
}

fn assert_components(components: &[CheckComponent], expected: &[(CheckKind, f64)]) {
    assert_eq!(components.len(), expected.len(), "{components:?}");
    for (component, &(kind, ratio)) in components.iter().zip(expected) {
        assert_eq!(component.kind, kind);
        assert!((component.ratio - ratio).abs() < 1e-8, "{component:?}");
    }
}

#[test]
fn symmetric_beam_with_nonfinite_stirrup_spacing_is_unchecked_with_only_independent_ng() {
    let options = MemberDesignCheckOptions {
        term: LoadTerm::Short,
        rc_damage_control: true,
        ..Default::default()
    };

    // 強軸許容曲げ 74.591892 kN·m、許容せん断 139.730012 kN（間隔100 mm）／92.778438 kN（0 mm）。
    for (pitch_mm, shear, bond) in [
        (100.0, 1.431_331_730, 0.338_108_100),
        (0.0, 2.155_673_294, 0.387_398_027),
    ] {
        let outcomes = symmetric_beam_outcomes(pitch_mm, 2, 45e6, 200_000.0, &options);
        let [CheckOutcome::Checked(result)] = outcomes.as_slice() else {
            panic!("{pitch_mm}: {outcomes:?}")
        };
        assert_components(
            &result.components,
            &[
                (CheckKind::Bending, 0.603_282_727),
                (CheckKind::Shear, shear),
                (CheckKind::Bond, bond),
            ],
        );
    }

    for pitch_mm in NONFINITE_STIRRUP_PITCHES {
        let outcomes = symmetric_beam_outcomes(pitch_mm, 2, 45e6, 200_000.0, &options);
        assert!(
            matches!(outcomes.as_slice(), [CheckOutcome::Skipped { reason }]
                if reason.contains("あばら筋間隔が非有限")),
            "{pitch_mm}: {outcomes:?}"
        );
        let outcomes = symmetric_beam_outcomes(pitch_mm, 2, 300e6, 300_000.0, &options);
        let [CheckOutcome::Checked(ng), CheckOutcome::Skipped { reason }] = outcomes.as_slice()
        else {
            panic!("{pitch_mm}: {outcomes:?}")
        };
        assert!(reason.contains("あばら筋間隔が非有限"), "{reason}");
        assert!(!ng.ok() && !ng.detail.contains("あばら間隔"), "{ng:?}");
        assert_components(&ng.components, &[(CheckKind::Bending, 4.021_884_847)]);
    }
}

#[test]
fn symmetric_beam_with_nonfinite_stirrup_spacing_retains_spacing_independent_bond() {
    let gravity = [(
        ElemId(0),
        MemberForces {
            at: vec![(0.5, [0.0; 6])],
        },
    )];
    let q0 = HashMap::from([(ElemId(0), 150_000.0)]);
    for (term, method, long_member_forces, q_simple_by_elem, mz, qy, expected) in [
        (
            LoadTerm::Short,
            BondMethod::Rc1991,
            None,
            None,
            45e6,
            200_000.0,
            vec![(CheckKind::Bond, 1.268_083_462)],
        ),
        (
            LoadTerm::Short,
            BondMethod::Rc1991,
            Some(gravity.as_slice()),
            Some(&q0),
            45e6,
            300_000.0,
            vec![(CheckKind::Bond, 1.275_366_859)],
        ),
        (
            LoadTerm::Long,
            BondMethod::Rc1999,
            None,
            None,
            300e6,
            300_000.0,
            vec![
                (CheckKind::Bending, 7.657_483_123),
                (CheckKind::Bond, 3.873_980_265),
            ],
        ),
    ] {
        let options = MemberDesignCheckOptions {
            term,
            bond_method: method,
            qd_method: QdMethod::Qd1,
            long_member_forces,
            q_simple_by_elem,
            ..Default::default()
        };
        for pitch_mm in NONFINITE_STIRRUP_PITCHES {
            let outcomes = symmetric_beam_outcomes(pitch_mm, 2, mz, qy, &options);
            let [CheckOutcome::Checked(ng), CheckOutcome::Skipped { reason }] = outcomes.as_slice()
            else {
                panic!("{pitch_mm}: {outcomes:?}")
            };
            assert!(reason.contains("あばら筋間隔が非有限"), "{reason}");
            assert!(!ng.ok(), "{ng:?}");
            assert_components(&ng.components, &expected);
        }
    }
}

#[test]
fn symmetric_beam_without_stirrups_retains_concrete_shear_and_1999_bond() {
    let options = MemberDesignCheckOptions {
        term: LoadTerm::Short,
        rc_damage_control: true,
        ..Default::default()
    };
    // 0脚では短期せん断の補強項と、短期1999付着の横補強筋効果 W がともに0。
    for pitch_mm in [100.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let outcomes = symmetric_beam_outcomes(pitch_mm, 0, 300e6, 300_000.0, &options);
        let result = match (pitch_mm.is_finite(), outcomes.as_slice()) {
            (true, [CheckOutcome::Checked(result)]) => result,
            (false, [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }])
                if reason.contains("あばら筋間隔が非有限") =>
            {
                result
            }
            _ => panic!("{pitch_mm}: {outcomes:?}"),
        };
        assert!(!result.ok(), "{result:?}");
        assert_components(
            &result.components,
            &[
                (CheckKind::Bending, 4.021_884_847),
                (CheckKind::Shear, 6.467_019_883),
                (CheckKind::Bond, 2.582_653_510),
            ],
        );
    }
}

#[test]
fn simplified_zero_bending_beam_reports_unavailable_shear_instead_of_a_pass() {
    let model = input::model(true);
    let section = section(250.0, 350.0, 19.0, &[3], &[]);
    let concrete = &model.materials[0];
    let rebar = &model.materials[1];
    let mut ctx = DesignCtx {
        length: 3000.0,
        rebar_material: Some(rebar.clone()),
        shear_rebar_material: Some(rebar.clone()),
        ..Default::default()
    };
    for (qy, reason) in [
        (f64::NAN, "解析せん断力Qyが非有限"),
        (f64::INFINITY, "解析せん断力Qyが非有限"),
        (1e6, "引張側に主筋がなく"),
    ] {
        let forces = MemberForcesAt::from_local(0.0, [0.0, qy, 0.0, 0.0, 0.0, 0.0]);
        let results: Vec<_> = RcDesign
            .check_outcomes(&forces, &section, concrete, &ctx, false)
            .collect();
        assert!(
            matches!(results.as_slice(),
            [CheckOutcome::Skipped { reason: actual }] if actual.contains(reason)),
            "{results:?}"
        );
    }

    let zero = MemberForcesAt::from_local(0.0, [0.0; 6]);
    assert!(matches!(RcDesign.check(&zero, &section, concrete, &ctx),
        CheckOutcome::Checked(result) if result.ok()));
    ctx.term = LoadTerm::Short;
    ctx.rc_damage_control = false;
    ctx.seismic_qd = Some(SeismicQd {
        method: QdMethod::Qd2,
        long_at: vec![(0.0, [0.0, -20_000.0, 0.0, 0.0, 0.0, 0.0])],
        n_factor: 1.5,
        n_mechanism: 1.0,
        clear_length: 3000.0,
        q_simple: None,
    });
    assert!(matches!(RcDesign.check(&zero, &section, concrete, &ctx),
        CheckOutcome::Skipped { reason } if reason.contains("引張側に主筋がなく")));
}

#[test]
fn round_main_rebar_holds_bond_but_retains_bending_and_shear_failures() {
    let mut model = input::model(false);
    model.nodes[1].coord[0] = 1500.0;
    model.nodes[1].restraint = sepika_core::dof::Dof6Mask::FIXED;
    model.elements[0].rigid_zone.face_i = Some(250.0);
    model.elements[0].rigid_zone.face_j = Some(250.0);
    model.sections[0] = section(400.0, 600.0, 20.0, &[4], &[4]);
    let mut main = material("SR235", MaterialCategory::Rebar);
    main.id = sepika_core::ids::MaterialId(2);
    model.sections[0].rebar_material = Some(main.id);
    model.materials.push(main);

    let face_i = 250.0 / 1500.0;
    for (term, mz, qy) in [
        (LoadTerm::Long, 90e6, 240_000.0),
        (LoadTerm::Short, 136e6, 240_000.0),
    ] {
        for method in [BondMethod::Rc1999, BondMethod::Rc1991] {
            let options = MemberDesignCheckOptions {
                term,
                rc_damage_control: true,
                bond_method: method,
                ..Default::default()
            };
            let outcomes = |model: &sepika_core::model::Model, mz, qy| {
                run_member_design_checks(
                    model,
                    &[(
                        ElemId(0),
                        MemberForces {
                            at: vec![(face_i, [0.0, qy, 0.0, 0.0, 0.0, mz])],
                        },
                    )],
                    &[],
                    &options,
                )
                .unwrap()
                .member_checks
                .into_iter()
                .filter_map(|(_, pos, outcome)| (pos == face_i).then_some(outcome))
                .collect::<Vec<_>>()
            };
            let time_history = |model: &sepika_core::model::Model, mz, qy| {
                let ctx = DesignCtx {
                    term,
                    bond_method: method,
                    rc_damage_control: true,
                    length: 1500.0,
                    clear_length: Some(1000.0),
                    rebar_material: Some(model.materials[2].clone()),
                    shear_rebar_material: Some(model.materials[1].clone()),
                    ..Default::default()
                };
                RcDesign
                    .check_time_history(
                        &MemberForcesAt::from_local(face_i, [0.0, qy, 0.0, 0.0, 0.0, mz]),
                        &model.sections[0],
                        &model.materials[0],
                        &ctx,
                        false,
                        None,
                    )
                    .collect::<Vec<_>>()
            };
            for grade in ["SR235", "SR295"] {
                model.materials[2] = material(grade, MaterialCategory::Rebar);
                model.materials[2].id = sepika_core::ids::MaterialId(2);
                let is_round_bond_reason = |reason: &str| {
                    reason.contains("丸鋼主筋")
                        && reason.contains("端部フック")
                        && reason.contains("付着長さ")
                };
                let reference = outcomes(&model, mz, qy);
                assert!(
                    matches!(reference.last(), Some(CheckOutcome::Skipped { reason }) if is_round_bond_reason(reason)),
                    "{grade} {term:?} {method:?}: {reference:?}"
                );
                assert!(reference.iter().all(|outcome| match outcome {
                    CheckOutcome::Checked(result) =>
                        !result.ok() && result.components.iter().all(|c| c.kind != CheckKind::Bond),
                    CheckOutcome::Skipped { .. } => true,
                }));

                let light = (1e6, 1_000.0);
                let heavy = (400e6, 600_000.0);
                for (entry, light, heavy) in [
                    (
                        "通常検定",
                        outcomes(&model, light.0, light.1),
                        outcomes(&model, heavy.0, heavy.1),
                    ),
                    (
                        "時刻歴簡易検定",
                        time_history(&model, light.0, light.1),
                        time_history(&model, heavy.0, heavy.1),
                    ),
                ] {
                    assert!(
                        matches!(light.as_slice(), [CheckOutcome::Skipped { reason }] if is_round_bond_reason(reason)),
                        "{entry} {grade} {term:?} {method:?}: {light:?}"
                    );
                    let [CheckOutcome::Checked(ng), CheckOutcome::Skipped { reason }] =
                        heavy.as_slice()
                    else {
                        panic!("{entry} {grade} {term:?} {method:?}: {heavy:?}");
                    };
                    assert!(is_round_bond_reason(reason));
                    assert!(!ng.ok());
                    assert_eq!(
                        ng.components.iter().map(|c| c.kind).collect::<Vec<_>>(),
                        [CheckKind::Bending, CheckKind::Shear],
                    );
                    assert!(ng.components.iter().all(|c| c.ratio > 1.0));
                }
            }

            let mut deformed = model.clone();
            deformed.sections[0].rebar_material = Some(deformed.materials[1].id);
            let control = outcomes(&deformed, mz, qy);
            assert!(
                matches!(control.as_slice(), [CheckOutcome::Checked(result)]
                 if result.components.iter().any(|c| c.kind == CheckKind::Bond && c.ratio > 0.0)),
                "{term:?} {method:?}: {control:?}"
            );
        }
    }
}
