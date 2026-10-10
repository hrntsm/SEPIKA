//! 非線形解析の入力チェック（[`super::nonlinear_input_issues`]）のテスト。

use super::*;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory, Node, Section,
};
use sepika_core::section_shape::{RcBeamRebar, SectionShape};

fn steel_material() -> Material {
    Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(0),
        name: "SN400".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: None,
        fy: Some(235.0),
    }
}

/// コンクリート区分の材料（RC・SRC 断面の部材に割り当てる）。
fn concrete_material() -> Material {
    Material {
        category: MaterialCategory::Concrete,
        name: "FC24".into(),
        fy: None,
        fc: Some(24.0),
        ..steel_material()
    }
}

/// 主筋・せん断補強筋の材料（`MaterialId(1)`）。RC・SRC 断面へ割り当てる。
fn rebar_material() -> Material {
    Material {
        id: MaterialId(1),
        name: "SD345".into(),
        category: MaterialCategory::Rebar,
        fy: Some(345.0),
        fc: None,
        ..steel_material()
    }
}

/// 内蔵鉄骨の材料（`MaterialId(2)`）。鋼種名から F 値を引くため名前が要る。
fn steel_grade_material(grade: &str) -> Material {
    Material {
        id: MaterialId(2),
        name: grade.into(),
        ..steel_material()
    }
}

fn rc_section() -> Section {
    let mut sec = rc_section_shape();
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(1));
    sec
}

fn rc_section_shape() -> Section {
    use sepika_core::section_shape::BeamStirrup;
    SectionShape::RcBeamRect {
        b: 400.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: vec![6],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 200.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(0), "G1".into())
}

/// `rc_section` の主筋材料を取り除いた断面（未割当の入力不備を模擬）。
fn rc_section_without_rebar_material() -> Section {
    let mut sec = rc_section();
    sec.rebar_material = None;
    sec
}

/// 1 部材（2 節点の梁）だけのモデル。断面形状・材料は引数で差し替える。
///
/// 材料は断面が持つため、`material` は断面の主材料として割り当てる。主筋
/// （`MaterialId(1)`）・内蔵鉄骨（`MaterialId(2)`）はモデルへ常に登録しておき、
/// 断面側の割り当ての有無で不備を作り分ける。
fn beam_model(mut section: Section, material: Material) -> Model {
    section.material = Some(MaterialId(0));
    beam_model_inner(
        section,
        vec![material, rebar_material(), steel_grade_material("SN400B")],
    )
}

/// 材料一覧まで指定する版。
fn beam_model_inner(section: Section, materials: Vec<Material>) -> Model {
    let mk = |id: u32, c: [f64; 3]| Node {
        id: NodeId(id),
        coord: c,
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    };
    Model {
        nodes: vec![mk(0, [0.0, 0.0, 0.0]), mk(1, [6000.0, 0.0, 0.0])],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials,
        ..Default::default()
    }
}

/// 有効な入力（耐力算定に必要な強度が揃っている）は不備なしと判定される。
#[test]
fn test_valid_inputs_produce_no_issues() {
    // 鋼材部材（形状未設定）＋ 正の fy
    let mut sec = rc_section();
    sec.shape = None;
    let model = beam_model(sec, steel_material());
    assert!(nonlinear_input_issues(&model).is_empty());
    assert!(ensure_nonlinear_input(&model).is_ok());

    // RC 断面 ＋ 正の Fc
    let model = beam_model(rc_section(), concrete_material());
    assert!(nonlinear_input_issues(&model).is_empty());

    // 鋼断面 ＋ 正の fy
    let model = beam_model(steel_h_section(), steel_material());
    assert!(nonlinear_input_issues(&model).is_empty());

    // SRC 断面 ＋ 内蔵鉄骨の材料（主材料 fy 未設定でも降伏強度を解決できる）
    let model = beam_model(src_section(), concrete_material());
    assert!(nonlinear_input_issues(&model).is_empty());

    // 鋼断面 ＋ コンクリート区分の材料 ＋ fy。
    // 構造種別は材料の区分で決まる仕様であり、断面形状は力学的な性質ではないため
    // 区分の矛盾とはしない。
    let mut mat = concrete_material();
    mat.fy = Some(235.0);
    let model = beam_model(steel_h_section(), mat);
    assert!(
        nonlinear_input_issues(&model).is_empty(),
        "{:?}",
        nonlinear_input_issues(&model)
    );
}

/// 主筋の材料が未割当、または材料はあっても fy が無い RC 部材はエラーとする
/// （材料名 "SD345" からは推定しない）。既定 345 N/mm² で埋めると SD295 の
/// 部材で曲げ降伏耐力を過大評価する（危険側）。
#[test]
fn test_issue_when_rebar_yield_strength_unresolvable() {
    let model = beam_model(rc_section_without_rebar_material(), concrete_material());
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("主筋の材料"), "{}", issues[0]);

    let mut mats = vec![
        concrete_material(),
        rebar_material(),
        steel_grade_material("SN400B"),
    ];
    mats[1].fy = None;
    let mut sec = rc_section();
    sec.material = Some(MaterialId(0));
    let model = beam_model_inner(sec, mats);
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("主筋の材料"), "{}", issues[0]);
}

/// RC 断面なのに Fc が未設定または 0 以下の部材はエラーとする。
/// Fc=0 相当で解析を通すと Mc=0 となりヒンジが一切検出されない（危険側）。
#[test]
fn test_issue_when_rc_member_fc_missing_or_not_positive() {
    for fc in [None, Some(0.0)] {
        let mut mat = concrete_material();
        mat.fc = fc;
        let model = beam_model(rc_section(), mat);
        let issues = nonlinear_input_issues(&model);
        assert_eq!(issues.len(), 1, "{:?}", issues);
        assert!(issues[0].contains("Fc"), "{}", issues[0]);
        assert!(ensure_nonlinear_input(&model).is_err());
    }
}

/// せん断補強筋に未対応グレード（KH785）を割り当てた部材はエラーとする。
/// 未対応グレードを普通強度式で代替すると耐力を過大評価する（危険側）。
#[test]
fn test_issue_when_shear_rebar_grade_unsupported() {
    let mut mats = vec![
        concrete_material(),
        rebar_material(),
        steel_grade_material("SN400B"),
    ];
    mats[1].name = "KH785".into();
    let mut sec = rc_section();
    sec.material = Some(MaterialId(0));
    let model = beam_model_inner(sec, mats);
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("KH785"), "{}", issues[0]);
    assert!(issues[0].contains("未対応"), "{}", issues[0]);
    assert!(ensure_nonlinear_input(&model).is_err());
}

/// 対応グレード（SR235）でも fy が未設定なら入力不備として停止する。
/// 既定 295 で代替すると σwy が 235→295 に増え、耐力を過大評価する（危険側）。
#[test]
fn test_issue_when_supported_shear_rebar_fy_missing() {
    let make = |fy: Option<f64>| {
        let mut shear = rebar_material();
        shear.id = MaterialId(3);
        shear.name = "SR235".into();
        shear.fy = fy;
        let mut sec = rc_section();
        sec.material = Some(MaterialId(0));
        sec.shear_rebar_material = Some(MaterialId(3));
        beam_model_inner(
            sec,
            vec![
                concrete_material(),
                rebar_material(),
                steel_grade_material("SN400B"),
                shear,
            ],
        )
    };

    let model = make(None);
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("SR235"), "{}", issues[0]);
    assert!(issues[0].contains("fy"), "{}", issues[0]);
    assert!(ensure_nonlinear_input(&model).is_err());

    // fy を設定すれば不備なし。
    let model = make(Some(235.0));
    assert!(
        nonlinear_input_issues(&model).is_empty(),
        "{:?}",
        nonlinear_input_issues(&model)
    );
}

/// 断面形状未設定の部材で正の耐力を算定できない材料はエラーとする。
/// - fy なし: せん断降伏耐力が ∞ となり降伏しない。
/// - fy=0（非正値）: 「設定済み」と素通しすると要素生成（`steel_fiber_material`）が
///   解析スレッド内で panic し、UI には「解析スレッドが異常終了しました」としか
///   表示されず原因が利用者に伝わらない（時刻歴解析スレッドの panic 不具合の回帰）。
/// - Fc=0（非正値）: コンクリートのファイバが剛性 0 となり剛性行列が特異化する。
#[test]
fn test_issue_when_shapeless_member_lacks_positive_strength() {
    let shapeless = || {
        let mut sec = rc_section();
        sec.shape = None;
        sec
    };

    let mut mat = steel_material();
    mat.fy = None;
    let model = beam_model(shapeless(), mat);
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("fy"), "{}", issues[0]);

    let mut mat = steel_material();
    mat.fy = Some(0.0);
    let model = beam_model(shapeless(), mat);
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("fy"), "{}", issues[0]);
    assert!(ensure_nonlinear_input(&model).is_err());

    let mut mat = steel_material();
    mat.fy = None;
    mat.fc = Some(0.0);
    let model = beam_model(shapeless(), mat);
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("Fc"), "{}", issues[0]);
}

/// H 形鋼断面を持つ部材の断面（鋼材ファイバ領域あり）。
fn steel_h_section() -> Section {
    SectionShape::SteelH {
        root_r: Some(0.0),
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
    }
    .to_section(SectionId(0), "H400".into())
}

/// 鋼材断面形状なのに fy 未設定の部材はエラーとする。
/// ファイバー断面は降伏進展を追うことが目的のため、弾性で代替すると
/// 鋼材がいくら応力が上がっても降伏せず耐力を過大評価する（危険側）。
#[test]
fn test_issue_when_steel_shape_has_no_fy() {
    let mut mat = steel_material();
    mat.fy = None;
    mat.fc = Some(24.0);
    let model = beam_model(steel_h_section(), mat);
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("fy"), "{}", issues[0]);
    assert!(ensure_nonlinear_input(&model).is_err());
}

/// SRC 断面（内蔵鉄骨あり）。内蔵鉄骨の材料は `MaterialId(2)` を割り当てる。
fn src_section() -> Section {
    let mut sec = src_section_bare();
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(1));
    sec.steel_material = Some(MaterialId(2));
    sec
}

/// 内蔵鉄骨・鉄筋の材料を割り当てていない SRC 断面。
fn src_section_bare() -> Section {
    let rebar = match rc_section().shape {
        Some(SectionShape::RcBeamRect { rebar, .. }) => rebar,
        _ => unreachable!(),
    };
    SectionShape::SrcBeamRect {
        b: 500.0,
        d: 700.0,
        rebar,
        steel_height: 300.0,
        steel_width: 150.0,
        steel_web_thick: 6.5,
        steel_flange_thick: 9.0,
    }
    .to_section(SectionId(0), "SRC".into())
}

/// SRC 断面で内蔵鉄骨の材料も主材料 fy も解決できない部材はエラーとする。
/// Fc・主筋の材料が揃っていても、内蔵鉄骨のファイバに降伏強度が要る。
#[test]
fn test_issue_when_src_section_has_no_steel_yield() {
    let mut sec = src_section();
    sec.steel_material = None;
    let model = beam_model(sec, concrete_material());
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("降伏強度"), "{}", issues[0]);
}

/// 断面に材料が割り当てられていない部材はエラーとする。
#[test]
fn test_issue_when_member_has_no_material() {
    let mut sec = rc_section();
    sec.shape = None;
    let mut model = beam_model(sec, steel_material());
    model.sections[0].material = None;
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(
        issues[0].contains("材料が設定されていません"),
        "{}",
        issues[0]
    );
}

/// 弾性でモデル化することが仕様の要素（節点バネ）は検査対象外。
#[test]
fn test_elastic_only_element_kinds_are_not_checked() {
    let mut sec = rc_section();
    sec.shape = None;
    let mut mat = steel_material();
    mat.fy = None;
    let mut model = beam_model(sec, mat);
    model.elements[0].kind = ElementKind::NodalSpring;
    assert!(nonlinear_input_issues(&model).is_empty());
}

/// 配筋を持つ RC 断面に鋼材区分の材料が付いた部材はエラーとする。
/// 鋼材として検定・ヒンジ算定すると耐力を大きく過大評価する（危険側）。
#[test]
fn test_issue_when_rc_section_has_steel_material() {
    let model = beam_model(rc_section(), steel_material());
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("区分が鋼材"), "{}", issues[0]);
}

/// 線材の主材料に鉄筋を割り当てるのは入力の誤りとする。
/// RC 断面の主筋・せん断補強筋は断面の専用の欄で持つ。
#[test]
fn test_issue_when_member_material_is_rebar() {
    let mut mat = concrete_material();
    mat.category = MaterialCategory::Rebar;
    mat.name = "SD345".into();
    let model = beam_model(rc_section(), mat);
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("区分が鉄筋"), "{}", issues[0]);
}

/// 実配筋の幾何が不整合な部材は非線形解析の冒頭で止める。
/// 不整合を鋼材相当の My・せん断耐力へフォールバックさせない。
#[test]
fn test_issue_when_rebar_geometry_is_invalid() {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};

    let mut sec = SectionShape::RcColumnRect {
        b: 400.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![4, 2],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    }
    .to_section(SectionId(0), "C1".into());
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(1));
    assert!(sec
        .shape
        .as_ref()
        .is_some_and(|s| s.validate_rebar().is_err()));
    let model = beam_model(sec, concrete_material());
    let issues = nonlinear_input_issues(&model);
    assert_eq!(issues.len(), 1, "{:?}", issues);
    assert!(issues[0].contains("幾何が不整合"), "{}", issues[0]);
    assert!(ensure_nonlinear_input(&model).is_err());
}

/// 複数件の不備はメッセージへ 5 件まで列挙し、残りは件数で示す。
#[test]
fn test_error_message_lists_head_and_remaining_count() {
    let mut mat = concrete_material();
    mat.fc = None;
    let mut model = beam_model(rc_section(), mat);
    let base = model.elements[0].clone();
    for i in 1..8u32 {
        let mut e = base.clone();
        e.id = ElemId(i);
        model.elements.push(e);
    }
    assert_eq!(nonlinear_input_issues(&model).len(), 8);
    let msg = ensure_nonlinear_input(&model).expect_err("不備があればエラー");
    assert_eq!(msg.lines().count(), MAX_LISTED + 1);
    assert!(msg.contains("他 3 件"), "{}", msg);
}

#[test]
fn rc_ratio_factory_fixture_and_invalid_geometry_are_diagnosed() {
    let mut model = beam_model(rc_section(), concrete_material());
    model.elements[0].force_regime = ForceRegime::UniaxialBendingShear;
    model.nodes[1].coord[0] = 3600.0;
    model.materials[0].young = 20_000.0;
    model.materials[1].young = 200_000.0;
    let bar_dia = (2400.0 / std::f64::consts::PI).sqrt();
    model.sections[0].shape = Some(SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: bar_dia,
            top: vec![3],
            bottom: vec![3],
            cover: 60.0 - 10.0 - bar_dia / 2.0,
            stirrup: sepika_core::section_shape::BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    });
    model.set_member_rc_beam_reference(
        ElemId(0),
        Some(sepika_core::model::RcBeamReference::AntisymmetricHalfMember),
    );
    model.sections[0].iy = 5_400_000_000.0;
    model.sections[0].iz = 1_350_000_000.0;
    model.sections[0].width = 300.0;
    assert!(ensure_nonlinear_input(&model).is_ok());
    assert!(
        (super::super::springs::flexural_alpha_y(&model.elements[0], &model) - 0.265_720_5).abs()
            < 1e-12
    );
    let (_, _, backbone) = super::super::springs::build_flexural_springs(
        &model.elements[0],
        &model,
        sepika_core::model::HysteresisModel::Takeda,
        crate::factory::StrengthBasis::Nominal,
    );
    let [theta_y, my] = backbone.points[2];
    assert!((my / (backbone.k_rot * theta_y) - 0.265_720_5).abs() < 1e-12);
    for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        let mut invalid = model.clone();
        if let Some(SectionShape::RcBeamRect { b, .. }) = invalid.sections[0].shape.as_mut() {
            *b = bad;
        }
        assert!(ensure_nonlinear_input(&invalid)
            .unwrap_err()
            .contains("梁幅 b・全せい D"));
    }
    for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        for material in [0, 1] {
            let mut invalid = model.clone();
            invalid.materials[material].young = bad;
            assert!(ensure_nonlinear_input(&invalid)
                .unwrap_err()
                .contains("正の有限値"));
        }
    }
    let mut invalid = model.clone();
    if let Some(SectionShape::RcBeamRect { rebar, .. }) = invalid.sections[0].shape.as_mut() {
        rebar.cover = -100.0;
    }
    assert!(ensure_nonlinear_input(&invalid).is_err());
}

fn public_generation_diagnostic(model: &Model) -> String {
    public_generation_diagnostic_for_kind(model, sepika_core::model::AnalysisKind::Incremental)
}

fn public_generation_diagnostic_for_kind(
    model: &Model,
    kind: sepika_core::model::AnalysisKind,
) -> String {
    let result = std::panic::catch_unwind(|| {
        crate::factory::build_nonlinear_behavior(
            &model.elements[0],
            model,
            crate::factory::StrengthBasis::Nominal,
            kind,
        )
    });
    *result
        .err()
        .expect("不正入力で公開生成を停止すること")
        .downcast::<String>()
        .expect("入力診断をpanicへ引き継ぐこと")
}

#[test]
fn slab_cooperating_rc_concentrated_spring_stops_without_tension_slab_rebar() {
    use sepika_core::ids::{FloorRegionId, SlabId};
    use sepika_core::model::{FloorRegion, Slab, SlabPlate, SlabShape};
    let mut model = beam_model(rc_section(), concrete_material());
    model.elements[0].force_regime = ForceRegime::UniaxialBendingShear;
    model
        .sections
        .push(SectionShape::RcSlab { thickness: 150.0 }.to_section(SectionId(1), "S15".into()));
    for (id, coord) in [(2, [6000.0, 2500.0, 0.0]), (3, [0.0, 2500.0, 0.0])] {
        model.nodes.push(Node {
            id: NodeId(id),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    model.floor_regions.push(FloorRegion {
        slab_ids: vec![SlabId(0)],
        ..FloorRegion::new(
            FloorRegionId(0),
            vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        )
    });
    model.slabs.push(Slab {
        id: SlabId(0),
        shape: SlabShape::Enclosed,
        plate: SlabPlate {
            section: Some(SectionId(1)),
            ..Default::default()
        },
        tip_loads: vec![],
    });
    assert!(
        crate::frame::beam::stiffness_breakdown(&model, &model.elements[0])
            .unwrap()
            .slab
            > 1.0
    );
    let error = ensure_nonlinear_input(&model).unwrap_err();
    assert!(error.contains("スラブ引張筋面積と正負別骨格"), "{error}");
    assert_eq!(public_generation_diagnostic(&model), error);
    model.set_member_rc_beam_reference(
        ElemId(0),
        Some(sepika_core::model::RcBeamReference::AntisymmetricHalfMember),
    );
    if let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() {
        rebar.top = rebar.bottom.clone();
    }
    model.slabs.clear();
    model.floor_regions.clear();
    assert!(ensure_nonlinear_input(&model).is_ok());
    let _behavior = crate::factory::build_nonlinear_behavior(
        &model.elements[0],
        &model,
        crate::factory::StrengthBasis::Nominal,
        sepika_core::model::AnalysisKind::Incremental,
    );
}

#[test]
fn missing_rc_beam_tension_rebar_stops_analysis_and_has_no_public_backbone() {
    for (top, bottom, missing_side) in [
        (vec![], vec![], "下端"),
        (vec![], vec![4], "上端"),
        (vec![6], vec![], "下端"),
    ] {
        let mut model = beam_model(rc_section(), concrete_material());
        model.elements[0].force_regime = ForceRegime::UniaxialBendingShear;
        if let Some(SectionShape::RcBeamRect { rebar, .. }) = model.sections[0].shape.as_mut() {
            rebar.top = top;
            rebar.bottom = bottom;
        }
        let error = ensure_nonlinear_input(&model).unwrap_err();
        assert!(error.contains("部材 ID 0"), "{error}");
        assert!(error.contains(missing_side), "{error}");
        assert_eq!(public_generation_diagnostic(&model), error);
        let view = crate::factory::build_hinge_view(
            &model.elements[0],
            &model,
            crate::factory::StrengthBasis::Nominal,
            sepika_core::model::AnalysisKind::Incremental,
            0.0,
            8,
            8,
        )
        .unwrap();
        assert!(view.backbone.is_none());
    }
}

#[test]
fn src_stiffness_errors_reach_nonlinear_input_diagnostics_and_public_factories() {
    let mut model = beam_model(src_section(), concrete_material());
    model.materials[0].young = 20500.0;
    model.materials[0].poisson = 0.2;
    for (kind, regime) in [
        (ElementKind::Fiber, ForceRegime::Auto),
        (ElementKind::MultiSpring, ForceRegime::Auto),
        (ElementKind::Beam, ForceRegime::UniaxialBendingShear),
    ] {
        model.elements[0].kind = kind;
        model.elements[0].force_regime = regime;
        assert!(ensure_nonlinear_input(&model).is_ok());
        let _behavior = crate::factory::build_nonlinear_behavior(
            &model.elements[0],
            &model,
            crate::factory::StrengthBasis::Nominal,
            sepika_core::model::AnalysisKind::Incremental,
        );
        for field in ["Ec", "Fc", "nu"] {
            let bad_values = if field == "nu" {
                vec![-1.0, 0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY]
            } else {
                vec![0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY]
            };
            for bad in bad_values {
                let mut invalid = model.clone();
                match field {
                    "Ec" => invalid.materials[0].young = bad,
                    "Fc" => invalid.materials[0].fc = Some(bad),
                    _ => invalid.materials[0].poisson = bad,
                }
                let reason =
                    crate::frame::beam::stiffness_breakdown(&invalid, &invalid.elements[0])
                        .unwrap_err();
                let error = ensure_nonlinear_input(&invalid).unwrap_err();
                assert!(error.contains("部材 ID 0"), "{error}");
                assert!(error.contains(&reason), "{error}");
                let factory_error = public_generation_diagnostic(&invalid);
                // Fiber の既存 Fc 検査は等価性能より先に、同じ不正を理由付きで止める。
                let expected_reason = if field == "Fc" { "Fc" } else { &reason };
                assert!(factory_error.contains(expected_reason), "{factory_error}");
            }
        }
        let mut missing_fc = model.clone();
        missing_fc.materials[0].fc = None;
        assert!(ensure_nonlinear_input(&missing_fc)
            .unwrap_err()
            .contains("Fc"));
        assert!(public_generation_diagnostic(&missing_fc).contains("Fc"));
        let mut missing_material = model.clone();
        missing_material.sections[0].material = None;
        assert!(ensure_nonlinear_input(&missing_material)
            .unwrap_err()
            .contains("材料"));
        let factory_error = public_generation_diagnostic(&missing_material);
        // Fiber の入口では未割当主材料から Fc を解決できない理由が先に示される。
        assert!(
            factory_error.contains("材料") || factory_error.contains("Fc"),
            "{factory_error}"
        );
    }
}

fn explicit_rc_reference_fixture() -> Model {
    let mut model = beam_model(rc_section(), concrete_material());
    let bar_dia = (2400.0 / std::f64::consts::PI).sqrt();
    let mut section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: bar_dia,
            top: vec![3],
            bottom: vec![3],
            cover: 60.0 - 10.0 - bar_dia / 2.0,
            stirrup: sepika_core::section_shape::BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    }
    .to_section(SectionId(0), "RC基準".into());
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    model.sections[0] = section;
    model.materials[0].young = 20000.0;
    model.materials[0].density = 2.4e-9;
    model.materials[1].young = 200000.0;
    model.elements[0].force_regime = ForceRegime::UniaxialBendingShear;
    model.set_member_hysteresis(ElemId(0), sepika_core::model::HysteresisModel::Retrograde);
    model.set_member_rc_beam_reference(
        ElemId(0),
        Some(sepika_core::model::RcBeamReference::AntisymmetricHalfMember),
    );
    model
}

#[test]
fn actual_rc_factory_connects_additional_angle_and_publishes_both_backbones() {
    use crate::behavior::{Ctx, ElementBehavior, LocalVec, MassOption};
    use sepika_core::model::AnalysisKind;
    let model = explicit_rc_reference_fixture();
    ensure_nonlinear_input(&model).unwrap();
    let data = &model.elements[0];
    let mut behavior = crate::factory::build_nonlinear_behavior(
        data,
        &model,
        crate::factory::StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    );
    let reference = crate::factory::build_hinge_view(
        data,
        &model,
        crate::factory::StrengthBasis::Nominal,
        AnalysisKind::Incremental,
        0.0,
        8,
        24,
    )
    .unwrap();
    let total = reference.total_backbone.unwrap();
    let additional = reference.backbone.unwrap();
    assert!((total[2][0] - 0.005927325831114165).abs() < 1e-14);
    assert!((additional[2][0] - 0.003132825831114165).abs() < 1e-14);
    assert!(additional[1][0].abs() < 1e-15);
    let elastic = crate::frame::beam::BeamElement::new(data, &model);
    let ctx = Ctx { model: &model };
    assert!(elastic
        .mass_matrix(MassOption::Consistent)
        .data
        .iter()
        .any(|value| value.abs() > 0.0));
    let reference_mass = behavior.mass_matrix(MassOption::Consistent);
    assert!(reference_mass.data.iter().all(|value| value.is_finite()));
    assert!(reference_mass.data.iter().any(|value| value.abs() > 0.0));
    // Euler-Bernoulliの非零回転質量係数を、局所強軸の単位回転で直接照合する。
    let mut unit = [0.0; 12];
    unit[5] = 1.0;
    let unit_global = elastic.axis.rotate_to_global(&unit);
    let quadratic = |matrix: &crate::behavior::LocalMat, left: &[f64; 12], right: &[f64; 12]| {
        (0..12)
            .map(|i| {
                (0..12)
                    .map(|j| left[i] * matrix.get(i, j) * right[j])
                    .sum::<f64>()
            })
            .sum::<f64>()
    };
    let properties = elastic.mass_properties;
    let expected = properties.mass_per_length * 6000.0_f64.powi(3) / 105.0
        + properties.rotary_inertia_z_per_length * 6000.0 * 2.0 / 15.0;
    assert!(
        (quadratic(&reference_mass, &unit_global, &unit_global) / expected - 1.0).abs() < 1e-12
    );
    unit[11] = 1.0;
    let equal_rotations = elastic.axis.rotate_to_global(&unit);
    let initial = behavior.tangent_stiffness(&ctx);
    assert!((quadratic(&initial, &unit_global, &equal_rotations) / 108e9 - 1.0).abs() < 1e-12);
    assert_eq!(
        behavior.mass_matrix(MassOption::Lumped).data,
        elastic.mass_matrix(MassOption::Lumped).data
    );
    let mut local = [0.0; 12];
    local[5] = 0.005927325831114165;
    local[11] = local[5];
    let global = elastic.axis.rotate_to_global(&local);
    behavior.update_state(
        &LocalVec {
            data: global.into_iter().collect(),
        },
        true,
        &ctx,
    );
    let force_global: [f64; 12] = std::array::from_fn(|i| behavior.internal_force(&ctx).data[i]);
    let force_local = elastic.axis.rotate_to_local(&force_global);
    assert!((force_local[5] - 301806000.0).abs() < 0.1);
    assert!((force_local[11] - 301806000.0).abs() < 0.1);
    for extra in behavior.end_spring_rotations().unwrap() {
        assert!((extra - 0.003132825831114165).abs() < 1e-12);
    }
}

#[test]
fn rc_reference_kind_and_active_load_diagnostics_do_not_use_inactive_settings() {
    use sepika_core::ids::LoadCaseId;
    use sepika_core::model::{AnalysisKind, HysteresisModel, LoadCase, LoadCaseKind, MemberLoad};
    let mut model = explicit_rc_reference_fixture();
    model.set_member_hysteresis_th(ElemId(0), Some(HysteresisModel::OriginOriented));
    assert!(ensure_nonlinear_input_for_kind(&model, AnalysisKind::Incremental).is_ok());
    assert!(
        ensure_nonlinear_input_for_kind(&model, AnalysisKind::TimeHistory)
            .unwrap_err()
            .contains("原点指向型")
    );
    model.set_member_hysteresis_th(ElemId(0), Some(HysteresisModel::Retrograde));
    model.set_member_hysteresis(ElemId(0), HysteresisModel::OriginOriented);
    assert!(ensure_nonlinear_input_for_kind(&model, AnalysisKind::TimeHistory).is_ok());
    assert!(ensure_nonlinear_input_for_kind(&model, AnalysisKind::Incremental).is_err());
    model.set_member_hysteresis(ElemId(0), HysteresisModel::Retrograde);
    model.load_cases.push(LoadCase {
        id: LoadCaseId(0),
        name: "長期".into(),
        kind: LoadCaseKind::Dead,
        nodal: vec![],
        member: vec![MemberLoad::full_length_uniform(
            ElemId(0),
            [0.0, 0.0, -1.0],
            6000.0,
            10.0,
        )],
    });
    assert!(ensure_rc_beam_reference_loads(&model, &[]).is_ok());
    assert!(ensure_rc_beam_reference_loads(&model, &[LoadCaseId(0)])
        .unwrap_err()
        .contains("一定せん断・三角形"));
    model.set_member_rc_beam_reference(ElemId(0), None);
    let reason = ensure_nonlinear_input(&model).unwrap_err();
    assert!(reason.contains("未指定"));
    assert!(public_generation_diagnostic(&model).contains("未指定"));
    let view = crate::factory::build_hinge_view(
        &model.elements[0],
        &model,
        crate::factory::StrengthBasis::Nominal,
        AnalysisKind::Incremental,
        0.0,
        8,
        24,
    )
    .unwrap();
    assert!(view.backbone.is_none());
    assert!(view.unavailability_reason.unwrap().contains("未指定"));
}

#[test]
fn rc_alpha_materials_are_required_and_explicit_basis_never_falls_back_to_other_laws() {
    use sepika_core::model::{AnalysisKind, HysteresisModel};
    let model = explicit_rc_reference_fixture();
    for role in ["Ec", "Es"] {
        let mut missing = model.clone();
        if role == "Ec" {
            missing.sections[0].material = None;
        } else {
            missing.sections[0].rebar_material = None;
        }
        let error =
            crate::factory::springs::flexural_alpha_y_checked(&missing.elements[0], &missing)
                .unwrap_err();
        assert!(error.to_string().contains(role), "{error}");
    }
    for rule in [
        HysteresisModel::Standard,
        HysteresisModel::TsujiYamada,
        HysteresisModel::SteelBuckling,
    ] {
        let mut other = model.clone();
        other.set_member_hysteresis(ElemId(0), rule);
        assert!(ensure_nonlinear_input_for_kind(&other, AnalysisKind::Incremental).is_err());
        assert!(public_generation_diagnostic(&other).contains("対象外"));
    }
}

#[test]
fn rc_reference_cracking_uses_the_same_rectangular_geometry_as_alpha_and_yield() {
    use sepika_core::model::AnalysisKind;
    let model = explicit_rc_reference_fixture();
    let view = |model: &Model| {
        crate::factory::build_hinge_view(
            &model.elements[0],
            model,
            crate::factory::StrengthBasis::Nominal,
            AnalysisKind::Incremental,
            0.0,
            8,
            24,
        )
        .unwrap()
    };
    let before = view(&model);
    let mut supplied = model.clone();
    supplied.sections[0].depth = 1200.0;
    supplied.sections[0].property_basis.depth = sepika_core::model::PropertyBasis::Supplied;
    assert!(ensure_nonlinear_input(&supplied).is_ok());
    let after = view(&supplied);
    assert_eq!(before.total_backbone, after.total_backbone);
    assert_eq!(before.backbone, after.backbone);
    let mc = after.total_backbone.unwrap()[1][1];
    assert!((mc - 0.56 * 24.0_f64.sqrt() * 18_000_000.0).abs() < 1e-6);
}

#[test]
fn rc_alpha_checks_each_material_before_their_ratio() {
    let model = explicit_rc_reference_fixture();
    assert!(crate::factory::springs::flexural_alpha_y_checked(&model.elements[0], &model).is_ok());
    for (index, role) in [(0, "Ec"), (1, "Es")] {
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut invalid = model.clone();
            invalid.materials[index].young = value;
            let error =
                crate::factory::springs::flexural_alpha_y_checked(&invalid.elements[0], &invalid)
                    .unwrap_err();
            assert!(error.to_string().contains(role), "{error}");
        }
    }
    let mut both_negative = model.clone();
    both_negative.materials[0].young = -20000.0;
    both_negative.materials[1].young = -200000.0;
    assert!(crate::factory::springs::flexural_alpha_y_checked(
        &both_negative.elements[0],
        &both_negative
    )
    .is_err());
}

#[test]
fn unsupported_concentrated_rc_law_never_falls_through_to_takeda_without_basis() {
    use sepika_core::model::{AnalysisKind, HysteresisModel};
    let mut model = explicit_rc_reference_fixture();
    model.set_member_rc_beam_reference(ElemId(0), None);
    model.set_member_hysteresis(ElemId(0), HysteresisModel::KarsanJirsa);
    for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
        let reason = ensure_nonlinear_input_for_kind(&model, kind).unwrap_err();
        assert!(reason.contains("Karsan"), "{reason}");
        assert!(reason.contains("未対応"), "{reason}");
        assert_eq!(public_generation_diagnostic_for_kind(&model, kind), reason);
        let view = crate::factory::build_hinge_view(
            &model.elements[0],
            &model,
            crate::factory::StrengthBasis::Nominal,
            kind,
            0.0,
            8,
            24,
        )
        .unwrap();
        assert!(view.backbone.is_none());
        assert!(view.total_backbone.is_none());
        assert!(view.unavailability_reason.unwrap().contains("Karsan"));
    }
    // 未実行側の指定が対応済みの実行側を拒否する根拠にはならない。
    model.set_member_rc_beam_reference(
        ElemId(0),
        Some(sepika_core::model::RcBeamReference::AntisymmetricHalfMember),
    );
    model.set_member_hysteresis_th(ElemId(0), Some(HysteresisModel::Takeda));
    assert!(ensure_nonlinear_input_for_kind(&model, AnalysisKind::TimeHistory).is_ok());
    assert!(ensure_nonlinear_input_for_kind(&model, AnalysisKind::Incremental).is_err());
}

#[test]
fn explicit_rc_basis_rejects_every_other_resolved_element_path() {
    use sepika_core::model::AnalysisKind;
    let base = explicit_rc_reference_fixture();
    let mut variants = Vec::new();
    for regime in [ForceRegime::AxialBendingInteract, ForceRegime::Auto] {
        let mut model = base.clone();
        model.elements[0].force_regime = regime;
        variants.push(model);
    }
    for element_kind in [
        ElementKind::Fiber,
        ElementKind::MultiSpring,
        ElementKind::Shell,
        ElementKind::Wall,
        ElementKind::PanelZone,
        ElementKind::Brace {
            tension_only: false,
        },
        ElementKind::NodalSpring,
        ElementKind::Isolator,
        ElementKind::Damper,
    ] {
        let mut model = base.clone();
        model.elements[0].kind = element_kind;
        variants.push(model);
    }
    let mut column = base.clone();
    column.sections[0].frame_use = Some(sepika_core::model::FrameSectionUse::Column);
    variants.push(column);
    for model in variants {
        for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
            let reason = ensure_nonlinear_input_for_kind(&model, kind).unwrap_err();
            assert!(reason.contains("材端集中ばね専用"), "{reason}");
            assert_eq!(public_generation_diagnostic_for_kind(&model, kind), reason);
            let view = crate::factory::build_hinge_view(
                &model.elements[0],
                &model,
                crate::factory::StrengthBasis::Nominal,
                kind,
                0.0,
                8,
                24,
            )
            .unwrap();
            assert!(view.backbone.is_none());
            assert!(view.total_backbone.is_none());
            assert!(view.mn_surface.is_none());
            assert!(view
                .unavailability_reason
                .unwrap()
                .contains("材端集中ばね専用"));
        }
    }
}

#[test]
fn unspecified_rc_basis_preserves_the_existing_fiber_path() {
    use sepika_core::model::AnalysisKind;
    let mut model = explicit_rc_reference_fixture();
    model.set_member_rc_beam_reference(ElemId(0), None);
    for element_kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        model.elements[0].kind = element_kind;
        model.elements[0].force_regime = ForceRegime::AxialBendingInteract;
        for kind in [AnalysisKind::Incremental, AnalysisKind::TimeHistory] {
            assert!(ensure_nonlinear_input_for_kind(&model, kind).is_ok());
            let view = crate::factory::build_hinge_view(
                &model.elements[0],
                &model,
                crate::factory::StrengthBasis::Nominal,
                kind,
                0.0,
                8,
                24,
            )
            .unwrap();
            assert!(view.unavailability_reason.is_none());
            assert!(view.mn_surface.is_some());
            let _element = crate::factory::build_nonlinear_behavior(
                &model.elements[0],
                &model,
                crate::factory::StrengthBasis::Nominal,
                kind,
            );
        }
    }
}

#[test]
fn rc_reference_backbone_diagnostics_follow_the_actual_strength_basis() {
    use crate::factory::StrengthBasis;
    use sepika_core::model::AnalysisKind;
    let base = explicit_rc_reference_fixture();
    let view = |model: &Model, basis| {
        crate::factory::build_hinge_view(
            &model.elements[0],
            model,
            basis,
            AnalysisKind::Incremental,
            0.0,
            8,
            24,
        )
        .unwrap()
    };
    let nominal = view(&base, StrengthBasis::Nominal).total_backbone.unwrap();
    for factor in [0.1, 0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut model = base.clone();
        model.materials[1].strength_factor = Some(factor);
        assert!(ensure_nonlinear_input_with_basis(
            &model,
            AnalysisKind::Incremental,
            StrengthBasis::Nominal
        )
        .is_ok());
        assert_eq!(
            view(&model, StrengthBasis::Nominal).total_backbone.as_ref(),
            Some(&nominal)
        );
        let _nominal = crate::factory::build_nonlinear_behavior(
            &model.elements[0],
            &model,
            StrengthBasis::Nominal,
            AnalysisKind::TimeHistory,
        );
        let reason = ensure_nonlinear_input_with_basis(
            &model,
            AnalysisKind::Incremental,
            StrengthBasis::MaterialStrength,
        )
        .unwrap_err();
        let unavailable = view(&model, StrengthBasis::MaterialStrength);
        assert!(unavailable.backbone.is_none());
        assert!(unavailable.total_backbone.is_none());
        assert!(reason.contains(&unavailable.unavailability_reason.unwrap()));
        let result = std::panic::catch_unwind(|| {
            crate::factory::build_nonlinear_behavior(
                &model.elements[0],
                &model,
                StrengthBasis::MaterialStrength,
                AnalysisKind::Incremental,
            )
        });
        let factory_reason = *result.err().unwrap().downcast::<String>().unwrap();
        assert_eq!(factory_reason, reason);
        if factor == 0.1 {
            assert!(reason.contains("Mc<My"), "{reason}");
        } else {
            assert!(reason.contains("My"), "{reason}");
        }
    }
    let mut reduced = base.clone();
    reduced.materials[1].strength_factor = Some(0.8);
    assert!(ensure_nonlinear_input_with_basis(
        &reduced,
        AnalysisKind::Incremental,
        StrengthBasis::MaterialStrength
    )
    .is_ok());
    let backbone = view(&reduced, StrengthBasis::MaterialStrength)
        .total_backbone
        .unwrap();
    assert!((backbone[2][1] - nominal[2][1] * 0.8).abs() < 1e-6);
}
