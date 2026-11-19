#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;

use sepika_core::{ids::ElemId, model::Model};
use sepika_design_jp::ultimate::{collect_rc_ultimate_checks, MemberDemand, UltimateShearOptions};

const GRAVITY_SHEAR_N: f64 = 12_000.0;

fn static_beam() -> (Model, [(ElemId, MemberDemand); 1]) {
    let mut model = beam_input::model(false);
    model.ultimate_gravity_combination = Some(0);
    let elem = &mut model.elements[0];
    elem.rigid_zone.face_i = Some(0.0);
    elem.rigid_zone.face_j = Some(0.0);
    let demand = MemberDemand {
        mz: 1.0e7,
        q_long: Some(GRAVITY_SHEAR_N),
        ..Default::default()
    };
    let input = [(elem.id, demand)];
    (model, input)
}

#[test]
fn static_hinge_rejects_nonpositive_and_nonfinite_strength_factors() {
    let (model, input) = static_beam();
    for factor in [0.0, -0.0, -0.5, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let opts = UltimateShearOptions {
            upper_strength_factor: factor,
            ..Default::default()
        };
        let reason = collect_rc_ultimate_checks(&model, &input, &opts).unwrap_err();
        assert!(
            reason.contains("ultimate_upper_strength_factor_invalid:"),
            "{reason}"
        );
        assert!(reason.contains("member=0"), "{reason}");
    }
}

#[test]
fn static_hinge_positive_strength_factors_scale_demand_and_margins() {
    let (model, input) = static_beam();
    let unit = collect_rc_ultimate_checks(&model, &input, &UltimateShearOptions::default())
        .unwrap()
        .remove(0);
    // SD345・主筋3本（径19 mm）・有効せい290.5 mm・内法3000 mmの両端ヒンジ需要。
    assert!((unit.qmu - 51_148.725_917).abs() < 1e-6);
    let unit_shear_margin = (unit.qsu - GRAVITY_SHEAR_N) / unit.qmu;
    let unit_bond_margin = (unit.qbu - GRAVITY_SHEAR_N) / unit.qmu;
    assert!(unit_shear_margin > 0.0 && unit_bond_margin > 0.0);

    for factor in [0.5, 1.0, 1.5] {
        let opts = UltimateShearOptions {
            upper_strength_factor: factor,
            ..Default::default()
        };
        let check = collect_rc_ultimate_checks(&model, &input, &opts)
            .unwrap()
            .remove(0);
        assert_eq!(check.unchecked_reason, None);
        assert_eq!(check.mu, unit.mu);
        assert_eq!(check.qsu, unit.qsu);
        assert_eq!(check.qbu, unit.qbu);
        assert!((check.qmu / unit.qmu - factor).abs() < 1e-12);
        assert!((check.shear_margin - unit_shear_margin / factor).abs() < 1e-12);
        assert!((check.bond_margin - unit_bond_margin / factor).abs() < 1e-12);
    }
}
