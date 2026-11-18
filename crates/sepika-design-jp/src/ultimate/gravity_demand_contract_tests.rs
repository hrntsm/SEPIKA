use super::*;

fn cft_model() -> sepika_core::model::Model {
    let mut cft = super::tests::column_and_beam_model();
    cft.elements.truncate(1);
    cft.sections[0].shape = Some(sepika_core::section_shape::SectionShape::CftBox {
        height: 400.0,
        width: 400.0,
        thick: 12.0,
        horizontal_thick: 12.0,
        corner_r: Some(0.0),
    });
    let mut steel = cft.materials[0].clone();
    steel.id = sepika_core::ids::MaterialId(1);
    steel.name = "SN400B".into();
    steel.category = sepika_core::model::MaterialCategory::Steel;
    steel.fy = Some(235.0);
    steel.fc = None;
    cft.materials.push(steel);
    cft.sections[0].steel_material = Some(sepika_core::ids::MaterialId(1));
    cft.steel_design_attrs = super::tests::cft_test_direct_lengths();
    cft
}

fn static_demands(
    model: &sepika_core::model::Model,
) -> Vec<(sepika_core::ids::ElemId, MemberDemand)> {
    model
        .elements
        .iter()
        .map(|e| {
            (
                e.id,
                MemberDemand {
                    q_long: Some(12000.0),
                    ..MemberDemand::axial(150000.0)
                },
            )
        })
        .collect()
}

#[test]
fn direct_rc_collector_rejects_missing_duplicate_nonfinite_and_unidentified_ql() {
    let model = super::tests::column_and_beam_model();
    let opts = UltimateShearOptions::default();
    assert!(collect_rc_ultimate_checks(&model, &[], &opts)
        .unwrap_err()
        .contains("ultimate_demand_missing"));
    let complete = static_demands(&model);
    assert_eq!(
        collect_rc_ultimate_checks(&model, &complete, &opts)
            .unwrap()
            .len(),
        2
    );
    let mut broken = complete.clone();
    broken.pop();
    assert!(collect_rc_ultimate_checks(&model, &broken, &opts)
        .unwrap_err()
        .contains("ID 1"));
    let mut broken = complete.clone();
    broken[0].1.q_long = None;
    assert!(collect_rc_ultimate_checks(&model, &broken, &opts)
        .unwrap_err()
        .contains("ultimate_gravity_missing"));
    let mut broken = complete.clone();
    broken[0].1.n_axial = f64::NAN;
    assert!(collect_rc_ultimate_checks(&model, &broken, &opts)
        .unwrap_err()
        .contains("ultimate_demand_nonfinite"));
    let mut broken = complete.clone();
    broken.push(complete[0]);
    assert!(collect_rc_ultimate_checks(&model, &broken, &opts)
        .unwrap_err()
        .contains("ultimate_demand_duplicate"));
}

#[test]
fn static_biaxial_shear_requires_explicit_zero_long_term_shear_on_both_axes() {
    let mut model = super::tests::column_and_beam_model();
    model.elements.truncate(1);
    let elem = model.elements[0].id;
    let opts = UltimateShearOptions {
        include_bond: false,
        biaxial_shear: true,
        ..Default::default()
    };
    let missing_weak =
        "ultimate_gravity_missing: 静的二軸せん断に必要な弱軸の長期せん断力がありません";
    for (strong, weak, reason) in [
        (0.0, Some(-0.0), None),
        (0.0, None, Some(missing_weak)),
        (12000.0, None, Some(missing_weak)),
        (12000.0, Some(0.0), Some("適用範囲外")),
        (0.0, Some(-4000.0), Some("適用範囲外")),
        (12000.0, Some(4000.0), Some("適用範囲外")),
    ] {
        let demand = MemberDemand {
            q_long: Some(strong),
            q_long_weak: weak,
            ..MemberDemand::axial(150000.0)
        };
        let checks = collect_rc_ultimate_checks(&model, &[(elem, demand)], &opts).unwrap();
        let check = &checks[0];
        assert!(check.shear_margin > 1.0);
        if let Some(reason) = reason {
            assert!(!check.ok);
            assert_eq!(check.biaxial_shear_margin, None);
            assert_resolved_demand_context(
                check.unchecked_reason.as_deref().unwrap(),
                reason,
                elem.0,
            );
        } else {
            assert!(check.ok);
            assert_eq!(check.unchecked_reason, None);
            assert!((check.biaxial_shear_margin.unwrap() - 2.083725).abs() < 1e-6);
        }
    }
    let missing_strong = MemberDemand {
        q_long_weak: Some(0.0),
        ..MemberDemand::axial(150000.0)
    };
    let reason = collect_rc_ultimate_checks(&model, &[(elem, missing_strong)], &opts).unwrap_err();
    assert_resolved_demand_context(&reason, "ultimate_gravity_missing", elem.0);
}

#[test]
fn static_biaxial_shear_keeps_the_strong_axis_failure_from_issue_1412() {
    let mut model = super::tests::column_and_beam_model();
    model.elements.truncate(1);
    let elem = model.elements[0].id;
    let mut opts = UltimateShearOptions {
        include_bond: false,
        ..Default::default()
    };
    let demand = MemberDemand {
        q_long: Some(500000.0),
        ..MemberDemand::axial(150000.0)
    };
    let uniaxial = collect_rc_ultimate_checks(&model, &[(elem, demand)], &opts).unwrap();
    assert!(!uniaxial[0].ok);
    assert!((uniaxial[0].shear_margin - 0.916367).abs() < 1e-6);
    opts.biaxial_shear = true;
    for weak in [None, Some(0.0), Some(4000.0)] {
        let demand = MemberDemand {
            q_long_weak: weak,
            ..demand
        };
        let checks = collect_rc_ultimate_checks(&model, &[(elem, demand)], &opts).unwrap();
        assert!(!checks[0].ok);
        assert_eq!(checks[0].unchecked_reason, None);
        assert_eq!(checks[0].biaxial_shear_margin, None);
        assert_eq!(checks[0].shear_margin, uniaxial[0].shear_margin);
    }
    let gravity_only = MemberDemand {
        q_long: Some(800000.0),
        q_long_weak: Some(0.0),
        ..MemberDemand::axial(20000000.0)
    };
    let checks = collect_rc_ultimate_checks(&model, &[(elem, gravity_only)], &opts).unwrap();
    assert_eq!(checks[0].qmu, 0.0);
    assert_eq!(checks[0].shear_margin, 0.0);
    assert!(!checks[0].ok);
    assert_eq!(checks[0].unchecked_reason, None);
}

#[test]
fn static_biaxial_shear_keeps_independently_failed_bending_check() {
    let mut model = super::tests::column_and_beam_model();
    model.elements.truncate(1);
    let elem = model.elements[0].id;
    let demand = MemberDemand {
        q_long: Some(0.0),
        q_long_weak: Some(4000.0),
        mz: 1.0e9,
        ..MemberDemand::axial(150000.0)
    };
    let opts = UltimateShearOptions {
        include_bond: false,
        biaxial_shear: true,
        biaxial_bending: true,
        ..Default::default()
    };
    let checks = collect_rc_ultimate_checks(&model, &[(elem, demand)], &opts).unwrap();
    let check = &checks[0];
    assert!(!check.ok);
    assert_eq!(check.unchecked_reason, None);
    assert_eq!(check.biaxial_shear_margin, None);
    assert!(check.shear_margin >= 1.0);
    assert!(check.biaxial_bending_margin.unwrap() < 1.0);
}

#[test]
fn direct_rc_total_uses_total_q_once_and_rejects_nonunit_amplification_and_added_ql() {
    let model = super::tests::column_and_beam_model();
    let mut opts = UltimateShearOptions {
        biaxial_shear: true,
        ..Default::default()
    };
    let mut demand: Vec<_> = model
        .elements
        .iter()
        .map(|e| {
            (
                e.id,
                MemberDemand::from_pushover(150000.0, 0.0, 0.0, 92000.0, -66000.0, 0.0),
            )
        })
        .collect();
    let forward = collect_rc_ultimate_checks(&model, &demand, &opts).unwrap();
    for check in &forward {
        assert_eq!(check.qmu, 92000.0);
        assert!((check.shear_margin - check.qsu / 92000.0).abs() < 1e-12);
        println!(
            "member={} totalQ=92000 Qsu={} margin={}",
            check.elem.0, check.qsu, check.shear_margin
        );
    }
    let column = &forward[0];
    assert!(column.ok);
    assert_eq!(column.unchecked_reason, None);
    assert!(
        (column.biaxial_shear_margin.unwrap() - 725654.571 / 92000.0_f64.hypot(66000.0)).abs()
            < 1e-7
    );
    for (_, d) in &mut demand {
        d.shear = Some(-68000.0);
        d.shear_weak = Some(66000.0);
    }
    let reverse = collect_rc_ultimate_checks(&model, &demand, &opts).unwrap();
    assert!(
        (reverse[0].biaxial_shear_margin.unwrap() - 725654.571 / 68000.0_f64.hypot(66000.0)).abs()
            < 1e-7
    );
    for (a, b) in forward.iter().zip(&reverse) {
        assert_eq!(b.qmu, 68000.0);
        assert!((b.shear_margin / a.shear_margin - 92000.0 / 68000.0).abs() < 1e-12);
    }
    opts.upper_strength_factor = 1.1;
    assert!(collect_rc_ultimate_checks(&model, &demand, &opts)
        .unwrap_err()
        .contains("ultimate_total_unsupported"));
    opts.upper_strength_factor = 1.0;
    demand[0].1.q_long = Some(0.0);
    assert!(collect_rc_ultimate_checks(&model, &demand, &opts)
        .unwrap_err()
        .contains("ultimate_total_unsupported"));
    demand[0].1.q_long = None;
    for (weak, cause) in [
        (Some(0.0), "ultimate_total_unsupported"),
        (Some(500.0), "ultimate_total_unsupported"),
        (Some(f64::NAN), "ultimate_demand_nonfinite"),
    ] {
        demand[0].1.q_long_weak = weak;
        let reason = collect_rc_ultimate_checks(&model, &demand, &opts).unwrap_err();
        assert_resolved_demand_context(&reason, cause, model.elements[0].id.0);
    }
}

#[test]
fn direct_collectors_reject_unspecified_combination_and_cft_missing_axial() {
    let mut rc = super::tests::column_and_beam_model();
    let demand = static_demands(&rc);
    rc.ultimate_gravity_combination = None;
    assert!(
        collect_rc_ultimate_checks(&rc, &demand, &UltimateShearOptions::default())
            .unwrap_err()
            .contains("gravity_combination_missing")
    );
    let mut cft = cft_model();
    assert!(collect_cft_ultimate_checks(&cft, &[])
        .unwrap_err()
        .contains("ultimate_demand_missing"));
    let id = cft.elements[0].id;
    let complete = collect_cft_ultimate_checks(&cft, &[(id, 150000.0)]).unwrap();
    assert_eq!(complete[0].n_design, 150000.0);
    println!(
        "direct CFT observed N={} margin={}",
        complete[0].n_design, complete[0].axial_margin
    );
    cft.ultimate_gravity_combination = None;
    assert!(collect_cft_ultimate_checks(&cft, &[(id, 150000.0)])
        .unwrap_err()
        .contains("gravity_combination_missing"));
}

fn assert_resolved_demand_context(reason: &str, cause: &str, member: u32) {
    assert!(reason.contains(cause), "{reason}");
    assert!(reason.contains(&format!("member={member}")), "{reason}");
    assert!(reason.contains("purpose=frame_long"), "{reason}");
    assert!(reason.contains("gravity_terms=["), "{reason}");
    assert!(reason.contains("case=0 coefficient=1"), "{reason}");
    assert!(reason.contains("case=1 coefficient=1"), "{reason}");
    assert!(!reason.contains("gravity_terms=unresolved"), "{reason}");
}

#[test]
fn direct_rc_rejection_identifies_member_frame_purpose_cases_coefficients_and_cause() {
    let model = super::tests::column_and_beam_model();
    let opts = UltimateShearOptions::default();
    let complete = static_demands(&model);
    let mut missing = complete.clone();
    missing.pop();
    let mut duplicate = complete.clone();
    duplicate.push(complete[0]);
    let mut nonfinite = complete.clone();
    nonfinite[0].1.my = f64::INFINITY;
    let mut missing_ql = complete.clone();
    missing_ql[0].1.q_long = None;
    let mut incomplete_total = complete;
    incomplete_total[0].1 = MemberDemand::from_pushover(150000.0, 0.0, 0.0, 92000.0, 0.0, 0.0);
    incomplete_total[0].1.rp = None;
    for (demand, cause, member) in [
        (missing, "ultimate_demand_missing", 1),
        (duplicate, "ultimate_demand_duplicate", 0),
        (nonfinite, "ultimate_demand_nonfinite", 0),
        (missing_ql, "ultimate_gravity_missing", 0),
        (incomplete_total, "ultimate_total_unsupported", 0),
    ] {
        let reason = collect_rc_ultimate_checks(&model, &demand, &opts).unwrap_err();
        assert_resolved_demand_context(&reason, cause, member);
        println!("direct RC rejection: {reason}");
    }
}

#[test]
fn direct_cft_rejection_identifies_member_frame_purpose_cases_coefficients_and_cause() {
    let model = cft_model();
    let id = model.elements[0].id;
    for (demand, cause) in [
        (vec![], "ultimate_demand_missing"),
        (
            vec![(id, 150000.0), (id, 150000.0)],
            "ultimate_demand_duplicate",
        ),
        (vec![(id, f64::NAN)], "ultimate_demand_nonfinite"),
    ] {
        let reason = collect_cft_ultimate_checks(&model, &demand).unwrap_err();
        assert_resolved_demand_context(&reason, cause, id.0);
        println!("direct CFT rejection: {reason}");
    }
}

#[test]
fn unresolved_gravity_is_reported_as_unresolved_in_direct_collector_errors() {
    for broken_reference in [false, true] {
        let mut rc = super::tests::column_and_beam_model();
        let mut cft = cft_model();
        for model in [&mut rc, &mut cft] {
            if broken_reference {
                model.combinations[0]
                    .terms
                    .push((sepika_core::ids::LoadCaseId(99), 0.0));
            } else {
                model.ultimate_gravity_combination = None;
            }
        }
        let rc_reason =
            collect_rc_ultimate_checks(&rc, &static_demands(&rc), &UltimateShearOptions::default())
                .unwrap_err();
        let cft_reason =
            collect_cft_ultimate_checks(&cft, &[(cft.elements[0].id, 150000.0)]).unwrap_err();
        for reason in [rc_reason, cft_reason] {
            assert!(reason.contains("member=0"), "{reason}");
            assert!(reason.contains("purpose=frame_long"), "{reason}");
            assert!(reason.contains("gravity_terms=unresolved"), "{reason}");
            assert!(reason.contains("gravity_resolution="), "{reason}");
            if broken_reference {
                assert!(reason.contains("case=99 coefficient=0"), "{reason}");
            } else {
                assert!(reason.contains("selected_terms=unresolved"), "{reason}");
            }
            println!("direct unresolved rejection: {reason}");
        }
    }
}

#[test]
fn material_diagnostics_precede_demand_context_when_material_input_is_missing() {
    let mut rc = super::tests::column_and_beam_model();
    rc.sections[0].material = None;
    assert!(
        collect_rc_ultimate_checks(&rc, &[], &UltimateShearOptions::default())
            .unwrap_err()
            .contains("材料が未設定")
    );
    let mut cft = cft_model();
    cft.materials[0].fc = None;
    assert!(collect_cft_ultimate_checks(&cft, &[])
        .unwrap_err()
        .contains("Fc が未設定または不正"));
}
