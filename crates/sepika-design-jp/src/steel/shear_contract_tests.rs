use super::test_support::{h_section, mat};
use super::SteelDesign;
use crate::{
    CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind,
};
use sepika_core::ids::SectionId;
use sepika_core::model::Section;
use sepika_core::section_shape::SectionShape;

#[test]
fn compression_precheck_keeps_independent_h_web_failure() {
    let section = h_section(400.0, 200.0, 8.0, 13.0);
    let forces = MemberForcesAt {
        pos: 0.5,
        n: -1.0,
        qy: 400_000.0,
        qz: 0.0,
        mx: 0.0,
        my: 0.0,
        mz: 0.0,
    };
    for (kind, expected) in [
        (MemberKind::Girder, 1.381_955_431_570_912_6),
        (MemberKind::Column, 1.478_027_199_541_083_2),
    ] {
        for buckling_length in [0.0, 20_000.0] {
            let outcomes = SteelDesign
                .check_outcomes(
                    &forces,
                    &section,
                    &mat("SN400B"),
                    &DesignCtx {
                        kind,
                        term: LoadTerm::Long,
                        length: 3000.0,
                        lk_y: Some(buckling_length),
                        lk_z: (buckling_length > 0.0).then_some(buckling_length),
                        ..Default::default()
                    },
                )
                .collect::<Vec<_>>();
            let CheckOutcome::Checked(failure) = &outcomes[0] else {
                panic!("{outcomes:?}")
            };
            let kinds = failure
                .components
                .iter()
                .map(|c| c.kind)
                .collect::<Vec<_>>();
            if buckling_length == 0.0 {
                assert_eq!(kinds, vec![CheckKind::Shear]);
                assert!(
                    matches!(outcomes.as_slice(), [_, CheckOutcome::Skipped { reason }] if reason.contains("圧縮材の強軸・弱軸の座屈長さ"))
                );
            } else {
                assert_eq!(kinds, vec![CheckKind::Provision, CheckKind::Shear]);
            }
            let shear = failure
                .components
                .iter()
                .find(|c| c.kind == CheckKind::Shear)
                .unwrap();
            assert!((shear.ratio - expected).abs() < 1e-12, "{outcomes:?}");
        }
    }
}

#[test]
fn invalid_lateral_buckling_length_keeps_independent_h_web_failure() {
    let outcomes = SteelDesign
        .check_outcomes(
            &MemberForcesAt {
                pos: 0.5,
                n: 0.0,
                qy: 400_000.0,
                qz: 0.0,
                mx: 0.0,
                my: 0.0,
                mz: 100_000.0,
            },
            &h_section(400.0, 200.0, 8.0, 13.0),
            &mat("SN400B"),
            &DesignCtx {
                kind: MemberKind::Girder,
                term: LoadTerm::Long,
                length: 3500.0,
                node_span_mm: Some(3000.0),
                lb: Some(-1.0),
                ..Default::default()
            },
        )
        .collect::<Vec<_>>();
    let [CheckOutcome::Checked(failure), CheckOutcome::Skipped { reason }] = outcomes.as_slice()
    else {
        panic!("{outcomes:?}");
    };
    let [shear] = failure.components.as_slice() else {
        panic!("{failure:?}");
    };
    assert_eq!(shear.kind, CheckKind::Shear);
    assert!((shear.ratio - 1.381_955_431_570_912_6).abs() < 1e-12);
    assert!(reason.contains("横座屈長さが負"));
}

fn shear_ratio(section: &Section, qy_n: f64, qz_n: f64, kind: MemberKind) -> Option<f64> {
    let result = SteelDesign
        .check(
            &MemberForcesAt {
                pos: 0.0,
                n: 0.0,
                qy: qy_n,
                qz: qz_n,
                mx: 0.0,
                my: 0.0,
                mz: 0.0,
            },
            section,
            &mat("SN400B"),
            &DesignCtx {
                term: LoadTerm::Short,
                kind,
                ..Default::default()
            },
        )
        .unwrap_checked();
    result
        .components
        .into_iter()
        .find(|component| component.kind == CheckKind::Shear)
        .map(|component| component.ratio)
}

#[test]
fn girder_and_column_use_their_web_shear_definitions() {
    let section = h_section(400.0, 200.0, 8.0, 13.0);
    let girder = shear_ratio(&section, 320_000.0, 0.0, MemberKind::Girder).unwrap();
    let column = shear_ratio(&section, 299_200.0, 0.0, MemberKind::Column).unwrap();
    assert!((girder - 100.0 * 3.0_f64.sqrt() / 235.0).abs() < 1e-12);
    assert!((column - girder).abs() < 1e-12);
}

#[test]
fn unverified_pure_shear_has_no_overall_pass_for_h_box_or_round_bar() {
    let h = h_section(400.0, 200.0, 8.0, 13.0);
    let box_section = SectionShape::SteelBox {
        height: 300.0,
        width: 300.0,
        thick: 12.0,
        horizontal_thick: 12.0,
        corner_r: Some(0.0),
    }
    .to_section(SectionId(0), "角形鋼管".into());
    let round = SectionShape::SteelRoundBar { dia: 100.0 }.to_section(SectionId(0), "丸鋼".into());
    for (section, qy, qz) in [
        (&h, 0.0, 10_000.0),
        (&h, 20_000.0, 10_000.0),
        (&box_section, 10_000.0, 0.0),
        (&round, 0.0, 10_000.0),
    ] {
        for kind in [MemberKind::Girder, MemberKind::Column] {
            let outcome = SteelDesign.check(
                &MemberForcesAt {
                    pos: 0.5,
                    n: 0.0,
                    qy,
                    qz,
                    mx: 0.0,
                    my: 0.0,
                    mz: 0.0,
                },
                section,
                &mat("SN400B"),
                &DesignCtx {
                    kind,
                    ..Default::default()
                },
            );
            assert!(
                matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("照査条件"))
            );
        }
    }
}

#[test]
fn h_beam_and_column_keep_confirmed_web_failure_and_incomplete_direction() {
    let section = h_section(400.0, 200.0, 8.0, 13.0);
    let forces = MemberForcesAt {
        pos: 0.5,
        n: 0.0,
        qy: 450_000.0,
        qz: 10_000.0,
        mx: 0.0,
        my: 0.0,
        mz: 1.0e6,
    };
    for kind in [MemberKind::Girder, MemberKind::Column] {
        let rows = SteelDesign
            .check_outcomes(
                &forces,
                &section,
                &mat("SN400B"),
                &DesignCtx {
                    kind,
                    term: LoadTerm::Short,
                    column_lateral_buckling_length: Some(Ok(3000.0)),
                    ..Default::default()
                },
            )
            .collect::<Vec<_>>();
        let [CheckOutcome::Checked(failure), CheckOutcome::Skipped { reason }] = rows.as_slice()
        else {
            panic!("{rows:?}")
        };
        assert_eq!(failure.components.len(), 1);
        assert_eq!(failure.components[0].kind, CheckKind::Shear);
        assert!(!failure.ok());
        assert!(reason.contains("弱軸せん断"), "{reason}");
    }
}

#[test]
fn circular_pipe_shear_uses_the_resultant_in_either_direction() {
    let section = SectionShape::SteelPipe {
        outer_dia: 200.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "円形鋼管".into());
    let expected = 200_000.0 * 3.0_f64.sqrt() / (std::f64::consts::PI * 1900.0 * 235.0);
    for kind in [MemberKind::Girder, MemberKind::Column] {
        for (qy_n, qz_n) in [(100_000.0, 0.0), (0.0, 100_000.0), (60_000.0, -80_000.0)] {
            assert!((shear_ratio(&section, qy_n, qz_n, kind).unwrap() - expected).abs() < 1e-12);
        }
    }
}

#[test]
fn pipe_and_h_web_combine_concurrent_axial_and_shear_stresses() {
    let pipe = SectionShape::SteelPipe {
        outer_dia: 200.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "円形鋼管".into());
    let h = h_section(400.0, 200.0, 8.0, 13.0);
    for (section, shear_area, directional_ratios) in [
        (&pipe, std::f64::consts::PI * 950.0, [0.6, -0.8]),
        (&h, 2992.0, [1.0, 0.0]),
    ] {
        for kind in [MemberKind::Girder, MemberKind::Column] {
            let [qy, qz] = directional_ratios.map(|v| v * 188.0 * shear_area / 3.0_f64.sqrt());
            let forces = MemberForcesAt {
                pos: 0.5,
                n: 188.0 * section.area,
                qy,
                qz,
                mx: 0.0,
                my: 0.0,
                mz: 0.0,
            };
            let result = SteelDesign
                .check(
                    &forces,
                    section,
                    &mat("SN400B"),
                    &DesignCtx {
                        kind,
                        term: LoadTerm::Short,
                        ..Default::default()
                    },
                )
                .unwrap_checked();
            assert!(
                (result.ratio() - 1.131_370_849_898_476).abs() < 1e-12,
                "{result:?}"
            );
            assert!(!result.ok());
            assert!(result
                .components
                .iter()
                .any(|c| c.detail.contains("式(5.24)")));
        }
    }
}

#[test]
fn pipe_bending_and_shear_has_no_pass_without_verified_same_point_stresses() {
    let pipe = SectionShape::SteelPipe {
        outer_dia: 200.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "円形鋼管".into());
    for kind in [MemberKind::Girder, MemberKind::Column] {
        let ctx = DesignCtx {
            kind,
            term: LoadTerm::Short,
            ..Default::default()
        };
        let mut forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 15_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1.0e6,
        };
        let outcome = SteelDesign.check(&forces, &pipe, &mat("SN400B"), &ctx);
        assert!(matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("式(5.24)")));
        forces.mz = 100e6;
        let failure = SteelDesign
            .check(&forces, &pipe, &mat("SN400B"), &ctx)
            .unwrap_checked();
        assert!(!failure.ok());
        assert!(failure.basis.contains("式(5.24)"));
    }
}

#[test]
fn h_web_bending_and_shear_uses_the_interior_web_edge() {
    let section = h_section(400.0, 200.0, 8.0, 13.0);
    let forces = MemberForcesAt {
        pos: 0.5,
        n: 0.0,
        qy: 0.8 * 235.0 / 3.0_f64.sqrt() * 2992.0,
        qz: 0.0,
        mx: 0.0,
        my: 0.0,
        mz: 200_000_000.0,
    };
    for kind in [MemberKind::Girder, MemberKind::Column] {
        let result = SteelDesign
            .check(
                &forces,
                &section,
                &mat("SN400B"),
                &DesignCtx {
                    kind,
                    term: LoadTerm::Short,
                    column_lateral_buckling_length: Some(Ok(1.0)),
                    ..Default::default()
                },
            )
            .unwrap_checked();
        assert!(!result.ok());
        assert!(
            (result.ratio() - 1.058_424_999_375_795).abs() < 1e-12,
            "{result:?}"
        );
    }
}

#[test]
fn h_column_keeps_confirmed_tension_bending_failure_with_unknown_weak_shear_interaction() {
    let result = SteelDesign
        .check(
            &MemberForcesAt {
                pos: 0.5,
                n: 1_000_000.0,
                qy: 0.0,
                qz: 15_000.0,
                mx: 0.0,
                my: 0.0,
                mz: 200e6,
            },
            &h_section(400.0, 200.0, 8.0, 13.0),
            &mat("SN400B"),
            &DesignCtx {
                kind: MemberKind::Column,
                term: LoadTerm::Short,
                column_lateral_buckling_length: Some(Ok(1000.0)),
                ..Default::default()
            },
        )
        .unwrap_checked();
    assert!(!result.ok());
    assert_eq!(result.components[0].kind, CheckKind::AxialBending);
    assert!(result.basis.contains("同一点の組合せ応力度は未算定"));
}
