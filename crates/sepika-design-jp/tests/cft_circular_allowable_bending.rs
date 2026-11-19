use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::cft::CftDesign;
use sepika_design_jp::{CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt};

#[test]
fn circular_cft_allowable_bending_matches_linear_stress_equilibrium() {
    let section = SectionShape::CftPipe {
        outer_dia: 400.0,
        thick: 12.0,
    }
    .to_section(SectionId(0), "円形CFT".into());
    let steel = Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    };
    let concrete = Material {
        id: MaterialId(1),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 25_000.0,
        fc: Some(24.0),
        fy: None,
        ..steel.clone()
    };
    let ctx = DesignCtx {
        term: LoadTerm::Short,
        length: 3000.0,
        lk_y: Some(1000.0),
        lk_z: Some(1000.0),
        steel_material: Some(steel),
        ..Default::default()
    };
    let c_nc_n = std::f64::consts::PI * 188.0_f64.powi(2) * 16.0;
    // コアは角度座標の適応 Simpson 積分で軸力の釣合いを解き、全断面圧縮では
    // 城戸・津田（2021）の式(8)を用いた。鋼管は弾性断面係数から算定した。
    for (axial_fraction, expected_kn_m, tolerance_kn_m) in [
        (0.0, 323.734_804_581_283, 1e-12),
        (1e-11, 323.734_804_584_623, 1e-12),
        (1e-9, 323.734_804_915_281, 1e-12),
        (0.07, 342.627_595_455_603, 1e-9),
        (0.09, 346.949_210_475_125, 1e-9),
        (0.25, 368.761_630_212_486, 1e-9),
        (0.499, 365.567_979_208_683, 1e-9),
        (0.499_999, 365.484_593_562_121, 1e-9),
        (0.5, 365.484_510_062_711, 1e-9),
        (0.500_001, 365.484_426_563_300, 1e-9),
        (0.75, 344.609_657_321_997, 1e-9),
        (0.999, 323.818_303_992_246, 1e-9),
        (1.0, 323.734_804_581_283, 1e-9),
    ] {
        let demand_kn_m = if axial_fraction == 0.499 {
            365.572
        } else {
            324.0
        };
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -axial_fraction * c_nc_n,
            qy: 0.0,
            qz: 0.0,
            my: 0.0,
            mz: demand_kn_m * 1e6,
        };
        let CheckOutcome::Checked(result) = CftDesign.check(&forces, &section, &concrete, &ctx)
        else {
            panic!("cN/cNc={axial_fraction}: 検定結果が必要です");
        };
        let axial_bending = result
            .components
            .iter()
            .find(|component| component.kind == CheckKind::AxialBending)
            .expect("軸力・曲げの検定");
        let actual_kn_m = demand_kn_m / axial_bending.ratio;
        assert!(
            (actual_kn_m - expected_kn_m).abs() < tolerance_kn_m,
            "cN/cNc={axial_fraction}: MA={actual_kn_m} kN·m, 期待値={expected_kn_m} kN·m",
        );
        assert_eq!(
            result.ok(),
            demand_kn_m <= expected_kn_m,
            "cN/cNc={axial_fraction}: 検定比={}",
            axial_bending.ratio,
        );
    }
}
