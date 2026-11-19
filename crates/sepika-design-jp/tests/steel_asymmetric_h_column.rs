use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, SectionId};
use sepika_core::model::{FrameSectionUse, Model, PropertyBasis, SteelDesignAttr};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, CheckResult, DesignCheck, DesignCtx,
    LoadTerm, MemberDesignCheckOptions, MemberForcesAt, MemberKind, SteelDesign, SteelFbBasis,
};
use sepika_solver::statics::linear::linear_static_once;

#[path = "fixtures/steel_h_column_combined_model.rs"]
mod input;

const N: f64 = -77_000.0;
const MZ: f64 = 160e6;
const FORCES: MemberForcesAt = MemberForcesAt {
    pos: 0.5,
    n: N,
    qy: 0.0,
    qz: 0.0,
    mx: 0.0,
    my: 0.0,
    mz: MZ,
};
const PURE_BENDING: MemberForcesAt = MemberForcesAt { n: 0.0, ..FORCES };
const NO_STABILITY_CHECK: &str = "横座屈・曲げねじり座屈";
const NO_WEAK_SHEAR_CHECK: &str = "フランジ内の設計用公称応力度の分布を規準から確定できず";

fn asymmetric_column(mz: f64) -> Model {
    let mut model = input::cantilever(N, mz, 0.0);
    let mut section = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 80.0,
        upper_thick: 12.0,
        lower_width: 180.0,
        lower_thick: 20.0,
        web_thick: 8.0,
    }
    .to_section(SectionId(0), "組立H".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Column);
    model.sections[0] = section;
    model
}

fn short_column_context() -> DesignCtx {
    DesignCtx {
        term: LoadTerm::Short,
        kind: MemberKind::Column,
        ..Default::default()
    }
}

fn column_flange_loss() -> SteelDesignAttr {
    SteelDesignAttr {
        elem: ElemId(0),
        joint_flange_loss: 5.0,
        joint_web_loss: 0.0,
        scallop_web_loss: 0.0,
        lb_direct: None,
        lateral_brace_count: None,
        lk_y_direct: None,
        lk_z_direct: None,
        c_direct: None,
    }
}

fn check(model: &Model, forces: &MemberForcesAt, ctx: &DesignCtx) -> CheckOutcome {
    SteelDesign.check(forces, &model.sections[0], &model.materials[0], ctx)
}

#[track_caller]
fn checked(outcome: CheckOutcome) -> CheckResult {
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    result
}

#[test]
fn zero_axial_force_checks_both_edges_for_each_strong_axis_bending_direction() {
    let model = asymmetric_column(MZ);
    let ctx = short_column_context();
    // 3枚の矩形による独立計算。Iy=168019265.000711 mm⁴、図心から上縁まで264.763326226 mm、
    // 下縁まで135.236673774 mm。160 kN·m に対する各外縁の |σb| を短期 F=235 N/mm² で照査。
    for (mz, sides, upper_mpa, lower_mpa, expected_c, expected_t) in [
        (
            MZ,
            "上、引張側=下",
            "-252.1266",
            "128.7821",
            1.0728793053780545,
            0.548008784632117,
        ),
        (
            -MZ,
            "下、引張側=上",
            "252.1266",
            "-128.7821",
            0.548008784632117,
            1.0728793053780545,
        ),
    ] {
        for (term, scale) in [(LoadTerm::Short, 1.0), (LoadTerm::Long, 1.5)] {
            let result = checked(check(
                &model,
                &MemberForcesAt {
                    n: 0.0,
                    mz,
                    ..FORCES
                },
                &DesignCtx {
                    term,
                    ..short_column_context()
                },
            ));
            assert!(!result.ok(), "{result:?}");
            assert!(result.basis.contains("座屈長さ0の断面照査"));
            assert_eq!(result.components.len(), 1);
            let component = &result.components[0];
            assert_eq!(component.kind, CheckKind::Bending);
            assert!((component.ratio - f64::max(expected_c, expected_t) * scale).abs() < 1e-12);
            let allowable_mpa = 235.0 / scale;
            for expected in [
                "Z上=634601.7305 mm³".into(),
                "Z下=1242409.0323 mm³".into(),
                format!("曲げの圧縮側={sides}"),
                format!("σb上={upper_mpa} N/mm²"),
                format!("σb下={lower_mpa} N/mm²"),
                format!("fb={allowable_mpa:.4} N/mm²"),
                format!("ft={allowable_mpa:.4} N/mm²"),
                format!("曲げ比（圧縮側）={:.4}", expected_c * scale),
                format!("曲げ比（引張側）={:.4}", expected_t * scale),
            ] {
                assert!(
                    component.detail.contains(&expected),
                    "{component:?}: {expected}"
                );
            }
        }
    }
    let result = checked(check(
        &model,
        &MemberForcesAt {
            n: -0.0,
            mz: -100e6,
            ..FORCES
        },
        &ctx,
    ));
    assert!(result.ok(), "{result:?}");
    assert!((result.ratio() - 0.6705495658612841).abs() < 1e-12);
}

#[test]
fn zero_axial_force_preserves_only_independent_bending_failure_in_unsupported_combinations() {
    let model = asymmetric_column(MZ);
    let mut direct_lengths = column_flange_loss();
    direct_lengths.joint_flange_loss = 0.0;
    direct_lengths.lb_direct = Some((0.0, 1.0, 0.0));
    for (forces, ctx, reason) in [
        (
            PURE_BENDING,
            DesignCtx {
                length: 1.0,
                ..short_column_context()
            },
            NO_STABILITY_CHECK,
        ),
        (
            PURE_BENDING,
            DesignCtx {
                lb: Some(1e-12),
                ..short_column_context()
            },
            NO_STABILITY_CHECK,
        ),
        (
            PURE_BENDING,
            DesignCtx {
                lk_y: Some(1.0),
                ..short_column_context()
            },
            NO_STABILITY_CHECK,
        ),
        (
            PURE_BENDING,
            DesignCtx {
                lk_z: Some(1.0),
                ..short_column_context()
            },
            NO_STABILITY_CHECK,
        ),
        (
            PURE_BENDING,
            DesignCtx {
                steel_attr: Some(direct_lengths),
                ..short_column_context()
            },
            NO_STABILITY_CHECK,
        ),
        (
            MemberForcesAt {
                my: 1.0,
                ..PURE_BENDING
            },
            short_column_context(),
            "二軸曲げ照査",
        ),
        (
            MemberForcesAt {
                qy: 1.0,
                ..PURE_BENDING
            },
            short_column_context(),
            "ウェブ内同一点",
        ),
        (
            MemberForcesAt {
                qz: 1.0,
                ..PURE_BENDING
            },
            short_column_context(),
            NO_WEAK_SHEAR_CHECK,
        ),
    ] {
        let outcome = check(
            &model,
            &MemberForcesAt {
                mz: -100e6,
                ..forces
            },
            &ctx,
        );
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason: detail } if detail.contains(reason)),
            "{outcome:?}"
        );
        let result = checked(check(&model, &MemberForcesAt { mz: -MZ, ..forces }, &ctx));
        assert!(!result.ok());
        assert!(result.basis.contains(reason), "{result:?}");
        assert_eq!(result.components[0].kind, CheckKind::Bending);
        assert!(result.components[0].detail.contains("曲げ比の下限"));
        assert!((result.ratio() - 1.0728793053780545).abs() < 1e-12);
    }
}

#[test]
fn weak_axis_bending_requires_both_flanges_for_the_independent_bending_failure() {
    let mut model = asymmetric_column(MZ);
    model.sections[0] = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 160.0,
        upper_thick: 4.0,
        lower_width: 180.0,
        lower_thick: 20.0,
        web_thick: 10.0,
    }
    .to_section(SectionId(0), "上フランジ幅厚比20の組立H".into());
    let ctx = short_column_context();
    let forces = MemberForcesAt {
        mz: -MZ,
        ..PURE_BENDING
    };

    // 3枚の矩形から求めた上縁 Z=620411.382725 mm³。弱軸曲げ40 kN·mでは上フランジの幅端に圧縮が生じる。
    let result = checked(check(&model, &forces, &ctx));
    assert!(!result.ok(), "{result:?}");
    assert!((result.ratio() - 1.0974187173016607).abs() < 1e-12);
    assert!(result.components[0].detail.contains("引張側=上"));

    for my in [-40e6, 40e6] {
        let outcome = check(&model, &MemberForcesAt { my, ..forces }, &ctx);
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason }
                if reason.contains("局部座屈")
                    && reason.contains("上フランジ b/t=20.0000")
                    && reason.contains("二軸曲げ照査")),
            "{outcome:?}"
        );
    }
}

#[test]
fn zero_axial_force_skips_when_applicable_edge_stresses_cannot_be_confirmed() {
    let model = asymmetric_column(MZ);
    let ctx = short_column_context();
    for (forces, ctx, reason) in [
        (
            PURE_BENDING,
            DesignCtx {
                steel_attr: Some(column_flange_loss()),
                ..short_column_context()
            },
            "断面欠損",
        ),
        (
            PURE_BENDING,
            DesignCtx {
                steel_fb_basis: SteelFbBasis::Standard1973,
                ..short_column_context()
            },
            "2019年版",
        ),
        (
            MemberForcesAt {
                mx: 1.0,
                ..PURE_BENDING
            },
            short_column_context(),
            "ねじりモーメントMx",
        ),
        (
            MemberForcesAt {
                mz: f64::NAN,
                ..PURE_BENDING
            },
            short_column_context(),
            "断面力が非有限",
        ),
    ] {
        let outcome = check(&model, &forces, &ctx);
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason: detail } if detail.contains(reason)),
            "{outcome:?}"
        );
    }
    let mut thin_web = model;
    thin_web.sections[0] = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 80.0,
        upper_thick: 12.0,
        lower_width: 180.0,
        lower_thick: 20.0,
        web_thick: 7.0,
    }
    .to_section(SectionId(0), "幅厚比上限超過の組立H".into());
    let outcome = check(&thin_web, &PURE_BENDING, &ctx);
    assert!(
        matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains("局部座屈を省く幅厚比")),
        "{outcome:?}"
    );
}

#[test]
fn issue_section_checks_each_flange_and_changes_governing_limit_when_moment_reverses() {
    let model = asymmetric_column(MZ);
    let section = &model.sections[0];
    // 3枚の矩形の面積、図心、平行軸の定理からの独立値。
    assert_eq!(section.area, 7504.0);
    assert!((section.iy - 168_019_265.00071073).abs() < 1e-6);
    for (mz, expected_c, expected_t, compression_side, upper_mpa, lower_mpa) in [
        (
            MZ,
            1.116543960824228,
            0.5043441291859436,
            "上",
            "-252.1266",
            "128.7821",
        ),
        (
            -MZ,
            0.5916734400782904,
            1.0292146499318813,
            "下",
            "252.1266",
            "-128.7821",
        ),
    ] {
        for (term, scale) in [(LoadTerm::Short, 1.0), (LoadTerm::Long, 1.5)] {
            let ctx = DesignCtx {
                term,
                ..short_column_context()
            };
            let result = checked(check(&model, &MemberForcesAt { mz, ..FORCES }, &ctx));
            let component = &result.components[0];
            assert_eq!(component.kind, CheckKind::AxialBending);
            assert!((result.ratio() - f64::max(expected_c, expected_t) * scale).abs() < 1e-12);
            assert!(!result.ok(), "{result:?}");
            for expected in [
                "Z上=634601.7305 mm³".to_owned(),
                "Z下=1242409.0323 mm³".to_owned(),
                "σc=10.2612 N/mm²".to_owned(),
                format!("曲げの圧縮側={compression_side}"),
                format!("σb上={upper_mpa} N/mm²"),
                format!("σb下={lower_mpa} N/mm²"),
                format!("軸曲げ比（圧縮側）={:.4}", expected_c * scale),
                format!("軸曲げ比（引張側）={:.4}", expected_t * scale),
            ] {
                assert!(
                    component.detail.contains(&expected),
                    "{result:?}: {expected}"
                );
            }
        }
    }
    let result = checked(check(
        &model,
        &MemberForcesAt {
            mz: -150e6,
            ..FORCES
        },
        &short_column_context(),
    ));
    assert!(result.ok(), "{result:?}");
}

#[test]
fn local_moment_from_rotated_model_selects_the_expected_flange_and_retains_only_failure() {
    for (global_moment, expected_mz, expected_ratio, side) in [
        ([0.0, MZ, 0.0], MZ, 1.116543960824228, "上"),
        ([MZ, 0.0, 0.0], -MZ, 1.0292146499318813, "下"),
        ([150e6, 0.0, 0.0], -150e6, 0.0, "下"),
    ] {
        let mut model = asymmetric_column(MZ);
        // 鉛直材の局所せいを全体XまたはYに向ける。
        if global_moment[0] != 0.0 {
            model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
        }
        model.load_cases[0].nodal[0].values[3..6].copy_from_slice(&global_moment);
        let solved = linear_static_once(&model, LoadCaseId(0)).unwrap();
        assert_eq!(solved.member_forces.len(), 1);
        for (_, local) in &solved.member_forces[0].1.at {
            assert!((local[0] - N).abs() < 1e-8, "{local:?}");
            assert!((local[5] - expected_mz).abs() < 1e-6, "{local:?}");
            assert!(local[1..5].iter().all(|&force| force == 0.0), "{local:?}");
        }
        let check_model = |model: &Model| {
            run_member_design_checks(
                model,
                &solved.member_forces,
                &solved.panel_moments,
                &MemberDesignCheckOptions {
                    term: LoadTerm::Short,
                    ..Default::default()
                },
            )
            .unwrap()
            .member_checks
        };
        let results = check_model(&model);
        assert_eq!(results.len(), 3);
        for (_, _, outcome) in results {
            if expected_ratio == 0.0 {
                assert!(
                    matches!(&outcome, CheckOutcome::Skipped { reason }
                    if reason.contains(NO_STABILITY_CHECK)),
                    "{outcome:?}"
                );
                continue;
            }
            let result = checked(outcome);
            assert!(!result.ok(), "{result:?}");
            assert!(result.basis.contains(NO_STABILITY_CHECK), "{result:?}");
            assert_eq!(result.components.len(), 1);
            let component = &result.components[0];
            assert_eq!(component.kind, CheckKind::AxialBending);
            assert!(
                (component.ratio - expected_ratio).abs() < 1e-12,
                "{component:?}"
            );
            assert!(
                component.detail.contains(&format!("曲げの圧縮側={side}")),
                "{component:?}"
            );
            assert!(component.detail.contains("軸曲げ比の下限"), "{component:?}");
        }
        if expected_mz == MZ {
            model.steel_design_attrs.push(column_flange_loss());
            let with_loss = check_model(&model);
            assert_eq!(with_loss.len(), 3);
            for (_, _, outcome) in with_loss {
                assert!(
                    matches!(&outcome, CheckOutcome::Skipped { reason }
                        if reason.contains("断面欠損")),
                    "{outcome:?}"
                );
            }
        }
    }
}

#[test]
fn unsupported_simultaneous_forces_never_turn_the_independent_bound_into_a_pass() {
    let model = asymmetric_column(MZ);
    let ctx = short_column_context();
    for (forces, reason) in [
        (MemberForcesAt { my: 1e6, ..FORCES }, "二軸曲げ照査"),
        (
            MemberForcesAt {
                qy: 50_000.0,
                ..FORCES
            },
            "ウェブ内同一点",
        ),
        (
            MemberForcesAt {
                qz: 50_000.0,
                ..FORCES
            },
            NO_WEAK_SHEAR_CHECK,
        ),
    ] {
        let passing_bound = check(
            &model,
            &MemberForcesAt {
                mz: 100e6,
                ..forces
            },
            &ctx,
        );
        assert!(
            matches!(&passing_bound, CheckOutcome::Skipped { reason: detail }
            if detail.contains(reason)),
            "{passing_bound:?}"
        );
        let result = checked(check(&model, &forces, &ctx));
        assert!(result.basis.contains(reason), "{result:?}");
        assert_eq!(result.components.len(), 1);
        assert!((result.ratio() - 1.116543960824228).abs() < 1e-12);
        assert!(!result.ok());
    }

    let result = checked(check(
        &model,
        &MemberForcesAt {
            qz: 50_000.0,
            ..FORCES
        },
        &DesignCtx {
            length: 20_000.0,
            ..ctx
        },
    ));
    assert!(!result.ok());
    assert_eq!(result.components[0].kind, CheckKind::Provision);
    assert!(result.basis.contains(NO_WEAK_SHEAR_CHECK), "{result:?}");
    assert!(result.basis.contains(NO_STABILITY_CHECK), "{result:?}");
}

#[test]
fn slenderness_failure_preserves_loss_and_stability_reasons() {
    let model = asymmetric_column(MZ);
    let ctx = DesignCtx {
        length: 20_000.0,
        steel_attr: Some(column_flange_loss()),
        ..short_column_context()
    };

    for mx in [0.0, 1.0] {
        let result = checked(check(&model, &MemberForcesAt { mx, ..FORCES }, &ctx));
        assert!(!result.ok());
        assert_eq!(result.components.len(), 1);
        let provision = &result.components[0];
        assert_eq!(provision.kind, CheckKind::Provision);
        assert!(result.basis.contains("有効細長比上限"), "{result:?}");
        assert!(result.basis.contains("断面欠損"), "{result:?}");
        assert!(result.basis.contains(NO_STABILITY_CHECK), "{result:?}");
        assert_eq!(
            provision.detail.contains("ねじりモーメントMx"),
            mx != 0.0,
            "{result:?}"
        );
    }
}

#[test]
fn incompatible_section_or_nonfinite_forces_do_not_return_a_stress_result() {
    let mut model = asymmetric_column(MZ);
    for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let outcome = check(
            &model,
            &MemberForcesAt { n, ..FORCES },
            &short_column_context(),
        );
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason }
            if reason.contains("軸力・断面性能・有効座屈長さ")),
            "{outcome:?}"
        );
    }
    for forces in [
        MemberForcesAt {
            mz: f64::NAN,
            ..FORCES
        },
        MemberForcesAt {
            my: f64::INFINITY,
            ..FORCES
        },
        MemberForcesAt {
            qy: f64::NEG_INFINITY,
            ..FORCES
        },
        MemberForcesAt {
            qz: f64::NAN,
            ..FORCES
        },
    ] {
        let outcome = check(&model, &forces, &short_column_context());
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason }
            if reason.contains("断面力が非有限")),
            "{outcome:?}"
        );
    }
    let ctx = DesignCtx {
        steel_attr: Some(column_flange_loss()),
        ..short_column_context()
    };
    let outcome = check(&model, &FORCES, &ctx);
    assert!(
        matches!(&outcome, CheckOutcome::Skipped { reason }
        if reason.contains("断面欠損")),
        "{outcome:?}"
    );

    model.sections[0].property_basis.iy = PropertyBasis::Supplied;
    let outcome = check(&model, &FORCES, &short_column_context());
    assert!(
        matches!(&outcome, CheckOutcome::Skipped { reason }
        if reason.contains("形状から求めた断面積")),
        "{outcome:?}"
    );
}

#[test]
fn section_only_boundary_requires_zero_buckling_lengths_and_keeps_other_axial_regimes() {
    let model = asymmetric_column(MZ);
    for ctx in [
        DesignCtx {
            lb: Some(10.0),
            ..short_column_context()
        },
        DesignCtx {
            lk_y: Some(10.0),
            ..short_column_context()
        },
        DesignCtx {
            lk_z: Some(10.0),
            ..short_column_context()
        },
    ] {
        let outcome = check(
            &model,
            &MemberForcesAt {
                mz: 100e6,
                ..FORCES
            },
            &ctx,
        );
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason }
            if reason.contains(NO_STABILITY_CHECK)),
            "{outcome:?}"
        );
    }
    for forces in [
        MemberForcesAt {
            n: 77_000.0,
            ..FORCES
        },
        MemberForcesAt { mz: 0.0, ..FORCES },
    ] {
        let result = checked(check(&model, &forces, &short_column_context()));
        assert!(
            !result.basis.contains("上下非対称組立H形鋼柱"),
            "{result:?}"
        );
        assert_eq!(result.components.len(), 2);
    }

    let mut symmetric = model;
    symmetric.sections[0] = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 160.0,
        upper_thick: 16.0,
        lower_width: 160.0,
        lower_thick: 16.0,
        web_thick: 8.0,
    }
    .to_section(SectionId(0), "対称組立H".into());
    let result = checked(check(&symmetric, &FORCES, &short_column_context()));
    assert!(
        !result.basis.contains("上下非対称組立H形鋼柱"),
        "{result:?}"
    );
    assert_eq!(result.components.len(), 2);
}
