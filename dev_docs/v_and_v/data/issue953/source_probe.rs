use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
use sepika_design_jp::secondary::holding_capacity::MemberRank;
use sepika_design_jp::secondary::src_rank::{src_column_rank, src_column_rank_ratios};
use sepika_design_jp::srrc::SrcDesign;
use sepika_design_jp::{
    CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind,
    QdMethod, SeismicQd,
};

fn shape() -> SectionShape {
    SectionShape::SrcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 25.0,
            x: vec![4],
            y: vec![2],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: 8.0,
        steel_flange_thick: 13.0,
    }
}

fn material(name: &str, category: MaterialCategory, fc: Option<f64>, fy: Option<f64>) -> Material {
    Material {
        id: MaterialId(0),
        name: name.into(),
        category,
        young: 205_000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc,
        fy,
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

fn context() -> DesignCtx {
    DesignCtx {
        kind: MemberKind::Column,
        term: LoadTerm::Short,
        rebar_material: Some(material(
            "SD345",
            MaterialCategory::Steel,
            None,
            Some(345.0),
        )),
        shear_rebar_material: Some(material(
            "SD295",
            MaterialCategory::Steel,
            None,
            Some(295.0),
        )),
        steel_material: Some(material(
            "SN400B",
            MaterialCategory::Steel,
            None,
            Some(235.0),
        )),
        seismic_qd: Some(SeismicQd {
            long_at: vec![(0.0, [0.0; 6])],
            n_factor: 2.0,
            n_mechanism: 1.0,
            q_simple: None,
            clear_length: 4000.0,
            method: QdMethod::Qd1,
        }),
        ..DesignCtx::default()
    }
}

fn forces(compression_n: f64) -> MemberForcesAt {
    MemberForcesAt {
        pos: 0.0,
        n: -compression_n,
        mz: 0.0,
        my: 0.0,
        qy: 800_000.0,
        qz: 800_000.0,
    }
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1.0e-9, "{actual} != {expected}");
}

#[test]
fn public_column_check_switches_to_force_based_shear_when_moment_is_zero() {
    let sec = shape().to_section(SectionId(0), "SRC 600角".into());
    let concrete = material("Fc24", MaterialCategory::Concrete, Some(24.0), None);
    for (compression_n, expected_ratio) in [
        (-1_219_330.648_674_539, 3.730_542_000_654),
        (0.0, 0.388_975_466_770),
        (3_300_000.0, 1.120_947_852_567),
        (9_994_811.831_860_6, 3.730_542_000_654),
        (10_094_811.831_860_6, 3.730_542_000_654),
    ] {
        let outcome = SrcDesign.check(&forces(compression_n), &sec, &concrete, &context());
        let CheckOutcome::Checked(result) = outcome else {
            panic!("SRC柱検定が未算定: {outcome:?}");
        };
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Shear)
            .unwrap();
        close(shear.ratio, expected_ratio);
        println!("N={compression_n:.6} N, shear ratio={:.12}", shear.ratio);
    }
}

#[test]
fn public_rank_has_a_fixed_strong_axis_and_unknown_grade_uses_f235() {
    let shape = shape();
    let (n_ratio, m_ratio) =
        src_column_rank_ratios(&shape, "SN400B", 24.0, 345.0, 3_300_000.0).unwrap();
    close(n_ratio, 0.283_771_469);
    close(m_ratio, 0.479_761_358);
    assert_eq!(src_column_rank(n_ratio, m_ratio, false), MemberRank::FA);
    let tension = src_column_rank_ratios(&shape, "SN400B", 24.0, 345.0, -2_000_000.0).unwrap();
    assert_eq!(tension, (0.0, m_ratio));
    assert_eq!(
        src_column_rank(n_ratio, 0.275_245_180, false),
        MemberRank::FB
    );
    assert_eq!(
        src_column_rank(0.315_248_518, m_ratio, false),
        MemberRank::FB
    );

    let (unknown_n, unknown_m) =
        src_column_rank_ratios(&shape, "任意鋼材", 24.0, 345.0, 3_400_000.0).unwrap();
    let (known_n, known_m) =
        src_column_rank_ratios(&shape, "SN400B", 24.0, 345.0, 3_400_000.0).unwrap();
    assert_eq!((unknown_n, unknown_m), (known_n, known_m));
    close(unknown_n, 0.292_370_605);
    assert_eq!(src_column_rank(unknown_n, unknown_m, false), MemberRank::FA);
    assert_eq!(
        src_column_rank(0.323_096_986, 0.281_827_592, false),
        MemberRank::FC
    );

    let sec = shape.to_section(SectionId(0), "SRC 600角".into());
    let concrete = material("Fc24", MaterialCategory::Concrete, Some(24.0), None);
    let mut ctx = context();
    ctx.steel_material = Some(material(
        "任意鋼材",
        MaterialCategory::Steel,
        None,
        Some(100.0),
    ));
    let result = SrcDesign.check(&forces(3_400_000.0), &sec, &concrete, &ctx);
    let CheckOutcome::Skipped { reason } = result else {
        panic!("未知鋼材でSRC柱検定が算定された: {result:?}");
    };
    assert!(reason.contains("任意鋼材"), "{reason}");
    println!(
        "rank N/N0={n_ratio:.12}, sM0/M0={m_ratio:.12}, for unknown-grade F235 N/N0={unknown_n:.12}"
    );
}
