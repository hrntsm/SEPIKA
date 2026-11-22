use sepika_core::panel_zone::{BeamFlangePositions, PanelGeometry, PanelShapeKind};
use sepika_design_jp::steel::panel_zone::{s_panel_zone_check, SPanelInput};

fn box_panel_input(bc_mm: f64) -> SPanelInput {
    SPanelInput {
        beam_flanges: Some(
            [BeamFlangePositions {
                upper: 250.0,
                lower: -250.0,
            }; 2],
        ),
        geometry: PanelGeometry {
            kind: PanelShapeKind::Box { bc: bc_mm },
            dc: 384.0,
            tp: 16.0,
            filled: true,
        },
        db: 500.0,
        fy: 235.0,
        axial_ratio: 0.0,
        beam_moment_left: 0.0,
        beam_moment_right: 0.0,
        col_shear_upper: 0.0,
        col_shear_lower: 0.0,
        design_moment: Some(742_000_000.0),
    }
}

#[test]
fn rejects_nan_shape_factor_with_section_geometry() {
    let err = s_panel_zone_check(&box_panel_input(f64::NAN)).unwrap_err();
    assert!(err.contains("形状係数が非有限"), "{err}");
    assert!(err.contains("bc: NaN"), "{err}");
}

#[test]
fn rejects_infinite_shape_factor_with_section_geometry() {
    // bc = -dc/3 で形状係数の分母が 0 になる。
    let err = s_panel_zone_check(&box_panel_input(-128.0)).unwrap_err();
    assert!(err.contains("形状係数が非有限"), "{err}");
    assert!(err.contains("bc: -128"), "{err}");
}

#[test]
fn finite_cft_box_shape_factor_preserves_yield_capacity_and_ratio() {
    let result = s_panel_zone_check(&box_panel_input(400.0)).unwrap();

    // 角形の手計算値は κ=37/33、実効体積=6,144,000 mm³。
    assert!(result.components[0].detail.contains("pMy=743482341.0 N*mm"));
    assert!((result.ratio() - 0.998_006_218_916_978).abs() < 1e-12);
}
