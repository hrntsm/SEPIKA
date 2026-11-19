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
    let mut model = input::cantilever(0.0, 0.0, 0.0);
    model.nodes[1].coord = [3000.0, 0.0, 0.0];
    model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
    let mut section = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 80.0,
        upper_thick: 12.0,
        lower_width: 180.0,
        lower_thick: 20.0,
        web_thick: 8.0,
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
