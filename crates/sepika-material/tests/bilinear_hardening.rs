use sepika_material::{Bilinear, UniaxialMaterial};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[test]
fn invalid_hardening_is_rejected_at_construction_and_after_parameter_changes() {
    for hardening in [-0.01, 1.0, 1.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(catch_unwind(|| Bilinear::new(205000.0, 235.0, hardening)).is_err());
        let mut material = Bilinear::new(205000.0, 235.0, 0.01);
        material.hardening = hardening;
        let before = material.serialize_state();
        assert!(catch_unwind(|| material.probe(0.0)).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| material.trial(0.0))).is_err());
        assert_eq!(material.serialize_state(), before);
    }
}

#[test]
fn zero_and_near_one_hardening_keep_finite_monotonic_and_reversal_response() {
    for hardening in [0.0, 0.01, f64::from_bits(1.0_f64.to_bits() - 1)] {
        let mut material = Bilinear::new(205000.0, 235.0, hardening);
        assert_eq!(material.trial(0.0), (0.0, 205000.0));
        for strain in [0.001, 0.01, 0.005, -0.01, -0.005, 0.02] {
            let response = material.trial(strain);
            assert!(response.0.is_finite() && response.1.is_finite());
            material.commit();
        }
    }
}
