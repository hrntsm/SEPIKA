use sepika_core::ids::SectionId;
use sepika_core::model::{Model, PropertyBasis, Section};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind,
    SteelDesign,
};

#[path = "fixtures/steel_h_column_combined_model.rs"]
mod input;

const ISSUE_FORCES: MemberForcesAt = MemberForcesAt {
    pos: 0.5,
    n: 77_000.0,
    qy: 0.0,
    qz: 365_000.0,
    mx: 0.0,
    my: 0.0,
    mz: 167e6,
};

const UNAVAILABLE: &str = "フランジ内の設計用公称応力度の分布を規準から確定できず";

#[test]
fn issue_1484_uses_the_larger_of_tension_and_lateral_buckling_checks() {
    let model = issue_model();
    // R0 の A=8192、Z=1148243.413333333、短期 ft=235、fb=192.38541862520228 から独立算定。
    for (term, n, mz, expected, details) in [
        (
            LoadTerm::Short,
            77_000.0,
            215e6,
            0.924_410_569_513_599_1,
            [
                "ft=235.0000",
                "fbX=192.3854",
                "引張側=0.8368",
                "曲げ圧縮側=0.9244",
            ],
        ),
        (
            LoadTerm::Long,
            77_000.0,
            215e6,
            1.386_615_854_270_398_6,
            [
                "ft=156.6667",
                "fbX=128.2569",
                "引張側=1.2552",
                "曲げ圧縮側=1.3866",
            ],
        ),
        (
            LoadTerm::Short,
            400_000.0,
            -44e6,
            0.370_840_536_907_217_36,
            [
                "ft=235.0000",
                "fbX=192.3854",
                "引張側=0.3708",
                "曲げ圧縮側=-0.0546",
            ],
        ),
    ] {
        let forces = MemberForcesAt {
            n,
            mz,
            qz: 0.0,
            ..ISSUE_FORCES
        };
        let outcome = check(
            &model,
            &model.sections[0],
            &forces,
            term,
            MemberKind::Column,
        );
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}");
        };
        let component = &result.components[0];
        assert_eq!(component.kind, CheckKind::AxialBending);
        assert!((component.ratio - expected).abs() < 1e-12, "{result:?}");
        assert_eq!(result.ok(), expected <= 1.0, "{result:?}");
        for detail in ["A_N=8192.0000", "Zt=Zc=1148243.4133", "式(6.3)", "式(6.4)"]
            .into_iter()
            .chain(details)
        {
            assert!(component.detail.contains(detail), "{component:?}");
        }
    }
}

fn issue_model() -> Model {
    input::cantilever(ISSUE_FORCES.n, ISSUE_FORCES.mz, ISSUE_FORCES.qz)
}

fn column_context(model: &Model, term: LoadTerm) -> DesignCtx {
    DesignCtx {
        term,
        kind: MemberKind::Column,
        length: 3000.0,
        lk_y: Some(3000.0),
        lk_z: Some(3000.0),
        column_lateral_buckling_length: Some(Ok(3000.0)),
        lb: Some(1000.0),
        steel_attr: Some(sepika_core::model::SteelDesignAttr {
            elem: model.elements[0].id,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lateral_brace_count: None,
            column_lateral_buckling_end_supports: None,
            lk_y_direct: None,
            lk_z_direct: None,
            c_direct: None,
        }),
        ..Default::default()
    }
}

fn check(
    model: &Model,
    section: &Section,
    forces: &MemberForcesAt,
    term: LoadTerm,
    kind: MemberKind,
) -> CheckOutcome {
    SteelDesign.check(
        forces,
        section,
        &model.materials[0],
        &DesignCtx {
            kind,
            ..column_context(model, term)
        },
    )
}

#[test]
fn other_tension_cases_keep_their_ratios_without_reporting_the_strong_axis_formulas() {
    let model = issue_model();
    for (my, mz, expected) in [
        (0.0, 0.0, 0.039_997_506_648_936_17),
        (3e6, 0.0, 0.113_579_520_415_849_69),
        (f64::MIN_POSITIVE, 215e6, 1.013_265_282_335_793),
    ] {
        let forces = MemberForcesAt {
            my,
            mz,
            qz: 0.0,
            ..ISSUE_FORCES
        };
        let outcome = check(
            &model,
            &model.sections[0],
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}");
        };
        assert!((result.ratio() - expected).abs() < 1e-12, "{result:?}");
        assert!(!result.basis.contains("式(6.3)"), "{result:?}");
        assert!(
            !result.components[0].detail.contains("式(6.4)"),
            "{result:?}"
        );
    }
}

#[test]
fn tension_and_strong_bending_keep_the_section_loss_and_lateral_support_limits() {
    let model = issue_model();
    let forces = MemberForcesAt {
        mz: 215e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    let mut ctx = column_context(&model, LoadTerm::Short);
    ctx.steel_attr.as_mut().unwrap().joint_flange_loss = 5.0;
    let outcome = SteelDesign.check(&forces, &model.sections[0], &model.materials[0], &ctx);
    assert!(matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("断面欠損") && reason.contains("有効断面積") && reason.contains("断面係数")));

    ctx.steel_attr.as_mut().unwrap().joint_flange_loss = 0.0;
    let support_reason = "横座屈長さは未算定です";
    ctx.column_lateral_buckling_length = Some(Err(support_reason));
    let outcome = SteelDesign.check(&forces, &model.sections[0], &model.materials[0], &ctx);
    assert!(matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains(support_reason)));

    let outcome = SteelDesign.check(
        &MemberForcesAt {
            mz: 300e6,
            ..forces
        },
        &model.sections[0],
        &model.materials[0],
        &ctx,
    );
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    assert!(result.basis.contains(support_reason), "{result:?}");
    assert!(
        (result.ratio() - 1.151_778_972_022_13).abs() < 1e-12,
        "{result:?}"
    );
    for detail in [
        "fbX上限=235.0000",
        "式(6.3) 引張側=1.1518",
        "式(6.4) 曲げ圧縮側=未算定",
        "軸曲げ比の下限=1.1518",
    ] {
        assert!(result.components[0].detail.contains(detail), "{result:?}");
    }
}

#[test]
fn simultaneous_strong_bending_and_weak_shear_leave_combination_unchecked_for_h_shapes() {
    let model = issue_model();
    let rolled = &model.sections[0];
    let built = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 200.0,
        upper_thick: 13.0,
        lower_width: 180.0,
        lower_thick: 12.0,
        web_thick: 8.0,
    }
    .to_section(SectionId(0), "組立断面".into());
    let mut from_name = rolled.clone();
    from_name.shape = None;
    from_name.property_basis.j = PropertyBasis::Supplied;
    for (section, unequal_flange_thickness) in
        [(rolled, false), (&built, true), (&from_name, false)]
    {
        for term in [LoadTerm::Long, LoadTerm::Short] {
            for (n, condition) in [
                (77_000.0, "引張軸力が同時に作用"),
                (-77_000.0, "圧縮軸力または軸力なしのH形鋼柱"),
                (-0.0, "圧縮軸力または軸力なしのH形鋼柱"),
            ] {
                for (qy, qz, my, mz) in [
                    (0.0, 365_000.0, 0.0, 50e6),
                    (200_000.0, -365_000.0, 3e6, -50e6),
                    (0.0, f64::MIN_POSITIVE, 0.0, 50e6),
                ] {
                    let forces = MemberForcesAt {
                        n,
                        qy,
                        qz,
                        my,
                        mz,
                        ..ISSUE_FORCES
                    };
                    let outcome = check(&model, section, &forces, term, MemberKind::Column);
                    assert!(
                        matches!(&outcome, CheckOutcome::Skipped { reason }
                            if reason.contains(condition)
                                && reason.contains(UNAVAILABLE)
                                && reason.contains("実ウェブの内法寸法に対する規準上の公称せん断応力度")
                                    == (unequal_flange_thickness && qy != 0.0)),
                        "{outcome:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn compression_and_zero_axial_counterexamples_stay_unchecked() {
    let model = issue_model();
    let section = &model.sections[0];
    // N=-77 kN では圧縮座屈込みの短期 fc=183.0658305530364 N/mm²。
    // 両ケース共通の短期 fb=192.38541862520228 N/mm²、Z=1148243.413333333 mm³。
    for (n, mz, expected) in [
        (
            -77_000.0,
            167e6,
            9.3994140625 / 183.0658305530364 + 145.439545361903 / 192.38541862520228,
        ),
        (0.0, 177e6, 177e6 / 1148243.413333333 / 192.38541862520228),
    ] {
        let forces = MemberForcesAt {
            n,
            mz,
            ..ISSUE_FORCES
        };
        let outcome = check(
            &model,
            section,
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains(UNAVAILABLE)),
            "{outcome:?}"
        );
        let axial_only = check(
            &model,
            section,
            &MemberForcesAt { qz: 0.0, ..forces },
            LoadTerm::Short,
            MemberKind::Column,
        );
        let CheckOutcome::Checked(result) = axial_only else {
            panic!("{axial_only:?}");
        };
        assert!(
            (result.components[0].ratio - expected).abs() < 1e-12,
            "{result:?}"
        );
    }
}

#[test]
fn confirmed_axial_bending_and_slenderness_failures_keep_the_unavailable_reason() {
    let model = issue_model();
    let section = &model.sections[0];
    for n in [-77_000.0, 0.0] {
        let forces = MemberForcesAt {
            n,
            mz: 250e6,
            ..ISSUE_FORCES
        };
        let outcome = check(
            &model,
            section,
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        let CheckOutcome::Checked(result) = outcome else {
            panic!("{outcome:?}")
        };
        assert!(!result.ok());
        assert!(result.basis.contains(UNAVAILABLE), "{result:?}");
        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].kind, CheckKind::AxialBending);
        let expected =
            n.abs() / 8192.0 / 183.0658305530364 + 250e6 / 1148243.413333333 / 192.38541862520228;
        assert!((result.ratio() - expected).abs() < 1e-12, "{result:?}");
    }

    let outcome = SteelDesign.check(
        &MemberForcesAt {
            n: -77_000.0,
            ..ISSUE_FORCES
        },
        section,
        &model.materials[0],
        &DesignCtx {
            kind: MemberKind::Column,
            length: 12_000.0,
            lk_y: Some(12_000.0),
            lk_z: Some(12_000.0),
            ..Default::default()
        },
    );
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}")
    };
    assert!(!result.ok());
    assert!(result.basis.contains(UNAVAILABLE), "{result:?}");
    assert_eq!(result.components.len(), 1);
    assert_eq!(result.components[0].kind, CheckKind::Provision);
    assert!(
        (result.ratio() - 12_000.0 / 46.01992322172358 / 200.0).abs() < 1e-12,
        "{result:?}"
    );
}

#[test]
fn zero_weak_shear_preserves_axial_bending_ratio_in_the_issue_example() {
    let model = issue_model();
    let forces = MemberForcesAt {
        qz: -0.0,
        ..ISSUE_FORCES
    };
    let outcome = check(
        &model,
        &model.sections[0],
        &forces,
        LoadTerm::Short,
        MemberKind::Column,
    );
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    let axial_bending = result
        .components
        .iter()
        .find(|component| component.kind == CheckKind::AxialBending)
        .unwrap();
    // 独立算術の N/A、Mz/Z、短期 fb は順に 9.3994140625、145.439545361903、192.38541862520228 N/mm²。
    let expected = 0.707_122_880_1;
    assert!((axial_bending.ratio - expected).abs() < 1e-8, "{result:?}");
}

#[test]
fn approved_h_checks_and_independent_box_and_brace_checks_remain_available() {
    let model = issue_model();
    let rolled = &model.sections[0];
    let box_section = SectionShape::SteelBox {
        height: 400.0,
        width: 200.0,
        thick: 13.0,
        horizontal_thick: 13.0,
        corner_r: Some(0.0),
    }
    .to_section(SectionId(0), rolled.name.clone());
    for n in [77_000.0, -77_000.0, 0.0] {
        let issue_forces = MemberForcesAt { n, ..ISSUE_FORCES };
        for (section, forces, kind) in [
            (
                rolled,
                &MemberForcesAt {
                    mz: 0.0,
                    my: 3e6,
                    qz: 0.0,
                    ..issue_forces
                },
                MemberKind::Column,
            ),
            (
                rolled,
                &MemberForcesAt {
                    qz: 0.0,
                    qy: 50_000.0,
                    ..issue_forces
                },
                MemberKind::Column,
            ),
            (rolled, &issue_forces, MemberKind::Brace),
            (
                &box_section,
                &MemberForcesAt {
                    qz: 0.0,
                    ..issue_forces
                },
                MemberKind::Column,
            ),
        ] {
            let outcome = check(&model, section, forces, LoadTerm::Short, kind);
            assert!(matches!(&outcome, CheckOutcome::Checked(_)), "{outcome:?}");
        }
        let torsion = check(
            &model,
            rolled,
            &MemberForcesAt {
                mx: 1.0,
                ..issue_forces
            },
            LoadTerm::Short,
            MemberKind::Column,
        );
        assert!(matches!(torsion, CheckOutcome::Skipped { reason }
            if reason.contains("ねじりモーメントMx") && !reason.contains(UNAVAILABLE)));
    }
}
