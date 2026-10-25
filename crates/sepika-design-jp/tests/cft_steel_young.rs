use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::cft::CftDesign;
use sepika_design_jp::{CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt};

fn material(category: MaterialCategory, young: f64, fc: Option<f64>) -> Material {
    Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category,
        young,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

fn check(pipe: bool, term: LoadTerm, steel_e: f64, lambda: f64, concrete_e: f64) -> CheckOutcome {
    let (shape, radius_mm) = if pipe {
        (
            SectionShape::CftPipe {
                outer_dia: 400.0,
                thick: 10.0,
            },
            137.93114224133723,
        )
    } else {
        (
            SectionShape::CftBox {
                height: 400.0,
                width: 400.0,
                thick: 10.0,
                corner_r: Some(0.0),
            },
            159.2691642053372,
        )
    };
    let section = shape.to_section(SectionId(0), "CFT767".into());
    let ctx = DesignCtx {
        term,
        length: lambda * radius_mm,
        steel_material: Some(material(MaterialCategory::Steel, steel_e, None)),
        ..Default::default()
    };
    CftDesign.check(
        &MemberForcesAt {
            pos: 0.0,
            n: -8.0e6,
            qy: 0.0,
            qz: 0.0,
            my: 0.0,
            mz: 0.0,
        },
        &section,
        &material(MaterialCategory::Concrete, concrete_e, Some(24.0)),
        &ctx,
    )
}

fn ratio(outcome: CheckOutcome) -> f64 {
    match outcome {
        CheckOutcome::Checked(result) => result.ratio(),
        CheckOutcome::Skipped { reason } => panic!("{reason}"),
    }
}

#[test]
fn assigned_steel_e_controls_box_pipe_long_short_compression() {
    for (pipe, term, expected) in [
        (
            false,
            LoadTerm::Long,
            [4.287224976022, 3.198638508546, 2.869374593798],
        ),
        (
            false,
            LoadTerm::Short,
            [2.369237795063, 1.847918106058, 1.680779614197],
        ),
        (
            true,
            LoadTerm::Long,
            [5.458664376647, 4.072633038394, 3.653401201482],
        ),
        (
            true,
            LoadTerm::Short,
            [3.016607251555, 2.352842408065, 2.140035070780],
        ),
    ] {
        for (steel_e, expected) in [100000.0, 205000.0, 300000.0].into_iter().zip(expected) {
            for concrete_e in [10000.0, 25000.0, 50000.0] {
                let actual = ratio(check(pipe, term, steel_e, 100.0, concrete_e));
                assert!(
                    (actual - expected).abs() < 1.0e-11,
                    "pipe={pipe}, {term:?}, E={steel_e}, actual={actual}"
                );
            }
        }
    }
}

#[test]
fn invalid_or_unrepresentable_steel_e_is_skipped_without_fixed_fallback() {
    for pipe in [false, true] {
        for term in [LoadTerm::Long, LoadTerm::Short] {
            for steel_e in [
                0.0,
                -1.0,
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::MAX,
                f64::from_bits(1),
            ] {
                let CheckOutcome::Skipped { reason } = check(pipe, term, steel_e, 100.0, 25000.0)
                else {
                    panic!("pipe={pipe}, {term:?}, E={steel_e}: 不正又は算定不能なEを適合へ補完してはならない");
                };
                assert!(
                    reason.contains("鋼管材料") && reason.contains('E'),
                    "{reason}"
                );
            }
        }
    }
}

#[test]
fn tiny_positive_e_uses_finite_formula_instead_of_zero_slenderness_fallback() {
    for (pipe, long_expected, short_expected) in [
        (false, 6.925207756232687, 3.4626038781163433),
        (true, 8.817448370742124, 4.408724185371062),
    ] {
        for (term, expected) in [
            (LoadTerm::Long, long_expected),
            (LoadTerm::Short, short_expected),
        ] {
            let actual = ratio(check(pipe, term, 1.0e-100, 100.0, 25000.0));
            assert!((actual - expected).abs() < 1.0e-11, "actual={actual}");
        }
    }
}

#[test]
fn each_material_limit_keeps_both_branches_and_the_boundary_coefficient() {
    let expected = [
        [
            3.6859531459345205,
            2.0870274983640638,
            4.693101305444809,
            2.6572859418668267,
        ],
        [
            3.6859565057132326,
            2.087029114056141,
            4.693105583247927,
            2.6572879990298706,
        ],
        [
            3.685481099980655,
            2.0868004906365263,
            4.692500277869416,
            2.6569969066511647,
        ],
    ];
    for (steel_e, lambdas) in [
        (
            100000.0,
            [83.6642422661249, 83.66432593045083, 83.66440959477676],
        ),
        (
            205000.0,
            [119.78896501609788, 119.78908480518268, 119.78920459426747],
        ),
        (
            300000.0,
            [144.91071838167983, 144.91086329254313, 144.9110082034064],
        ),
    ] {
        for (lambda, expected) in lambdas.into_iter().zip(expected) {
            for ((pipe, term), expected) in [
                (false, LoadTerm::Long),
                (false, LoadTerm::Short),
                (true, LoadTerm::Long),
                (true, LoadTerm::Short),
            ]
            .into_iter()
            .zip(expected)
            {
                let actual = ratio(check(pipe, term, steel_e, lambda, 25000.0));
                assert!((actual - expected).abs() < 1.0e-11, "pipe={pipe}, {term:?}, E={steel_e}, lambda={lambda}, actual={actual}, expected={expected}");
            }
        }
    }
}

#[test]
fn zero_slenderness_and_large_finite_e_keep_the_tension_limit() {
    for (pipe, term, expected) in [
        (false, LoadTerm::Long, 2.222716159146477),
        (false, LoadTerm::Short, 1.3385984873837093),
        (true, LoadTerm::Long, 2.83005011054715),
        (true, LoadTerm::Short, 1.7043565286596114),
    ] {
        for steel_e in [1.0e-100, 100000.0, 205000.0, 300000.0, 1.0e300] {
            let actual = ratio(check(pipe, term, steel_e, 0.0, 25000.0));
            assert!((actual - expected).abs() < 1.0e-11);
        }
        let actual = ratio(check(pipe, term, 1.0e300, 100.0, 25000.0));
        assert!((actual - expected).abs() < 1.0e-11);
    }
}

#[test]
fn unrepresentable_slenderness_ratio_is_skipped() {
    for pipe in [false, true] {
        for term in [LoadTerm::Long, LoadTerm::Short] {
            for (steel_e, lambda) in [(205000.0, 1.0e200), (1.0e307, 1.0e-11)] {
                let CheckOutcome::Skipped { reason } = check(pipe, term, steel_e, lambda, 25000.0)
                else {
                    panic!("pipe={pipe}, {term:?}, E={steel_e}, lambda={lambda}: 二乗のoverflow/underflowで有限な成功値へ補完してはならない");
                };
                assert!(reason.contains("中間値が表現範囲外"), "{reason}");
            }
        }
    }
}
