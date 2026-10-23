use super::*;
use crate::{CheckResult, LoadTerm, SteelFbBasis};
use sepika_core::ids::SectionId;
use sepika_core::model::PropertyBasis;

fn forces() -> MemberForcesAt {
    MemberForcesAt {
        pos: 0.5,
        n: -50_000.0,
        qy: 0.0,
        qz: 0.0,
        my: 0.0,
        mz: 50_000_000.0,
    }
}

fn context(kind: MemberKind) -> DesignCtx {
    DesignCtx {
        kind,
        term: LoadTerm::Long,
        length: 6000.0,
        steel_fb_basis: SteelFbBasis::Asd2019,
        ..Default::default()
    }
}

fn checked(section: &Section, ctx: &DesignCtx) -> CheckResult {
    SteelDesign
        .check(&forces(), section, &test_support::mat("SN400B"), ctx)
        .unwrap_checked()
}

fn assert_same_result(actual: &CheckResult, expected: &CheckResult) {
    assert_eq!(actual.basis, expected.basis);
    assert_eq!(actual.detail, expected.detail);
    assert_eq!(actual.components, expected.components);
}

#[test]
fn asd2019_h_uses_current_shape_j_instead_of_saved_cache() {
    for kind in [MemberKind::Girder, MemberKind::Column] {
        let ctx = context(kind);
        for root_r in [0.0, 13.0] {
            let mut section = SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(root_r),
            }
            .to_section(SectionId(416), "横座屈J算定元".into());
            let mut supplied = section.clone();
            supplied.property_basis.j = PropertyBasis::Supplied;
            supplied.j = 356_762.666_666_666_7;
            let expected = checked(&supplied, &ctx);
            assert_eq!(section.property_basis.j, PropertyBasis::Shape);
            for cache_mm4 in [0.0, 1.0, 1e12, f64::NAN] {
                section.j = cache_mm4;
                assert_same_result(&checked(&section, &ctx), &expected);
                assert_eq!(section.j.to_bits(), cache_mm4.to_bits());
            }
            supplied.j *= 10.0;
            assert_ne!(checked(&supplied, &ctx).ratio(), expected.ratio());
        }
    }
}

#[test]
fn asd2019_h_preserves_supplied_j_without_shape() {
    for kind in [MemberKind::Girder, MemberKind::Column] {
        let ctx = context(kind);
        let mut section = test_support::h_section(400.0, 200.0, 8.0, 13.0);
        section.property_basis.j = PropertyBasis::Supplied;
        section.thickness = Some(13.0);
        section.shape = None;
        let expected = checked(&section, &ctx);
        section.j *= 10.0;
        assert_ne!(checked(&section, &ctx).ratio(), expected.ratio());
    }
}

#[test]
fn asd2019_h_skips_unresolved_j_with_its_reason() {
    for kind in [MemberKind::Girder, MemberKind::Column] {
        for (basis, cache_mm4, remove_shape, expected_reason) in [
            (PropertyBasis::PendingShape, 1e12, false, "未算定"),
            (PropertyBasis::Shape, 1e12, true, "形状が未設定"),
            (PropertyBasis::Supplied, f64::NAN, false, "有限な非負値"),
            (PropertyBasis::Supplied, -1.0, false, "有限な非負値"),
            (
                PropertyBasis::Supplied,
                f64::INFINITY,
                false,
                "有限な非負値",
            ),
        ] {
            let mut section = test_support::h_section(400.0, 200.0, 8.0, 13.0);
            section.property_basis.j = basis;
            section.j = cache_mm4;
            if remove_shape {
                section.thickness = Some(13.0);
                section.shape = None;
            }
            match SteelDesign.check(
                &forces(),
                &section,
                &test_support::mat("SN400B"),
                &context(kind),
            ) {
                CheckOutcome::Skipped { reason } => {
                    assert!(reason.contains("鋼横座屈検定"), "{reason}");
                    assert!(reason.contains(&section.name), "{reason}");
                    assert!(reason.contains(expected_reason), "{reason}");
                }
                CheckOutcome::Checked(_) => panic!("未解決Jで検定済みにしない"),
            }
        }
    }
}

#[test]
fn checks_that_do_not_use_j_accept_unresolved_j() {
    for kind in [MemberKind::Girder, MemberKind::Column, MemberKind::Brace] {
        let mut section = test_support::h_section(400.0, 200.0, 8.0, 13.0);
        let mut ctx = context(kind);
        ctx.steel_fb_basis = SteelFbBasis::Standard1973;
        let expected = checked(&section, &ctx);
        for basis in [PropertyBasis::PendingShape, PropertyBasis::Supplied] {
            section.property_basis.j = basis;
            section.j = f64::NAN;
            assert_same_result(&checked(&section, &ctx), &expected);
        }
    }
    for kind in [MemberKind::Girder, MemberKind::Column] {
        let mut section = SectionShape::SteelBox {
            height: 400.0,
            width: 300.0,
            thick: 12.0,
            corner_r: Some(30.0),
        }
        .to_section(SectionId(416), "J不要Box".into());
        let ctx = context(kind);
        let expected = checked(&section, &ctx);
        section.property_basis.j = PropertyBasis::PendingShape;
        section.j = f64::NAN;
        assert_same_result(&checked(&section, &ctx), &expected);
    }
}

#[test]
fn asd2019_requires_j_only_above_the_resolved_buckling_length_threshold() {
    for kind in [MemberKind::Girder, MemberKind::Column] {
        for route in 0..if kind == MemberKind::Girder { 3 } else { 1 } {
            for lb_mm in [0.0, 5e-10, 1e-9, 2e-9] {
                let mut ctx = context(kind);
                match route {
                    0 => ctx.length = lb_mm,
                    1 => ctx.lb = Some(lb_mm),
                    _ => {
                        ctx.steel_attr = Some(sepika_core::model::SteelDesignAttr {
                            elem: sepika_core::ids::ElemId(0),
                            joint_flange_loss: 0.0,
                            joint_web_loss: 0.0,
                            scallop_web_loss: 0.0,
                            lb_direct: Some((lb_mm, lb_mm, lb_mm)),
                            lateral_brace_count: None,
                            lk_y_direct: None,
                            lk_z_direct: None,
                            c_direct: None,
                        });
                    }
                }
                let mut section = test_support::h_section(400.0, 200.0, 8.0, 13.0);
                let expected = checked(&section, &ctx);
                for (basis, cache_mm4, reason_fragment) in [
                    (PropertyBasis::PendingShape, 1e12, "未算定"),
                    (PropertyBasis::Supplied, f64::NAN, "有限な非負値"),
                    (PropertyBasis::Supplied, -1.0, "有限な非負値"),
                ] {
                    section.property_basis.j = basis;
                    section.j = cache_mm4;
                    let outcome =
                        SteelDesign.check(&forces(), &section, &test_support::mat("SN400B"), &ctx);
                    if lb_mm <= 1e-9 {
                        assert_same_result(&outcome.unwrap_checked(), &expected);
                    } else {
                        assert!(
                            matches!(outcome, CheckOutcome::Skipped { reason }
                                if reason.contains(reason_fragment)),
                            "{kind:?}, route={route}, lb={lb_mm}"
                        );
                    }
                }
            }
        }
    }
}
