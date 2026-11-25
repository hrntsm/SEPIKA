use super::*;
use crate::rc::{concrete_allowable_shear, rebar_allowable_shear};
use crate::steel::{steel_f_value_prefix, steel_ft};
use crate::{LoadTerm, SeismicQd};
use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::MaterialCategory;
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
};

pub(crate) fn make_material(fc: f64, grade: &str) -> Material {
    Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(0),
        name: grade.to_string(),
        category: MaterialCategory::Concrete,
        young: 205000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: Some(fc),
        fy: None,
    }
}

pub(crate) fn make_material_no_fc(grade: &str) -> Material {
    Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(0),
        name: grade.to_string(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: None,
        fy: None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn src_rect_shape(
    b: f64,
    d: f64,
    main_count: u32,
    main_dia: f64,
    main_layers: u32,
    cover: f64,
    shear_dia: f64,
    shear_pitch: f64,
    shear_legs: u32,
    steel_height: f64,
    steel_width: f64,
    steel_web_thick: f64,
    steel_flange_thick: f64,
) -> SectionShape {
    SectionShape::SrcColumnRect {
        b,
        d,
        rebar: RcRectColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(main_dia),
            x: vec![main_count / 2; main_layers as usize],
            y: vec![main_count / 2; main_layers as usize],
            cover,
            hoop: RectColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(shear_dia),
                pitch: shear_pitch,
                legs_x: shear_legs,
                legs_y: shear_legs,
            },
        },
        steel_height,
        steel_width,
        steel_web_thick,
        steel_flange_thick,
    }
}

/// SRC 柱の標準テスト断面（500x500、8-D22 主筋、内蔵鉄骨 300x200 H形）。
pub(crate) fn src_column_shape() -> SectionShape {
    src_rect_shape(
        500.0, 500.0, 8, 22.0, 2, 40.0, 10.0, 100.0, 2, 300.0, 200.0, 9.0, 14.0,
    )
}

/// 実配筋 SRC 梁のテスト断面（RC 部分は [`RcBeamRebar`]）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn src_beam_rect_shape(
    b: f64,
    d: f64,
    main_dia: f64,
    top: Vec<u32>,
    bottom: Vec<u32>,
    cover: f64,
    shear_dia: f64,
    shear_pitch: f64,
    shear_legs: u32,
    steel_height: f64,
    steel_width: f64,
    steel_web_thick: f64,
    steel_flange_thick: f64,
) -> SectionShape {
    SectionShape::SrcBeamRect {
        b,
        d,
        rebar: RcBeamRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(main_dia),
            top,
            bottom,
            cover,
            stirrup: BeamStirrup {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(shear_dia),
                pitch: shear_pitch,
                legs: shear_legs,
            },
        },
        steel_height,
        steel_width,
        steel_web_thick,
        steel_flange_thick,
    }
}

/// 実配筋 SRC 矩形柱のテスト断面（RC 部分は [`RcRectColumnRebar`]）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn src_column_rect_shape(
    b: f64,
    d: f64,
    main_dia: f64,
    x: Vec<u32>,
    y: Vec<u32>,
    cover: f64,
    hoop_dia: f64,
    hoop_pitch: f64,
    legs_x: u32,
    legs_y: u32,
    steel_height: f64,
    steel_width: f64,
    steel_web_thick: f64,
    steel_flange_thick: f64,
) -> SectionShape {
    SectionShape::SrcColumnRect {
        b,
        d,
        rebar: RcRectColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(main_dia),
            x,
            y,
            cover,
            hoop: RectColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(hoop_dia),
                pitch: hoop_pitch,
                legs_x,
                legs_y,
            },
        },
        steel_height,
        steel_width,
        steel_web_thick,
        steel_flange_thick,
    }
}

pub(crate) fn make_section(shape: SectionShape) -> Section {
    shape.to_section(SectionId(0), "test".to_string())
}

pub(crate) fn zero_forces() -> MemberForcesAt {
    MemberForcesAt {
        pos: 0.0,
        n: 0.0,
        qy: 0.0,
        qz: 0.0,
        mx: 0.0,
        my: 0.0,
        mz: 0.0,
    }
}

/// 鉄筋の材料（材料名がグレード名、`fy` が降伏点）。
pub(crate) fn make_rebar_material(grade: &str, fy: f64) -> Material {
    Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(1),
        name: grade.to_string(),
        category: MaterialCategory::Rebar,
        young: 205000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: None,
        fy: Some(fy),
    }
}

/// 内蔵鉄骨の材料（材料名が鋼種名。F 値の解決に用いる）。
pub(crate) fn make_steel_material(grade: &str) -> Material {
    Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(2),
        name: grade.to_string(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: None,
        fy: None,
    }
}

/// SRC 検定の標準の材料構成（主筋 SD345・せん断補強筋 SD295A・内蔵鉄骨 SN400B）。
/// 材料は断面が持つため、検定コンテキストへ渡して解決させる。
fn ctx_materials(ctx: DesignCtx) -> DesignCtx {
    DesignCtx {
        rebar_material: Some(make_rebar_material("SD345", 345.0)),
        shear_rebar_material: Some(make_rebar_material("SD295A", 295.0)),
        steel_material: Some(make_steel_material("SN400B")),
        ..ctx
    }
}

pub(crate) fn ctx_beam(term: LoadTerm) -> DesignCtx {
    ctx_materials(DesignCtx {
        term,
        kind: MemberKind::Girder,
        ..Default::default()
    })
}

pub(crate) fn ctx_column(term: LoadTerm) -> DesignCtx {
    ctx_materials(DesignCtx {
        term,
        kind: MemberKind::Column,
        lk_y: Some(3000.0),
        lk_z: Some(3000.0),
        ..Default::default()
    })
}

/// SRC 柱の短期 RC 部許容せん断力 rQAS1 は α を含まない
/// （SRC規準。rQAS1 = b・rj・(fs + 0.5・pw・wft)）。
#[test]
fn test_src_column_short_rc_allowable_has_no_alpha() {
    let shape = src_rect_shape(
        400.0, 700.0, 6, 22.0, 2, 40.0, 13.0, 100.0, 2, 500.0, 200.0, 9.0, 14.0,
    );
    let rebar = match &shape {
        SectionShape::SrcColumnRect { rebar, .. } => rebar.clone(),
        _ => unreachable!(),
    };
    let props = column_axis_props(400.0, 700.0, &rebar, true);
    let fs = concrete_allowable_shear(24.0, false);
    let w_ft = rebar_allowable_shear("SD345", false);
    // m_for_alpha=0 → α は上限側（=2）に張り付く条件。α が式に入っていれば
    // rQA1 が 2 倍近く動くが、柱の短期 rQAS1 は α 非依存であること。
    let shear = src_column_shear_check(
        100_000.0,
        0.0,
        100_000.0,
        props.j,
        props.d,
        props.b,
        390.0, // b′ を大きく取り rQA2 を支配させない
        props.pw,
        fs,
        Some(w_ft),
        0.0,
        0.0,
        2.0,
        0.0,
        Some(SrcShortShearDemand {
            steel_n: Some(0.0),
            rc_n: Some(100_000.0),
        }),
        LoadTerm::Short,
    )
    .unwrap();
    let pw = props.pw.min(0.006);
    let expected_rqa1 = props.b * props.j * (fs + 0.5 * pw * w_ft);
    let expected_rqa2 = props.b * props.j * (2.0 * (390.0 / props.b) * fs + pw * w_ft);
    let expected = expected_rqa1.min(expected_rqa2);
    assert!(
        (shear.r_qa.unwrap() - expected).abs() / expected < 1e-9,
        "rQA={} expected={}（α を含まない短期式）",
        shear.r_qa.unwrap(),
        expected
    );
}

/// SRC 柱の長期は併用式 QA = (1+β)・b・rj・a′・fs を全せん断力と比較する
/// （SRC規準18条。a′ = rα（b′/b ≥ rα/3 のとき）または 3b′/b）。
#[test]
fn test_src_column_long_combined_formula() {
    let shape = src_rect_shape(
        400.0, 700.0, 6, 22.0, 2, 40.0, 13.0, 100.0, 2, 500.0, 200.0, 9.0, 14.0,
    );
    let rebar = match &shape {
        SectionShape::SrcColumnRect { rebar, .. } => rebar.clone(),
        _ => unreachable!(),
    };
    let props = column_axis_props(400.0, 700.0, &rebar, true);
    let fs = concrete_allowable_shear(24.0, true);
    let beta = 0.25;
    let q = 150_000.0;
    // m=0 → α=2（上限）。b′/b=0.5 < α/3=2/3 なので a′=3b′/b=1.5 が採用される。
    let b_prime = 0.5 * props.b;
    let shear = src_column_shear_check(
        q,
        0.0,
        q,
        props.j,
        props.d,
        props.b,
        b_prime,
        props.pw,
        fs,
        None,
        100.0,
        1000.0,
        2.0,
        beta,
        None,
        LoadTerm::Long,
    )
    .unwrap();
    let a_prime = 1.5;
    let qa = (1.0 + beta) * props.b * props.j * a_prime * fs;
    assert!(
        (shear.r_qa.unwrap() - qa).abs() / qa < 1e-9,
        "QA={} expected={}",
        shear.r_qa.unwrap(),
        qa
    );
    assert!((shear.ratio - q / qa).abs() < 1e-9, "ratio={}", shear.ratio);
}

#[test]
fn src_column_long_strong_shear_uses_the_web_clear_depth_and_material_young_ratio() {
    let section = make_section(src_column_shape());
    let material = Material {
        young: 20_500.0,
        ..make_material(24.0, "Fc24")
    };
    let context = ctx_column(LoadTerm::Long);
    let forces = MemberForcesAt {
        qy: 100_000.0,
        ..zero_forces()
    };

    let CheckOutcome::Checked(result) = SrcDesign.check(&forces, &section, &material, &context)
    else {
        panic!("SRC柱の長期せん断検定を算定できません");
    };
    let shear = result
        .components
        .iter()
        .find(|component| component.kind == crate::CheckKind::Shear)
        .unwrap();
    // ウェブ内法272 mm、Es/Ec=10の式(18.14)による強軸許容せん断力は284536.845 N。
    assert!((shear.ratio - 100_000.0 / 284_536.845).abs() < 1e-12);
}

#[test]
fn test_src_fc_missing_skip() {
    let shape = src_column_shape();
    let sec = make_section(shape);
    let mat = make_material_no_fc("SD345");
    let ctx = ctx_column(LoadTerm::Long);
    let design = SrcDesign;
    let outcome = design.check(&zero_forces(), &sec, &mat, &ctx);
    match outcome {
        CheckOutcome::Skipped { reason } => assert!(reason.contains("Fc")),
        CheckOutcome::Checked(_) => panic!("Fc 未設定は検定不能(Skipped)のはず"),
    }
}

#[test]
fn src_column_uses_the_short_column_formula_only_with_verified_lengths() {
    let section = make_section(src_column_shape());
    let material = make_material(24.0, "Fc24");
    let forces = MemberForcesAt {
        n: -100_000.0,
        mz: 1.0e7,
        ..zero_forces()
    };
    let mut context = ctx_column(LoadTerm::Long);
    context.length = 3000.0;
    context.lk_z = Some(6000.0);
    for (length_mm, applies) in [
        (None, true),
        (Some(0.0), false),
        (Some(6000.0), true),
        (Some(6000.01), false),
    ] {
        context.lk_y = length_mm;
        let outcome = SrcDesign.check(&forces, &section, &material, &context);
        if applies {
            assert!(outcome
                .unwrap_checked()
                .components
                .iter()
                .any(|c| c.kind == crate::CheckKind::AxialBending));
        } else {
            assert!(matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("12倍")));
        }
    }
}

#[test]
fn src_strength_checks_do_not_extrapolate_beyond_the_concrete_strength_table() {
    let section = make_section(src_column_shape());
    let outcome = SrcDesign.check(
        &zero_forces(),
        &section,
        &make_material(60.001, "Fc60超"),
        &ctx_column(LoadTerm::Long),
    );
    assert!(matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("60 N/mm²")));
}

#[test]
fn src_main_rebar_does_not_use_unknown_or_non_table_strengths() {
    let material = make_material(24.0, "Fc24");
    for (shape, context) in [
        (
            src_beam_rect_shape(
                500.0,
                600.0,
                22.0,
                vec![4],
                vec![4],
                40.0,
                10.0,
                100.0,
                2,
                300.0,
                200.0,
                9.0,
                14.0,
            ),
            ctx_beam as fn(LoadTerm) -> DesignCtx,
        ),
        (src_column_shape(), ctx_column),
    ] {
        let section = make_section(shape);
        for term in [LoadTerm::Long, LoadTerm::Short] {
            let mut context = context(term);
            for grade in [
                "USD590",
                "USD590A",
                "USD590B",
                "USD685",
                "USD685A",
                "USD685B",
                "未登録材種",
                "SR295",
            ] {
                context.rebar_material = Some(make_rebar_material(grade, 685.0));
                let outcome = SrcDesign.check(&zero_forces(), &section, &material, &context);
                assert!(matches!(outcome, CheckOutcome::Skipped { reason }
                    if reason.contains(&section.name) && reason.contains(grade) && reason.contains("異形主筋")));
            }
        }
    }
}

#[test]
fn src_column_web_above_width_to_thickness_limit_is_outside_check_scope() {
    let mut shape = src_column_shape();
    let SectionShape::SrcColumnRect {
        steel_web_thick, ..
    } = &mut shape
    else {
        unreachable!();
    };
    *steel_web_thick = 2.75;
    let section = make_section(shape);
    let mut context = ctx_column(LoadTerm::Long);
    context.steel_material = Some(make_steel_material("SS400"));
    let outcome = SrcDesign.check(
        &zero_forces(),
        &section,
        &make_material(24.0, "Fc24"),
        &context,
    );
    assert!(
        matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("解説表7.1") && reason.contains("幅厚比"))
    );
}

#[test]
fn test_src_shape_mismatch_skip() {
    let sec = Section {
        frame_use: Some(sepika_core::model::FrameSectionUse::Column),
        id: SectionId(0),
        name: "no-shape".to_string(),
        area: 1.0,
        iy: 1.0,
        iz: 1.0,
        j: 1.0,
        depth: 500.0,
        width: 500.0,
        as_y: 0.0,
        as_z: 0.0,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: None,
        material: Some(MaterialId(0)),
        rebar_material: None,
        shear_rebar_material: None,
        steel_material: None,
        property_basis: Default::default(),
    };
    let mat = make_material(24.0, "SD345");
    let ctx = ctx_column(LoadTerm::Long);
    let design = SrcDesign;
    let outcome = design.check(&zero_forces(), &sec, &mat, &ctx);
    match outcome {
        CheckOutcome::Skipped { reason } => assert!(reason.contains("断面形状不一致")),
        CheckOutcome::Checked(_) => panic!("断面形状不一致は検定不能(Skipped)のはず"),
    }
}

#[test]
fn src_long_column_checks_do_not_use_unsupported_hoop_strength() {
    let sec = make_section(src_column_shape());
    let mat = make_material(24.0, "SD345");
    let ctx = DesignCtx {
        shear_rebar_material: Some(make_rebar_material("KH785", 785.0)),
        ..ctx_column(LoadTerm::Long)
    };
    assert!(SrcDesign
        .check(&zero_forces(), &sec, &mat, &ctx)
        .unwrap_checked()
        .ok());
}

#[test]
fn src_short_column_shear_checks_require_hoop_strength() {
    let sec = make_section(src_column_shape());
    let mat = make_material(24.0, "SD345");
    let mut shear_mat = make_rebar_material("SR235", 235.0);
    shear_mat.fy = None;
    let ctx = DesignCtx {
        shear_rebar_material: Some(shear_mat),
        ..ctx_column(LoadTerm::Short)
    };
    let forces = MemberForcesAt {
        mz: 250.0e6,
        qy: 500.0e3,
        ..zero_forces()
    };
    let design = SrcDesign;
    match design.check(&forces, &sec, &mat, &ctx) {
        CheckOutcome::Skipped { reason } => {
            assert!(
                reason.contains("SR235") && reason.contains("fy"),
                "{reason}"
            );
        }
        CheckOutcome::Checked(_) => panic!("fy 未設定は検定不能(Skipped)のはず"),
    }

    let ctx = DesignCtx {
        shear_rebar_material: Some(make_rebar_material("SR235", 235.0)),
        ..ctx_column(LoadTerm::Short)
    };
    match design.check(&forces, &sec, &mat, &ctx) {
        CheckOutcome::Checked(_) => {}
        CheckOutcome::Skipped { reason } => panic!("fy 設定時は検定するはず: {reason}"),
    }
}

/// 実配筋 SRC 梁で許容応力度検定が動き、鉄骨 sMo に RC 部分 rMA が累加される
/// （MA > sMo）。
#[test]
fn test_src_beam_rect_check_accumulates_steel_and_rc() {
    let shape = src_beam_rect_shape(
        400.0,
        700.0,
        22.0,
        vec![3],
        vec![3],
        40.0,
        10.0,
        100.0,
        2,
        500.0,
        200.0,
        9.0,
        14.0,
    );
    let sec = make_section(shape);
    let mat = make_material(24.0, "SD345");
    let ctx = ctx_beam(LoadTerm::Long);

    let forces = MemberForcesAt {
        mz: 1.0,
        ..zero_forces()
    };
    let r = SrcDesign.check(&forces, &sec, &mat, &ctx).unwrap_checked();
    assert!(
        r.ratio().is_finite() && r.ratio() > 0.0,
        "ratio={}",
        r.ratio()
    );

    let ma = 1.0 / r.ratio();
    let (_sa, sz, _) = steel_h_props(500.0, 200.0, 9.0, 14.0);
    let f_value = steel_f_value_prefix("SN400B", 14.0).unwrap();
    let s_mo = sz * steel_ft(f_value, LoadTerm::Long);
    assert!(
        ma > s_mo * 1.0001,
        "RC 部分の累加で MA > sMo のはず: ma={ma}, sMo={s_mo}"
    );
}

/// 実配筋 SRC 柱で許容応力度検定が動き、鉄骨 sMo に RC 部分の曲げ耐力が
/// 累加される（N=0 の MA_z > sMo）。
#[test]
fn test_src_column_rect_check_accumulates_steel_and_rc() {
    let shape = src_column_rect_shape(
        500.0,
        500.0,
        22.0,
        vec![4],
        vec![4],
        40.0,
        10.0,
        100.0,
        2,
        2,
        300.0,
        200.0,
        9.0,
        14.0,
    );
    let sec = make_section(shape);
    let mat = make_material(24.0, "SD345");
    let ctx = ctx_column(LoadTerm::Long);

    let forces = MemberForcesAt {
        mz: 1.0,
        ..zero_forces()
    };
    let r = SrcDesign.check(&forces, &sec, &mat, &ctx).unwrap_checked();
    assert!(
        r.ratio().is_finite() && r.ratio() > 0.0,
        "ratio={}",
        r.ratio()
    );

    let ma_z = 1.0 / r.ratio();
    let (_sa, sz_z, _) = steel_h_props(300.0, 200.0, 9.0, 14.0);
    let f_value = steel_f_value_prefix("SN400B", 14.0).unwrap();
    let s_mo = sz_z * steel_ft(f_value, LoadTerm::Long);
    assert!(
        ma_z >= s_mo * 0.99,
        "RC 部分の累加で MA_z >= sMo のはず: ma_z={ma_z}, sMo={s_mo}"
    );
}

/// 実配筋 SRC 柱・梁の未入力配筋は検定不能（Skipped）とし、panic しない。
#[test]
fn test_src_new_types_unset_rebar_skip() {
    let beam = src_beam_rect_shape(
        400.0,
        700.0,
        22.0,
        vec![],
        vec![],
        40.0,
        10.0,
        100.0,
        2,
        500.0,
        200.0,
        9.0,
        14.0,
    );
    let col = src_column_rect_shape(
        500.0,
        500.0,
        22.0,
        vec![],
        vec![],
        40.0,
        10.0,
        100.0,
        2,
        2,
        300.0,
        200.0,
        9.0,
        14.0,
    );
    let mat = make_material(24.0, "SD345");
    let beam_out = SrcDesign.check(
        &zero_forces(),
        &make_section(beam),
        &mat,
        &ctx_beam(LoadTerm::Long),
    );
    let col_out = SrcDesign.check(
        &zero_forces(),
        &make_section(col),
        &mat,
        &ctx_column(LoadTerm::Long),
    );
    match beam_out {
        CheckOutcome::Skipped { reason } => assert!(reason.contains("配筋"), "{reason}"),
        CheckOutcome::Checked(_) => panic!("未入力梁配筋は Skipped のはず"),
    }
    match col_out {
        CheckOutcome::Skipped { reason } => assert!(reason.contains("配筋"), "{reason}"),
        CheckOutcome::Checked(_) => panic!("未入力柱配筋は Skipped のはず"),
    }
}

#[test]
fn sm520_out_of_range_src_beam_and_column_do_not_restore_partial_rc_capacity() {
    let material = make_material(24.0, "Fc24");
    let forces = MemberForcesAt {
        pos: 0.5,
        n: 0.0,
        qy: 0.0,
        qz: 0.0,
        mx: 0.0,
        my: 0.0,
        mz: 0.0,
    };
    for name in ["SM520B", "SM520C"] {
        for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
            let mut steel = make_material_no_fc(name);
            steel.fy = fy;
            let rebar = make_rebar_material("SD345", 345.0);
            let shear = make_rebar_material("SD295", 295.0);
            for kind in [MemberKind::Column, MemberKind::Girder] {
                let shape = if kind == MemberKind::Column {
                    src_rect_shape(
                        1500.0, 1500.0, 8, 22.0, 2, 40.0, 10.0, 100.0, 2, 1000.0, 400.0, 100.001,
                        80.0,
                    )
                } else {
                    src_beam_rect_shape(
                        1500.0,
                        1500.0,
                        22.0,
                        vec![4],
                        vec![4],
                        40.0,
                        10.0,
                        100.0,
                        2,
                        1000.0,
                        400.0,
                        100.001,
                        80.0,
                    )
                };
                let sec = shape.to_section(SectionId(0), "SRC範囲外".into());
                let ctx = DesignCtx {
                    kind,
                    steel_material: Some(steel.clone()),
                    rebar_material: Some(rebar.clone()),
                    shear_rebar_material: Some(shear.clone()),
                    ..Default::default()
                };
                assert!(
                    matches!(SrcDesign.check(&forces, &sec, &material, &ctx), CheckOutcome::Skipped { reason } if reason.contains("適用範囲"))
                );
            }
        }
    }
}

fn gravity_contract_beam_section() -> Section {
    make_section(src_beam_rect_shape(
        400.0,
        700.0,
        22.0,
        vec![3],
        vec![3],
        40.0,
        10.0,
        100.0,
        2,
        500.0,
        200.0,
        9.0,
        14.0,
    ))
}

#[test]
fn src_beam_short_bending_failure_is_independent_of_the_seismic_qd_context() {
    let section = gravity_contract_beam_section();
    let material = make_material(24.0, "Fc24");
    let forces = MemberForcesAt {
        mz: 1_000_000_000.0,
        ..zero_forces()
    };
    let mut context = ctx_beam(LoadTerm::Short);
    let baseline = SrcDesign
        .check(&forces, &section, &material, &context)
        .unwrap_checked();
    assert_eq!(baseline.components.len(), 1);
    assert_eq!(baseline.components[0].kind, crate::CheckKind::Bending);
    assert!(!baseline.ok());

    for method in [
        crate::QdMethod::Qd1,
        crate::QdMethod::Qd2,
        crate::QdMethod::Min,
    ] {
        context.seismic_qd = Some(SeismicQd {
            long_at: Vec::new(),
            n_factor: 1.5,
            n_mechanism: 1.0,
            q_simple: None,
            clear_length: 6000.0,
            method,
        });
        let outcomes: Vec<_> = SrcDesign
            .check_outcomes(&forces, &section, &material, &context, true)
            .collect();
        let [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }] = outcomes.as_slice()
        else {
            panic!("曲げの不適合と地震時せん断力の診断が必要です: {outcomes:?}");
        };
        assert_eq!(result.components, baseline.components);
        assert!(reason.contains("付1.4-4"), "{reason}");
    }
}

fn strength912_section(column: bool, web_mm: f64, flange_mm: f64) -> Section {
    make_section(if column {
        src_column_rect_shape(
            300.0,
            600.0,
            16.0,
            vec![2],
            vec![2],
            20.0,
            8.0,
            100.0,
            2,
            2,
            400.0,
            200.0,
            web_mm,
            flange_mm,
        )
    } else {
        src_beam_rect_shape(
            300.0,
            600.0,
            16.0,
            vec![2],
            vec![2],
            20.0,
            8.0,
            100.0,
            2,
            400.0,
            200.0,
            web_mm,
            flange_mm,
        )
    })
}

#[test]
fn src912_unknown_strength_and_missing_or_nonsteel_material_are_not_checked() {
    let concrete = make_material(24.0, "Fc24");
    for column in [false, true] {
        for term in [LoadTerm::Long, LoadTerm::Short] {
            let mut ctx = if column {
                ctx_column(term)
            } else {
                ctx_beam(term)
            };
            let sec = strength912_section(column, 8.0, 12.0);
            let forces = MemberForcesAt {
                n: if column { 2_000_000.0 } else { 0.0 },
                mz: if column { 0.0 } else { 211_982_789.84174365 },
                ..zero_forces()
            };
            for fy in [
                None,
                Some(100.0),
                Some(235.0),
                Some(345.0),
                Some(0.0),
                Some(-1.0),
                Some(f64::NAN),
                Some(f64::INFINITY),
                Some(f64::NEG_INFINITY),
            ] {
                let mut steel = make_steel_material("直接入力鋼材");
                steel.fy = fy;
                ctx.steel_material = Some(steel);
                let CheckOutcome::Skipped { reason } =
                    SrcDesign.check(&forces, &sec, &concrete, &ctx)
                else {
                    panic!(
                        "未知鋼材 column={column}, term={term:?}, fy={fy:?} を補完してはならない"
                    );
                };
                assert!(reason.contains("直接入力"), "{reason}");
                assert!(
                    reason.contains(if fy.is_some_and(|v| v.is_finite() && v > 0.0) {
                        "適用根拠が未確定"
                    } else {
                        "正の有限値"
                    }),
                    "{reason}"
                );
            }
            ctx.steel_material = None;
            assert!(matches!(SrcDesign.check(&forces, &sec, &concrete, &ctx),
                CheckOutcome::Skipped { reason } if reason.contains("未割当")));
            let mut steel = make_steel_material("SN400B");
            steel.category = MaterialCategory::Concrete;
            ctx.steel_material = Some(steel);
            assert!(matches!(SrcDesign.check(&forces, &sec, &concrete, &ctx),
                CheckOutcome::Skipped { reason } if reason.contains("鋼材ではありません")));
        }
    }
}

#[test]
fn src912_issue_beam_independent_capacity_ignores_known_grade_fy() {
    let sec = strength912_section(false, 8.0, 12.0);
    let concrete = make_material(24.0, "Fc24");
    let forces = MemberForcesAt {
        mz: 211_982_789.84174365,
        ..zero_forces()
    };
    let mut ctx = ctx_beam(LoadTerm::Long);
    for fy in [
        None,
        Some(100.0),
        Some(235.0),
        Some(345.0),
        Some(0.0),
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        ctx.steel_material.as_mut().unwrap().fy = fy;
        let check = SrcDesign
            .check(&forces, &sec, &concrete, &ctx)
            .unwrap_checked();
        assert!((check.ratio() - 1.0).abs() < 1e-12, "{check:?}");
        assert!((forces.mz / check.ratio() - 211_982_789.84174365).abs() < 1e-6);
    }
}

#[test]
fn src912_each_actual_plate_thickness_must_be_positive_finite() {
    let concrete = make_material(24.0, "Fc24");
    for column in [false, true] {
        let ctx = if column {
            ctx_column(LoadTerm::Long)
        } else {
            ctx_beam(LoadTerm::Long)
        };
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for (web, flange) in [(invalid, 12.0), (8.0, invalid)] {
                let sec = strength912_section(column, web, flange);
                assert!(
                    matches!(SrcDesign.check(&zero_forces(), &sec, &concrete, &ctx),
                    CheckOutcome::Skipped { reason } if reason.contains("実板厚")),
                    "column={column}, web={web}, flange={flange}"
                );
            }
        }
    }
}

#[test]
fn src912_known_grade_actual_thickness_boundaries_have_independent_capacities() {
    let concrete = make_material(24.0, "Fc24");
    // 鋭角Hの矩形積分Zと4-D16/上下各2-D16の面積から独立算定した長期耐力。
    for (name, web, flange, beam_ma_nmm, column_nt_n) in [
        ("SN400B", 40.0, 12.0, 323022641.93063253, 3281179.9263202487),
        ("SN400B", 8.0, 40.0, 467529635.70841026, 3080646.5929869157),
        ("SM520B", 40.0, 12.0, 466183303.26396585, 4868379.926320249),
        ("SM520B", 8.0, 40.0, 684481102.3750769, 4565446.592986915),
        ("SM520B", 75.001, 12.0, 598360479.0826325, 7322994.72632025),
        ("SM520B", 8.0, 75.001, 927395068.9897438, 7106329.792986916),
        ("SM520B", 100.0, 12.0, 718328924.5972992, 9359579.926320247),
        ("SM520B", 8.0, 100.0, 1059555235.7084103, 9186246.592986915),
    ] {
        for column in [false, true] {
            let sec = strength912_section(column, web, flange);
            let mut ctx = if column {
                ctx_column(LoadTerm::Long)
            } else {
                ctx_beam(LoadTerm::Long)
            };
            ctx.steel_material = Some(make_steel_material(name));
            ctx.steel_material.as_mut().unwrap().fy = Some(999.0);
            let forces = MemberForcesAt {
                n: if column { column_nt_n * 1.1 } else { 0.0 },
                mz: if column { 0.0 } else { beam_ma_nmm * 1.1 },
                ..zero_forces()
            };
            let result = SrcDesign
                .check(&forces, &sec, &concrete, &ctx)
                .unwrap_checked();
            assert!(
                (result.ratio() - 1.1).abs() < 1e-12,
                "{name}, column={column}, tw={web}, tf={flange}: {result:?}"
            );
            assert!(!result.ok());
        }
    }
    for (name, thickness) in [
        ("SN400B", 40.001),
        ("SN400B", 100.0),
        ("SM520B", 40.001),
        ("SM520B", 75.0),
    ] {
        for column in [false, true] {
            let mut ctx = if column {
                ctx_column(LoadTerm::Long)
            } else {
                ctx_beam(LoadTerm::Long)
            };
            ctx.steel_material = Some(make_steel_material(name));
            for (web, flange) in [(thickness, 12.0), (8.0, thickness)] {
                let sec = strength912_section(column, web, flange);
                assert!(
                    matches!(SrcDesign.check(&zero_forces(), &sec, &concrete, &ctx),
                        CheckOutcome::Skipped { reason } if reason.contains("幅厚比の適用条件"))
                );
            }
        }
    }
    for name in ["SN400B", "SM520B"] {
        for column in [false, true] {
            let mut ctx = if column {
                ctx_column(LoadTerm::Long)
            } else {
                ctx_beam(LoadTerm::Long)
            };
            ctx.steel_material = Some(make_steel_material(name));
            for fy in [None, Some(100.0), Some(999.0)] {
                ctx.steel_material.as_mut().unwrap().fy = fy;
                for (web, flange) in [(100.001, 12.0), (8.0, 100.001)] {
                    let sec = strength912_section(column, web, flange);
                    assert!(
                        matches!(SrcDesign.check(&zero_forces(), &sec, &concrete, &ctx),
                        CheckOutcome::Skipped { reason } if reason.contains("適用範囲"))
                    );
                }
            }
        }
    }
}

#[test]
fn src_column_long_term_result_keeps_concrete_shear_allowable() {
    let section = make_section(src_column_rect_shape(
        500.0,
        500.0,
        20.0,
        vec![4],
        vec![4],
        40.0,
        10.0,
        100.0,
        0,
        0,
        300.0,
        200.0,
        40.0,
        14.0,
    ));
    let material = Material {
        young: 205_000.0 / 15.0,
        ..make_material(24.0, "Fc24")
    };
    let forces = MemberForcesAt {
        qy: 537471.99,
        ..zero_forces()
    };
    let result = SrcDesign
        .check(&forces, &section, &material, &ctx_column(LoadTerm::Long))
        .unwrap_checked();
    let shear = result
        .components
        .iter()
        .find(|c| c.kind == crate::CheckKind::Shear)
        .unwrap();
    assert!(
        (shear.ratio - 537471.99 / 467389.8).abs() < 1e-12,
        "{shear:?}"
    );
    assert!(!result.ok());
}
