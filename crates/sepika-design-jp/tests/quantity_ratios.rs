use sepika_core::model::{Model, QuantityCfg};
use sepika_design_jp::quantity::try_compute_quantity_takeoff;

#[test]
fn invalid_takeoff_ratio_returns_reason_for_its_input() {
    let model = Model::default();
    for (ratio, cause) in [
        (-0.008, "負値"),
        (f64::NAN, "非有限"),
        (f64::INFINITY, "非有限"),
        (f64::NEG_INFINITY, "非有限"),
    ] {
        for (cfg, name) in [
            (
                QuantityCfg {
                    beam_main_ratio: ratio,
                    ..QuantityCfg::default()
                },
                "主架構小梁の主筋比",
            ),
            (
                QuantityCfg {
                    beam_stirrup_ratio: ratio,
                    ..QuantityCfg::default()
                },
                "主架構小梁のスターラップ比",
            ),
            (
                QuantityCfg {
                    slab_rebar_ratio: ratio,
                    ..QuantityCfg::default()
                },
                "床・片持ち床の鉄筋比",
            ),
        ] {
            let reason = try_compute_quantity_takeoff(&model, &cfg).unwrap_err();
            assert_eq!(reason, format!("{name}が{cause}です ({ratio})"));
        }
    }
}

#[test]
fn takeoff_accepts_finite_ratios_above_the_gui_editing_limit() {
    let cfg = QuantityCfg {
        beam_main_ratio: 1.1,
        beam_stirrup_ratio: 1.2,
        slab_rebar_ratio: 1.3,
        ..QuantityCfg::default()
    };
    assert!(try_compute_quantity_takeoff(&Model::default(), &cfg).is_ok());
}
