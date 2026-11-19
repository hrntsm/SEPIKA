use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, SectionId};
use sepika_core::model::{
    FrameSectionUse, Material, MaterialCategory, PropertyBasis, Section, SteelDesignAttr,
};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, CheckResult, DesignCheck, DesignCtx,
    LoadTerm, MemberDesignCheckOptions, MemberForcesAt, MemberKind, SteelDesign,
};
use sepika_solver::statics::linear::linear_static_once;

#[path = "fixtures/steel_h_column_combined_model.rs"]
mod input;

const FORCE: MemberForcesAt = MemberForcesAt {
    pos: 0.5,
    n: 0.0,
    qy: 404_000.0,
    qz: 0.0,
    mx: 0.0,
    my: 0.0,
    mz: 0.0,
};
const MEMBERS: [MemberKind; 2] = [MemberKind::Column, MemberKind::Girder];
const UNCHECKED: &str =
    "実ウェブの内法寸法に対する規準上の公称せん断応力度の評価方法と許容値の適用条件を確定できない";
const WEAK_UNCHECKED: &str = "弱軸せん断検定は未算定です";
const WEAK_FORCE: MemberForcesAt = MemberForcesAt {
    qy: 0.0,
    qz: 450_000.0,
    ..FORCE
};

fn section(upper_thick: f64, lower_thick: f64) -> Section {
    built_h(80.0, upper_thick, 180.0, lower_thick)
}

fn built_h(upper_width: f64, upper_thick: f64, lower_width: f64, lower_thick: f64) -> Section {
    SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width,
        upper_thick,
        lower_width,
        lower_thick,
        web_thick: 8.0,
    }
    .to_section(SectionId(0), "組立H形鋼".into())
}

fn check(
    section: &Section,
    kind: MemberKind,
    forces: MemberForcesAt,
    term: LoadTerm,
    length: f64,
) -> CheckOutcome {
    check_with_context(
        section,
        forces,
        &DesignCtx {
            kind,
            term,
            length,
            ..Default::default()
        },
    )
}

fn check_with_context(section: &Section, forces: MemberForcesAt, ctx: &DesignCtx) -> CheckOutcome {
    let material = Material {
        id: MaterialId(0),
        name: "直接鋼材".into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    };
    SteelDesign.check(&forces, section, &material, ctx)
}

fn checked(outcome: CheckOutcome) -> CheckResult {
    match outcome {
        CheckOutcome::Checked(result) => result,
        CheckOutcome::Skipped { reason } => panic!("{reason}"),
    }
}

#[test]
fn column_and_girder_design_read_strong_shear_from_solved_member_forces() {
    for (frame_use, tip_mm, reference, load_axis) in [
        (
            FrameSectionUse::Column,
            [0.0, 0.0, 3000.0],
            [1.0, 0.0, 0.0],
            0,
        ),
        (
            FrameSectionUse::Girder,
            [3000.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2,
        ),
    ] {
        let mut model = input::cantilever(0.0, 0.0, 0.0);
        let mut section = section(12.0, 20.0);
        section.material = Some(MaterialId(0));
        section.frame_use = Some(frame_use);
        model.sections[0] = section;
        model.nodes[1].coord = tip_mm;
        model.elements[0].local_axis.ref_vector = reference;
        model.load_cases[0].nodal[0].values[load_axis] = FORCE.qy;

        let solved = linear_static_once(&model, LoadCaseId(0)).unwrap();
        assert_eq!(solved.member_forces.len(), 1);
        for (_, local) in &solved.member_forces[0].1.at {
            assert!(
                (local[1].abs() - FORCE.qy).abs() < 1e-8,
                "{frame_use:?} {local:?}"
            );
            assert_eq!(local[2], 0.0, "{frame_use:?} {local:?}");
        }
        let results = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(results.member_checks.len(), 3, "{frame_use:?}");
        for (_, pos, outcome) in results.member_checks {
            if frame_use == FrameSectionUse::Column && pos < 1.0 {
                let result = checked(outcome);
                assert!(result.basis.contains(UNCHECKED), "{result:?}");
                assert_eq!(result.components.len(), 1);
                assert_eq!(result.components[0].kind, CheckKind::Bending);
                // 固定端 Mz=1212 kN·m と上縁の独立断面係数 634601.730518 mm³。
                let expected = 8.12706073823876 * (1.0 - pos);
                assert!((result.ratio() - expected).abs() < 1e-12, "{result:?}");
                assert!(!result.ok(), "{result:?}");
            } else {
                assert!(
                    matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains(UNCHECKED)),
                    "{frame_use:?} {outcome:?}"
                );
            }
        }
    }
}

#[test]
fn unequal_built_h_pure_strong_shear_is_unchecked_in_columns_and_girders() {
    // 3矩形の独立計算による比較値。実ウェブの内法368 mm、図心は下端から135.236674 mm。
    // 比較値を公開検定の判定には用いない。
    let fs_short_mpa = 235.0 / 3.0_f64.sqrt();
    let web_area_mm2 = 368.0 * 8.0;
    assert_eq!(web_area_mm2, 2944.0);
    let nominal_comparison = FORCE.qy / web_area_mm2 / fs_short_mpa;
    let local_comparison =
        FORCE.qy * 503_969.989_516_323_3 / (168_019_265.000_710_73 * 8.0) / fs_short_mpa;
    assert!((nominal_comparison - 1.011_431_149_193_204).abs() < 1e-12);
    assert!((local_comparison - 1.116_425_476_568_934).abs() < 1e-12);

    for kind in MEMBERS {
        let outcome = check(&section(12.0, 20.0), kind, FORCE, LoadTerm::Short, 3_000.0);
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason }
                if reason.contains("上下のフランジ厚が異なる組立H形鋼")
                    && reason.contains(UNCHECKED)),
            "{outcome:?}"
        );
    }
}

#[test]
fn applicability_does_not_depend_on_thin_flange_side_or_shear_sign_and_magnitude() {
    let section = section(20.0, 12.0);
    for kind in MEMBERS {
        for (qy, expected_reason) in [
            (-FORCE.qy, UNCHECKED),
            (f64::MIN_POSITIVE, UNCHECKED),
            (f64::NAN, "強軸せん断力Qyが非有限"),
            (f64::INFINITY, "強軸せん断力Qyが非有限"),
        ] {
            let outcome = check(
                &section,
                kind,
                MemberForcesAt { qy, ..FORCE },
                LoadTerm::Long,
                3_000.0,
            );
            assert!(
                matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains(expected_reason)),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn zero_strong_shear_and_equal_flange_thickness_keep_existing_results() {
    let unequal = section(12.0, 20.0);
    let equal = section(12.0, 12.0);
    let symmetric = built_h(180.0, 12.0, 180.0, 12.0);
    for kind in MEMBERS {
        for (section, forces, expected) in [
            (
                &unequal,
                MemberForcesAt {
                    n: 75_040.0,
                    qy: -0.0,
                    ..FORCE
                },
                0.042_553_191_489_361_7,
            ),
            (
                &symmetric,
                MemberForcesAt {
                    qy: 0.0,
                    qz: 100_000.0,
                    ..FORCE
                },
                0.255_917_672_513_132,
            ),
            (&equal, FORCE, 0.989_911_337_508_242),
        ] {
            let result = checked(check(section, kind, forces, LoadTerm::Short, 3_000.0));
            assert!((result.ratio() - expected).abs() < 1e-12, "{result:?}");
        }
    }
}

#[test]
fn statutory_slenderness_failure_takes_precedence_in_compression() {
    let section = section(12.0, 20.0);
    let forces = MemberForcesAt {
        n: -10_000.0,
        ..FORCE
    };
    for kind in MEMBERS {
        let result = checked(check(
            &section,
            kind,
            forces.clone(),
            LoadTerm::Short,
            12_000.0,
        ));
        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].kind, CheckKind::Provision);
        assert!(result.ratio() > 1.0, "{result:?}");
        assert!(result.basis.contains("第65条"), "{result:?}");
        assert!(result.basis.contains(UNCHECKED), "{result:?}");
    }
}

#[test]
fn axial_material_limit_failure_retains_the_strong_shear_reason() {
    let section = section(12.0, 20.0);
    for kind in MEMBERS {
        for sign in [-1.0, 1.0] {
            let result = checked(check(
                &section,
                kind,
                MemberForcesAt {
                    n: sign * 2_000_000.0,
                    ..FORCE
                },
                LoadTerm::Short,
                0.0,
            ));
            assert_eq!(result.components.len(), 1);
            assert_eq!(result.components[0].kind, CheckKind::Axial);
            assert!(
                (result.ratio() - 1.134_146_894_705_802).abs() < 1e-12,
                "{result:?}"
            );
            assert!(result.basis.contains(UNCHECKED), "{result:?}");

            let outcome = check(
                &section,
                kind,
                MemberForcesAt {
                    n: sign * 1_763_440.0,
                    ..FORCE
                },
                LoadTerm::Short,
                0.0,
            );
            assert!(
                matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains(UNCHECKED)),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn axial_material_limit_failure_requires_a_shape_area_and_no_section_loss() {
    let intact = section(12.0, 20.0);
    let mut supplied = intact.clone();
    supplied.property_basis.area = PropertyBasis::Supplied;
    for kind in MEMBERS {
        let context = DesignCtx {
            kind,
            term: LoadTerm::Short,
            ..Default::default()
        };
        let with_loss = DesignCtx {
            steel_attr: Some(SteelDesignAttr {
                elem: ElemId(0),
                joint_flange_loss: 5.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
            }),
            kind,
            term: LoadTerm::Short,
            ..Default::default()
        };
        for (section, ctx) in [(&supplied, &context), (&intact, &with_loss)] {
            for (forces, expected) in [(FORCE, UNCHECKED), (WEAK_FORCE, WEAK_UNCHECKED)] {
                let outcome = check_with_context(
                    section,
                    MemberForcesAt {
                        n: 2_000_000.0,
                        ..forces
                    },
                    ctx,
                );
                assert!(
                    matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains(expected)),
                    "{outcome:?}"
                );
            }
        }
    }
}

#[test]
fn strong_shear_unavailability_retains_existing_weak_combination_and_axial_failure() {
    let section = section(12.0, 20.0);
    let forces = MemberForcesAt {
        n: -10_000.0,
        mz: 300e6,
        ..FORCE
    };
    let check_column = |forces| check(&section, MemberKind::Column, forces, LoadTerm::Short, 0.0);
    let axial_bending = checked(check_column(MemberForcesAt { qy: 0.0, ..forces }));
    for qz in [0.0, 100_000.0] {
        let result = checked(check_column(MemberForcesAt { qz, ..forces }));
        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].kind, CheckKind::AxialBending);
        assert!(result.ratio() > 1.0, "{result:?}");
        assert_eq!(result.ratio(), axial_bending.ratio());
        assert!(result.basis.contains(UNCHECKED), "{result:?}");
        assert_eq!(
            result.basis.contains("フランジ内の設計用公称応力度"),
            qz != 0.0,
            "{result:?}"
        );
        assert_eq!(result.basis.contains(WEAK_UNCHECKED), qz != 0.0);
    }
}

#[test]
fn unequal_built_h_pure_weak_shear_requires_a_stress_evaluation_and_allowable_pairing() {
    let section = built_h(80.0, 20.0, 240.0, 12.0);
    for kind in MEMBERS {
        for term in [LoadTerm::Short, LoadTerm::Long] {
            let outcome = check(&section, kind, WEAK_FORCE, term, 3_000.0);
            assert!(
                matches!(&outcome, CheckOutcome::Skipped { reason }
                    if reason.contains("上下非対称組立H形鋼")
                        && reason.contains("規準上の公称せん断応力度の評価方法と材料の許容せん断応力度の適用条件を確定できない")
                        && reason.contains(WEAK_UNCHECKED)),
                "{kind:?} {outcome:?}"
            );
        }
    }
}

#[test]
fn weak_shear_condition_applies_to_width_or_thickness_asymmetry() {
    for section in [
        built_h(80.0, 12.0, 240.0, 12.0),
        built_h(180.0, 20.0, 180.0, 12.0),
    ] {
        for kind in MEMBERS {
            for (qz, expected) in [
                (f64::NAN, "弱軸せん断力Qzが非有限"),
                (f64::NEG_INFINITY, "弱軸せん断力Qzが非有限"),
                (-f64::MIN_POSITIVE, WEAK_UNCHECKED),
            ] {
                let outcome = check(
                    &section,
                    kind,
                    MemberForcesAt { qz, ..WEAK_FORCE },
                    LoadTerm::Short,
                    3_000.0,
                );
                assert!(
                    matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains(expected)),
                    "{kind:?} {outcome:?}"
                );
            }
        }
    }
}

#[test]
fn weak_shear_unavailability_preserves_statutory_slenderness_and_torsion_priority() {
    let section = built_h(80.0, 20.0, 240.0, 12.0);
    for kind in MEMBERS {
        let forces = MemberForcesAt {
            n: -10_000.0,
            ..WEAK_FORCE
        };
        let result = checked(check(
            &section,
            kind,
            forces.clone(),
            LoadTerm::Short,
            24_000.0,
        ));
        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].kind, CheckKind::Provision);
        assert!(result.ratio() > 1.0 && result.basis.contains(WEAK_UNCHECKED));

        let result = checked(check(
            &section,
            kind,
            MemberForcesAt { mx: 1e6, ..forces },
            LoadTerm::Short,
            24_000.0,
        ));
        assert_eq!(result.components[0].kind, CheckKind::Provision);
        assert!(result.components[0].detail.contains("ねじりモーメントMx"));
        assert!(!result.basis.contains(WEAK_UNCHECKED));
    }
}

#[test]
fn weak_shear_retains_only_the_independently_established_axial_material_failure() {
    let section = built_h(80.0, 20.0, 240.0, 12.0);
    for kind in MEMBERS {
        for (qy, qz) in [(0.0, WEAK_FORCE.qz), (FORCE.qy, WEAK_FORCE.qz)] {
            let shear = MemberForcesAt {
                qy,
                qz,
                ..WEAK_FORCE
            };
            for sign in [-1.0, 1.0] {
                let result = checked(check(
                    &section,
                    kind,
                    MemberForcesAt {
                        n: sign * 2e6,
                        ..shear
                    },
                    LoadTerm::Short,
                    0.0,
                ));
                assert_eq!(result.components.len(), 1);
                assert_eq!(result.components[0].kind, CheckKind::Axial);
                assert!((result.ratio() - 2e6 / 1_744_640.0).abs() < 1e-12);
                assert!(result.basis.contains(WEAK_UNCHECKED));
                assert_eq!(result.basis.contains(UNCHECKED), qy != 0.0);
            }
        }
        let outcome = check(
            &section,
            kind,
            MemberForcesAt {
                n: 2e6,
                qz: f64::INFINITY,
                ..WEAK_FORCE
            },
            LoadTerm::Short,
            0.0,
        );
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason } if reason.contains("弱軸せん断力Qzが非有限")),
            "{kind:?} {outcome:?}"
        );
    }
}
