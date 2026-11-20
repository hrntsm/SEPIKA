use approx::assert_abs_diff_eq;
use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory, Section};
use sepika_core::section_shape::{RcRectColumnRebar, RebarSize, RectColumnHoop, SectionShape};
use sepika_design_jp::secondary::holding_capacity::MemberRank;
use sepika_design_jp::secondary::src_rank::{src_column_rank, src_column_rank_ratios};

fn column(web_mm: f64, flange_mm: f64) -> Section {
    let mut section = SectionShape::SrcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: RebarSize::DiameterMm(25.0),
            x: vec![4],
            y: vec![2],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: web_mm,
        steel_flange_thick: flange_mm,
    }
    .to_section(SectionId(0), "SRC 600角".into());
    section.steel_material = Some(MaterialId(1));
    section.thickness = Some(1.0);
    section
}

fn steel(name: &str, fy: Option<f64>) -> Material {
    Material {
        id: MaterialId(1),
        name: name.into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy,
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

#[test]
fn issue953_ratios_use_the_known_grade_and_reject_the_unknown_grade() {
    let section = column(8.0, 13.0);
    for (name, n_expected, m_expected) in [
        ("SN400B", 0.292_370_604_915_024, 0.479_761_357_753_610),
        ("SN490B", 0.274_939_520_230_019, 0.560_512_148_953_901),
    ] {
        for fy in [None, Some(100.0), Some(999.0)] {
            let known = steel(name, fy);
            let (n, m) =
                src_column_rank_ratios(&section, Some(&known), 24.0, 345.0, 3_400_000.0).unwrap();
            assert_abs_diff_eq!(n, n_expected, epsilon = 1e-12);
            assert_abs_diff_eq!(m, m_expected, epsilon = 1e-12);
            assert_eq!(src_column_rank(n, m, false), MemberRank::FA);
            let (tension, m_tension) =
                src_column_rank_ratios(&section, Some(&known), 24.0, 345.0, -2_000_000.0).unwrap();
            assert_eq!(tension, 0.0);
            assert_abs_diff_eq!(m_tension, m_expected, epsilon = 1e-12);
        }
    }
    for name in ["任意鋼材", "SN400B-未登録"] {
        for fy in [100.0, 235.0, 345.0] {
            let unknown = steel(name, Some(fy));
            let reason = src_column_rank_ratios(&section, Some(&unknown), 24.0, 345.0, 3_400_000.0)
                .unwrap_err();
            assert!(
                reason.contains(name) && reason.contains("適用根拠"),
                "{reason}"
            );
        }
    }
}

#[test]
fn both_actual_plate_thicknesses_determine_the_grade_strength() {
    for (name, web_mm, flange_mm, n_expected, m_expected) in [
        (
            "SN400B",
            40.0,
            13.0,
            0.240_209_280_743_352,
            0.632_982_941_808_280,
        ),
        (
            "SN400B",
            40.001,
            13.0,
            0.247_251_230_149_334,
            0.612_089_103_495_213,
        ),
        (
            "SN400B",
            8.0,
            40.0,
            0.246_078_591_939_861,
            0.688_687_139_634_260,
        ),
        (
            "SN400B",
            8.0,
            40.001,
            0.252_870_876_599_915,
            0.669_308_933_385_998,
        ),
        (
            "SN400B",
            100.0,
            13.0,
            0.188_499_699_295_873,
            0.747_121_830_881_161,
        ),
        (
            "SM520B",
            40.0,
            13.0,
            0.205_146_472_728_461,
            0.722_634_498_207_091,
        ),
        (
            "SM520B",
            40.001,
            13.0,
            0.210_260_198_423_271,
            0.710_866_207_153_148,
        ),
        (
            "SM520B",
            75.0,
            13.0,
            0.167_973_284_304_710,
            0.787_675_355_750_434,
        ),
        (
            "SM520B",
            75.001,
            13.0,
            0.170_777_664_905_178,
            0.782_564_460_895_371,
        ),
        (
            "SM520B",
            100.0,
            13.0,
            0.149_627_049_832_565,
            0.817_053_018_085_847,
        ),
    ] {
        let section = column(web_mm, flange_mm);
        let steel = steel(name, Some(999.0));
        let (n, m) = src_column_rank_ratios(&section, Some(&steel), 24.0, 345.0, 3_400_000.0)
            .unwrap_or_else(|reason| panic!("{name} tw={web_mm} tf={flange_mm}: {reason}"));
        assert_abs_diff_eq!(n, n_expected, epsilon = 1e-12);
        assert_abs_diff_eq!(m, m_expected, epsilon = 1e-12);
    }
    for name in ["SN400B", "SM520B"] {
        let steel = steel(name, Some(999.0));
        for (web_mm, flange_mm) in [(100.001, 13.0), (8.0, 100.001)] {
            let reason = src_column_rank_ratios(
                &column(web_mm, flange_mm),
                Some(&steel),
                24.0,
                345.0,
                3_400_000.0,
            )
            .unwrap_err();
            assert!(
                reason.contains(name) && reason.contains("適用範囲"),
                "{reason}"
            );
        }
        for invalid_mm in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            for (web_mm, flange_mm) in [(invalid_mm, 13.0), (8.0, invalid_mm)] {
                let reason = src_column_rank_ratios(
                    &column(web_mm, flange_mm),
                    Some(&steel),
                    24.0,
                    345.0,
                    3_400_000.0,
                )
                .unwrap_err();
                assert!(reason.contains("実板厚"), "{reason}");
            }
        }
    }
}

#[test]
fn each_sn_plate_meets_its_designated_grade_thickness_range() {
    for (name, minimum_mm, n_expected, m_expected) in [
        ("SN400A", 6.0, 0.311_978_924_878_245, 0.333_776_816_597_366),
        ("SN400B", 6.0, 0.311_978_924_878_245, 0.333_776_816_597_366),
        ("SN490B", 6.0, 0.300_255_427_674_188, 0.409_287_280_410_680),
        ("SN400C", 16.0, 0.272_145_171_982_913, 0.559_408_444_610_443),
        ("SN490C", 16.0, 0.250_013_749_795_655, 0.637_146_692_988_121),
    ] {
        let steel = steel(name, Some(999.0));
        let ratios = |web_mm, flange_mm| {
            src_column_rank_ratios(
                &column(web_mm, flange_mm),
                Some(&steel),
                24.0,
                345.0,
                3_400_000.0,
            )
        };
        let (n, m) = ratios(minimum_mm, minimum_mm).unwrap();
        assert_abs_diff_eq!(n, n_expected, epsilon = 1e-12);
        assert_abs_diff_eq!(m, m_expected, epsilon = 1e-12);

        let too_thin_mm = minimum_mm - 0.001;
        for (web_mm, flange_mm) in [(too_thin_mm, minimum_mm), (minimum_mm, too_thin_mm)] {
            let reason = ratios(web_mm, flange_mm).unwrap_err();
            assert!(
                reason.contains(name) && reason.contains("JIS G 3136"),
                "{reason}"
            );
            assert!(
                reason.contains(&format!("{minimum_mm} mm 以上")),
                "{reason}"
            );
        }
        for (web_mm, flange_mm) in [(100.001, minimum_mm), (minimum_mm, 100.001)] {
            let reason = ratios(web_mm, flange_mm).unwrap_err();
            assert!(
                reason.contains(name) && reason.contains("適用範囲"),
                "{reason}"
            );
        }
    }

    for (name, web_mm, flange_mm, n_expected, m_expected) in [
        (
            "SN400C",
            100.0,
            16.0,
            0.187_309_618_514_902,
            0.751_706_835_351_704,
        ),
        (
            "SN490C",
            16.0,
            100.0,
            0.157_350_848_138_565,
            0.847_220_915_361_451,
        ),
    ] {
        let (n, m) = src_column_rank_ratios(
            &column(web_mm, flange_mm),
            Some(&steel(name, Some(999.0))),
            24.0,
            345.0,
            3_400_000.0,
        )
        .unwrap();
        assert_abs_diff_eq!(n, n_expected, epsilon = 1e-12);
        assert_abs_diff_eq!(m, m_expected, epsilon = 1e-12);
    }
}

#[test]
fn missing_or_inapplicable_steel_reports_why_the_ratio_is_unavailable() {
    let section = column(8.0, 13.0);
    let ratios = |steel: Option<&Material>| {
        src_column_rank_ratios(&section, steel, 24.0, 345.0, 3_400_000.0)
    };
    let reason = ratios(None).unwrap_err();
    assert!(
        reason.contains("内蔵鉄骨") && reason.contains("未割当"),
        "{reason}"
    );
    let mut concrete = steel("Fc24", None);
    concrete.category = MaterialCategory::Concrete;
    let reason = ratios(Some(&concrete)).unwrap_err();
    assert!(
        reason.contains("Fc24") && reason.contains("鋼材ではありません"),
        "{reason}"
    );
    for name in ["STK400", "SNR400B", "SA440"] {
        let incompatible = steel(name, Some(235.0));
        let reason = ratios(Some(&incompatible)).unwrap_err();
        assert!(
            reason.contains(name) && reason.contains("H 形鋼としての適用根拠"),
            "{reason}"
        );
    }
    for name in ["SM400", "SM490", "SM520", "SN400", "SN490"] {
        for fy in [None, Some(235.0)] {
            let incomplete = steel(name, fy);
            let reason = ratios(Some(&incomplete)).unwrap_err();
            assert!(
                reason.contains(name) && reason.contains("JIS の種類記号が不完全"),
                "{reason}"
            );
        }
    }
    for fy in [
        None,
        Some(100.0),
        Some(0.0),
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
        Some(f64::NEG_INFINITY),
    ] {
        let unknown = steel("任意鋼材", fy);
        let reason = ratios(Some(&unknown)).unwrap_err();
        assert!(
            reason.contains("任意鋼材") && reason.contains("H 形鋼としての適用根拠"),
            "{reason}"
        );
        assert!(!reason.contains("fy"), "{reason}");
    }
}

#[test]
fn invalid_geometry_or_strength_does_not_produce_ratios() {
    let mut section = column(8.0, 13.0);
    let steel = steel("SN400B", None);
    for (fc, rebar_sy, axial_n) in [(0.0, 345.0, 0.0), (24.0, 0.0, 0.0), (24.0, 345.0, f64::NAN)] {
        assert!(src_column_rank_ratios(&section, Some(&steel), fc, rebar_sy, axial_n).is_err());
    }
    let Some(SectionShape::SrcColumnRect { rebar, .. }) = &mut section.shape else {
        unreachable!()
    };
    rebar.x = vec![1];
    rebar.y = vec![1];
    let reason = src_column_rank_ratios(&section, Some(&steel), 24.0, 345.0, 0.0).unwrap_err();
    assert!(reason.contains("配筋形状"), "{reason}");
    section.shape = None;
    let reason = src_column_rank_ratios(&section, Some(&steel), 24.0, 345.0, 0.0).unwrap_err();
    assert!(reason.contains("SRC 矩形柱の断面形状"), "{reason}");
}
