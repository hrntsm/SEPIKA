use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
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

fn material(name: &str, category: MaterialCategory) -> Material {
    let standard = sepika_core::standard_material::standard_material_properties(name).unwrap();
    Material {
        id: MaterialId(if category == MaterialCategory::Concrete {
            0
        } else {
            1
        }),
        name: name.into(),
        category,
        young: standard.young,
        poisson: standard.poisson,
        density: standard.density,
        shear: None,
        fc: standard.fc,
        fy: standard.fy,
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

fn section(case: &Case) -> sepika_core::model::Section {
    let shape = SectionShape::RcBeamRect {
        b: case.width_mm,
        d: case.depth_mm,
        rebar: RcBeamRebar {
            main_dia: case.main_dia_mm,
            top: case.top.to_vec(),
            bottom: case.bottom.to_vec(),
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    if let SectionShape::RcBeamRect { b, d, rebar } = &shape {
        rebar.validate(*b, *d).unwrap();
    }
    let mut section = shape.to_section(SectionId(0), "G1".into());
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section.frame_use = section.rc_frame_use();
    section
}

fn bending(case: &Case, pos: f64, slab: bool, demand_nmm: f64) -> sepika_design_jp::CheckComponent {
    let ctx = DesignCtx {
        rebar_material: Some(material("SD345", MaterialCategory::Rebar)),
        shear_rebar_material: Some(material("SD345", MaterialCategory::Rebar)),
        beam_has_slab: slab,
        ..Default::default()
    };
    let outcome = RcDesign.check(
        &MemberForcesAt {
            pos,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            my: 0.0,
            mz: if case.negative {
                -demand_nmm
            } else {
                demand_nmm
            },
        },
        &section(case),
        &material("FC24", MaterialCategory::Concrete),
        &ctx,
    );
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
        let current_ma = case.ma_t.min(case.expected[0][2]);
        for (pos, slab) in [(0.0, false), (0.5, false), (0.0, true), (0.5, true)] {
            let expected_ma = if pos == 0.5 && slab && !case.negative {
                case.ma_t
            } else {
                current_ma
            };
            let component = bending(case, pos, slab, 200e6);
            close(component.ratio, 200e6 / expected_ma);
            for (key, expected) in [
                ("MA_t=", case.ma_t),
                ("MA_c=", case.expected[0][2]),
                ("MA=", expected_ma),
            ] {
                let shown: f64 = component
                    .detail
                    .split(key)
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!((shown - expected).abs() <= 0.051, "{}", component.detail);
            }
        }
        println!(
            "{} negative={}: xn/Icr/MAc={values:?}, MAt={}, MA(current/centroid/layers)={:?}",
            case.name,
            case.negative,
            case.ma_t,
            values.map(|v| case.ma_t.min(v[2]))
        );
    }
}

#[test]
fn concrete_governed_asymmetry_can_accept_bending_rejected_by_real_positions() {
    let case = CASES
        .iter()
        .find(|c| c.name == "two_one_shallow" && !c.negative)
        .unwrap();
    assert!(sepika_core::material_grade::REBAR_NOMINAL_SIZES.contains(&case.main_dia_mm));
    assert!(sepika_core::material_grade::MAIN_REBAR_GRADES.contains(&"SD345"));
    let current = bending(case, 0.0, false, 45e6);
    assert!(current.ratio < 1.0);
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
