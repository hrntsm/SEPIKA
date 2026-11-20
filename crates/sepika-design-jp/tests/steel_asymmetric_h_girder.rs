use sepika_core::ids::{ElemId, MaterialId, SectionId};
use sepika_core::model::{FrameSectionUse, Model, PropertyBasis, SteelDesignAttr};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, CheckResult, DesignCheck, DesignCtx,
    LoadTerm, MemberDesignCheckOptions, MemberForcesAt, MemberKind, SteelDesign, SteelFbBasis,
};
use sepika_element::frame::beam::MemberForces;

#[path = "fixtures/steel_h_column_combined_model.rs"]
mod input;

const FORCES: MemberForcesAt = MemberForcesAt {
    pos: 0.5,
    n: 0.0,
    qy: 0.0,
    qz: 0.0,
    mx: 0.0,
    my: 0.0,
    mz: 160e6,
};
const ISSUE_RATIO: f64 = 1.07287930536;
const SMALL_MOMENT_NMM: f64 = 50e6;

fn girder() -> Model {
    built_h_girder(80.0, 12.0, 20.0, 8.0)
}

fn built_h_girder(upper_width: f64, upper_thick: f64, lower_thick: f64, web_thick: f64) -> Model {
    let mut model = input::cantilever(0.0, 0.0, 0.0);
    model.nodes[1].coord = [3000.0, 0.0, 0.0];
    model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
    let mut section = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width,
        upper_thick,
        lower_width: 180.0,
        lower_thick,
        web_thick,
    }
    .to_section(SectionId(0), "組立H大梁".into());
    section.material = Some(MaterialId(0));
    section.frame_use = Some(FrameSectionUse::Girder);
    model.sections[0] = section;
    model
}

fn context() -> DesignCtx {
    DesignCtx {
        term: LoadTerm::Short,
        kind: MemberKind::Girder,
        length: 3000.0,
        lb: Some(0.0),
        ..Default::default()
    }
}

fn attributes(lb_direct: Option<(f64, f64, f64)>) -> SteelDesignAttr {
    SteelDesignAttr {
        elem: ElemId(0),
        joint_flange_loss: 0.0,
        joint_web_loss: 0.0,
        scallop_web_loss: 0.0,
        lb_direct,
        lateral_brace_count: None,
        column_lateral_buckling_end_supports: None,
        lk_y_direct: None,
        lk_z_direct: None,
        c_direct: None,
    }
}

fn check(model: &Model, forces: MemberForcesAt, ctx: &DesignCtx) -> CheckOutcome {
    SteelDesign.check(&forces, &model.sections[0], &model.materials[0], ctx)
}

#[track_caller]
fn checked(outcome: CheckOutcome) -> CheckResult {
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    result
}

#[track_caller]
fn assert_skipped(outcome: CheckOutcome, expected: &[&str]) {
    let CheckOutcome::Skipped { reason } = outcome else {
        panic!("{outcome:?}");
    };
    for snippet in expected {
        assert!(reason.contains(snippet), "{reason}: {snippet}");
    }
}

#[test]
fn pure_strong_axis_bending_checks_both_edges_for_either_moment_sign() {
    let model = girder();
    // 独立の長方形3枚による合成断面の図心と平行軸の定理から求めた期待値。
    for (mz, upper, lower, compression_side, tension_side, compression_ratio, tension_ratio) in [
        (
            160e6,
            "-252.1266",
            "128.7821",
            "上",
            "下",
            ISSUE_RATIO,
            0.54800878464,
        ),
        (
            -160e6,
            "252.1266",
            "-128.7821",
            "下",
            "上",
            0.54800878464,
            ISSUE_RATIO,
        ),
    ] {
        for (term, factor, allowable) in [
            (LoadTerm::Short, 1.0, "235.0000"),
            (LoadTerm::Long, 1.5, "156.6667"),
        ] {
            let ctx = DesignCtx { term, ..context() };
            let result = checked(check(&model, MemberForcesAt { mz, ..FORCES }, &ctx));
            assert!(
                (result.ratio() - ISSUE_RATIO * factor).abs() < 1e-10,
                "{result:?}"
            );
            assert!(!result.ok(), "{result:?}");
            assert_eq!(result.components.len(), 1);
            let component = &result.components[0];
            assert_eq!(component.kind, CheckKind::Bending);
            for expected in [
                "Z上=634601.7305 mm³".to_string(),
                "Z下=1242409.0323 mm³".to_string(),
                format!("σb上={upper} N/mm²"),
                format!("σb下={lower} N/mm²"),
                format!("圧縮側={compression_side}、引張側={tension_side}"),
                format!("fb={allowable} N/mm², ft={allowable} N/mm²"),
                format!("曲げ比（圧縮側）={:.4}", compression_ratio * factor),
                format!("曲げ比（引張側）={:.4}", tension_ratio * factor),
            ] {
                assert!(
                    component.detail.contains(&expected),
                    "{result:?}: {expected}"
                );
            }
        }
    }
}

#[test]
fn width_to_thickness_limit_checks_the_compression_flange_for_each_moment_sign() {
    let model = built_h_girder(400.0, 8.0, 20.0, 8.0);
    assert_skipped(
        check(&model, MemberForcesAt { mz: 80e6, ..FORCES }, &context()),
        &["局部座屈", "上フランジ b/t=25.0000（上限 15.6538）"],
    );
    let result = checked(check(
        &model,
        MemberForcesAt {
            mz: -80e6,
            ..FORCES
        },
        &context(),
    ));
    assert!(result.ok(), "{result:?}");
    // 長方形3枚から求めた全断面の遠い側の縁応力度を 235 N/mm² で除した値。
    assert!((result.ratio() - 0.241703302140).abs() < 1e-10);
    assert!(result.components[0].detail.contains("圧縮側=下、引張側=上"));
}

#[test]
fn web_uses_the_welded_girder_limit_for_pure_bending() {
    const WEB_TEST_FORCES: MemberForcesAt = MemberForcesAt { mz: 80e6, ..FORCES };
    // 1.6√(E/F) < 372/6 = 62 < 2.4√(E/F)
    let within_limit = built_h_girder(200.0, 8.0, 20.0, 6.0);
    let result = checked(check(&within_limit, WEB_TEST_FORCES, &context()));
    assert!(result.ok(), "{result:?}");

    let above_limit = built_h_girder(200.0, 8.0, 20.0, 5.0);
    assert_skipped(
        check(&above_limit, WEB_TEST_FORCES, &context()),
        &["局部座屈", "ウェブ d/t=74.4000（上限 70.8850）"],
    );
}

#[test]
fn local_buckling_not_calculated_preserves_only_certain_bending_failure() {
    let model = built_h_girder(400.0, 8.0, 20.0, 8.0);
    // 335 kN·m では全断面の片縁で許容値を超えるが、上下縁の応力度の和は 470 N/mm² 以下。
    assert_skipped(
        check(
            &model,
            MemberForcesAt {
                mz: 335e6,
                ..FORCES
            },
            &context(),
        ),
        &["局部座屈", "上フランジ"],
    );
    let result = checked(check(
        &model,
        MemberForcesAt {
            mz: 340e6,
            ..FORCES
        },
        &context(),
    ));
    assert!(!result.ok(), "{result:?}");
    assert!(result.basis.contains("上フランジ b/t=25.0000"));
    assert!(result.basis.contains("有効断面による曲げ照査は未算定"));
    assert!(result.basis.contains("表示する検定比は下限"));
    assert!(result.components[0].detail.contains("曲げ比の下限"));
    assert!((result.ratio() - 1.007177896613).abs() < 1e-10);
}

#[test]
fn width_to_thickness_limits_and_failure_bound_use_the_plate_specific_f() {
    let sn400b_girder = |upper_width, web_thick| {
        let mut model = built_h_girder(upper_width, 8.0, 50.0, web_thick);
        model.materials[0].name = "SN400B".into();
        model.materials[0].fy = None;
        model
    };
    let ctx = context();
    // 上フランジとウェブの F=235、下フランジの F=215 N/mm²。
    assert_skipped(
        check(&sn400b_girder(256.0, 8.0), FORCES, &ctx),
        &["上フランジ b/t=16.0000（上限 15.6538）"],
    );
    assert_skipped(
        check(&sn400b_girder(200.0, 4.75), FORCES, &ctx),
        &["ウェブ d/t=72.0000（上限 70.8850）"],
    );

    let model = sn400b_girder(400.0, 8.0);
    // 上下縁の許容値合計 450 N/mm² を超える境界は約422.155 kN·m。
    assert_skipped(
        check(
            &model,
            MemberForcesAt {
                mz: 410e6,
                ..FORCES
            },
            &ctx,
        ),
        &["上フランジ"],
    );
    let result = checked(check(
        &model,
        MemberForcesAt {
            mz: 425e6,
            ..FORCES
        },
        &ctx,
    ));
    assert!(!result.ok(), "{result:?}");
    assert!((result.ratio() - 1.006739764585).abs() < 1e-10);
}

#[test]
fn unavailable_width_to_thickness_limit_cannot_confirm_compliance() {
    let mut model = girder();
    for young in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        model.materials[0].young = young;
        assert_skipped(
            check(
                &model,
                MemberForcesAt {
                    mz: SMALL_MOMENT_NMM,
                    ..FORCES
                },
                &context(),
            ),
            &["ヤング係数", "幅厚比の限界値を算定できません"],
        );
    }
}

#[test]
fn common_model_check_uses_zero_only_in_its_explicit_zone() {
    let mut model = girder();
    let forces = [(
        ElemId(0),
        MemberForces {
            at: [0.0, 0.5, 1.0]
                .map(|pos| (pos, [0.0, 0.0, 0.0, 0.0, 0.0, SMALL_MOMENT_NMM]))
                .to_vec(),
        },
    )];
    let options = MemberDesignCheckOptions {
        term: LoadTerm::Short,
        ..Default::default()
    };
    let check_model = |model: &Model| {
        run_member_design_checks(model, &forces, &[], &options)
            .unwrap()
            .member_checks
    };
    let results = check_model(&model);
    assert_eq!(results.len(), 3);
    for (_, _, outcome) in results {
        assert_skipped(outcome, &["横座屈長さが0と直接指定されておらず"]);
    }
    model
        .steel_design_attrs
        .push(attributes(Some((0.0, 1000.0, 0.0))));
    let results = check_model(&model);
    assert_eq!(results.len(), 3);
    for (_, pos, outcome) in results {
        if pos == 0.5 {
            assert_skipped(outcome, &["正の横座屈長さ"]);
        } else {
            let result = checked(outcome);
            assert!(result.ok(), "{result:?}");
            assert!(
                (result.ratio() - 0.335274782926).abs() < 1e-10,
                "{result:?}"
            );
            assert!(result.basis.contains("横座屈長さ0の断面照査"));
        }
    }
}

#[test]
fn unconfirmed_lateral_buckling_keeps_only_independent_failure() {
    let model = girder();
    for ctx in [
        DesignCtx {
            lb: None,
            length: 0.0,
            ..context()
        },
        DesignCtx {
            lb: Some(1200.0),
            ..context()
        },
    ] {
        for mz in [FORCES.mz, -FORCES.mz] {
            let result = checked(check(&model, MemberForcesAt { mz, ..FORCES }, &ctx));
            assert!(!result.ok(), "{result:?}");
            assert!((result.ratio() - ISSUE_RATIO).abs() < 1e-10);
            assert!(result.basis.contains("材料の許容上限による不適合"));
            assert!(result.basis.contains("横座屈の算定条件が未確定"));
            assert!(result.components[0].detail.contains("曲げ比の下限"));
        }
        assert_skipped(
            check(
                &model,
                MemberForcesAt {
                    mz: SMALL_MOMENT_NMM,
                    ..FORCES
                },
                &ctx,
            ),
            &["横座屈の算定条件が未確定"],
        );
    }
}

#[test]
fn statutory_slenderness_failure_retains_unchecked_girder_combination_reason() {
    let model = girder();
    let ctx = DesignCtx {
        lk_z: Some(30_000.0),
        ..context()
    };
    let result = checked(check(&model, MemberForcesAt { n: -1.0, ..FORCES }, &ctx));
    assert!(!result.ok(), "{result:?}");
    assert_eq!(result.components.len(), 1);
    assert_eq!(result.components[0].kind, CheckKind::Provision);
    assert!(result.basis.contains("有効細長比上限"), "{result:?}");
    assert!(
        result.basis.contains("軸力Nと強軸曲げMzの組合せは未算定"),
        "{result:?}"
    );
}

#[test]
fn inapplicable_conditions_do_not_return_a_pure_bending_result() {
    let mut model = girder();
    for (forces, reasons) in [
        (
            MemberForcesAt {
                n: 50_000.0,
                ..FORCES
            },
            vec!["軸力Nと強軸曲げMz"],
        ),
        (
            MemberForcesAt { my: 1.0, ..FORCES },
            vec!["弱軸曲げMyと強軸曲げMz"],
        ),
        (
            MemberForcesAt { qy: 1.0, ..FORCES },
            vec!["強軸せん断Qyと強軸曲げMz", "実ウェブの内法寸法"],
        ),
        (
            MemberForcesAt { qz: 1.0, ..FORCES },
            vec!["弱軸せん断Qzと強軸曲げMz", "弱軸せん断に対する規準上"],
        ),
        (
            MemberForcesAt { mx: 1.0, ..FORCES },
            vec!["ねじりモーメントMxが作用"],
        ),
        (
            MemberForcesAt {
                mz: f64::NAN,
                ..FORCES
            },
            vec!["断面力が非有限"],
        ),
    ] {
        assert_skipped(check(&model, forces, &context()), &reasons);
    }
    for lb in [-1.0, f64::NAN, f64::INFINITY] {
        let ctx = DesignCtx {
            lb: Some(lb),
            ..context()
        };
        assert_skipped(
            check(&model, FORCES, &ctx),
            &["横座屈長さの直接入力が負または非有限"],
        );
    }
    let ctx = DesignCtx {
        steel_fb_basis: SteelFbBasis::Standard1973,
        ..context()
    };
    assert_skipped(check(&model, FORCES, &ctx), &["2019年版を適用する場合"]);
    let mut loss = attributes(None);
    loss.joint_flange_loss = 1.0;
    let ctx = DesignCtx {
        steel_attr: Some(loss),
        ..context()
    };
    assert_skipped(check(&model, FORCES, &ctx), &["断面欠損を控除した断面係数"]);
    model.sections[0].property_basis.iy = PropertyBasis::Supplied;
    assert_skipped(
        check(&model, FORCES, &context()),
        &["形状から求めた強軸断面二次モーメント"],
    );
}
