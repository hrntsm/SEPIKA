use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::cft::CftDesign;
use sepika_design_jp::{CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt};

#[test]
fn circular_cft_allowable_bending_reaches_steel_only_at_core_compression_limit() {
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
    let demand_kn_m = 324.0;

    // 全断面圧縮は城戸・津田（2021）の式(8)と鋼管の弾性断面係数で算定。
    // 部分圧縮の期待値は角度座標で独立に積分し、既存の中点則による誤差を許容する。
    for (axial_fraction, expected_kn_m, tolerance_kn_m) in [
        (0.25, 368.761_630_212, 0.003),
        (0.499_999, 365.484_593_562, 0.009),
        (0.5, 365.484_510_063, 1e-6),
        (0.500_001, 365.484_426_563, 1e-6),
        (0.75, 344.609_657_322, 1e-6),
        (0.999, 323.818_303_992, 1e-6),
        (1.0, 323.734_804_581, 1e-6),
    ] {
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
        if axial_fraction == 0.999 {
            assert!(axial_bending.ratio > 1.0);
            assert!(!result.ok());
        }
    }
}
