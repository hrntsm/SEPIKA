#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;
#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column_input;

use sepika_core::{
    dof::Dof6Mask,
    ids::ElemId,
    model::{default_combinations, default_load_cases, Model},
};
use sepika_design_jp::{
    ultimate::{
        collect_rc_ultimate_checks, MemberDemand, ShearMethod, StaticBeamHingeMechanism,
        UltimateShearOptions,
    },
    MemberKind,
};

fn members() -> [(MemberKind, Model); 3] {
    [
        (MemberKind::Girder, beam_input::model(false)),
        (MemberKind::Column, column_input::column_model(false, 0.0)),
        (MemberKind::Column, column_input::column_model(true, 0.0)),
    ]
    .map(|(kind, mut model)| {
        model.load_cases = default_load_cases();
        model.combinations = default_combinations();
        model.ultimate_gravity_combination = Some(0);
        if kind == MemberKind::Girder {
            model.nodes[1].restraint = Dof6Mask::FIXED;
        }
        (kind, model)
    })
}

fn demands() -> [MemberDemand; 2] {
    [
        MemberDemand {
            mz: -1e7,
            static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::ITopJBottom),
            q_long: Some(0.0),
            q_long_weak: Some(0.0),
            ..MemberDemand::axial(0.0)
        },
        MemberDemand::from_pushover(0.0, -1e7, 0.0, 30_000.0, 20_000.0, 0.0),
    ]
}

#[test]
fn confirmed_faces_supply_the_same_demands_and_capacities_as_the_equivalent_clear_span() {
    for (kind, mut model) in members() {
        for (span_mm, offset_mm) in [(1600.0, 0.0), (0.5, 0.0), (1600.0, 200.0)] {
            let axis = if kind == MemberKind::Girder { 0 } else { 2 };
            model.elements[0].end_offsets_mm[1][axis] = offset_mm;
            model.elements[0].rigid_zone.face_i = Some(300.0);
            model.elements[0].rigid_zone.face_j = Some(2700.0 + offset_mm - span_mm);
            let mut equivalent = model.clone();
            equivalent.elements[0].end_offsets_mm = [[0.0; 3]; 2];
            equivalent.nodes[1].coord[axis] = span_mm;
            equivalent.elements[0].rigid_zone.face_i = Some(0.0);
            equivalent.elements[0].rigid_zone.face_j = Some(0.0);

            for shear_method in [ShearMethod::Plastic, ShearMethod::Ductility] {
                let opts = UltimateShearOptions {
                    shear_method,
                    biaxial_shear: true,
                    include_bond: true,
                    ..Default::default()
                };
                for demand in demands() {
                    let input = [(ElemId(0), demand)];
                    let faced = &collect_rc_ultimate_checks(&model, &input, &opts).unwrap()[0];
                    let clear = &collect_rc_ultimate_checks(&equivalent, &input, &opts).unwrap()[0];
                    assert_eq!(faced.kind, kind);
                    assert!(faced.qsu.is_finite() && faced.qsu > 0.0);
                    assert!(faced.qbu.is_finite() && faced.qbu > 0.0);
                    for (actual, expected) in [(faced.qsu, clear.qsu), (faced.qbu, clear.qbu)] {
                        assert!((actual - expected).abs() < 1e-6);
                    }
                    if kind == MemberKind::Girder && demand.shear.is_none() {
                        for check in [faced, clear] {
                            assert!(!check.ok && check.qmu.is_nan());
                            assert!(check
                                .unchecked_reason
                                .as_deref()
                                .unwrap()
                                .contains("ultimate_beam_hinge_shear_upper_bound_unverified:"));
                        }
                        continue;
                    }
                    assert_eq!(faced.unchecked_reason, None);
                    assert_eq!(clear.unchecked_reason, None);
                    assert!(
                        faced.detail.contains(&format!("L={span_mm} mm")),
                        "{}",
                        faced.detail
                    );
                    let expected_qmu = demand.shear.unwrap_or_else(|| 2.0 * faced.mu / span_mm);
                    assert!((faced.qmu - expected_qmu).abs() < 1e-6);
                    if kind == MemberKind::Column {
                        assert_eq!(faced.biaxial_shear_margin, clear.biaxial_shear_margin);
                        assert!(faced.biaxial_shear_margin.unwrap().is_finite());
                    }
                }
            }
        }
    }
}

#[test]
fn missing_or_inconsistent_faces_leave_the_member_unchecked_in_both_modes() {
    let opts = UltimateShearOptions::default();
    for (kind, mut model) in members() {
        for (i, j, code, message) in [
            (None, None, "uncomputed", "両端のフェース距離が未算定"),
            (Some(0.0), None, "uncomputed", "j端のフェース距離が未算定"),
            (None, Some(0.0), "uncomputed", "i端のフェース距離が未算定"),
            (
                Some(1000.0),
                Some(2000.0),
                "invalid",
                "正の内法長さが成立しません",
            ),
            (
                Some(1600.0),
                Some(1600.0),
                "invalid",
                "正の内法長さが成立しません",
            ),
            (
                Some(4000.0),
                Some(0.0),
                "invalid",
                "正の内法長さが成立しません",
            ),
            (Some(-1.0), Some(0.0), "invalid", "i端のフェース距離"),
            (Some(0.0), Some(f64::NAN), "invalid", "j端のフェース距離"),
            (
                Some(f64::INFINITY),
                Some(0.0),
                "invalid",
                "i端のフェース距離",
            ),
        ] {
            model.elements[0].rigid_zone.face_i = i;
            model.elements[0].rigid_zone.face_j = j;
            for demand in demands() {
                let checks =
                    collect_rc_ultimate_checks(&model, &[(ElemId(0), demand)], &opts).unwrap();
                assert_eq!(checks.len(), 1);
                let check = &checks[0];
                assert_eq!(check.kind, kind);
                assert!(!check.ok);
                let reason = check.unchecked_reason.as_deref().unwrap();
                for expected in [&format!("ultimate_clear_span_{code}:"), message, "member=0"] {
                    assert!(reason.contains(expected), "{reason}");
                }
                assert!(check.qmu.is_nan() && check.qsu.is_nan() && check.qbu.is_nan());
                assert!(check.shear_margin.is_nan() && check.bond_margin.is_nan());
                assert!(check.biaxial_shear_margin.is_none());
            }
        }
    }
}

#[test]
fn unresolved_actual_axis_leaves_the_member_unchecked_with_known_zero_faces() {
    for (_, mut model) in members() {
        model.elements[0].rigid_zone.face_i = Some(0.0);
        model.elements[0].rigid_zone.face_j = Some(0.0);
        model.elements[0].end_offsets_mm[1][0] = f64::NAN;
        let checks = collect_rc_ultimate_checks(
            &model,
            &[(ElemId(0), demands()[0])],
            &UltimateShearOptions::default(),
        )
        .unwrap();
        let reason = checks[0].unchecked_reason.as_deref().unwrap();
        assert!(reason.contains("ultimate_clear_span_invalid:"), "{reason}");
        assert!(reason.contains("j端のXYZ偏心が非有限"), "{reason}");
    }
}
