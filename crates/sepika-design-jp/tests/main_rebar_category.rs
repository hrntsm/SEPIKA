#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod rc_beam;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod rc_column;
#[path = "fixtures/src_h_steel_product_model.rs"]
mod src;

use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, SectionId};
use sepika_core::model::{
    default_combinations, default_load_cases, FrameSectionUse, MaterialCategory, Model,
};
use sepika_core::section_shape::{RebarSize, SectionShape, StandardRebar};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, StaticBeamHingeMechanism, UltimateShearOptions,
};
use sepika_design_jp::{
    run_member_design_checks, BondMethod, CheckComponent, CheckKind, CheckOutcome, LoadTerm,
    MemberDesignCheckOptions, QdMethod,
};
use sepika_element::frame::beam::MemberForces;
use std::collections::HashMap;

fn assign_main_rebar(model: &mut Model, category: MaterialCategory) {
    let previous = model.sections[0].rebar_material.unwrap();
    let mut material = model.materials[previous.index()].clone();
    material.id = MaterialId(model.materials.len() as u32);
    material.category = category;
    model.sections[0].rebar_material = Some(material.id);
    model.materials.push(material);
}

fn assert_material_diagnostic(reason: &str, category: MaterialCategory) {
    for text in ["SD345", "主筋", category.label(), "断面タブ", "「鉄筋」"] {
        assert!(reason.contains(text), "{reason}");
    }
}

fn rc_members() -> [Model; 3] {
    [
        rc_beam::model(false),
        rc_column::column_model(false, 0.0),
        rc_column::column_model(true, 0.0),
    ]
}

fn allowable_checks(
    model: &Model,
    torsion_nmm: f64,
    options: &MemberDesignCheckOptions<'_>,
) -> Vec<(ElemId, f64, CheckOutcome)> {
    allowable_checks_with_demand(
        model,
        [0.0, 1_000.0, 0.0, torsion_nmm, 0.0, 1_000_000.0],
        options,
    )
}

fn allowable_checks_with_demand(
    model: &Model,
    demand: [f64; 6],
    options: &MemberDesignCheckOptions<'_>,
) -> Vec<(ElemId, f64, CheckOutcome)> {
    run_member_design_checks(
        model,
        &[(
            ElemId(0),
            MemberForces {
                at: vec![(0.5, demand)],
            },
        )],
        &[],
        options,
    )
    .unwrap()
    .member_checks
}

fn components_with_wrong_material(
    model: &Model,
    demand: [f64; 6],
    options: &MemberDesignCheckOptions<'_>,
) -> Vec<CheckComponent> {
    let mut model = model.clone();
    assign_main_rebar(&mut model, MaterialCategory::Steel);
    let (checked, skipped): (Vec<_>, Vec<_>) =
        allowable_checks_with_demand(&model, demand, options)
            .into_iter()
            .partition(|(_, _, outcome)| matches!(outcome, CheckOutcome::Checked(_)));
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    let CheckOutcome::Skipped { reason } = &skipped[0].2 else {
        unreachable!()
    };
    assert_material_diagnostic(reason, MaterialCategory::Steel);
    let components: Vec<_> = checked
        .into_iter()
        .flat_map(|(_, _, outcome)| checked_components(outcome))
        .collect();
    assert!(components.iter().all(|row| row.ratio > 1.0));
    components
}

fn checked_components(outcome: CheckOutcome) -> Vec<CheckComponent> {
    match outcome {
        CheckOutcome::Checked(result) => result.components,
        CheckOutcome::Skipped { reason } => panic!("検定が未算定です: {reason}"),
    }
}

#[test]
fn rc_allowable_preserves_elastic_shear_and_cover_failures() {
    for mut model in rc_members() {
        match model.sections[0].shape.as_mut().unwrap() {
            SectionShape::RcBeamRect { rebar, .. } => rebar.cover = 20.0,
            SectionShape::RcColumnRect { rebar, .. } => rebar.cover = 20.0,
            SectionShape::RcColumnCircle { rebar, .. } => rebar.cover = 20.0,
            _ => unreachable!(),
        }
        let demand = [0.0, 10_000_000.0, 0.0, 0.0, 0.0, 1_000_000.0];
        let options = short_term();
        let correct = checked_components(
            allowable_checks_with_demand(&model, demand, &options)
                .remove(0)
                .2,
        );
        let components = components_with_wrong_material(&model, demand, &options);
        let independent: Vec<_> = correct
            .into_iter()
            .filter(|row| matches!(row.kind, CheckKind::Shear | CheckKind::Provision))
            .collect();
        assert_eq!(components, independent);
        assert_eq!(components.len(), 2);
        let missing_gravity = MemberDesignCheckOptions {
            long_member_forces: Some(&[]),
            ..short_term()
        };
        let components = components_with_wrong_material(&model, demand, &missing_gravity);
        assert_eq!(components.len(), 1, "{components:?}");
        assert_eq!(components[0].kind, CheckKind::Provision);
    }
}

#[test]
fn rc_allowable_preserves_explicit_deformed_bar_bond_failure() {
    let mut model = rc_beam::model(false);
    let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[0].shape else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::Standard(StandardRebar::D19);
    let components = components_with_wrong_material(
        &model,
        [0.0, 10_000_000.0, 0.0, 0.0, 0.0, 1_000_000.0],
        &MemberDesignCheckOptions {
            bond_method: BondMethod::Rc1991,
            ..short_term()
        },
    );
    assert!(components.iter().any(|row| row.kind == CheckKind::Bond));
    assert!(components.iter().any(|row| row.kind == CheckKind::Shear));
}

#[test]
fn rc_allowable_keeps_qd2_failure_without_replacing_unknown_qd1() {
    for model in rc_members() {
        let long = [(
            ElemId(0),
            MemberForces {
                at: vec![(0.5, [0.0; 6])],
            },
        )];
        let q0 = HashMap::from([(ElemId(0), 0.0)]);
        for method in [QdMethod::Qd1, QdMethod::Min, QdMethod::Qd2] {
            let components = components_with_wrong_material(
                &model,
                [0.0, 10_000_000.0, 0.0, 0.0, 0.0, 1_000_000.0],
                &MemberDesignCheckOptions {
                    long_member_forces: Some(&long),
                    q_simple_by_elem: Some(&q0),
                    qd_method: method,
                    ..short_term()
                },
            );
            if method == QdMethod::Qd2 {
                assert_eq!(components.len(), 1, "{components:?}");
                assert_eq!(components[0].kind, CheckKind::Shear);
            } else {
                assert!(components.is_empty(), "{method:?} {components:?}");
            }
        }
    }
}

#[test]
fn src_allowable_preserves_shear_failure_for_beam_and_column() {
    for column in [false, true] {
        let model = src::model(column, "SN400B", 0.0, 0.0);
        let demand = [0.0, 10_000_000.0, 0.0, 0.0, 0.0, 1_000_000.0];
        for term in [LoadTerm::Long, LoadTerm::Short] {
            let options = MemberDesignCheckOptions {
                term,
                ..Default::default()
            };
            let correct = checked_components(
                allowable_checks_with_demand(&model, demand, &options)
                    .remove(0)
                    .2,
            );
            let components = components_with_wrong_material(&model, demand, &options);
            assert_eq!(components.len(), 1, "{components:?}");
            assert_eq!(
                &components[0],
                correct
                    .iter()
                    .find(|row| row.kind == CheckKind::Shear)
                    .unwrap(),
            );
        }
    }
}

#[test]
fn src_beam_seismic_shear_keeps_steel_failure_and_only_a_confirmed_rc_demand() {
    let model = src::model(false, "SN400B", 0.0, 0.0);
    let demand = [0.0, 10_000_000.0, 0.0, 0.0, 0.0, 1_000_000.0];
    for method in [QdMethod::Qd1, QdMethod::Min, QdMethod::Qd2] {
        for q_long_n in [0.0, 10_000_000.0] {
            let long = [(
                ElemId(0),
                MemberForces {
                    at: vec![(0.5, [0.0, q_long_n, 0.0, 0.0, 0.0, 0.0])],
                },
            )];
            let components = components_with_wrong_material(
                &model,
                demand,
                &MemberDesignCheckOptions {
                    long_member_forces: Some(&long),
                    qd_method: method,
                    ..short_term()
                },
            );
            if q_long_n == 0.0 && method != QdMethod::Qd2 {
                assert!(components.is_empty(), "{method:?} {components:?}");
            } else {
                assert_eq!(components.len(), 1, "{components:?}");
                assert_eq!(components[0].kind, CheckKind::Shear);
                assert_eq!(
                    components[0].detail.contains("rQ=未算定"),
                    method != QdMethod::Qd2
                );
            }
        }
    }
}

fn short_term() -> MemberDesignCheckOptions<'static> {
    MemberDesignCheckOptions {
        term: LoadTerm::Short,
        ..Default::default()
    }
}

fn assert_allowable_requires_rebar(mut model: Model) {
    let original_assignment = model.sections[0].rebar_material;
    for category in [MaterialCategory::Steel, MaterialCategory::Concrete] {
        assign_main_rebar(&mut model, category);
        let checks = allowable_checks(&model, 0.0, &short_term());
        assert!(!checks.is_empty());
        for (member, _, outcome) in checks {
            assert_eq!(member, ElemId(0));
            let CheckOutcome::Skipped { reason } = outcome else {
                panic!("誤った主筋材料で検定を算定しました: {outcome:?}");
            };
            assert_material_diagnostic(&reason, category);
            assert!(reason.contains(&model.sections[0].name), "{reason}");
        }
    }
    model.sections[0].rebar_material = original_assignment;
    let expected_kind = if model.sections[0].frame_use == Some(FrameSectionUse::Column) {
        CheckKind::AxialBending
    } else {
        CheckKind::Bending
    };
    let checks = allowable_checks(&model, 0.0, &short_term());
    assert!(
        checks.iter().any(|(_, _, outcome)| {
            matches!(outcome, CheckOutcome::Checked(result) if result.components.iter().any(
                |component| component.kind == expected_kind && component.ratio.is_finite()
            ))
        }),
        "{checks:?}"
    );
}

#[test]
fn rc_allowable_checks_require_rebar_for_beam_and_columns() {
    for model in rc_members() {
        assert_allowable_requires_rebar(model);
    }
}

#[test]
fn src_allowable_checks_require_rebar_for_beam_and_column() {
    for column in [false, true] {
        assert_allowable_requires_rebar(src::model(column, "SN400B", 0.0, 0.0));
    }
}

#[test]
fn allowable_checks_keep_both_material_and_torsion_diagnostics() {
    let mut model = rc_beam::model(false);
    assign_main_rebar(&mut model, MaterialCategory::Steel);
    let checks = allowable_checks(&model, 1_000_000.0, &short_term());
    assert!(!checks.is_empty());
    for (_, _, outcome) in checks {
        let CheckOutcome::Skipped { reason } = outcome else {
            panic!("主筋・ねじりに未算定条件があります: {outcome:?}");
        };
        assert_material_diagnostic(&reason, MaterialCategory::Steel);
        assert!(reason.contains("ねじり"), "{reason}");
    }
}

#[test]
fn allowable_beam_keeps_material_and_missing_gravity_diagnostics() {
    for mut model in [rc_beam::model(false), src::model(false, "SN400B", 0.0, 0.0)] {
        assign_main_rebar(&mut model, MaterialCategory::Steel);
        let checks = allowable_checks(
            &model,
            0.0,
            &MemberDesignCheckOptions {
                long_member_forces: Some(&[]),
                ..short_term()
            },
        );
        assert!(!checks.is_empty());
        for (_, _, outcome) in checks {
            let CheckOutcome::Skipped { reason } = outcome else {
                panic!("主筋・重力参照に未算定条件があります: {outcome:?}");
            };
            assert_material_diagnostic(&reason, MaterialCategory::Steel);
            assert!(reason.contains("重力参照内力が不足"), "{reason}");
        }
    }
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
fn rc_ultimate_checks_require_rebar_for_beam_and_columns() {
    for mut model in rc_members() {
        set_ultimate_input(&mut model);
        let original_assignment = model.sections[0].rebar_material;
        let demand = MemberDemand::from_pushover(0.0, 1_000_000.0, 0.0, 1_000.0, 0.0, 0.0);
        let run = |model: &Model| {
            collect_rc_ultimate_checks(
                model,
                &[(ElemId(0), demand)],
                &UltimateShearOptions::default(),
            )
            .unwrap()
            .remove(0)
        };
        for category in [MaterialCategory::Steel, MaterialCategory::Concrete] {
            assign_main_rebar(&mut model, category);
            let check = run(&model);
            let reason = check.unchecked_reason.as_deref().unwrap();
            assert_eq!(check.elem, ElemId(0));
            assert_material_diagnostic(reason, category);
            assert!(reason.contains("部材 ID 0"), "{reason}");
            assert!(!check.ok && check.mu.is_nan() && check.qmu.is_nan());
        }
        model.sections[0].rebar_material = original_assignment;
        let check = run(&model);
        assert!(check.mu > 0.0 && check.mu.is_finite(), "{check:?}");
        assert!(check.ok && check.unchecked_reason.is_none(), "{check:?}");
    }
}

#[test]
fn static_rc_beam_keeps_shear_and_only_confirmed_bond_failure_with_wrong_main_rebar() {
    for main_dia in [
        RebarSize::Standard(StandardRebar::D19),
        RebarSize::DiameterMm(19.0),
    ] {
        let mut model = rc_beam::model(false);
        set_ultimate_input(&mut model);
        assign_main_rebar(&mut model, MaterialCategory::Steel);
        model.materials.last_mut().unwrap().name = "SR235".into();
        let Some(SectionShape::RcBeamRect { rebar, .. }) = &mut model.sections[0].shape else {
            unreachable!()
        };
        rebar.main_dia = main_dia;
        let check = collect_rc_ultimate_checks(
            &model,
            &[(
                ElemId(0),
                MemberDemand {
                    static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
                    q_long: Some(10_000_000.0),
                    ..MemberDemand::axial(0.0)
                },
            )],
            &UltimateShearOptions::default(),
        )
        .unwrap()
        .remove(0);
        assert!(!check.ok && check.unchecked_reason.is_none(), "{check:?}");
        assert!(check.mu.is_nan() && check.qmu.is_nan(), "{check:?}");
        assert_eq!(check.shear_margin, 0.0);
        match main_dia {
            RebarSize::Standard(_) => {
                assert!(check.qbu.is_finite(), "{check:?}");
                assert_eq!(check.bond_margin, 0.0);
            }
            RebarSize::DiameterMm(_) => {
                assert!(
                    check.qbu.is_nan() && check.bond_margin.is_nan(),
                    "{check:?}"
                );
                assert!(check.detail.contains("付着は未算定"), "{check:?}");
            }
        }
        for text in ["SR235", "主筋", "鋼材", "断面タブ", "「鉄筋」"] {
            assert!(check.detail.contains(text), "{check:?}");
        }
    }
}

#[test]
fn rc_ultimate_keeps_failures_on_members_with_and_without_wrong_main_rebar() {
    let mut model = rc_column::column_model(false, 0.0);
    set_ultimate_input(&mut model);
    let mut other_section = model.sections[0].clone();
    other_section.id = SectionId(1);
    other_section.name = "別断面のRC柱".into();
    let mut other_element = model.elements[0].clone();
    other_element.id = ElemId(1);
    other_element.section = Some(other_section.id);
    model.sections.push(other_section);
    model.elements.push(other_element);
    assign_main_rebar(&mut model, MaterialCategory::Steel);
    let demand = MemberDemand::from_pushover(0.0, 1_000_000.0, 0.0, 10_000_000.0, 0.0, 0.0);
    let checks = collect_rc_ultimate_checks(
        &model,
        &[(ElemId(0), demand), (ElemId(1), demand)],
        &UltimateShearOptions::default(),
    )
    .unwrap();
    assert_eq!(checks.len(), 2);
    for check in &checks {
        assert!(!check.ok && check.unchecked_reason.is_none(), "{check:?}");
        assert!(check.shear_margin < 1.0, "{check:?}");
    }
    assert!(checks[0].mu.is_nan(), "{:?}", checks[0]);
    assert!(
        checks[0].qbu.is_nan() && checks[0].bond_margin.is_nan(),
        "{:?}",
        checks[0]
    );
    assert!(checks[0].detail.contains("付着は未算定"), "{:?}", checks[0]);
    assert_material_diagnostic(&checks[0].detail, MaterialCategory::Steel);
    assert!(
        checks[1].mu.is_finite() && checks[1].qbu.is_finite(),
        "{:?}",
        checks[1]
    );
}
