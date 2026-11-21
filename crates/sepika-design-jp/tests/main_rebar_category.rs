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
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, StaticBeamHingeMechanism, UltimateShearOptions,
};
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_element::frame::beam::MemberForces;

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
    run_member_design_checks(
        model,
        &[(
            ElemId(0),
            MemberForces {
                at: vec![(0.5, [0.0, 1_000.0, 0.0, torsion_nmm, 0.0, 1_000_000.0])],
            },
        )],
        &[],
        options,
    )
    .unwrap()
    .member_checks
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
fn static_rc_beam_keeps_independent_gravity_failure_with_wrong_main_rebar() {
    let mut model = rc_beam::model(false);
    set_ultimate_input(&mut model);
    assign_main_rebar(&mut model, MaterialCategory::Steel);
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
    assert_eq!(check.bond_margin, 0.0);
    assert_material_diagnostic(&check.detail, MaterialCategory::Steel);
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
    assert_material_diagnostic(&checks[0].detail, MaterialCategory::Steel);
    assert!(checks[1].mu.is_finite(), "{:?}", checks[1]);
}
