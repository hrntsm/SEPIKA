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

fn rolled_h(width: f64, web_thick: f64, root_r: f64) -> Section {
    SectionShape::SteelH {
        height: 400.0,
        width,
        web_thick,
        flange_thick: 13.0,
        root_r: Some(root_r),
    }
    .to_section(SectionId(0), "圧延H形鋼".into())
}

#[test]
fn issue_1912_gates_both_full_section_ratios_on_the_compressed_web() {
    let model = issue_model();
    let section = rolled_h(200.0, 2.0, 0.0);
    for sign in [1.0, -1.0] {
        let forces = MemberForcesAt {
            mz: sign * 100e6,
            qz: 0.0,
            ..ISSUE_FORCES
        };
        let outcome = check(
            &model,
            &section,
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        assert!(matches!(outcome, CheckOutcome::Skipped { reason }
            if reason.contains("局部座屈") && reason.contains("ウェブ d/t=187.0000（上限 47.2567）")));

        let control = check(
            &model,
            &model.sections[0],
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        let CheckOutcome::Checked(result) = control else {
            panic!("{control:?}");
        };
        assert!(result.ok(), "{result:?}");
        // A=8192 mm²、Z=1148243.413333333 mm³、短期 ft=235、fb=192.3854186252 N/mm²。
        let axial_bending = &result.components[0];
        assert_eq!(axial_bending.kind, CheckKind::AxialBending);
        assert!(
            (axial_bending.ratio - 0.410_591_328_440_000_8).abs() < 1e-12,
            "{result:?}"
        );
        assert!(axial_bending.detail.contains("式(6.3) 引張側=0.4106"));
        assert!(axial_bending.detail.contains("式(6.4) 曲げ圧縮側=0.4038"));
    }
}

#[test]
fn a_slender_plate_is_checked_only_if_it_has_compression() {
    let model = issue_model();
    let web_slender = rolled_h(200.0, 2.0, 0.0);
    let flange_slender = rolled_h(500.0, 10.0, 0.0);

    // 曲げ勾配 1/8 N/mm³ に対し、ウェブ端 y=187 mm とフランジ端 y=200 mm が応力度ゼロの境界。
    for (section, boundary_stress_mpa, compressed_plate) in [
        (&web_slender, 23.375, "ウェブ d/t=187.0000"),
        (&flange_slender, 25.0, "下フランジ b/t=19.2308"),
    ] {
        for (offset_mpa, has_compression) in [(-0.125, true), (0.0, false), (0.125, false)] {
            let forces = MemberForcesAt {
                n: section.area * (boundary_stress_mpa + offset_mpa),
                mz: -section.iy / 8.0,
                qz: 0.0,
                ..ISSUE_FORCES
            };
            let outcome = check(
                &model,
                section,
                &forces,
                LoadTerm::Short,
                MemberKind::Column,
            );
            if has_compression {
                assert!(matches!(outcome, CheckOutcome::Skipped { reason }
                    if reason.contains(compressed_plate)));
            } else {
                assert!(matches!(outcome, CheckOutcome::Checked(result) if result.ok()));
            }
        }
    }
}

#[test]
fn rolled_and_welded_h_use_different_web_widths() {
    let model = issue_model();
    let rolled = rolled_h(200.0, 7.5, 16.0);
    let built = SectionShape::SteelBuiltH {
        height: 400.0,
        upper_width: 200.0,
        upper_thick: 13.0,
        lower_width: 200.0,
        lower_thick: 13.0,
        web_thick: 7.5,
    }
    .to_section(SectionId(0), "溶接組立H形鋼".into());
    let forces = MemberForcesAt {
        mz: 100e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    // 圧延材はフィレット先端間342 mm、溶接組立材は内法374 mm。
    let rolled_result = check(
        &model,
        &rolled,
        &forces,
        LoadTerm::Short,
        MemberKind::Column,
    );
    assert!(matches!(rolled_result, CheckOutcome::Checked(result) if result.ok()));
    let built_result = check(&model, &built, &forces, LoadTerm::Short, MemberKind::Column);
    assert!(matches!(built_result, CheckOutcome::Skipped { reason }
        if reason.contains("ウェブ d/t=49.8667（上限 47.2567）")));
}

#[test]
fn a_rolled_section_with_supplied_properties_can_use_the_web_clear_depth() {
    let model = issue_model();
    let mut section = rolled_h(200.0, 7.5, 16.0);
    section.property_basis.area = PropertyBasis::Supplied;
    section.property_basis.iy = PropertyBasis::Supplied;
    section.property_basis.iz = PropertyBasis::Supplied;
    section.property_basis.j = PropertyBasis::Supplied;
    let SectionShape::SteelH { root_r, .. } = section.shape.as_mut().unwrap() else {
        unreachable!()
    };
    *root_r = None;
    let forces = MemberForcesAt {
        mz: 100e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    let outcome = check(
        &model,
        &section,
        &forces,
        LoadTerm::Short,
        MemberKind::Column,
    );
    assert!(matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("ウェブ d/t=49.8667（上限 47.2567）")));

    let mut section = model.sections[0].clone();
    section.property_basis.area = PropertyBasis::Supplied;
    section.property_basis.iy = PropertyBasis::Supplied;
    section.property_basis.iz = PropertyBasis::Supplied;
    section.property_basis.j = PropertyBasis::Supplied;
    let SectionShape::SteelH { root_r, .. } = section.shape.as_mut().unwrap() else {
        unreachable!()
    };
    *root_r = None;
    let outcome = check(
        &model,
        &section,
        &forces,
        LoadTerm::Short,
        MemberKind::Column,
    );
    assert!(matches!(outcome, CheckOutcome::Checked(result) if result.ok()));
    for (area, iy) in [
        (PropertyBasis::Shape, PropertyBasis::Supplied),
        (PropertyBasis::Supplied, PropertyBasis::Shape),
    ] {
        section.property_basis.area = area;
        section.property_basis.iy = iy;
        let outcome = check(
            &model,
            &section,
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        assert!(matches!(outcome, CheckOutcome::Skipped { reason }
            if reason.contains("フィレット半径が未知") && reason.contains("全断面性能を確定できません")));
    }
    section.property_basis.iy = PropertyBasis::PendingShape;
    assert!(
        matches!(check(&model, &section, &forces, LoadTerm::Short, MemberKind::Column),
        CheckOutcome::Skipped { reason } if reason.contains("全断面性能を確定できません"))
    );
}

#[test]
fn inconsistent_shape_derived_properties_cannot_establish_plate_compression() {
    let model = issue_model();
    let section = rolled_h(200.0, 8.0, 16.0);
    let forces = MemberForcesAt {
        mz: 100e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    for (area_factor, iy_factor) in [(1.01, 1.0), (1.0, 1.01)] {
        let mut section = section.clone();
        section.area *= area_factor;
        section.iy *= iy_factor;
        let outcome = check(
            &model,
            &section,
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        assert!(matches!(outcome, CheckOutcome::Skipped { reason }
            if reason.contains("全断面性能を確定できません") && reason.contains("一致しません")));
    }
}

#[test]
fn slenderness_uses_each_plates_actual_thickness_to_select_f() {
    let mut model = issue_model();
    model.materials[0].name = "SN400B".into();
    let section = SectionShape::SteelBuiltH {
        height: 870.0,
        upper_width: 1440.0,
        upper_thick: 45.0,
        lower_width: 1440.0,
        lower_thick: 45.0,
        web_thick: 16.0,
    }
    .to_section(SectionId(0), "溶接組立H形鋼".into());
    let forces = MemberForcesAt {
        mz: 100e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    let outcome = check(
        &model,
        &section,
        &forces,
        LoadTerm::Short,
        MemberKind::Column,
    );
    // 16 mm の F=235 では d/t=48.75 が超過。45 mm の F=215 では b/t=16 が上限内。
    assert!(matches!(outcome, CheckOutcome::Skipped { reason }
        if reason.contains("ウェブ d/t=48.7500（上限 47.2567）") && !reason.contains("フランジ")));
}

#[test]
fn slender_plates_leave_independent_failures_and_unavailability_visible() {
    let model = issue_model();
    let section = rolled_h(200.0, 2.0, 0.0);
    let forces = MemberForcesAt {
        n: 1_500_000.0,
        mz: 400e6,
        qy: 200_000.0,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    let mut ctx = column_context(&model, LoadTerm::Short);
    ctx.column_lateral_buckling_length = Some(Err("実材端の横座屈支点が未確認です"));
    let outcomes: Vec<_> = SteelDesign
        .check_outcomes(&forces, &section, &model.materials[0], &ctx)
        .collect();
    assert!(
        matches!(outcomes.as_slice(), [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }]
        if !result.ok()
            && result.components.iter().map(|c| c.kind).eq([CheckKind::Axial, CheckKind::Shear])
            && reason.contains("局部座屈")
            && reason.contains("横座屈支点が未確認"))
    );
}

#[test]
fn a_supplied_full_section_area_retains_the_independent_axial_failure() {
    let model = issue_model();
    let mut section = rolled_h(200.0, 2.0, 0.0);
    section.property_basis.area = PropertyBasis::Supplied;
    section.area = 5950.0;
    let forces = MemberForcesAt {
        n: 1_500_000.0,
        mz: 400e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    let ctx = column_context(&model, LoadTerm::Short);
    let outcomes: Vec<_> = SteelDesign
        .check_outcomes(&forces, &section, &model.materials[0], &ctx)
        .collect();
    assert!(
        matches!(outcomes.as_slice(), [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }]
        if matches!(&result.components[..], [axial]
                if axial.kind == CheckKind::Axial && (axial.ratio - 1.072_769_533_345_253_2).abs() < 1e-12)
            && reason.contains("局部座屈"))
    );
}

#[test]
fn issue_1931_applies_the_web_limit_for_each_loading() {
    let model = issue_model();
    let slender = rolled_h(200.0, 2.0, 0.0);
    let intermediate = rolled_h(200.0, 6.0, 0.0);
    let compact = &model.sections[0];
    // 短期、d=374 mm、F=235、E=205000。中間断面の d/t は62.3333で、軸力によって緩和の成否が分かれる。
    for (section, n, mz, rejected_by) in [
        (&slender, 0.0, 100e6, Some("上限 70.8850")),
        (&slender, 0.0, 1_000e6, Some("上限 70.8850")),
        (&intermediate, 0.0, -100e6, None),
        (&intermediate, -77_000.0, 0.0, Some("上限 47.2567")),
        (compact, -77_000.0, 0.0, None),
        (&intermediate, -77_000.0, 100e6, None),
        (&intermediate, -190_850.0, 100e6, None),
        (&intermediate, -190_864.0, 100e6, Some("ウェブ d/t=62.3333")),
        (&intermediate, -300_000.0, 100e6, Some("上限 57.4430")),
        (&intermediate, -600_000.0, 100e6, Some("上限 47.2567")),
        (compact, -800_000.0, 20e6, None),
    ] {
        let forces = MemberForcesAt {
            n,
            mz,
            qz: 0.0,
            ..ISSUE_FORCES
        };
        let outcome = check(
            &model,
            section,
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        if let Some(limit) = rejected_by {
            assert!(
                matches!(&outcome, CheckOutcome::Skipped { reason }
                if reason.contains("ウェブ d/t") && reason.contains(limit)),
                "{outcome:?}"
            );
        } else {
            assert!(
                matches!(&outcome, CheckOutcome::Checked(result)
                if result.ok() && result.components[0].kind == CheckKind::AxialBending),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn issue_1931_checks_the_flange_that_can_be_in_compression() {
    let model = issue_model();
    let section = rolled_h(500.0, 8.0, 0.0);
    for (n, mz, flange) in [
        (0.0, 100e6, "上フランジ"),
        (0.0, -100e6, "下フランジ"),
        (-77_000.0, 0.0, "上下フランジ"),
        (-77_000.0, -100e6, "上下フランジ"),
    ] {
        let forces = MemberForcesAt {
            n,
            mz,
            qz: 0.0,
            ..ISSUE_FORCES
        };
        let outcome = check(
            &model,
            &section,
            &forces,
            LoadTerm::Short,
            MemberKind::Column,
        );
        assert!(
            matches!(&outcome, CheckOutcome::Skipped { reason }
            if reason.contains(&format!("{flange} b/t=19.2308（上限 15.6538）"))
                && !reason.contains("ウェブ d/t")),
            "{outcome:?}"
        );
    }
}

#[test]
fn issue_1931_requires_known_flat_web_area_to_relax_the_compression_limit() {
    let model = issue_model();
    let with_fillet = rolled_h(200.0, 6.0, 16.0);
    let mut unknown = with_fillet.clone();
    unknown.property_basis.area = PropertyBasis::Supplied;
    unknown.property_basis.iy = PropertyBasis::Supplied;
    unknown.property_basis.iz = PropertyBasis::Supplied;
    unknown.property_basis.j = PropertyBasis::Supplied;
    let SectionShape::SteelH { root_r, .. } = unknown.shape.as_mut().unwrap() else {
        unreachable!()
    };
    *root_r = None;
    let forces = MemberForcesAt {
        n: -77_000.0,
        mz: 100e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    let known = check(
        &model,
        &with_fillet,
        &forces,
        LoadTerm::Short,
        MemberKind::Column,
    );
    assert!(matches!(known, CheckOutcome::Checked(result) if result.ok()));
    let unresolved = check(
        &model,
        &unknown,
        &forces,
        LoadTerm::Short,
        MemberKind::Column,
    );
    assert!(
        matches!(&unresolved, CheckOutcome::Skipped { reason }
        if reason.contains("ウェブ d/t=62.3333（上限 47.2567）")),
        "{unresolved:?}"
    );
    let pure_bending = check(
        &model,
        &unknown,
        &MemberForcesAt { n: 0.0, ..forces },
        LoadTerm::Short,
        MemberKind::Column,
    );
    assert!(matches!(pure_bending, CheckOutcome::Checked(result) if result.ok()));
}

#[test]
fn issue_1931_preserves_independent_compression_and_web_shear_failures() {
    let model = issue_model();
    let section = rolled_h(200.0, 2.0, 0.0);
    let mut ctx = column_context(&model, LoadTerm::Short);
    ctx.lk_z = Some(7_000.0);
    for mz in [0.0, 100e6] {
        // N/A=134.5 N/mm² は短期材料許容上限235以下だが、弱軸座屈を考慮したfcを超える。
        let forces = MemberForcesAt {
            n: -800_000.0,
            mz,
            qy: 200_000.0,
            qz: 0.0,
            ..ISSUE_FORCES
        };
        let outcomes: Vec<_> = SteelDesign
            .check_outcomes(&forces, &section, &model.materials[0], &ctx)
            .collect();
        assert!(
            matches!(outcomes.as_slice(), [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }]
            if !result.ok()
                && result.components.iter().map(|c| c.kind).eq([CheckKind::Axial, CheckKind::Shear])
                && result.components[0].ratio > 1.0
                && result.components[0].detail.contains("fc=")
                && reason.contains("局部座屈")),
            "{outcomes:?}"
        );
    }
}

#[test]
fn issue_1931_keeps_the_statutory_slenderness_failure() {
    let model = issue_model();
    let section = rolled_h(200.0, 2.0, 0.0);
    let mut ctx = column_context(&model, LoadTerm::Short);
    ctx.lk_z = Some(20_000.0);
    let forces = MemberForcesAt {
        n: -77_000.0,
        mz: 100e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    let outcomes: Vec<_> = SteelDesign
        .check_outcomes(&forces, &section, &model.materials[0], &ctx)
        .collect();
    assert!(
        matches!(outcomes.as_slice(), [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }]
        if matches!(&result.components[..], [provision] if provision.kind == CheckKind::Provision && provision.ratio > 1.0)
            && reason.contains("局部座屈")),
        "{outcomes:?}"
    );
}

#[test]
fn issue_1931_keeps_an_independent_material_failure_if_buckling_length_is_unknown() {
    let model = issue_model();
    let section = rolled_h(200.0, 2.0, 0.0);
    let mut ctx = column_context(&model, LoadTerm::Short);
    ctx.lk_z = Some(f64::NAN);
    let forces = MemberForcesAt {
        n: -1_500_000.0,
        mz: 100e6,
        qz: 0.0,
        ..ISSUE_FORCES
    };
    let outcomes: Vec<_> = SteelDesign
        .check_outcomes(&forces, &section, &model.materials[0], &ctx)
        .collect();
    assert!(
        matches!(outcomes.as_slice(), [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }]
        if matches!(&result.components[..], [axial] if axial.kind == CheckKind::Axial && axial.detail.contains("材料許容上限="))
            && reason.contains("座屈長さ") && reason.contains("局部座屈")),
        "{outcomes:?}"
    );
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
