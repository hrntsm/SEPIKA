pub(crate) fn assert_cft_long_case2_summary(summary: &serde_json::Value, axial_ok: bool) {
    assert_eq!(summary["n_cft_checks"], 1);
    assert_eq!(summary["n_cft_axial_ng"], u64::from(!axial_ok));
    assert_eq!(summary["n_cft_mu_unavailable"], u64::from(!axial_ok));
    assert_eq!(summary["all_checks_calculated_and_ok"], axial_ok);
    let row = &summary["cft_members"][0];
    assert_eq!(row["class"], "Long");
    assert_eq!(row["axial_ok"], axial_ok);
    assert!((row["ncu"].as_f64().unwrap() - 7_468_425.346_669_381).abs() < 0.01);
    if axial_ok {
        assert!((row["n_design"].as_f64().unwrap() - 4_000_000.0).abs() < 0.01);
        assert!((row["mu_nm"].as_f64().unwrap() - 450_915_163.991_432_7).abs() < 0.1);
        assert!(row["mu_nm_unavailable_reason"].is_null());
    } else {
        assert!((row["n_design"].as_f64().unwrap() - 8_000_000.0).abs() < 0.01);
        assert!(row["mu_nm"].is_null());
        assert!(row["mu_nm_unavailable_reason"]
            .as_str()
            .unwrap()
            .contains("軸NG"));
        assert!(row["axial_margin"].as_f64().unwrap() < 1.0);
    }
}
