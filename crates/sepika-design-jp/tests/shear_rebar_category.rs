#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod rc_beam;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod rc_column;
#[path = "fixtures/src_h_steel_product_model.rs"]
mod src;

use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId};
use sepika_core::model::{default_combinations, default_load_cases, MaterialCategory, Model};
use sepika_core::section_shape::{RebarSize, SectionShape, StandardRebar};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, ShearMethod, StaticBeamHingeMechanism, UltimateCheck,
    UltimateShearOptions,
};
use sepika_design_jp::{
    run_member_design_checks, BondMethod, CheckComponent, CheckKind, CheckOutcome, LoadTerm,
    MemberDesignCheckOptions, QdMethod, RcColumnLongTermShearInput,
};
use sepika_element::frame::beam::MemberForces;

fn assign_shear_rebar(model: &mut Model, category: MaterialCategory) -> MaterialId {
    let previous = model.sections[0].shear_rebar_material.unwrap();
    let mut material = model.materials[previous.index()].clone();
    material.id = MaterialId(model.materials.len() as u32);
    material.category = category;
    model.sections[0].shear_rebar_material = Some(material.id);
    model.materials.push(material);
    previous
}

fn assert_diagnostic(reason: &str, model: &Model, member: &str, category: MaterialCategory) {
    for text in [
        "部材 ID 0",
        member,
        &format!("断面「{}」", model.sections[0].name),
        "せん断補強筋材料「SD345」",
        &format!("材料区分は「{}」", category.label()),
        "断面タブでせん断補強筋に「鉄筋」区分の材料を割り当て",
    ] {
        assert!(reason.contains(text), "{text}: {reason}");
    }
}

fn rc_members() -> [(Model, &'static str); 3] {
    [
        (rc_beam::model(false), "RC梁"),
        (rc_column::column_model(false, 0.0), "RC柱"),
        (rc_column::column_model(true, 0.0), "RC柱"),
    ]
}

fn src_members() -> [(Model, &'static str); 2] {
    [
        (src::model(false, "SN400B", 0.0, 0.0), "SRC梁"),
        (src::model(true, "SN400B", 0.0, 0.0), "SRC柱"),
    ]
}

fn allowable(
    model: &Model,
    forces: [f64; 6],
    opts: &MemberDesignCheckOptions<'_>,
) -> (Vec<CheckComponent>, Vec<String>) {
    let results = run_member_design_checks(
        model,
        &[(
            ElemId(0),
            MemberForces {
                at: vec![(0.5, forces)],
            },
        )],
        &[],
        opts,
    )
    .unwrap();
    let mut components = Vec::new();
    let mut diagnostics = Vec::new();
    for (id, pos, result) in results.member_checks {
        assert_eq!((id, pos), (ElemId(0), 0.5));
        match result {
            CheckOutcome::Checked(result) => components.extend(result.components),
            CheckOutcome::Skipped { reason } => diagnostics.push(reason),
        }
    }
    (components, diagnostics)
}

#[test]
fn allowable_requires_rebar_category_and_rechecks_same_grade_and_fy_after_correction() {
    let opts = MemberDesignCheckOptions {
        term: LoadTerm::Short,
        bond_method: BondMethod::Rc1999,
        ..Default::default()
    };
    let demand = [0.0, 1_000.0, 0.0, 0.0, 0.0, 1_000_000.0];
    for (mut model, member) in rc_members().into_iter().chain(src_members()) {
        let (normal, diagnostics) = allowable(&model, demand, &opts);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(normal.iter().any(|row| row.kind == CheckKind::Shear));
        assert!(normal.iter().all(|row| row.ratio <= 1.0));

        for category in [MaterialCategory::Steel, MaterialCategory::Concrete] {
            let original = assign_shear_rebar(&mut model, category);
            let (partial, diagnostics) = allowable(&model, demand, &opts);
            assert!(partial.is_empty(), "{partial:?}");
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert_diagnostic(&diagnostics[0], &model, member, category);
            model.materials[model.sections[0].shear_rebar_material.unwrap().index()].category =
                MaterialCategory::Rebar;
            assert_eq!(
                allowable(&model, demand, &opts),
                (normal.clone(), Vec::new())
            );
            model.sections[0].shear_rebar_material = Some(original);
        }
    }
}

#[test]
fn allowable_rc_keeps_bending_and_cover_but_only_independent_bond_and_shear() {
    for (mut model, member) in rc_members() {
        match model.sections[0].shape.as_mut().unwrap() {
            SectionShape::RcBeamRect { rebar, .. } => rebar.cover = 20.0,
            SectionShape::RcColumnRect { rebar, .. } => rebar.cover = 20.0,
            SectionShape::RcColumnCircle { rebar, .. } => rebar.cover = 20.0,
            _ => unreachable!(),
        }
        let demand = [0.0, 10_000_000.0, 0.0, 0.0, 0.0, 2_000_000_000.0];
        for term in [LoadTerm::Long, LoadTerm::Short] {
            for bond_method in [BondMethod::Rc1991, BondMethod::Rc1999] {
                let opts = MemberDesignCheckOptions {
                    term,
                    bond_method,
                    rc_column_long_term_shear: RcColumnLongTermShearInput::SingleCase,
                    ..Default::default()
                };
                let (normal, _) = allowable(&model, demand, &opts);
                let mut wrong = model.clone();
                assign_shear_rebar(&mut wrong, MaterialCategory::Steel);
                let (partial, diagnostics) = allowable(&wrong, demand, &opts);
                assert_diagnostic(&diagnostics[0], &wrong, member, MaterialCategory::Steel);
                let beam = member == "RC梁";
                let expected: Vec<_> = normal
                    .into_iter()
                    .filter(|row| {
                        row.ratio > 1.0
                            && match row.kind {
                                CheckKind::Shear => !beam && term == LoadTerm::Long,
                                CheckKind::Bond => {
                                    beam && (term == LoadTerm::Long
                                        || bond_method == BondMethod::Rc1991)
                                }
                                _ => true,
                            }
                    })
                    .collect();
                assert_eq!(partial, expected);
                assert!(partial.iter().any(|row| row.kind == CheckKind::Provision));
                assert!(partial.iter().any(|row| row.kind
                    == if beam {
                        CheckKind::Bending
                    } else {
                        CheckKind::AxialBending
                    }));
                if beam && (term == LoadTerm::Long || bond_method == BondMethod::Rc1991) {
                    assert!(partial.iter().any(|row| row.kind == CheckKind::Bond));
                }
                if !beam && term == LoadTerm::Long {
                    assert!(partial.iter().any(|row| row.kind == CheckKind::Shear));
                }
            }
        }
    }
}

#[test]
fn allowable_retains_rc_beam_shear_if_reinforcement_term_is_zero() {
    let mut model = rc_beam::model(false);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        unreachable!()
    };
    rebar.stirrup.pitch = 500.0;
    let opts = MemberDesignCheckOptions {
        term: LoadTerm::Short,
        ..Default::default()
    };
    let demand = [0.0, 1_000_000.0, 0.0, 0.0, 0.0, 0.0];
    let (normal, _) = allowable(&model, demand, &opts);
    assign_shear_rebar(&mut model, MaterialCategory::Concrete);
    let (partial, diagnostics) = allowable(&model, demand, &opts);
    assert_diagnostic(&diagnostics[0], &model, "RC梁", MaterialCategory::Concrete);
    let shear = |rows: Vec<CheckComponent>| {
        rows.into_iter()
            .find(|row| row.kind == CheckKind::Shear)
            .unwrap()
    };
    assert_eq!(shear(normal), shear(partial));
}

#[test]
fn allowable_src_keeps_steel_shear_but_withholds_dependent_rc_shear() {
    for (mut model, member) in src_members() {
        for term in [LoadTerm::Long, LoadTerm::Short] {
            let opts = MemberDesignCheckOptions {
                term,
                ..Default::default()
            };
            let demand = [0.0, 10_000_000.0, 0.0, 0.0, 0.0, 2_000_000_000.0];
            let (normal, _) = allowable(&model, demand, &opts);
            let original = assign_shear_rebar(&mut model, MaterialCategory::Steel);
            let (partial, diagnostics) = allowable(&model, demand, &opts);
            assert_diagnostic(&diagnostics[0], &model, member, MaterialCategory::Steel);
            assert!(partial.iter().all(|row| row.ratio > 1.0));
            assert!(partial.iter().any(|row| {
                matches!(row.kind, CheckKind::AxialBending | CheckKind::Bending)
                    && normal.contains(row)
            }));
            let shear = partial
                .iter()
                .find(|row| row.kind == CheckKind::Shear)
                .unwrap();
            if member == "SRC柱" && term == LoadTerm::Long {
                assert!(normal.contains(shear));
            } else {
                assert!(shear.detail.contains("sQA") && shear.detail.contains("未算定"));
            }
            model.sections[0].shear_rebar_material = Some(original);
        }
    }
}

#[test]
fn allowable_beams_keep_bending_failure_and_material_diagnostic_without_gravity() {
    for (mut model, member) in [
        rc_members().into_iter().next().unwrap(),
        src_members().into_iter().next().unwrap(),
    ] {
        assign_shear_rebar(&mut model, MaterialCategory::Steel);
        let (partial, diagnostics) = allowable(
            &model,
            [0.0, 1_000_000.0, 0.0, 0.0, 0.0, 2_000_000_000.0],
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                bond_method: BondMethod::Rc1991,
                long_member_forces: Some(&[]),
                qd_method: QdMethod::Qd2,
                ..Default::default()
            },
        );
        assert_eq!(partial.len(), 1, "{partial:?}");
        assert_eq!(partial[0].kind, CheckKind::Bending);
        assert!(partial[0].ratio > 1.0);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_diagnostic(&diagnostics[0], &model, member, MaterialCategory::Steel);
        assert!(diagnostics[0].contains("重力参照内力が不足"));
    }
}

fn ultimate(model: &Model, demand: MemberDemand, opts: &UltimateShearOptions) -> UltimateCheck {
    let checks = collect_rc_ultimate_checks(model, &[(ElemId(0), demand)], opts).unwrap();
    let [result] = checks.try_into().expect("RC部材1本の終局検定結果");
    assert_eq!(result.elem, ElemId(0));
    result
}

fn set_ultimate_input(model: &mut Model) {
    model.load_cases = default_load_cases();
    model.combinations = default_combinations();
    model.ultimate_gravity_combination = Some(0);
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
}

#[test]
fn ultimate_withholds_shear_and_bond_and_keeps_independent_moment_and_axial_capacity() {
    for (mut model, member) in rc_members() {
        set_ultimate_input(&mut model);

        let static_demand = MemberDemand {
            mz: 1e6,
            q_long: Some(0.0),
            q_long_weak: Some(0.0),
            static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
            ..Default::default()
        };
        let total_demand = MemberDemand::from_pushover(0.0, 1e6, 0.0, 1000.0, 0.0, 0.0);
        for shear_method in [ShearMethod::Plastic, ShearMethod::Ductility] {
            let opts = UltimateShearOptions {
                shear_method,
                biaxial_shear: true,
                biaxial_bending: true,
                ..Default::default()
            };
            for demand in [static_demand, total_demand] {
                let normal = ultimate(&model, demand, &opts);
                assert!(normal.qsu.is_finite() && normal.qsu > 0.0, "{normal:?}");
                assert!(normal.qbu.is_finite() && normal.qbu > 0.0, "{normal:?}");

                let original = assign_shear_rebar(&mut model, MaterialCategory::Steel);
                let partial = ultimate(&model, demand, &opts);
                assert!(!partial.ok);
                assert_diagnostic(
                    partial.unchecked_reason.as_deref().unwrap(),
                    &model,
                    member,
                    MaterialCategory::Steel,
                );
                assert!(partial.qsu.is_nan() && partial.shear_margin.is_nan());
                assert!(partial.qbu.is_nan() && partial.bond_margin.is_nan());
                assert!(partial.biaxial_shear_margin.is_none());
                assert_eq!(partial.mu, normal.mu);
                assert_eq!(
                    partial.biaxial_bending_margin,
                    normal.biaxial_bending_margin
                );
                let axial = |check: &UltimateCheck| check.axial.map(|axial| (axial.nuc, axial.nut));
                assert_eq!(axial(&partial), axial(&normal));
                if member == "RC柱" {
                    let failed = ultimate(&model, MemberDemand { mz: 10e9, ..demand }, &opts);
                    assert!(failed.biaxial_bending_margin.unwrap() < 1.0);
                    assert!(!failed.ok && failed.unchecked_reason.is_none());
                    assert_diagnostic(&failed.detail, &model, member, MaterialCategory::Steel);
                }

                model.materials[model.sections[0].shear_rebar_material.unwrap().index()].category =
                    MaterialCategory::Rebar;
                let repaired = ultimate(&model, demand, &opts);
                assert_eq!(repaired.detail, normal.detail);
                assert_eq!(repaired.ok, normal.ok);
                assert_eq!(repaired.unchecked_reason, normal.unchecked_reason);
                model.sections[0].shear_rebar_material = Some(original);
            }
        }
    }
}

#[test]
fn ultimate_requires_finite_positive_shear_rebar_fy() {
    for (mut model, member) in rc_members() {
        set_ultimate_input(&mut model);
        assign_shear_rebar(&mut model, MaterialCategory::Rebar);
        let material_id = model.sections[0].shear_rebar_material.unwrap();
        let original_fy = model.materials[material_id.index()].fy;
        let demand = MemberDemand {
            mz: 1e6,
            q_long: Some(0.0),
            q_long_weak: Some(0.0),
            static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
            ..Default::default()
        };

        for shear_method in [ShearMethod::Plastic, ShearMethod::Ductility] {
            let opts = UltimateShearOptions {
                shear_method,
                biaxial_shear: true,
                biaxial_bending: true,
                ..Default::default()
            };
            let normal = ultimate(&model, demand, &opts);
            assert!(normal.qsu.is_finite() && normal.qsu > 0.0, "{normal:?}");
            assert!(normal.qbu.is_finite() && normal.qbu > 0.0, "{normal:?}");
            for fy in [
                Some(f64::INFINITY),
                Some(f64::NAN),
                Some(f64::NEG_INFINITY),
                Some(0.0),
                Some(-1.0),
            ] {
                model.materials[material_id.index()].fy = fy;
                let partial = ultimate(&model, demand, &opts);
                assert!(!partial.ok);
                assert_fy_diagnostic(partial.unchecked_reason.as_deref().unwrap(), member);
                assert!(partial.qsu.is_nan() && partial.shear_margin.is_nan());
                assert_eq!(partial.qbu, normal.qbu);
                if normal.bond_margin.is_nan() {
                    assert!(partial.bond_margin.is_nan());
                } else {
                    assert_eq!(partial.bond_margin, normal.bond_margin);
                }
                assert!(partial.biaxial_shear_margin.is_none());
                assert_eq!(partial.mu, normal.mu);
                assert_eq!(
                    partial.biaxial_bending_margin,
                    normal.biaxial_bending_margin
                );
                let axial =
                    |result: &UltimateCheck| result.axial.map(|axial| (axial.nuc, axial.nut));
                assert_eq!(axial(&partial), axial(&normal));
                if member == "RC柱" {
                    let failed = ultimate(&model, MemberDemand { mz: 10e9, ..demand }, &opts);
                    assert!(failed.biaxial_bending_margin.unwrap() < 1.0);
                    assert!(!failed.ok && failed.unchecked_reason.is_none());
                    assert_fy_diagnostic(&failed.detail, member);
                }
            }
            model.materials[material_id.index()].fy = original_fy;
            let repaired = ultimate(&model, demand, &opts);
            assert_eq!(repaired.qsu, normal.qsu);
            assert_eq!(repaired.qbu, normal.qbu);
            assert_eq!(repaired.unchecked_reason, normal.unchecked_reason);
        }
    }
}

fn assert_fy_diagnostic(reason: &str, member: &str) {
    for text in [
        "部材 ID 0",
        "せん断補強筋材料「SD345」",
        "降伏強度 Fy",
        "材料タブ",
        "正の有限値",
    ] {
        assert!(reason.contains(text), "{member} {text}: {reason}");
    }
}

#[test]
fn allowable_shear_uses_the_supported_grade_table_with_infinite_fy() {
    let opts = MemberDesignCheckOptions {
        term: LoadTerm::Short,
        ..Default::default()
    };
    let demand = [0.0, 1_000.0, 0.0, 0.0, 0.0, 1_000_000.0];
    for (mut model, member) in rc_members().into_iter().chain(src_members()) {
        let baseline = allowable(&model, demand, &opts);
        assert!(baseline.0.iter().any(|row| row.kind == CheckKind::Shear));
        assert!(baseline.1.is_empty(), "{member}: {:?}", baseline.1);
        assign_shear_rebar(&mut model, MaterialCategory::Rebar);
        let material_id = model.sections[0].shear_rebar_material.unwrap();
        model.materials[material_id.index()].fy = Some(f64::INFINITY);
        assert_eq!(allowable(&model, demand, &opts), baseline, "{member}");
    }
}

#[test]
fn static_ultimate_keeps_independent_shear_and_bond_failures_with_invalid_fy() {
    for (mut model, member) in rc_members() {
        set_ultimate_input(&mut model);
        match model.sections[0].shape.as_mut().unwrap() {
            SectionShape::RcBeamRect { rebar, .. } => rebar.stirrup.pitch = 0.0,
            SectionShape::RcColumnRect { rebar, .. } => rebar.hoop.pitch = 0.0,
            SectionShape::RcColumnCircle { rebar, .. } => rebar.hoop.pitch = 0.0,
            _ => unreachable!(),
        }
        let demand = MemberDemand {
            mz: -1e7,
            q_long: Some(10e6),
            static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
            ..Default::default()
        };
        let opts = UltimateShearOptions {
            shear_method: ShearMethod::Plastic,
            ..Default::default()
        };
        let baseline = ultimate(&model, demand, &opts);
        assign_shear_rebar(&mut model, MaterialCategory::Rebar);
        model.materials.last_mut().unwrap().fy = Some(f64::INFINITY);
        let partial = ultimate(&model, demand, &opts);
        assert_eq!(partial.qsu, baseline.qsu);
        assert_eq!(partial.qbu, baseline.qbu);
        assert_eq!(partial.shear_margin, baseline.shear_margin);
        assert_eq!(partial.bond_margin, baseline.bond_margin);
        assert!(partial.shear_margin < 1.0 && partial.bond_margin < 1.0);
        assert!(!partial.ok && partial.unchecked_reason.is_none());
        assert_fy_diagnostic(&partial.detail, member);
    }
}

#[test]
fn static_ultimate_keeps_bond_failure_with_positive_stirrup_ratio_and_invalid_fy() {
    let mut model = rc_beam::model(false);
    set_ultimate_input(&mut model);
    let Some(SectionShape::RcBeamRect { b, rebar, .. }) = &model.sections[0].shape else {
        unreachable!()
    };
    assert!(rebar.pw(*b) > 0.0);
    let demand = MemberDemand {
        mz: -1e7,
        q_long: Some(10e6),
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
        ..Default::default()
    };
    let baseline = model.clone();
    assign_shear_rebar(&mut model, MaterialCategory::Rebar);
    model.materials.last_mut().unwrap().fy = Some(f64::INFINITY);

    for shear_method in [ShearMethod::Plastic, ShearMethod::Ductility] {
        let opts = UltimateShearOptions {
            shear_method,
            ..Default::default()
        };
        let normal = ultimate(&baseline, demand, &opts);
        let partial = ultimate(&model, demand, &opts);
        assert!(partial.qsu.is_nan() && partial.shear_margin.is_nan());
        assert!(partial.qbu.is_finite() && partial.qbu > 0.0);
        assert_eq!(partial.qbu, normal.qbu);
        assert_eq!(partial.bond_margin, 0.0);
        assert!(!partial.ok && partial.unchecked_reason.is_none());
        assert!(partial
            .detail
            .contains("上端引張・下端引張の双方の付着耐力を超えます"));
        assert_fy_diagnostic(&partial.detail, "RC梁");
    }
}

#[test]
fn ultimate_retains_confirmed_shear_failure_without_reinforcement_contribution() {
    for (mut model, member) in rc_members() {
        set_ultimate_input(&mut model);
        match model.sections[0].shape.as_mut().unwrap() {
            SectionShape::RcBeamRect { rebar, .. } => rebar.stirrup.pitch = 0.0,
            SectionShape::RcColumnRect { rebar, .. } => rebar.hoop.pitch = 0.0,
            SectionShape::RcColumnCircle { rebar, .. } => rebar.hoop.pitch = 0.0,
            _ => unreachable!(),
        }
        let normal = model.clone();
        assign_shear_rebar(&mut model, MaterialCategory::Concrete);
        for shear_method in [ShearMethod::Plastic, ShearMethod::Ductility] {
            let opts = UltimateShearOptions {
                shear_method,
                ..Default::default()
            };
            for demand in [
                MemberDemand {
                    mz: -1e7,
                    q_long: Some(10e6),
                    static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
                    ..Default::default()
                },
                MemberDemand::from_pushover(0.0, -1e7, 0.0, 10e6, 0.0, 0.0),
            ] {
                let baseline = ultimate(&normal, demand, &opts);
                let partial = ultimate(&model, demand, &opts);
                assert_eq!(partial.qsu, baseline.qsu);
                assert_eq!(partial.shear_margin, baseline.shear_margin);
                assert!(partial.shear_margin < 1.0, "{partial:?}");
                assert!(!partial.ok && partial.unchecked_reason.is_none());
                match shear_method {
                    ShearMethod::Plastic => {
                        assert_eq!(partial.qbu, baseline.qbu);
                        assert_eq!(partial.bond_margin, baseline.bond_margin);
                        assert!(partial.bond_margin < 1.0);
                    }
                    ShearMethod::Ductility => {
                        assert!(partial.qbu.is_nan() && partial.bond_margin.is_nan());
                    }
                }
                assert_diagnostic(&partial.detail, &model, member, MaterialCategory::Concrete);
            }
        }
    }
}

#[test]
fn ultimate_retains_weak_axis_failure_if_only_its_shear_capacity_is_independent() {
    let mut model = rc_column::column_model(false, 0.0);
    model.load_cases = default_load_cases();
    model.combinations = default_combinations();
    model.ultimate_gravity_combination = Some(0);
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(0.0);
    let Some(SectionShape::RcColumnRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        unreachable!()
    };
    rebar.hoop.legs_y = 0;
    assign_shear_rebar(&mut model, MaterialCategory::Steel);
    for shear_method in [ShearMethod::Plastic, ShearMethod::Ductility] {
        let opts = UltimateShearOptions {
            shear_method,
            biaxial_shear: true,
            ..Default::default()
        };
        let damage = ultimate(
            &model,
            MemberDemand::from_pushover(0.0, 0.0, 0.0, 0.0, 10e6, 0.0),
            &opts,
        );
        assert!(damage.shear_margin.is_nan());
        assert!(damage.biaxial_shear_margin.is_none());
        assert!(!damage.ok && damage.unchecked_reason.is_none());
        assert!(
            damage.detail.contains("弱軸せん断耐力を超えます"),
            "{damage:?}"
        );
        assert_diagnostic(&damage.detail, &model, "RC柱", MaterialCategory::Steel);
    }
}

#[test]
fn allowable_rc_column_retains_independent_shear_in_the_weak_direction() {
    let mut model = rc_column::column_model(false, 0.0);
    let Some(SectionShape::RcColumnRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        unreachable!()
    };
    rebar.hoop.legs_y = 0;
    assign_shear_rebar(&mut model, MaterialCategory::Steel);
    let (partial, diagnostics) = allowable(
        &model,
        [0.0, 0.0, 10_000_000.0, 0.0, 0.0, 0.0],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            ..Default::default()
        },
    );
    assert_diagnostic(&diagnostics[0], &model, "RC柱", MaterialCategory::Steel);
    let shear = partial
        .iter()
        .find(|row| row.kind == CheckKind::Shear)
        .unwrap();
    assert!(shear.ratio > 1.0);
    assert!(shear.detail.contains("QAy=未算定"), "{shear:?}");
    assert!(!shear.detail.contains("QAz=未算定"), "{shear:?}");
}

#[test]
fn allowable_rc_columns_require_gravity_reference_for_each_independent_short_term_shear() {
    for circle in [false, true] {
        let mut model = rc_column::column_model(circle, 0.0);
        match model.sections[0].shape.as_mut().unwrap() {
            SectionShape::RcColumnRect { rebar, .. } => rebar.hoop.pitch = 0.0,
            SectionShape::RcColumnCircle { rebar, .. } => rebar.hoop.pitch = 0.0,
            _ => unreachable!(),
        }
        assign_shear_rebar(&mut model, MaterialCategory::Steel);
        let demand = [0.0, 20e6, 10e6, 0.0, 0.0, 0.0];
        for method in [QdMethod::Qd1, QdMethod::Min, QdMethod::Qd2] {
            let wrong_station = [(
                ElemId(0),
                MemberForces {
                    at: vec![(0.0, [0.0; 6])],
                },
            )];
            let (partial, diagnostics) = allowable(
                &model,
                demand,
                &MemberDesignCheckOptions {
                    term: LoadTerm::Short,
                    long_member_forces: Some(&wrong_station),
                    qd_method: method,
                    ..Default::default()
                },
            );
            assert!(partial.is_empty(), "{method:?}: {partial:?}");
            assert_diagnostic(&diagnostics[0], &model, "RC柱", MaterialCategory::Steel);
            for axis in ["Qy方向", "Qz方向"] {
                assert!(diagnostics[0].contains(&format!("{axis}の設計用せん断力")));
            }
        }

        let qz_only = [(
            ElemId(0),
            MemberForces {
                at: vec![(0.5, [0.0, f64::NAN, 0.0, 0.0, 0.0, 0.0])],
            },
        )];
        let opts = MemberDesignCheckOptions {
            term: LoadTerm::Short,
            long_member_forces: Some(&qz_only),
            qd_method: QdMethod::Qd2,
            ..Default::default()
        };
        let (partial, diagnostics) = allowable(&model, demand, &opts);
        assert!(diagnostics[0].contains("Qy方向"));
        assert!(!diagnostics[0].contains("Qz方向"));
        let (weak_axis, _) = allowable(&model, [0.0, 0.0, 10e6, 0.0, 0.0, 0.0], &opts);
        let [partial_shear] = partial.as_slice() else {
            panic!("{partial:?}");
        };
        let [weak_shear] = weak_axis.as_slice() else {
            panic!("{weak_axis:?}");
        };
        assert_eq!(partial_shear.kind, CheckKind::Shear);
        assert_eq!(partial_shear.ratio, weak_shear.ratio);
        assert!(partial_shear.ratio > 1.0);
    }
}

#[test]
fn allowable_rc1999_keeps_short_term_bond_failure_when_hoops_add_no_confinement() {
    for zero_pitch in [true, false] {
        let mut model = rc_beam::model(false);
        let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
            unreachable!()
        };
        if zero_pitch {
            rebar.stirrup.pitch = 0.0;
        } else {
            rebar.stirrup.legs = 0;
        }
        let opts = MemberDesignCheckOptions {
            term: LoadTerm::Short,
            bond_method: BondMethod::Rc1999,
            ..Default::default()
        };
        let demand = [0.0, 0.0, 0.0, 0.0, 0.0, 2e9];
        let (baseline, _) = allowable(&model, demand, &opts);
        let bond = baseline
            .iter()
            .find(|row| row.kind == CheckKind::Bond)
            .unwrap();
        assert!(bond.ratio > 1.0 && bond.detail.contains("W=0.000"));
        assign_shear_rebar(&mut model, MaterialCategory::Steel);
        let (partial, diagnostics) = allowable(&model, demand, &opts);
        assert!(partial.contains(bond));
        assert_diagnostic(&diagnostics[0], &model, "RC梁", MaterialCategory::Steel);
    }
}

#[test]
fn ultimate_keeps_zero_shear_capacity_when_the_adopted_formula_has_no_reinforcement_contribution() {
    for (method, fc, rp, pitch) in [
        (ShearMethod::Ductility, 24.0, 0.05, 100.0),
        (ShearMethod::Ductility, 24.0, 0.0, 2000.0),
        (ShearMethod::Plastic, 140.0, 0.0, 100.0),
    ] {
        let mut model = rc_column::column_model(false, 0.0);
        set_ultimate_input(&mut model);
        model.materials[0].fc = Some(fc);
        let Some(SectionShape::RcColumnRect { rebar, .. }) = model.sections[0].shape.as_mut()
        else {
            unreachable!()
        };
        rebar.hoop.pitch = pitch;

        let opts = UltimateShearOptions {
            shear_method: method,
            include_bond: true,
            ..Default::default()
        };
        let demand = MemberDemand::from_pushover(0.0, 0.0, 0.0, 10_000.0, 0.0, rp);
        let baseline = ultimate(&model, demand, &opts);
        assert_eq!(baseline.qsu, 0.0);
        assign_shear_rebar(&mut model, MaterialCategory::Concrete);
        let partial = ultimate(&model, demand, &opts);
        assert_eq!(partial.qsu, baseline.qsu);
        assert_eq!(partial.shear_margin, 0.0);
        assert!(!partial.ok && partial.unchecked_reason.is_none());
        assert_diagnostic(&partial.detail, &model, "RC柱", MaterialCategory::Concrete);

        if method == ShearMethod::Ductility {
            assert_eq!((baseline.qbu, baseline.bond_margin), (0.0, 0.0));
            assert_eq!(
                (partial.qbu, partial.bond_margin),
                (baseline.qbu, baseline.bond_margin)
            );
            assert!(
                partial
                    .detail
                    .contains("Vbu=0 N, τbu=未算定, 付着余裕度 Vbu/|Qtotal|=0.000"),
                "{partial:?}"
            );

            let main = model.sections[0].rebar_material.unwrap();
            model.materials[main.index()].name = "SR235".into();
            let round = ultimate(&model, demand, &opts);
            assert!(
                round.qbu.is_nan() && round.bond_margin.is_nan(),
                "{round:?}"
            );
            assert!(
                round.detail.contains("ultimate_bond_round_bar_unsupported"),
                "{round:?}"
            );

            model.materials[main.index()].category = MaterialCategory::Steel;
            let unknown = ultimate(&model, demand, &opts);
            assert!(
                unknown.qbu.is_nan() && unknown.bond_margin.is_nan(),
                "{unknown:?}"
            );
            assert!(
                unknown
                    .detail
                    .contains("数値径から異形鉄筋であることを確認できない"),
                "{unknown:?}"
            );
        }
    }
}

#[test]
fn ultimate_biaxial_shear_at_zero_capacity_requires_positive_demand_for_independent_failure() {
    for (method, fc, rp) in [
        (ShearMethod::Ductility, 24.0, 0.05),
        (ShearMethod::Plastic, 140.0, 0.0),
    ] {
        let mut model = rc_column::column_model(false, 0.0);
        set_ultimate_input(&mut model);
        model.materials[0].fc = Some(fc);
        assign_shear_rebar(&mut model, MaterialCategory::Steel);
        let opts = UltimateShearOptions {
            shear_method: method,
            biaxial_shear: true,
            include_bond: false,
            ..Default::default()
        };

        for (qy, qz) in [(0.0, 0.0), (10_000.0, 0.0), (0.0, 10_000.0)] {
            let demand = MemberDemand::from_pushover(0.0, 0.0, 0.0, qy, qz, rp);
            let result = ultimate(&model, demand, &opts);
            assert_eq!(result.qsu, 0.0, "{result:?}");
            assert!(!result.ok);
            if qy == 0.0 && qz == 0.0 {
                assert_eq!(result.biaxial_shear_margin, Some(f64::INFINITY));
                let reason = result
                    .unchecked_reason
                    .as_deref()
                    .expect("材料の未算定理由");
                assert_diagnostic(reason, &model, "RC柱", MaterialCategory::Steel);
            } else {
                assert_eq!(result.biaxial_shear_margin, Some(0.0));
                assert!(result.unchecked_reason.is_none(), "{result:?}");
                assert_diagnostic(&result.detail, &model, "RC柱", MaterialCategory::Steel);
            }
        }
    }
}

#[test]
fn both_invalid_rebar_assignments_keep_independent_failures_and_both_diagnostics() {
    let mut model = rc_beam::model(false);
    set_ultimate_input(&mut model);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D19);
    rebar.stirrup.legs = 0;
    let main = model.sections[0].rebar_material.unwrap();
    assign_shear_rebar(&mut model, MaterialCategory::Concrete);
    model.materials[main.index()].category = MaterialCategory::Steel;

    let assert_both_diagnostics = |reason: &str, model: &Model, member: &str| {
        assert_diagnostic(reason, model, member, MaterialCategory::Concrete);
        for expected in [
            "主筋材料「SD345」の材料区分は「鋼材」",
            "断面タブで主筋に「鉄筋」区分の材料を割り当て",
        ] {
            assert!(reason.contains(expected), "{reason}");
        }
    };

    let (partial, diagnostics) = allowable(
        &model,
        [0.0, 10e6, 0.0, 0.0, 0.0, 0.0],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            bond_method: BondMethod::Rc1991,
            ..Default::default()
        },
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_both_diagnostics(&diagnostics[0], &model, "RC梁");
    assert!(partial.iter().all(|row| row.ratio > 1.0));
    assert!(partial.iter().any(|row| row.kind == CheckKind::Shear));
    assert!(partial.iter().any(|row| row.kind == CheckKind::Bond));

    let result = ultimate(
        &model,
        MemberDemand::from_pushover(0.0, 0.0, 0.0, 10e6, 0.0, 0.0),
        &UltimateShearOptions {
            shear_method: ShearMethod::Plastic,
            ..Default::default()
        },
    );
    assert!(result.mu.is_nan());
    assert!(
        result.shear_margin < 1.0 && result.bond_margin < 1.0,
        "{result:?}"
    );
    assert!(!result.ok && result.unchecked_reason.is_none());
    assert_both_diagnostics(&result.detail, &model, "RC梁");

    for (mut model, member) in src_members() {
        let main = model.sections[0].rebar_material.unwrap();
        assign_shear_rebar(&mut model, MaterialCategory::Concrete);
        model.materials[main.index()].category = MaterialCategory::Steel;
        let (partial, diagnostics) = allowable(
            &model,
            [0.0, 10e6, 0.0, 0.0, 0.0, 0.0],
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                ..Default::default()
            },
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_both_diagnostics(&diagnostics[0], &model, member);
        let [steel_shear] = partial.as_slice() else {
            panic!("{partial:?}");
        };
        assert_eq!(steel_shear.kind, CheckKind::Shear);
        assert!(steel_shear.ratio > 1.0);
        assert!(steel_shear.detail.contains("sQA") && steel_shear.detail.contains("未算定"));
    }
}
