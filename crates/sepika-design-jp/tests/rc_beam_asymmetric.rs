use sepika_core::ids::ElemId;
use sepika_core::model::MaterialCategory;
use sepika_design_jp::{run_member_design_checks, CheckComponent, MemberDesignCheckOptions};
use sepika_element::frame::beam::MemberForces;

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
    assert!(sepika_core::material_grade::REBAR_NOMINAL_SIZES.contains(&case.main_dia_mm));
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
