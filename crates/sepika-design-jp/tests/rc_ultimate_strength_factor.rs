#[path = "fixtures/rc_beam_asymmetric_model.rs"]
mod beam_input;

use sepika_core::{dof::Dof6Mask, ids::ElemId, model::Model};
use sepika_design_jp::ultimate::{
    collect_rc_ultimate_checks, MemberDemand, StaticBeamHingeMechanism, UltimateShearOptions,
};

const GRAVITY_SHEAR_N: f64 = 12_000.0;

fn static_beam() -> (Model, [(ElemId, MemberDemand); 1]) {
    let mut model = beam_input::model(false);
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.ultimate_gravity_combination = Some(0);
    let elem = &mut model.elements[0];
    elem.rigid_zone.face_i = Some(0.0);
    elem.rigid_zone.face_j = Some(0.0);
    let demand = MemberDemand {
        mz: 1.0e7,
        static_beam_hinge_mechanism: Some(StaticBeamHingeMechanism::IBottomJTop),
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
fn a_positive_factor_does_not_verify_the_beam_required_shear_upper_bound() {
    let (model, input) = static_beam();
    for factor in [f64::from_bits(1), 0.5, 1.0, 1.5] {
        let opts = UltimateShearOptions {
            upper_strength_factor: factor,
            ..Default::default()
        };
        let check = collect_rc_ultimate_checks(&model, &input, &opts)
            .unwrap()
            .remove(0);
        assert!(check.mu > 0.0 && check.qsu > 0.0 && check.qbu > 0.0);
        assert!(!check.ok && check.qmu.is_nan());
        assert!(check.shear_margin.is_nan() && check.bond_margin.is_nan());
        assert!(check
            .unchecked_reason
            .unwrap()
            .contains("ultimate_beam_hinge_shear_upper_bound_unverified:"));
    }
}
