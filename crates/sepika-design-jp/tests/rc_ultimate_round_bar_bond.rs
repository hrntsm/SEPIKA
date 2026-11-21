#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column_input;

use sepika_core::{
    dof::Dof6Mask,
    ids::MaterialId,
    model::{default_combinations, default_load_cases, MaterialCategory, Model},
};
use sepika_design_jp::{
    ultimate::{
        collect_rc_ultimate_checks, MemberDemand, ShearMethod, StaticBeamHingeMechanism,
        UltimateCheck, UltimateDemandMode, UltimateShearOptions,
    },
    MemberKind,
};

const ROUND_BOND_UNSUPPORTED: &str = "ultimate_bond_round_bar_unsupported:";

fn members() -> [Model; 3] {
    let mut beam = beam_input::model(false);
    beam.nodes[1].restraint = Dof6Mask::FIXED;
    [
        beam,
        column_input::column_model(false, 0.0),
        column_input::column_model(true, 0.0),
    ]
    .map(|mut model| {
        model.load_cases = default_load_cases();
        model.combinations = default_combinations();
        model.ultimate_gravity_combination = Some(0);
        model.elements[0].rigid_zone.face_i = Some(0.0);
        model.elements[0].rigid_zone.face_j = Some(0.0);
        model
    })
}

fn assign_round_main_bars(model: &mut Model, grade: &str) {
    let mut material = beam_input::material(grade, MaterialCategory::Rebar);
    material.id = MaterialId(2);
    model.sections[0].rebar_material = Some(material.id);
    model.materials.push(material);
    model.validate().unwrap();
}

fn demands() -> [MemberDemand; 2] {
    [
        MemberDemand {
            mz: -1e7,
            q_long: Some(0.0),
            q_long_weak: Some(0.0),
            static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
            ..Default::default()
        },
        MemberDemand::from_pushover(0.0, -1e7, 0.0, 30_000.0, 0.0, 0.0),
    ]
}

fn check(model: &Model, demand: MemberDemand, opts: &UltimateShearOptions) -> UltimateCheck {
    let checks =
        collect_rc_ultimate_checks(model, &[(model.elements[0].id, demand)], opts).unwrap();
    let [result] = checks.try_into().expect("RC部材1本の終局検定結果");
    result
}

#[test]
fn round_main_bars_leave_bond_unchecked_and_keep_independent_capacities() {
    for (grade, deformed_model) in ["SR235", "SR295", "SR235"].into_iter().zip(members()) {
        let mut model = deformed_model.clone();
        assign_round_main_bars(&mut model, grade);
        for shear_method in [ShearMethod::Plastic, ShearMethod::Ductility] {
            let opts = UltimateShearOptions {
                shear_method,
                biaxial_shear: true,
                biaxial_bending: true,
                ..Default::default()
            };
            let without_bond = UltimateShearOptions {
                include_bond: false,
                ..opts.clone()
            };
            for demand in demands() {
                let result = check(&model, demand, &opts);
                let disabled = check(&model, demand, &without_bond);
                let deformed = check(&deformed_model, demand, &opts);
                assert!(!result.ok && result.qbu.is_nan() && result.bond_margin.is_nan());
                let reason = result.unchecked_reason.as_deref().unwrap();
                for expected in [ROUND_BOND_UNSUPPORTED, grade, "丸鋼", "member=0"] {
                    assert!(reason.contains(expected), "{reason}");
                }
                assert!(result.detail.contains(ROUND_BOND_UNSUPPORTED));
                let unavailable = match shear_method {
                    ShearMethod::Plastic => "Qbu=未算定",
                    ShearMethod::Ductility => "Vbu=未算定",
                };
                assert!(result.detail.contains(unavailable), "{}", result.detail);
                assert!(result.mu.is_finite() && result.mu > 0.0);
                assert!(result.qsu.is_finite() && result.qsu > 0.0);
                assert_eq!(result.mu, disabled.mu);
                assert_eq!(result.qsu, disabled.qsu);
                assert_eq!(disabled.qbu, 0.0);
                assert!(!disabled.detail.contains(ROUND_BOND_UNSUPPORTED));
                assert!(deformed.qbu.is_finite() && deformed.qbu > 0.0);
                assert!(!deformed.detail.contains(ROUND_BOND_UNSUPPORTED));
                if result.kind == MemberKind::Girder
                    && demand.mode == UltimateDemandMode::StaticHinge
                {
                    assert!(result.shear_margin.is_nan() && disabled.shear_margin.is_nan());
                    assert!(disabled.bond_margin.is_nan());
                    assert!(reason.contains("ultimate_beam_hinge_shear_upper_bound_unverified:"));
                } else {
                    assert_eq!(result.qmu, disabled.qmu);
                    assert_eq!(result.shear_margin, disabled.shear_margin);
                    assert_eq!(result.biaxial_shear_margin, disabled.biaxial_shear_margin);
                    assert_eq!(disabled.bond_margin, f64::INFINITY);
                    assert!(disabled.ok && disabled.unchecked_reason.is_none());
                    assert!(deformed.bond_margin.is_finite() && deformed.bond_margin > 0.0);
                }
                if result.kind == MemberKind::Column {
                    let axial = result.axial.unwrap();
                    let disabled_axial = disabled.axial.unwrap();
                    assert!(axial.nuc > 0.0 && axial.nut < 0.0);
                    assert_eq!(
                        (axial.nuc, axial.nut),
                        (disabled_axial.nuc, disabled_axial.nut)
                    );
                    assert_eq!(
                        result.biaxial_bending_margin,
                        disabled.biaxial_bending_margin
                    );
                    assert!(result.biaxial_bending_margin.unwrap() >= 1.0);
                }
            }
        }
    }
}

#[test]
fn independently_confirmed_shear_or_bending_failure_takes_precedence_over_unchecked_bond() {
    for mut model in members() {
        assign_round_main_bars(&mut model, "SR295");
        for shear_method in [ShearMethod::Plastic, ShearMethod::Ductility] {
            let opts = UltimateShearOptions {
                shear_method,
                biaxial_bending: true,
                ..Default::default()
            };
            let [static_demand, total_demand] = demands();
            for demand in [
                MemberDemand {
                    q_long: Some(10e6),
                    ..static_demand
                },
                MemberDemand {
                    shear: Some(10e6),
                    ..total_demand
                },
            ] {
                let result = check(&model, demand, &opts);
                assert!(result.shear_margin < 1.0);
                assert!(result
                    .biaxial_bending_margin
                    .is_none_or(|margin| margin >= 1.0));
                assert!(!result.ok && result.unchecked_reason.is_none());
                assert!(result.qbu.is_nan() && result.bond_margin.is_nan());
                assert!(result.detail.contains(ROUND_BOND_UNSUPPORTED));
            }
            if model.sections[0].rc_frame_use() == Some(sepika_core::model::FrameSectionUse::Column)
            {
                let result = check(
                    &model,
                    MemberDemand {
                        mz: 10e9,
                        ..total_demand
                    },
                    &opts,
                );
                assert!(result.shear_margin >= 1.0);
                assert!(result.biaxial_bending_margin.unwrap() < 1.0);
                assert!(!result.ok && result.unchecked_reason.is_none());
                assert!(result.qbu.is_nan() && result.bond_margin.is_nan());
                assert!(result.detail.contains(ROUND_BOND_UNSUPPORTED));
            }
        }
    }
}
