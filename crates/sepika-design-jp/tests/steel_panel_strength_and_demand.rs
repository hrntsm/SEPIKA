use sepika_core::panel_zone::{BeamFlangePositions, PanelGeometry, PanelShapeKind};
use sepika_design_jp::steel::panel_zone::{s_panel_zone_check, SPanelInput};

const DESIGN_MOMENT_NMM: f64 = 742_000_000.0;

fn box_panel() -> SPanelInput {
    SPanelInput {
        beam_flanges: Some(
            [BeamFlangePositions {
                upper: 250.0,
                lower: -250.0,
            }; 2],
        ),
        geometry: PanelGeometry {
            kind: PanelShapeKind::SquareBox,
            dc: 384.0,
            tp: 16.0,
            filled: false,
        },
        db: 500.0,
        fy: 235.0,
        axial_ratio: 0.0,
        beam_moment_left: f64::NAN,
        beam_moment_right: f64::INFINITY,
        col_shear_upper: f64::NAN,
        col_shear_lower: f64::NEG_INFINITY,
        design_moment: Some(DESIGN_MOMENT_NMM),
    }
}

#[test]
fn finite_direct_moment_uses_box_panel_capacity_even_with_unused_nonfinite_end_forces() {
    // 式(5.1)と標準形式の正方形等厚鋼管の形状係数 κ = 9/8 から独立に算出。
    let expected_capacity_nmm = 740_979_033.481_555;
    let expected_ratio = 1.001_377_861_548_454_3;

    let result = s_panel_zone_check(&box_panel()).unwrap();
    let ratio = result.ratio();
    let capacity_nmm = DESIGN_MOMENT_NMM / ratio;
    assert!(
        (capacity_nmm - expected_capacity_nmm).abs() < 1e-3,
        "{capacity_nmm}"
    );
    assert!((ratio - expected_ratio).abs() < 1e-12, "{ratio}");
    assert!(!result.ok());
}

#[test]
fn zero_design_moment_passes_with_zero_ratio() {
    let result = s_panel_zone_check(&SPanelInput {
        design_moment: Some(0.0),
        ..box_panel()
    })
    .unwrap();

    assert_eq!(result.ratio(), 0.0);
    assert!(result.ok());
}

#[test]
fn infinite_material_strength_is_rejected() {
    let reason = s_panel_zone_check(&SPanelInput {
        fy: f64::INFINITY,
        ..box_panel()
    })
    .unwrap_err();

    assert!(
        reason.contains("基準強度 F") && reason.contains("有限値"),
        "{reason}"
    );
}

#[test]
fn nan_direct_moment_is_rejected() {
    let reason = s_panel_zone_check(&SPanelInput {
        design_moment: Some(f64::NAN),
        ..box_panel()
    })
    .unwrap_err();

    assert!(
        reason.contains("設計用パネルモーメント") && reason.contains("有限値"),
        "{reason}"
    );
}

#[test]
fn moment_overflow_from_finite_end_forces_is_rejected() {
    let reason = s_panel_zone_check(&SPanelInput {
        beam_moment_left: f64::MAX,
        beam_moment_right: f64::MAX,
        col_shear_upper: 0.0,
        col_shear_lower: 0.0,
        design_moment: None,
        ..box_panel()
    })
    .unwrap_err();

    assert!(
        reason.contains("設計用パネルモーメント") && reason.contains("有限値"),
        "{reason}"
    );
}
