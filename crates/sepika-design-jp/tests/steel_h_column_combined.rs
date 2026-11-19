use sepika_core::ids::SectionId;
use sepika_core::model::{Model, PropertyBasis, Section};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind,
    SteelDesign, SteelFbBasis,
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
            term,
            kind,
            length: 3000.0,
            lb: Some(1000.0),
            steel_fb_basis: SteelFbBasis::Asd2019,
            ..Default::default()
        },
    )
}

#[test]
fn simultaneous_tension_strong_bending_and_weak_shear_skip_h_columns() {
    let model = input::cantilever(ISSUE_FORCES.qz);
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
    for section in [rolled, &built, &from_name] {
        for term in [LoadTerm::Long, LoadTerm::Short] {
            for forces in [
                ISSUE_FORCES,
                MemberForcesAt {
                    qy: 10_000.0,
                    qz: -365_000.0,
                    my: 3e6,
                    mz: -167e6,
                    ..ISSUE_FORCES
                },
                MemberForcesAt {
                    qz: f64::MIN_POSITIVE,
                    ..ISSUE_FORCES
                },
            ] {
                let outcome = check(&model, section, &forces, term, MemberKind::Column);
                assert!(
                    matches!(&outcome, CheckOutcome::Skipped { reason }
                        if reason.contains("H形鋼柱に弱軸せん断・強軸曲げ・引張軸力が同時に作用")
                            && reason.contains("フランジ内の設計用公称応力度の分布を規準から確定できず")),
                    "{outcome:?}"
                );
            }
        }
    }
}

#[test]
fn zero_weak_shear_preserves_axial_bending_ratio_in_the_issue_example() {
    let model = input::cantilever(0.0);
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
    // 独立算術の N/A、Mz/Z、短期 fb は順に 9.3994140625、145.439545361903、192.337746536794 N/mm²。
    let expected = 9.3994140625 / 235.0 + 145.439545361903 / 192.337746536794;
    assert!((axial_bending.ratio - expected).abs() < 1e-12, "{result:?}");
}

#[test]
fn skip_is_limited_to_the_specified_forces_and_h_columns() {
    let model = input::cantilever(ISSUE_FORCES.qz);
    let rolled = &model.sections[0];
    let box_section = SectionShape::SteelBox {
        height: 400.0,
        width: 200.0,
        thick: 13.0,
        horizontal_thick: 13.0,
        corner_r: Some(0.0),
    }
    .to_section(SectionId(0), rolled.name.clone());
    for (section, forces, kind) in [
        (
            rolled,
            MemberForcesAt {
                n: 0.0,
                ..ISSUE_FORCES
            },
            MemberKind::Column,
        ),
        (
            rolled,
            MemberForcesAt {
                n: -77_000.0,
                ..ISSUE_FORCES
            },
            MemberKind::Column,
        ),
        (
            rolled,
            MemberForcesAt {
                mz: 0.0,
                my: 3e6,
                ..ISSUE_FORCES
            },
            MemberKind::Column,
        ),
        (
            rolled,
            MemberForcesAt {
                qz: 0.0,
                qy: 50_000.0,
                ..ISSUE_FORCES
            },
            MemberKind::Column,
        ),
        (rolled, ISSUE_FORCES, MemberKind::Girder),
        (rolled, ISSUE_FORCES, MemberKind::Brace),
        (&box_section, ISSUE_FORCES, MemberKind::Column),
    ] {
        let outcome = check(&model, section, &forces, LoadTerm::Short, kind);
        assert!(matches!(&outcome, CheckOutcome::Checked(_)), "{outcome:?}");
    }
}
