use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory, Section};
use sepika_core::section_shape::{RcRectColumnRebar, RebarSize, RectColumnHoop, SectionShape};
use sepika_design_jp::{
    CheckComponent, CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt,
    MemberKind, QdMethod, SeismicQd, SrcColumnHeadLimit, SrcColumnSeismicBending,
    SrcColumnSeismicBendingAxis, SrcColumnSeismicBendingEnd, SrcDesign,
};

const STEEL_STRONG_N_MM: f64 = 269_837_202.133_333_3;
const STEEL_WEAK_N_MM: f64 = 40_770_833.066_666_67;

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

fn section() -> Section {
    SectionShape::SrcColumnRect {
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
        steel_web_thick: 8.0,
        steel_flange_thick: 13.0,
    }
    .to_section(SectionId(0), "SRC 600角".into())
}

fn axis(steel_n_mm: f64, head_rc_n_mm: f64, base_rc_n_mm: f64) -> SrcColumnSeismicBendingAxis {
    SrcColumnSeismicBendingAxis {
        head: SrcColumnSeismicBendingEnd {
            steel_allowable_n_mm: steel_n_mm,
            rc_ultimate_n_mm: head_rc_n_mm,
        },
        base: SrcColumnSeismicBendingEnd {
            steel_allowable_n_mm: steel_n_mm,
            rc_ultimate_n_mm: base_rc_n_mm,
        },
        head_limit: SrcColumnHeadLimit::BothEndsYield,
    }
}

fn context(qy_rc_n_mm: [f64; 2], qz_rc_n_mm: [f64; 2]) -> DesignCtx {
    DesignCtx {
        kind: MemberKind::Column,
        term: LoadTerm::Short,
        rebar_material: Some(material(
            "SD345",
            MaterialCategory::Rebar,
            None,
            Some(345.0),
        )),
        shear_rebar_material: Some(material(
            "SD295",
            MaterialCategory::Rebar,
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
        src_column_seismic_bending: Some(SrcColumnSeismicBending {
            qy: axis(STEEL_STRONG_N_MM, qy_rc_n_mm[0], qy_rc_n_mm[1]),
            qz: axis(STEEL_WEAK_N_MM, qz_rc_n_mm[0], qz_rc_n_mm[1]),
        }),
        ..Default::default()
    }
}

fn forces(compression_n: f64) -> MemberForcesAt {
    MemberForcesAt {
        pos: 0.0,
        n: -compression_n,
        qy: 800_000.0,
        qz: 800_000.0,
        mx: 0.0,
        my: 0.0,
        mz: 0.0,
    }
}

fn check(sec: &Section, f: &MemberForcesAt, ctx: &DesignCtx) -> CheckOutcome {
    SrcDesign.check(
        f,
        sec,
        &material("Fc24", MaterialCategory::Concrete, Some(24.0), None),
        ctx,
    )
}

fn shear(ctx: &DesignCtx, f: &MemberForcesAt) -> CheckComponent {
    let out = check(&section(), f, ctx);
    let CheckOutcome::Checked(result) = out else {
        panic!("{out:?}");
    };
    result
        .components
        .into_iter()
        .find(|c| c.kind == CheckKind::Shear)
        .unwrap()
}

fn value(shear: &CheckComponent, direction: &str, label: &str) -> f64 {
    let direction_detail = shear.detail.split_once(direction).unwrap().1;
    direction_detail
        .split_once(label)
        .unwrap()
        .1
        .split([',', ' '])
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected}"
    );
}

fn assert_skipped(out: CheckOutcome, expected: &str) {
    assert!(
        matches!(&out, CheckOutcome::Skipped { reason } if reason.contains(expected)),
        "{out:?}"
    );
}

#[test]
fn qd1_keeps_zero_rc_demand_at_pure_compression_and_tension_boundaries() {
    // 付1.3-10〜15の純圧縮 rN0 と全主筋の純引張下限。#953の独立例。
    for compression_n in [9_994_811.831_860_598, -1_354_811.831_860_598] {
        let f = forces(compression_n);
        let mut ctx = context([0.0; 2], [0.0; 2]);
        let zero = shear(&ctx, &f);
        close(zero.ratio, 0.332_355_603_500_943_1, 1e-10);
        for (direction, steel_n, steel_ratio) in [
            ("Qy方向", 134_918.601_066_666_66, 0.332_355_603_500_943_1),
            ("Qz方向", 20_385.416_533_333_337, 0.043_341_134_004_731_524),
        ] {
            close(value(&zero, direction, "sQD="), steel_n, 1e-6);
            close(value(&zero, direction, "sQD/sQA="), steel_ratio, 1e-10);
            assert_eq!(value(&zero, direction, "rQD="), 0.0);
            assert_eq!(value(&zero, direction, "rQD/rQA="), 0.0);
        }
        ctx.seismic_qd.as_mut().unwrap().long_at[0] = (
            0.5e-6,
            [f64::NAN, -15_000.0, 75_000.0, f64::NAN, f64::NAN, f64::NAN],
        );
        assert_eq!(shear(&ctx, &f), zero);
    }
}

#[test]
fn qd1_uses_both_column_ends_and_directional_ultimate_moments() {
    // #953の独立例。柱頭3,300 kN・柱脚1,000 kNの圧縮。
    let ctx = context(
        [937_029_839.646_543_6, 590_432_617.424_321_4],
        [753_887_231.892_659_7, 479_789_651.155_705_9],
    );
    let compression = shear(&ctx, &forces(3_300_000.0));
    close(compression.ratio, 0.913_634_597_685_867_3, 1e-10);
    close(
        value(&compression, "Qy方向", "rQD="),
        381_865.614_267_716_3,
        1e-6,
    );
    close(
        value(&compression, "Qz方向", "rQD="),
        308_419.220_762_091_4,
        1e-6,
    );
    close(
        value(&compression, "Qz方向", "rQD/rQA="),
        0.737_910_039_949_319_7,
        1e-10,
    );

    // 両端が純引張下限の0.9倍。弱軸は中間主筋を含む多段式の独立値。
    let ctx = context([32_515_483.964_654_326; 2], [21_451_187.337_792_844; 2]);
    let tension = shear(&ctx, &forces(-1_219_330.648_674_539));
    close(
        value(&tension, "Qy方向", "rQD="),
        16_257.741_982_327_163,
        1e-6,
    );
    close(
        value(&tension, "Qz方向", "rQD="),
        10_725.593_668_896_421,
        1e-6,
    );
    close(
        value(&tension, "Qz方向", "rQD/rQA="),
        0.025_661_575_932_716_07,
        1e-10,
    );
}

#[test]
fn qd1_limits_column_head_rc_moment_using_the_connected_beams() {
    // #953の独立例。柱両端3,300 kN、左右梁各450 kN·m。
    let mut ctx = context([937_029_839.646_543_6; 2], [753_887_231.892_659_7; 2]);
    let f = forces(3_300_000.0);
    for (top_story, expected) in [
        (false, 279_298.159_378_302_6),
        (true, 391_798.159_378_302_6),
    ] {
        ctx.src_column_seismic_bending
            .as_mut()
            .unwrap()
            .qy
            .head_limit = SrcColumnHeadLimit::ConnectedBeams {
            sum_n_mm: 900_000_000.0,
            top_story,
        };
        let actual = shear(&ctx, &f);
        close(value(&actual, "Qy方向", "rQD="), expected, 1e-6);
        close(
            value(&actual, "Qy方向", "sQD="),
            134_918.601_066_666_66,
            1e-6,
        );
    }
    ctx.src_column_seismic_bending
        .as_mut()
        .unwrap()
        .qy
        .head_limit = SrcColumnHeadLimit::ConnectedBeams {
        sum_n_mm: 1.0,
        top_story: false,
    };
    assert_skipped(check(&section(), &f, &ctx), "上限が負");
}

#[test]
fn column_qd_reports_missing_bending_or_alternative_without_a_ratio() {
    let mut ctx = context([0.0; 2], [0.0; 2]);
    let f = forces(0.0);
    for method in [QdMethod::Min, QdMethod::Qd2] {
        ctx.seismic_qd.as_mut().unwrap().method = method;
        assert_skipped(check(&section(), &f, &ctx), "一次設計用地震力");
    }
    ctx.src_column_seismic_bending = None;
    ctx.seismic_qd.as_mut().unwrap().method = QdMethod::default();
    let out = check(&section(), &f, &ctx);
    assert_skipped(out.clone(), "一次設計用地震力");
    assert_skipped(out, "柱頭・柱脚の軸力");
    ctx.seismic_qd.as_mut().unwrap().method = QdMethod::Qd1;
    assert_skipped(check(&section(), &f, &ctx), "柱頭の接続梁");
}

#[test]
fn column_qd_never_reports_compliance_for_nonfinite_axial_force_or_moments() {
    let ctx = context([0.0; 2], [0.0; 2]);
    let sec = section();
    let concrete = material("Fc24", MaterialCategory::Concrete, Some(24.0), None);
    let outcomes = |f: &MemberForcesAt| {
        SrcDesign
            .check_outcomes(f, &sec, &concrete, &ctx, false)
            .collect::<Vec<_>>()
    };

    for (f, reason) in [
        (forces(f64::NAN), "軸力N"),
        (
            MemberForcesAt {
                my: f64::NAN,
                ..forces(0.0)
            },
            "モーメントMy",
        ),
    ] {
        assert_skipped(check(&sec, &f, &ctx), reason);
        let rows = outcomes(&f);
        assert!(
            matches!(rows.as_slice(), [CheckOutcome::Skipped { reason: actual }]
            if actual.contains(reason)),
            "{rows:?}"
        );
    }

    for (f, reason) in [
        (
            MemberForcesAt {
                my: f64::NAN,
                mz: 4.0e9,
                ..forces(0.0)
            },
            "モーメントMy",
        ),
        (
            MemberForcesAt {
                my: -4.0e9,
                mz: f64::INFINITY,
                ..forces(0.0)
            },
            "モーメントMz",
        ),
        (
            MemberForcesAt {
                my: f64::INFINITY,
                mz: f64::NAN,
                ..forces(40.0e6)
            },
            "モーメントMy",
        ),
    ] {
        assert_skipped(check(&sec, &f, &ctx), reason);
        let rows = outcomes(&f);
        assert!(
            matches!(
                rows.as_slice(),
                [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason: actual }]
                    if result.ratio().is_finite() && !result.ok()
                        && result.components.len() == 1
                        && result.components[0].kind == CheckKind::AxialBending
                        && actual.contains(reason)
            ),
            "{rows:?}"
        );
    }
}

#[test]
fn qd1_preserves_independent_shear_failure_when_an_evaluation_moment_is_nonfinite() {
    // #953の独立例にそろえ、柱頭・柱脚ともに圧縮3,300 kNを用いる。
    let mut ctx = context([937_029_839.646_543_6; 2], [753_887_231.892_659_7; 2]);
    ctx.seismic_qd.as_mut().unwrap().long_at[0].0 = 0.5;
    let sec = section();
    let concrete = material("Fc24", MaterialCategory::Concrete, Some(24.0), None);
    let f = MemberForcesAt {
        pos: 0.5,
        ..forces(3_300_000.0)
    };
    let CheckOutcome::Checked(result) = check(&sec, &f, &ctx) else {
        panic!("両方向の曲げが有限");
    };
    let expected = result
        .components
        .iter()
        .find(|c| c.kind == CheckKind::Shear)
        .unwrap();
    close(expected.ratio, 1.120_947_853, 1e-9);
    close(
        value(expected, "Qy方向", "rQD="),
        468_514.919_823_271_8,
        1e-6,
    );
    close(
        value(expected, "Qy方向", "rQA="),
        417_963.171_748_244_4,
        1e-6,
    );

    let unknown_moment = MemberForcesAt { my: f64::NAN, ..f };
    assert_skipped(check(&sec, &unknown_moment, &ctx), "モーメントMy");
    let rows: Vec<_> = SrcDesign
        .check_outcomes(&unknown_moment, &sec, &concrete, &ctx, false)
        .collect();
    assert!(
        matches!(
            rows.as_slice(),
            [CheckOutcome::Checked(known), CheckOutcome::Skipped { reason }]
                if known.components.as_slice() == std::slice::from_ref(expected)
                    && reason.contains("モーメントMy")
        ),
        "{rows:?}"
    );

    let unknown_axial = MemberForcesAt { n: f64::NAN, ..f };
    let rows: Vec<_> = SrcDesign
        .check_outcomes(&unknown_axial, &sec, &concrete, &ctx, false)
        .collect();
    assert!(
        matches!(rows.as_slice(), [CheckOutcome::Skipped { reason }]
        if reason.contains("軸力N")),
        "{rows:?}"
    );

    let mut invalid_qa = sec.clone();
    let Some(SectionShape::SrcColumnRect {
        steel_height,
        steel_flange_thick,
        ..
    }) = &mut invalid_qa.shape
    else {
        unreachable!()
    };
    *steel_height = 2.0 * *steel_flange_thick;
    let rows: Vec<_> = SrcDesign
        .check_outcomes(&unknown_moment, &invalid_qa, &concrete, &ctx, false)
        .collect();
    assert!(
        matches!(rows.as_slice(), [CheckOutcome::Skipped { reason }]
        if reason.contains("モーメントMy") && reason.contains("材料別許容せん断力")),
        "{rows:?}"
    );

    ctx.src_column_seismic_bending = None;
    let rows: Vec<_> = SrcDesign
        .check_outcomes(&unknown_moment, &sec, &concrete, &ctx, false)
        .collect();
    assert!(
        matches!(rows.as_slice(), [CheckOutcome::Skipped { reason }]
        if reason.contains("モーメントMy") && reason.contains("柱頭・柱脚の軸力")),
        "{rows:?}"
    );
}

#[test]
fn qd1_rejects_invalid_demand_inputs_and_allowables() {
    let mut ctx = context([0.0; 2], [0.0; 2]);
    let f = forces(0.0);
    for invalid in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        ctx.seismic_qd.as_mut().unwrap().clear_length = invalid;
        assert_skipped(check(&section(), &f, &ctx), "内法高さ");
    }
    ctx.seismic_qd.as_mut().unwrap().clear_length = 4000.0;
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        ctx.src_column_seismic_bending
            .as_mut()
            .unwrap()
            .qz
            .base
            .rc_ultimate_n_mm = invalid;
        assert_skipped(check(&section(), &f, &ctx), "Qz方向の柱頭・柱脚");
    }
    ctx.src_column_seismic_bending
        .as_mut()
        .unwrap()
        .qz
        .base
        .rc_ultimate_n_mm = f64::MAX;
    ctx.src_column_seismic_bending
        .as_mut()
        .unwrap()
        .qz
        .head
        .rc_ultimate_n_mm = f64::MAX;
    assert_skipped(check(&section(), &f, &ctx), "設計用せん断力が非有限");
    ctx.src_column_seismic_bending.as_mut().unwrap().qz = axis(STEEL_WEAK_N_MM, 1e-321, 0.0);
    assert_skipped(check(&section(), &f, &ctx), "設計用せん断力が数値範囲外");
    ctx.src_column_seismic_bending.as_mut().unwrap().qz = axis(STEEL_WEAK_N_MM, 0.0, 0.0);
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        ctx.src_column_seismic_bending
            .as_mut()
            .unwrap()
            .qy
            .head_limit = SrcColumnHeadLimit::ConnectedBeams {
            sum_n_mm: invalid,
            top_story: false,
        };
        assert_skipped(check(&section(), &f, &ctx), "接続梁の材料別曲げ耐力");
    }
    ctx.src_column_seismic_bending
        .as_mut()
        .unwrap()
        .qy
        .head_limit = SrcColumnHeadLimit::BothEndsYield;
    let mut q_nonfinite = forces(0.0);
    q_nonfinite.qz = f64::NAN;
    assert_skipped(
        check(&section(), &q_nonfinite, &ctx),
        "せん断力（Qy・Qz）が非有限",
    );

    let mut invalid = section();
    let Some(SectionShape::SrcColumnRect {
        steel_height,
        steel_flange_thick,
        ..
    }) = &mut invalid.shape
    else {
        unreachable!()
    };
    *steel_height = 2.0 * *steel_flange_thick;
    assert_skipped(check(&invalid, &f, &ctx), "許容せん断力には有限な正値");
}
