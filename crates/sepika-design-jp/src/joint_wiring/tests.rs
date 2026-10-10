use super::*;
use crate::CheckResult;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
    MultiOpeningMode, Node, RigidZone, Section, WallAttr, WallOpening,
};
use sepika_core::section_shape::SectionShape;
use smallvec::SmallVec;

/// 矩形壁（4000×3000, t=180）1 枚のモデル。
/// `wall_attr` を指定すると `model.wall_attrs` に登録する。
fn wall_model(wall_attr: Option<WallAttr>) -> Model {
    wall_model_sized(4000.0, 3000.0, 180.0, wall_attr)
}

/// 矩形壁（`l`×`h`, 厚さ `thickness`）1 枚のモデル。
/// `wall_model` の寸法可変版（近接開口・包絡開口のテストで、開口周比 r0
/// を任意の壁面積に対して調整するために用いる）。
///
/// 耐震壁は四周を柱・梁に囲まれた壁を対象とするため、四周に線材を配置する
/// （`wall_is_seismic` の四周条件）。検定側が集計する側柱の断面諸元は
/// `MemberInfo` として別途与えるため、ここでは断面を割り当てない。
fn wall_model_sized(l: f64, h: f64, thickness: f64, wall_attr: Option<WallAttr>) -> Model {
    let mut nodes: Vec<Node> = Vec::new();
    let coords = [[0.0, 0.0, 0.0], [l, 0.0, 0.0], [l, 0.0, h], [0.0, 0.0, h]];
    for (i, c) in coords.iter().enumerate() {
        nodes.push(Node {
            id: NodeId(i as u32),
            coord: *c,
            restraint: if i < 2 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    let sections = vec![Section {
        frame_use: None,
        id: SectionId(0),
        name: "wall".to_string(),
        area: 0.0,
        iy: 1.0,
        iz: 1.0,
        j: 1.0,
        depth: 0.0,
        width: 0.0,
        as_y: 0.0,
        as_z: 0.0,
        floor: None,
        panel_thickness: None,
        thickness: Some(thickness),
        shape: Some(SectionShape::RcWall {
            thickness,
            pwh_ratio: Some(0.006),
            ps: 0.006,
        }),
        // 材料は断面が持つ。壁筋（縦筋・横筋）は SD345。
        material: Some(MaterialId(0)),
        rebar_material: Some(MaterialId(1)),
        shear_rebar_material: Some(MaterialId(1)),
        steel_material: None,
        property_basis: Default::default(),
    }];
    let materials = vec![
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "Fc24".to_string(),
            category: MaterialCategory::Concrete,
            young: 23000.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
        },
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(1),
            name: "SD345".to_string(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(345.0),
        },
    ];
    let frame_member = |id: u32, n0: u32, n1: u32| ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: {
            let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
            v.push(NodeId(n0));
            v.push(NodeId(n1));
            v
        },
        section: None,
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone::default(),
        plastic_zone: None,
        spring: None,
    };
    let elements = vec![
        ElementData {
            id: ElemId(0),
            kind: ElementKind::Wall,
            nodes: {
                let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
                v.push(NodeId(0));
                v.push(NodeId(1));
                v.push(NodeId(2));
                v.push(NodeId(3));
                v
            },
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            plastic_zone: None,
            spring: None,
        },
        frame_member(1, 0, 1), // 下梁
        frame_member(2, 3, 2), // 上梁
        frame_member(3, 0, 3), // 左側柱
        frame_member(4, 1, 2), // 右側柱
    ];
    Model {
        nodes,
        elements,
        sections,
        materials,
        wall_attrs: wall_attr.into_iter().collect(),
        ..Default::default()
    }
}

/// 壁要素 ElemId(0) の耐震壁(RC)検定結果（なければ None）。
fn wall_check_result(model: &Model, forces: ForcesAt<'_>) -> Option<CheckResult> {
    wall_check_outcome(model, forces).and_then(|outcome| match outcome {
        CheckOutcome::Checked(cr) => Some(cr),
        CheckOutcome::Skipped { .. } => None,
    })
}

/// 壁要素 ElemId(0) の耐震壁(RC)検定アウトカム（検定不能なら `Skipped`）。
fn wall_check_outcome(model: &Model, forces: ForcesAt<'_>) -> Option<CheckOutcome> {
    let member_forces = vec![(ElemId(0), forces)];
    collect_joint_checks(model, &member_forces, LoadTerm::Short)
        .into_iter()
        .find(|(_, label, _)| label == "耐震壁(RC)")
        .map(|(_, _, outcome)| outcome)
}

/// 開口あり（`wall_attrs` に `opening_area>0` を登録）の壁は、無開口より
/// 検定比が大きくなる（開口低減係数 r<1 で Qa が下がるため）。
#[test]
fn wall_with_opening_has_larger_ratio_than_without() {
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];

    let model_no_attr = wall_model(None);
    let res_no_opening =
        wall_check_result(&model_no_attr, &forces).expect("無開口の壁は検定されるはず");

    // opening_area = 0.1・l・h → r0 ≈ 0.316（<0.4 で耐震壁として扱われる）。
    let model_with_opening = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 0.1 * 4000.0 * 3000.0,
        opening_weight: 0.0,
        slit: Default::default(),
        openings: vec![],
        finish_intensity: 0.0,
    }));
    let res_opening =
        wall_check_result(&model_with_opening, &forces).expect("小開口は耐震壁のまま検定される");

    assert!(
        res_opening.ratio() > res_no_opening.ratio(),
        "開口あり ratio={} <= 開口なし ratio={}",
        res_opening.ratio(),
        res_no_opening.ratio()
    );
}

/// 対応グレード（SR235）でも壁横筋の fy が未設定なら検定不能（Skipped）とし、
/// fy を設定すれば検定する。
#[test]
fn wall_shear_rebar_fy_missing_is_skipped() {
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];

    let mut model = wall_model(None);
    model.materials[1].name = "SR235".into();
    model.materials[1].fy = None;
    match wall_check_outcome(&model, &forces).expect("耐震壁(RC)の結果") {
        CheckOutcome::Skipped { reason } => {
            assert!(
                reason.contains("SR235") && reason.contains("fy"),
                "{reason}"
            );
        }
        CheckOutcome::Checked(_) => panic!("fy 未設定は検定不能(Skipped)のはず"),
    }

    model.materials[1].fy = Some(235.0);
    match wall_check_outcome(&model, &forces).expect("耐震壁(RC)の結果") {
        CheckOutcome::Checked(_) => {}
        CheckOutcome::Skipped { reason } => panic!("fy 設定時は検定するはず: {reason}"),
    }
}

/// 柱際スリットが指定された壁は耐震壁として扱われず、耐震壁検定自体が
/// 出力されない。
#[test]
fn wall_with_column_face_slit_is_not_checked() {
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];
    let model = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 0.0,
        opening_weight: 0.0,
        slit: sepika_core::model::WallSlit {
            column_face: [true, true],
            beam_face: [false, false],
        },
        openings: vec![],
        finish_intensity: 0.0,
    }));
    assert!(wall_check_result(&model, &forces).is_none());
}

/// 壁横筋に未対応グレード（KH785）を割り当てた壁は、普通強度式で代替せず
/// 検定不能（`Skipped`）とし、壁要素 ID と理由を返す（検定比 0 の偽の安全側
/// 結果を出さない）。
#[test]
fn wall_with_unsupported_shear_rebar_grade_is_skipped_with_reason() {
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];
    let mut model = wall_model(None);
    model.materials[1].name = "KH785".to_string();

    let outcome =
        wall_check_outcome(&model, &forces).expect("未対応グレードの壁も検定行（検定不能）を返す");
    match outcome {
        CheckOutcome::Skipped { reason } => {
            assert!(reason.contains("耐震壁 ID 0"), "壁要素 ID を含む: {reason}");
            assert!(reason.contains("KH785"), "材料名を含む: {reason}");
            assert!(
                reason.contains("SR235・SR295・SD295・SD345・SD390・SD490"),
                "対応グレードの一覧を含む: {reason}"
            );
        }
        CheckOutcome::Checked(_) => panic!("未対応グレードの壁は検定してはならない"),
    }
}

/// 開口寸法の与え方（実寸法・面積のみ・複数開口）が検定比へ反映されること。
///
/// - 単一の個別開口（縦長: l0=750, h0=2000）と、同面積を合計面積のみで
///   与えた場合（壁と同じ辺長比の擬似等価開口に復元される）とで、
///   γ支配項が変わるため検定比が一致しない。
///   面積は共通（750×2000=1,500,000）のため開口周比 r0（耐震壁判定用）は
///   両者で等しいが、実寸法は壁（l=4000,h=3000）と辺長比が異なる縦長形状
///   のため γ3=1−h0/h が支配的になり、擬似等価開口（壁と同じ辺長比）を
///   使った場合の γ1=γ2=γ3 とは異なる低減係数 r になる。
/// - 複数開口（2個）は [`equivalent_opening`] による等価開口に統合され、
///   その等価開口を直接 `RcWallInput` へ供給した場合と同じ検定比になる。
#[test]
fn wall_opening_dimension_paths_are_reflected() {
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];

    let model_single_dims = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 0.0,
        opening_weight: 0.0,
        slit: Default::default(),
        finish_intensity: 0.0,
        openings: vec![WallOpening {
            width: 750.0,
            height: 2000.0,
            offset: None,
        }],
    }));
    let res_single_dims = wall_check_result(&model_single_dims, &forces)
        .expect("r0<0.4 の単一開口は耐震壁として検定されるはず");

    let model_area_only = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 750.0 * 2000.0,
        opening_weight: 0.0,
        slit: Default::default(),
        openings: vec![],
        finish_intensity: 0.0,
    }));
    let res_area_only = wall_check_result(&model_area_only, &forces)
        .expect("同面積を面積のみで与えた壁も耐震壁として検定されるはず");

    assert!(
        (res_single_dims.ratio() - res_area_only.ratio()).abs() > 1e-6,
        "個別寸法 ratio={} と面積のみ ratio={} が一致してしまっている",
        res_single_dims.ratio(),
        res_area_only.ratio()
    );

    // 複数開口（2個）は equivalent_opening による等価開口に統合され、
    // その等価開口を直接 RcWallInput へ供給した場合と同じ検定比になる。
    let dims = [(600.0, 800.0), (500.0, 700.0)];
    let model_multiple = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 0.0,
        opening_weight: 0.0,
        slit: Default::default(),
        finish_intensity: 0.0,
        openings: dims
            .iter()
            .map(|&(w, h)| WallOpening {
                width: w,
                height: h,
                offset: None,
            })
            .collect(),
    }));
    let res_multiple =
        wall_check_result(&model_multiple, &forces).expect("2個の開口は耐震壁として検定されるはず");

    // 期待値: equivalent_opening を直接呼んで壁と同じ辺長比の等価開口を
    // 構築し、同一の RcWallInput（側柱なし・l_clear=l）で検定した結果。
    let (l, h) = (4000.0_f64, 3000.0_f64);
    let (l0p, h0p) = equivalent_opening(&dims, l, h);
    let inp = RcWallInput {
        t: 180.0,
        l,
        l_clear: l,
        fc: 24.0,
        concrete_class: Default::default(),
        ps: 0.006,
        w_ft: crate::rc::rebar_allowable_shear("SD345", false),
        side_columns: vec![],
        opening: Some((l0p, h0p, h, l)),
        q_design: 500_000.0,
        long_term: false,
    };
    let expected = rc_wall_shear_check(&inp);

    assert!(
        (res_multiple.ratio() - expected.ratio()).abs() < 1e-9,
        "複数開口 ratio={} と等価開口直接計算 ratio={} が不一致",
        res_multiple.ratio(),
        expected.ratio()
    );
}

/// 開口周比 r0>0.4 となる大開口の壁は耐震壁として扱われず、検定自体が
/// 出力されない（面積のみ指定・個別開口の面積和の双方）。
#[test]
fn wall_large_opening_ratio_is_not_checked() {
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];

    // opening_area = 0.5・l・h → r0 = sqrt(0.5) ≈ 0.707 > 0.4。
    let model_area_only = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 0.5 * 4000.0 * 3000.0,
        opening_weight: 0.0,
        slit: Default::default(),
        openings: vec![],
        finish_intensity: 0.0,
    }));
    assert!(wall_check_result(&model_area_only, &forces).is_none());

    // 開口2個の面積和 = 2,000,000 + 3,000,000 = 5,000,000
    // → r0 = sqrt(5,000,000 / (4000*3000)) = sqrt(0.41667) ≈ 0.645 > 0.4。
    let model_openings = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 0.0,
        opening_weight: 0.0,
        slit: Default::default(),
        finish_intensity: 0.0,
        openings: vec![
            WallOpening {
                width: 2000.0,
                height: 1000.0,
                offset: None,
            },
            WallOpening {
                width: 2000.0,
                height: 1500.0,
                offset: None,
            },
        ],
    }));
    assert!(wall_check_result(&model_openings, &forces).is_none());
}

/// 近接する2開口（水平純間隔200mm、高さ位置が一致）は、`Auto` モードでは
/// 包絡可能条件（純間隔が両開口の当該方向寸法の小さい方以下）を満たすため
/// 幅2000×高2000の単一の包絡開口に統合され、実寸法経路（単一開口）として
/// 検定される。既定の `Equivalent` モードでは個別開口のまま
/// `equivalent_opening` で等価開口に統合されるため、両モードで検定比が
/// 異なる（r0 の判定を通すため、壁は 8000×4000 とやや大きめに取る）。
#[test]
fn wall_auto_mode_envelopes_close_openings_and_differs_from_equivalent() {
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];
    let openings = vec![
        WallOpening {
            width: 1000.0,
            height: 2000.0,
            offset: Some([0.0, 0.0]),
        },
        WallOpening {
            width: 800.0,
            height: 2000.0,
            offset: Some([1200.0, 0.0]),
        },
    ];

    // 既定（Equivalent）モード: 個別開口のまま equivalent_opening で統合。
    let model_equiv = wall_model_sized(
        8000.0,
        4000.0,
        180.0,
        Some(WallAttr {
            elem: ElemId(0),
            opening_area: 0.0,
            opening_weight: 0.0,
            slit: Default::default(),
            openings: openings.clone(),
            finish_intensity: 0.0,
        }),
    );
    let res_equiv = wall_check_result(&model_equiv, &forces)
        .expect("Equivalent モードは耐震壁として検定されるはず");

    // Auto モード: 純間隔(200)が両開口の幅(800,1000)以下・高さ方向の
    // 純間隔が 0（重なり）のため包絡可能 → 幅2000×高2000の単一開口
    // （実寸法経路）に統合される。
    let mut model_auto = wall_model_sized(
        8000.0,
        4000.0,
        180.0,
        Some(WallAttr {
            elem: ElemId(0),
            opening_area: 0.0,
            opening_weight: 0.0,
            slit: Default::default(),
            openings,
            finish_intensity: 0.0,
        }),
    );
    model_auto.multi_opening_mode = MultiOpeningMode::Auto;
    let res_auto = wall_check_result(&model_auto, &forces)
        .expect("Auto モードで包絡後も耐震壁として検定されるはず");

    // 期待値: 幅2000×高2000の単一開口を実寸法経路で直接検定した結果。
    let model_single = wall_model_sized(
        8000.0,
        4000.0,
        180.0,
        Some(WallAttr {
            elem: ElemId(0),
            opening_area: 0.0,
            opening_weight: 0.0,
            slit: Default::default(),
            finish_intensity: 0.0,
            openings: vec![WallOpening {
                width: 2000.0,
                height: 2000.0,
                offset: None,
            }],
        }),
    );
    let res_single = wall_check_result(&model_single, &forces)
        .expect("包絡開口相当の単一開口も耐震壁として検定されるはず");

    assert!(
        (res_auto.ratio() - res_single.ratio()).abs() < 1e-9,
        "Auto ratio={} と包絡開口(実寸法)直接計算 ratio={} が不一致",
        res_auto.ratio(),
        res_single.ratio()
    );
    assert!(
        (res_auto.ratio() - res_equiv.ratio()).abs() > 1e-6,
        "Auto ratio={} と Equivalent ratio={} が一致してしまっている",
        res_auto.ratio(),
        res_equiv.ratio()
    );
}

/// 遠く離れた小開口2つは、既定（Equivalent）モードでは面積和が小さく
/// 耐震壁として検定されるが、`Envelope` モードでは全開口を包絡した巨大な
/// 矩形の面積で開口周比 r0 を評価するため r0>0.4 となり、耐震壁として
/// 扱われず検定自体が出力されない。
#[test]
fn wall_envelope_mode_excludes_wall_when_envelope_ratio_too_large() {
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];
    let openings = vec![
        WallOpening {
            width: 200.0,
            height: 200.0,
            offset: Some([0.0, 0.0]),
        },
        WallOpening {
            width: 200.0,
            height: 200.0,
            offset: Some([3500.0, 2500.0]),
        },
    ];

    // 既定（Equivalent）モード: 面積和 = 200*200*2 = 80,000
    // → r0 = sqrt(80,000 / (4000*3000)) ≈ 0.0816 ≤ 0.4 で耐震壁として検定。
    let model_equiv = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 0.0,
        opening_weight: 0.0,
        slit: Default::default(),
        openings: openings.clone(),
        finish_intensity: 0.0,
    }));
    assert!(
        wall_check_result(&model_equiv, &forces).is_some(),
        "Equivalent モードでは小開口のため耐震壁として検定されるはず"
    );

    // Envelope モード: 包絡矩形は幅3700×高2700 = 9,990,000
    // → r0 = sqrt(9,990,000 / (4000*3000)) ≈ 0.912 > 0.4 で耐震壁から除外。
    let mut model_envelope = wall_model(Some(WallAttr {
        elem: ElemId(0),
        opening_area: 0.0,
        opening_weight: 0.0,
        slit: Default::default(),
        openings,
        finish_intensity: 0.0,
    }));
    model_envelope.multi_opening_mode = MultiOpeningMode::Envelope;
    assert!(
        wall_check_result(&model_envelope, &forces).is_none(),
        "Envelope モードでは包絡矩形が大きく耐震壁から除外されるはず"
    );
}

fn wall_with_columns_model() -> Model {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};
    let mut model = wall_model(None);
    // 四周のうち両側の鉛直辺（ElemId 3・4）へ 600×600 RC 側柱の断面を与える。
    let col_shape = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![4],
            y: vec![4],
            cover: 50.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    // 材料は断面が持つ。
    let mut col_sec = col_shape.to_section(SectionId(1), "C600".into());
    col_sec.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
    col_sec.material = Some(MaterialId(0));
    col_sec.rebar_material = Some(MaterialId(1));
    col_sec.shear_rebar_material = Some(MaterialId(1));
    model.sections.push(col_sec);
    for e in model
        .elements
        .iter_mut()
        .filter(|e| e.id == ElemId(3) || e.id == ElemId(4))
    {
        e.section = Some(SectionId(1));
        e.local_axis = LocalAxis {
            ref_vector: [1.0, 0.0, 0.0],
        };
    }
    model.materials[0].fc = Some(17.0);
    if let Some(SectionShape::RcWall { ps, pwh_ratio, .. }) = &mut model.sections[0].shape {
        *ps = 0.002;
        *pwh_ratio = Some(0.015);
    }
    model
}

#[test]
fn wall_with_side_columns_emits_reference_skeleton_and_missing_columns_are_skipped() {
    let mut model = wall_with_columns_model();
    let forces: [(f64, [f64; 6]); 1] = [(0.0, [-1_000_000.0, 800_000.0, 0.0, 0.0, 0.0, 1.0e9])];
    let col_forces: [(f64, [f64; 6]); 1] = [(0.0, [-500_000.0, 0.0, 0.0, 0.0, 0.0, 0.0])];
    let member_forces = vec![
        (ElemId(0), forces.as_slice()),
        (ElemId(3), col_forces.as_slice()),
        (ElemId(4), col_forces.as_slice()),
    ];
    model.materials[0].fc = Some(17.0);
    if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut model.sections[0].shape {
        *pwh_ratio = Some(0.015);
    }
    let checks = collect_joint_checks(&model, &member_forces, LoadTerm::Short);

    let nl = checks
        .iter()
        .find(|(_, label, _)| label == "耐震壁(RC)せん断非線形")
        .expect("側柱付き壁でせん断非線形トリリニアが出力される");
    let CheckOutcome::Checked(cr) = &nl.2 else {
        panic!("せん断非線形検定は検定実施（Checked）のはず: {:?}", nl.2);
    };
    let full = crate::full_detail(cr);
    assert!(
        full.contains("Qc=") && full.contains("βs=") && full.contains("Qu="),
        "detail にトリリニア諸元が含まれる: {}",
        full
    );
    assert!(cr.ratio() > 0.0, "Qu 検定比が正: {}", cr.ratio());

    let mut columnless = wall_model(None);
    columnless
        .elements
        .retain(|element| ![ElemId(3), ElemId(4)].contains(&element.id));
    let plain = collect_joint_checks(&columnless, &member_forces, LoadTerm::Short);
    assert!(
        plain.iter().any(|(_, label, outcome)| label == "耐震壁(RC)せん断非線形"
            && matches!(outcome, CheckOutcome::Skipped { reason } if reason.contains("側柱主筋量"))),
        "側柱のない壁はトリリニア対象外"
    );
}

/// RC 十字形接合部で終局検定（Vju/Qdu）の「接合部終局(RC)」チェックが出力される。
#[test]
fn rc_cross_joint_emits_ultimate_check() {
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop};

    let col_rebar = |count: u32, dia: f64| RcRectColumnRebar {
        main_dia: dia,
        x: vec![count / 2],
        y: vec![count / 2],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: 10.0,
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    let beam_rebar = |top: Vec<u32>, bottom: Vec<u32>, dia: f64| RcBeamRebar {
        main_dia: dia,
        top,
        bottom,
        cover: 40.0,
        stirrup: BeamStirrup {
            dia: 10.0,
            pitch: 100.0,
            legs: 2,
        },
    };
    let col_shape = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: col_rebar(8, 25.0),
    };
    let beam_shape = SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: beam_rebar(vec![4], vec![4], 25.0),
    };

    // 中央節点 0 に上下柱・左右梁が取り付く十字形接合部。
    let coords = [
        [0.0, 0.0, 3000.0],     // 0: 中央（接合部）
        [0.0, 0.0, 0.0],        // 1: 柱下端
        [0.0, 0.0, 6000.0],     // 2: 柱上端
        [-6000.0, 0.0, 3000.0], // 3: 梁左端
        [6000.0, 0.0, 3000.0],  // 4: 梁右端
    ];
    let mut nodes = Vec::new();
    for (i, c) in coords.iter().enumerate() {
        nodes.push(Node {
            id: NodeId(i as u32),
            coord: *c,
            restraint: if i == 1 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    // 材料は断面が持つ。主材料 = コンクリート、主筋・せん断補強筋 = SD345。
    let with_mats = |mut sec: Section| {
        sec.frame_use = Some(match &sec.shape {
            Some(
                SectionShape::RcColumnRect { .. }
                | SectionShape::RcColumnCircle { .. }
                | SectionShape::SrcColumnRect { .. },
            ) => sepika_core::model::FrameSectionUse::Column,
            _ => sepika_core::model::FrameSectionUse::Girder,
        });
        sec.material = Some(MaterialId(0));
        sec.rebar_material = Some(MaterialId(1));
        sec.shear_rebar_material = Some(MaterialId(1));
        sec
    };
    let sections = vec![
        with_mats(col_shape.to_section(SectionId(0), "C600".into())),
        with_mats(beam_shape.to_section(SectionId(1), "B400x700".into())),
    ];
    let materials = vec![
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "Fc24".to_string(),
            category: MaterialCategory::Concrete,
            young: 23000.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
        },
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(1),
            name: "SD345".to_string(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(345.0),
        },
    ];
    let make_elem = |id: u32, sec: u32, n0: u32, n1: u32| ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: {
            let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
            v.push(NodeId(n0));
            v.push(NodeId(n1));
            v
        },
        section: Some(SectionId(sec)),
        local_axis: LocalAxis {
            ref_vector: [1.0, 0.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone::default(),
        plastic_zone: None,
        spring: None,
    };
    let elements = vec![
        make_elem(0, 0, 1, 0), // 柱下
        make_elem(1, 0, 0, 2), // 柱上
        make_elem(2, 1, 3, 0), // 梁左
        make_elem(3, 1, 0, 4), // 梁右
    ];
    let model = Model {
        nodes,
        elements,
        sections,
        materials,
        ..Default::default()
    };

    // 各部材の端部内力（[N,Qy,Qz,Mx,My,Mz]）。柱にせん断、梁にモーメント。
    let col_f: [(f64, [f64; 6]); 2] = [
        (0.0, [0.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
        (1.0, [0.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
    ];
    let beam_f: [(f64, [f64; 6]); 2] = [
        (0.0, [0.0, 0.0, 0.0, 0.0, 0.0, 2.0e8]),
        (1.0, [0.0, 0.0, 0.0, 0.0, 0.0, 2.0e8]),
    ];
    let member_forces: Vec<(ElemId, ForcesAt)> = vec![
        (ElemId(0), &col_f),
        (ElemId(1), &col_f),
        (ElemId(2), &beam_f),
        (ElemId(3), &beam_f),
    ];

    let checks = collect_joint_checks(&model, &member_forces, LoadTerm::Short);
    let ult = checks
        .iter()
        .find(|(_, label, _)| label == "接合部終局(RC)")
        .expect("十字形 RC 接合部は終局検定が出力されるはず");
    let CheckOutcome::Checked(cr) = &ult.2 else {
        panic!("接合部終局(RC) は検定実施（Checked）のはず: {:?}", ult.2);
    };
    // Vju/Qdu が有限で、詳細に κ=1.00（十字形）が含まれる。
    assert!(cr.ratio().is_finite());
    let full = crate::full_detail(cr);
    assert!(full.contains("κ=1.00"), "detail={}", full);
}

/// 中央節点に上下柱・左右梁が取り付く十字形接合部のモデル。
fn cross_joint_model(col_shape: SectionShape, beam_shape: SectionShape) -> Model {
    let coords = [
        [0.0, 0.0, 3000.0],     // 0: 中央（接合部）
        [0.0, 0.0, 0.0],        // 1: 柱下端
        [0.0, 0.0, 6000.0],     // 2: 柱上端
        [-6000.0, 0.0, 3000.0], // 3: 梁左端
        [6000.0, 0.0, 3000.0],  // 4: 梁右端
    ];
    let mut nodes = Vec::new();
    for (i, c) in coords.iter().enumerate() {
        nodes.push(Node {
            id: NodeId(i as u32),
            coord: *c,
            restraint: if i == 1 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    let with_mats = |mut sec: Section| {
        sec.frame_use = Some(match &sec.shape {
            Some(
                SectionShape::RcColumnRect { .. }
                | SectionShape::RcColumnCircle { .. }
                | SectionShape::SrcColumnRect { .. },
            ) => sepika_core::model::FrameSectionUse::Column,
            _ => sepika_core::model::FrameSectionUse::Girder,
        });
        sec.material = Some(MaterialId(0));
        sec.rebar_material = Some(MaterialId(1));
        sec.shear_rebar_material = Some(MaterialId(1));
        sec.steel_material = Some(MaterialId(2));
        sec
    };
    let sections = vec![
        with_mats(col_shape.to_section(SectionId(0), "C".into())),
        with_mats(beam_shape.to_section(SectionId(1), "B".into())),
    ];
    let materials = vec![
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "Fc24".to_string(),
            category: MaterialCategory::Concrete,
            young: 23000.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
        },
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(1),
            name: "SD345".to_string(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(345.0),
        },
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(2),
            name: "SN400B".to_string(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(235.0),
        },
    ];
    let make_elem = |id: u32, sec: u32, n0: u32, n1: u32| ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: {
            let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
            v.push(NodeId(n0));
            v.push(NodeId(n1));
            v
        },
        section: Some(SectionId(sec)),
        local_axis: LocalAxis {
            ref_vector: [1.0, 0.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone::default(),
        plastic_zone: None,
        spring: None,
    };
    let elements = vec![
        make_elem(0, 0, 1, 0), // 柱下
        make_elem(1, 0, 0, 2), // 柱上
        make_elem(2, 1, 3, 0), // 梁左
        make_elem(3, 1, 0, 4), // 梁右
    ];
    Model {
        nodes,
        elements,
        sections,
        materials,
        ..Default::default()
    }
}

/// 実配筋モデル（`RcBeamRect`＋`RcColumnRect`）の RC 十字形接合部で、
/// 許容応力度・終局の両検定が出力される。
#[test]
fn rc_cross_joint_new_types_emits_checks() {
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop};

    let col_shape = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 25.0,
            x: vec![4, 2],
            y: vec![4],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let beam_shape = SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: RcBeamRebar {
            main_dia: 25.0,
            top: vec![4, 2],
            bottom: vec![3, 2],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    let model = cross_joint_model(col_shape, beam_shape);

    let col_f: [(f64, [f64; 6]); 2] = [
        (0.0, [0.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
        (1.0, [0.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
    ];
    let beam_f: [(f64, [f64; 6]); 2] = [
        (0.0, [0.0, 0.0, 0.0, 0.0, 0.0, 2.0e8]),
        (1.0, [0.0, 0.0, 0.0, 0.0, 0.0, 2.0e8]),
    ];
    let member_forces: Vec<(ElemId, ForcesAt)> = vec![
        (ElemId(0), &col_f),
        (ElemId(1), &col_f),
        (ElemId(2), &beam_f),
        (ElemId(3), &beam_f),
    ];

    let checks = collect_joint_checks(&model, &member_forces, LoadTerm::Short);
    for label in ["接合部(RC)", "接合部終局(RC)"] {
        let found = checks
            .iter()
            .find(|(_, l, _)| l == label)
            .unwrap_or_else(|| panic!("{label} が出力されるはず"));
        let cr = found.2.clone().unwrap_checked();
        assert!(cr.ratio().is_finite(), "{label} ratio={}", cr.ratio());
    }
}

/// 実配筋の幾何が不整合な梁が取り付く接合部は、RC 接合部検定を実施せず
/// 検定不能として理由を示す。不整合な集約値で検定比を作らない。
#[test]
fn rc_joint_with_invalid_beam_rebar_is_skipped() {
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop};

    let col_shape = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 25.0,
            x: vec![4],
            y: vec![4],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let beam_shape = SectionShape::RcBeamRect {
        b: 400.0,
        d: 700.0,
        rebar: RcBeamRebar {
            main_dia: 25.0,
            top: vec![0],
            bottom: vec![6],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    if let SectionShape::RcBeamRect { b, d, ref rebar } = beam_shape {
        assert!(rebar.validate(b, d).is_err());
    } else {
        unreachable!();
    }
    let model = cross_joint_model(col_shape, beam_shape);

    let col_f: [(f64, [f64; 6]); 2] = [
        (0.0, [0.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
        (1.0, [0.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
    ];
    let beam_f: [(f64, [f64; 6]); 2] = [
        (0.0, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        (1.0, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ];
    let member_forces: Vec<(ElemId, ForcesAt)> = vec![
        (ElemId(0), &col_f),
        (ElemId(1), &col_f),
        (ElemId(2), &beam_f),
        (ElemId(3), &beam_f),
    ];

    let checks = collect_joint_checks(&model, &member_forces, LoadTerm::Short);
    for label in ["接合部(RC)", "接合部終局(RC)"] {
        let found = checks
            .iter()
            .find(|(_, l, _)| l == label)
            .unwrap_or_else(|| panic!("{label} が出力されるはず"));
        match &found.2 {
            CheckOutcome::Skipped { reason } => {
                assert!(reason.contains("幾何が不整合"), "{label} reason={reason}");
            }
            CheckOutcome::Checked(_) => panic!("{label} は不整合時に検定してはならない"),
        }
    }
}

/// 実配筋モデル（`SrcBeamRect`＋`SrcColumnRect`）の SRC 十字形接合部で、
/// パネルゾーン検定が出力される。
#[test]
fn src_cross_panel_new_types_emits_check() {
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop};

    let col_shape = SectionShape::SrcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: 25.0,
            x: vec![4, 2],
            y: vec![4],
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
        steel_web_thick: 9.0,
        steel_flange_thick: 16.0,
    };
    let beam_shape = SectionShape::SrcBeamRect {
        b: 400.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 25.0,
            top: vec![4, 2],
            bottom: vec![3, 2],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        },
        steel_height: 400.0,
        steel_width: 200.0,
        steel_web_thick: 9.0,
        steel_flange_thick: 16.0,
    };
    let model = cross_joint_model(col_shape, beam_shape);

    let col_f: [(f64, [f64; 6]); 2] = [
        (0.0, [-500_000.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
        (1.0, [-500_000.0, 100_000.0, 0.0, 0.0, 0.0, 0.0]),
    ];
    let beam_f: [(f64, [f64; 6]); 2] = [
        (0.0, [0.0, 0.0, 0.0, 0.0, 0.0, 2.0e8]),
        (1.0, [0.0, 0.0, 0.0, 0.0, 0.0, 2.0e8]),
    ];
    let member_forces: Vec<(ElemId, ForcesAt)> = vec![
        (ElemId(0), &col_f),
        (ElemId(1), &col_f),
        (ElemId(2), &beam_f),
        (ElemId(3), &beam_f),
    ];

    let checks = collect_joint_checks(&model, &member_forces, LoadTerm::Short);
    let found = checks
        .iter()
        .find(|(_, l, _)| l == "柱梁接合部(SRC)")
        .expect("SRC 十字形接合部はパネルゾーン検定が出力されるはず");
    let cr = found.2.clone().unwrap_checked();
    assert!(
        cr.ratio().is_finite() && cr.ratio() > 0.0,
        "ratio={}",
        cr.ratio()
    );
}

fn all_wall_outcomes(model: &Model) -> Vec<(NodeId, String, CheckOutcome)> {
    let forces = [(0.0, [-1_000_000.0, 800_000.0, 0.0, 0.0, 0.0, 1.0e9])];
    let columns = [(0.0, [0.0; 6])];
    collect_joint_checks(
        model,
        &[
            (ElemId(0), &forces),
            (ElemId(3), &columns),
            (ElemId(4), &columns),
        ],
        LoadTerm::Short,
    )
}

#[test]
fn horizontal_material_absence_and_invalid_strength_never_fallback() {
    for fy in [
        None,
        Some(0.0),
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        let mut model = wall_with_columns_model();
        model.materials[1].fy = fy;
        let checks = all_wall_outcomes(&model);
        for label in ["耐震壁(RC)", "耐震壁(RC)せん断非線形"] {
            let outcome = &checks.iter().find(|(_, l, _)| l == label).unwrap().2;
            assert!(
                matches!(outcome, CheckOutcome::Skipped { reason }
                if reason.contains("ID 0") && reason.contains("横筋") && reason.contains("fy")),
                "{outcome:?}"
            );
        }
    }
    for missing_vertical in [false, true] {
        let mut model = wall_with_columns_model();
        model.sections[0].shear_rebar_material = None;
        if missing_vertical {
            model.sections[0].rebar_material = None;
        }
        assert!(all_wall_outcomes(&model).iter().any(|(_,label,outcome)| label == "耐震壁(RC)"
            && matches!(outcome,CheckOutcome::Skipped { reason } if reason.contains("横筋") && reason.contains("未割当"))));
    }
}

#[test]
fn wall_skeleton_accepts_parallel_square_columns_and_rejects_relative_rotation() {
    for global_angle in [0.0_f64, 0.7, 1.9] {
        let rotate = |v: [f64; 3]| {
            let (sin, cos) = global_angle.sin_cos();
            [cos * v[0] - sin * v[1], sin * v[0] + cos * v[1], v[2]]
        };
        let mut accepted_detail = None;
        for (reference, accepted) in [
            ([1.0, 0.0, 0.0], true),
            ([0.0, 1.0, 0.0], true),
            ([1.0, 0.0, 5.0], true),
            ([1.0, 1.0, 0.0], false),
        ] {
            let mut model = wall_with_columns_model();
            for node in &mut model.nodes {
                node.coord = rotate(node.coord);
            }
            for element in &mut model.elements {
                let reference = if element.id == ElemId(3) || element.id == ElemId(4) {
                    reference
                } else {
                    element.local_axis.ref_vector
                };
                element.local_axis.ref_vector = rotate(reference);
            }
            let checks = all_wall_outcomes(&model);
            assert!(matches!(
                &checks
                    .iter()
                    .find(|(_, label, _)| label == "耐震壁(RC)")
                    .unwrap()
                    .2,
                CheckOutcome::Checked(_)
            ));
            let outcome = &checks
                .iter()
                .find(|(_, label, _)| label == "耐震壁(RC)せん断非線形")
                .unwrap()
                .2;
            if accepted {
                let CheckOutcome::Checked(result) = outcome else {
                    panic!("{outcome:?}");
                };
                let detail = crate::full_detail(result);
                if let Some(expected) = &accepted_detail {
                    assert_eq!(&detail, expected);
                } else {
                    accepted_detail = Some(detail);
                }
            } else {
                assert!(matches!(outcome, CheckOutcome::Skipped { reason }
                    if reason.contains("耐震壁 ID 0")
                    && reason.contains("側柱 ID") && reason.contains("断面方向")));
            }
        }
    }
}

#[test]
fn explicit_295_and_345_are_valid_and_vertical_input_does_not_control_beta() {
    for (name, fy) in [("SD295", 295.0), ("SD345", 345.0)] {
        let mut model = wall_with_columns_model();
        model.materials[1].name = name.into();
        model.materials[1].fy = Some(fy);
        let original = all_wall_outcomes(&model);
        let original_nl = &original
            .iter()
            .find(|(_, l, _)| l == "耐震壁(RC)せん断非線形")
            .unwrap()
            .2;
        assert!(
            matches!(original_nl, CheckOutcome::Checked(_)),
            "{original_nl:?}"
        );
        for vertical in [None, Some(MaterialId(2))] {
            let mut altered = model.clone();
            let mut material = altered.materials[1].clone();
            material.id = MaterialId(2);
            material.name = "SD345".into();
            material.fy = None;
            altered.materials.push(material);
            altered.sections[0].rebar_material = vertical;
            let checks = all_wall_outcomes(&altered);
            assert!(
                matches!(&checks.iter().find(|(_,l,_)| l == "耐震壁(RC)").unwrap().2,
                CheckOutcome::Skipped { reason } if reason.contains("縦筋"))
            );
            assert_eq!(
                format!(
                    "{:?}",
                    checks
                        .iter()
                        .find(|(_, l, _)| l == "耐震壁(RC)せん断非線形")
                        .unwrap()
                        .2
                ),
                format!("{original_nl:?}")
            );
        }
    }
}

#[test]
fn horizontal_ratio_is_explicit_and_missing_or_inconsistent_input_is_skipped() {
    for ratio in [
        None,
        Some(-0.01),
        Some(0.001),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        let mut model = wall_with_columns_model();
        if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut model.sections[0].shape {
            *pwh_ratio = ratio;
        }
        let checks = all_wall_outcomes(&model);
        assert!(matches!(
            &checks.iter().find(|(_, l, _)| l == "耐震壁(RC)").unwrap().2,
            CheckOutcome::Checked(_)
        ));
        assert!(
            matches!(&checks.iter().find(|(_,l,_)| l == "耐震壁(RC)せん断非線形").unwrap().2,CheckOutcome::Skipped {reason} if reason.contains("横筋比"))
        );
    }
}

#[test]
fn gui_mcp_common_dispatch_and_direct_entry_report_identical_missing_material() {
    use sepika_element::frame::beam::MemberForces;
    let mut model = wall_with_columns_model();
    model.sections[0].shear_rebar_material = None;
    let rows = vec![(0.0, [0.0; 6])];
    let direct = collect_joint_checks(&model, &[(ElemId(0), rows.as_slice())], LoadTerm::Short);
    let report = crate::run_member_design_checks(
        &model,
        &[(ElemId(0), MemberForces { at: rows })],
        &[],
        &crate::MemberDesignCheckOptions {
            term: LoadTerm::Short,
            ..Default::default()
        },
    );
    for label in ["耐震壁(RC)", "耐震壁(RC)せん断非線形"] {
        let direct = &direct.iter().find(|(_, l, _)| l == label).unwrap().2;
        let kind = if label.ends_with("せん断非線形") {
            crate::wall_check::WallCheckKind::ReferenceSkeleton
        } else {
            crate::wall_check::WallCheckKind::AllowableShear
        };
        let common = &report
            .wall_checks
            .iter()
            .find(|w| w.kind == kind)
            .unwrap()
            .outcome;
        assert_eq!(format!("{direct:?}"), format!("{common:?}"));
        assert!(
            matches!(common,CheckOutcome::Skipped {reason} if reason.contains("横筋") && reason.contains("ID 0"))
        );
    }
}

#[test]
fn concrete_missing_or_invalid_is_reported_at_wall_entry() {
    for strength in [
        None,
        Some(0.0),
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        let mut model = wall_with_columns_model();
        model.materials[0].fc = strength;
        let checks = all_wall_outcomes(&model);
        for label in ["耐震壁(RC)", "耐震壁(RC)せん断非線形"] {
            assert!(matches!(&checks.iter().find(|(_,l,_)|l==label).unwrap().2,
                CheckOutcome::Skipped {reason} if reason.contains("ID 0") && reason.contains("コンクリート") && reason.contains("Fc")));
        }
    }
    let mut model = wall_with_columns_model();
    model.sections[0].material = None;
    assert!(all_wall_outcomes(&model).iter().any(|(_,label,outcome)|label=="耐震壁(RC)せん断非線形"
        && matches!(outcome,CheckOutcome::Skipped {reason} if reason.contains("コンクリート") && reason.contains("Fc"))));
}

#[test]
fn wall_main_material_must_have_concrete_category() {
    for category in [
        MaterialCategory::Concrete,
        MaterialCategory::Steel,
        MaterialCategory::Rebar,
    ] {
        let mut model = wall_with_columns_model();
        model.materials[0].category = category;
        let checks = all_wall_outcomes(&model);
        for label in ["耐震壁(RC)", "耐震壁(RC)せん断非線形"] {
            let outcome = &checks
                .iter()
                .find(|(_, current, _)| current == label)
                .unwrap()
                .2;
            if category == MaterialCategory::Concrete {
                assert!(matches!(outcome, CheckOutcome::Checked(_)));
            } else {
                assert!(matches!(outcome, CheckOutcome::Skipped { reason }
                    if reason.contains("耐震壁 ID 0") && reason.contains("主材 ID 0")
                    && reason.contains("コンクリート役割に不適合")));
            }
        }
    }
}

#[test]
fn actual_side_column_missing_inputs_are_not_removed_from_reference_section() {
    for column_id in [ElemId(3), ElemId(4)] {
        for missing in [
            "主材なし",
            "主材不解決",
            "断面なし",
            "断面不解決",
            "形状なし",
        ] {
            let mut model = wall_with_columns_model();
            let mut section = model.sections[1].clone();
            section.id = SectionId(2);
            if missing == "主材なし" {
                section.material = None;
            } else if missing == "主材不解決" {
                section.material = Some(MaterialId(999));
            } else if missing == "形状なし" {
                section.shape = None;
            }
            model.sections.push(section);
            model
                .elements
                .iter_mut()
                .find(|e| e.id == column_id)
                .unwrap()
                .section = match missing {
                "断面なし" => None,
                "断面不解決" => Some(SectionId(999)),
                _ => Some(SectionId(2)),
            };
            let checks = all_wall_outcomes(&model);
            let outcome = &checks
                .iter()
                .find(|(_, label, _)| label == "耐震壁(RC)せん断非線形")
                .unwrap()
                .2;
            assert!(
                matches!(outcome, CheckOutcome::Skipped { reason }
                if reason.contains("耐震壁 ID 0") && reason.contains(&format!("側柱 ID {}", column_id.0))
                && reason.contains(if missing.starts_with("主材") { "コンクリート主材" } else { "断面" })),
                "{missing}: {outcome:?}"
            );
        }
    }
}

#[test]
fn mixed_side_column_concrete_and_asymmetric_sections_are_outside_reference_scope() {
    for column_id in [ElemId(3), ElemId(4)] {
        let mut single_column = wall_with_columns_model();
        single_column
            .elements
            .retain(|element| element.id != column_id);
        assert!(matches!(&all_wall_outcomes(&single_column).iter()
            .find(|(_, label, _)| label == "耐震壁(RC)せん断非線形").unwrap().2,
            CheckOutcome::Skipped { reason } if reason.contains("片側のみ") && reason.contains("非対称")));
        for change in ["軽量", "主筋", "断面寸法"] {
            let mut model = wall_with_columns_model();
            let mut section = model.sections[1].clone();
            section.id = SectionId(2);
            if change == "軽量" {
                let mut material = model.materials[0].clone();
                material.id = MaterialId(2);
                material.concrete_class = sepika_core::units::ConcreteClass::Lightweight1;
                model.materials.push(material);
                section.material = Some(MaterialId(2));
            } else if let Some(SectionShape::RcColumnRect { b, d, rebar }) = &mut section.shape {
                if change == "主筋" {
                    rebar.main_dia = 10.0;
                } else {
                    *b = 700.0;
                    *d = 700.0;
                }
            }
            model.sections.push(section);
            model
                .elements
                .iter_mut()
                .find(|e| e.id == column_id)
                .unwrap()
                .section = Some(SectionId(2));
            let checks = all_wall_outcomes(&model);
            assert!(
                matches!(&checks.iter().find(|(_, label, _)| label == "耐震壁(RC)せん断非線形").unwrap().2,
                CheckOutcome::Skipped { reason } if reason.contains("耐震壁 ID 0")
                    && reason.contains("側柱 ID")
                    && reason.contains(if change == "軽量" { "コンクリート種別" } else { "非対称" }))
            );
        }
    }
}

#[test]
fn reference_section_resolves_actual_columns_without_column_force_rows() {
    let model = wall_with_columns_model();
    let normal = all_wall_outcomes(&model);
    let rows = [(0.0, [-1_000_000.0, 800_000.0, 0.0, 0.0, 0.0, 1.0e9])];
    let wall_only = collect_joint_checks(&model, &[(ElemId(0), &rows)], LoadTerm::Short);
    let reference = |checks: &[(NodeId, String, CheckOutcome)]| {
        format!(
            "{:?}",
            checks
                .iter()
                .find(|(_, label, _)| label == "耐震壁(RC)せん断非線形")
                .unwrap()
                .2
        )
    };
    assert_eq!(reference(&normal), reference(&wall_only));
    assert!(matches!(
        &wall_only
            .iter()
            .find(|(_, label, _)| label == "耐震壁(RC)せん断非線形")
            .unwrap()
            .2,
        CheckOutcome::Checked(_)
    ));
}

#[test]
fn tilted_wall_reference_skeleton_is_skipped_without_changing_allowable_entry() {
    for angle in [0.5_f64, -0.5] {
        let mut model = wall_with_columns_model();
        let (sin, cos) = angle.sin_cos();
        let rotate = |v: [f64; 3]| [v[0], cos * v[1] - sin * v[2], sin * v[1] + cos * v[2]];
        for node in &mut model.nodes {
            node.coord = rotate(node.coord);
        }
        for element in &mut model.elements {
            element.local_axis.ref_vector = rotate(element.local_axis.ref_vector);
        }
        let checks = all_wall_outcomes(&model);
        assert!(matches!(
            &checks
                .iter()
                .find(|(_, label, _)| label == "耐震壁(RC)")
                .unwrap()
                .2,
            CheckOutcome::Checked(_)
        ));
        assert!(
            matches!(&checks.iter().find(|(_, label, _)| label == "耐震壁(RC)せん断非線形").unwrap().2,
            CheckOutcome::Skipped { reason } if reason.contains("耐震壁 ID 0")
                && reason.contains("傾斜壁") && reason.contains("適用未確認"))
        );
    }
}

#[test]
fn long_concrete_only_output_does_not_require_wall_rebar_material() {
    let mut model = wall_model(None);
    model.sections[0].rebar_material = None;
    model.sections[0].shear_rebar_material = None;
    let rows = [(0.0, [0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0])];
    let checks = collect_joint_checks(&model, &[(ElemId(0), &rows)], LoadTerm::Long);
    assert!(matches!(
        &checks.iter().find(|(_, l, _)| l == "耐震壁(RC)").unwrap().2,
        CheckOutcome::Checked(_)
    ));
    assert!(
        matches!(&checks.iter().find(|(_,l,_)|l=="耐震壁(RC)せん断非線形").unwrap().2,CheckOutcome::Skipped {reason} if reason.contains("横筋"))
    );
}

#[test]
fn changing_vertical_ratio_and_strength_keeps_horizontal_skeleton_unchanged() {
    let mut model = wall_with_columns_model();
    let original = all_wall_outcomes(&model);
    let original = &original
        .iter()
        .find(|(_, l, _)| l == "耐震壁(RC)せん断非線形")
        .unwrap()
        .2;
    let mut vertical = model.materials[1].clone();
    vertical.id = MaterialId(2);
    vertical.name = "SR235".into();
    vertical.fy = Some(235.0);
    model.materials.push(vertical);
    model.sections[0].rebar_material = Some(MaterialId(2));
    if let Some(SectionShape::RcWall { ps, .. }) = &mut model.sections[0].shape {
        *ps = 0.006;
    }
    let altered = all_wall_outcomes(&model);
    let altered = &altered
        .iter()
        .find(|(_, l, _)| l == "耐震壁(RC)せん断非線形")
        .unwrap()
        .2;
    assert_eq!(format!("{original:?}"), format!("{altered:?}"));
    assert!(matches!(altered, CheckOutcome::Checked(_)));
}

fn status_wall_model() -> (Model, sepika_load::wall_expand::WallExpansionIndex) {
    use sepika_core::ids::{WallPlateId, WallRegionId};
    use sepika_core::model::{WallPlate, WallPlateShape, WallRegion};
    let mut model = wall_with_columns_model();
    model.elements.retain(|e| e.kind != ElementKind::Wall);
    let mut steel = model.materials[1].clone();
    steel.id = MaterialId(2);
    steel.category = MaterialCategory::Steel;
    steel.name = "SN400B".into();
    model.materials.push(steel);
    let mut steel_sec = model.sections[0].clone();
    steel_sec.id = SectionId(2);
    steel_sec.shape = None;
    steel_sec.thickness = Some(6.0);
    steel_sec.material = Some(MaterialId(2));
    model.sections.push(steel_sec);
    let mut missing = model.sections[0].clone();
    missing.id = SectionId(3);
    missing.material = None;
    model.sections.push(missing);
    for section in [0, 2, 3] {
        model.add_enclosed_wall_plate_from_nodes(
            &[NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            WallPlate {
                dl_support: None,
                self_weight_shares: vec![],
                id: WallPlateId(0),
                shape: WallPlateShape::Enclosed,
                section: Some(SectionId(section)),
                opening_area: 0.0,
                opening_weight: 0.0,
                openings: vec![],
                loads: vec![],
                slit: Default::default(),
            },
        );
    }
    model.wall_regions.push(WallRegion {
        id: WallRegionId(0),
        name: "同節点3壁".into(),
        boundary: vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        wall_plate_ids: vec![WallPlateId(0), WallPlateId(1), WallPlateId(2)],
        posts: vec![],
    });
    let (model, index, _) = sepika_load::wall_expand::expand_wall_elements(&model);
    (model, index)
}

#[test]
fn wall_status_candidates_keep_plate_case_kind_and_independent_counts() {
    use crate::wall_check::{WallCheckKind, WallCheckSummary, WallSkipKind};
    use sepika_element::frame::beam::MemberForces;
    let (model, index) = status_wall_model();
    // Q=N=M=0 の独立入力では、正のRC耐力に対する許容検定比は厳密に0。
    let forces: Vec<_> = model
        .elements
        .iter()
        .map(|e| {
            (
                e.id,
                MemberForces {
                    at: vec![(0.0, [0.0; 6])],
                },
            )
        })
        .collect();
    let report = crate::run_member_design_checks(
        &model,
        &forces,
        &[],
        &crate::MemberDesignCheckOptions {
            wall_index: Some(&index),
            wall_case: "combo:7:DL+E",
            term: LoadTerm::Short,
            ..Default::default()
        },
    );
    assert!(report
        .joint_checks
        .iter()
        .all(|(_, l, _)| !l.contains("耐震壁")));
    assert_eq!(report.wall_checks.len(), 6);
    for check in &report.wall_checks {
        assert_eq!(check.case, "combo:7:DL+E");
        assert_eq!(index.plate_of(check.elem.unwrap()), check.plate);
    }
    let first_two: Vec<_> = report
        .wall_checks
        .iter()
        .filter(|w| w.plate.unwrap().0 < 2 && w.kind == WallCheckKind::AllowableShear)
        .cloned()
        .collect();
    let s = WallCheckSummary::from_checks(&first_two);
    assert_eq!(
        (
            s.n_walls,
            s.n_ok,
            s.n_skipped,
            s.n_ok_walls,
            s.n_skipped_walls
        ),
        (2, 1, 1, 1, 1)
    );
    assert_eq!(s.max_ratio, Some(0.0));
    let s = WallCheckSummary::for_kind(&report.wall_checks, WallCheckKind::AllowableShear);
    assert_eq!((s.n_walls, s.n_ok, s.n_skipped), (3, 1, 2));
    assert_eq!(
        report
            .wall_checks
            .iter()
            .filter(|w| w.plate.unwrap().0 == 1)
            .count(),
        2
    );
    assert!(report
        .wall_checks
        .iter()
        .filter(|w| w.plate.unwrap().0 == 1)
        .all(|w| w.skip_kind == Some(WallSkipKind::NotImplemented)));
    assert!(report
        .wall_checks
        .iter()
        .filter(|w| w.plate.unwrap().0 == 2)
        .all(|w| w.skip_kind == Some(WallSkipKind::MissingInput)));
    let s = WallCheckSummary::from_checks(&report.wall_checks);
    assert_eq!(
        (s.n_walls, s.n_checks, s.n_ok_walls, s.n_skipped_walls),
        (3, 6, 1, 2)
    );
}

#[test]
fn wall_status_missing_empty_invalid_response_and_self_weight_are_distinct() {
    use crate::wall_check::{WallCheckSummary, WallSkipKind};
    let (mut model, index) = status_wall_model();
    let rc = index
        .generated_elem_ids()
        .find(|id| index.plate_of(*id).unwrap().0 == 0)
        .unwrap();
    for responses in [vec![], vec![(rc, vec![])]] {
        let refs: Vec<_> = responses
            .iter()
            .map(|(id, f)| (*id, f.as_slice()))
            .collect();
        let checks =
            collect_wall_design_checks(&model, &refs, LoadTerm::Short, Some(&index), "case:4");
        assert!(checks
            .iter()
            .filter(|w| w.elem == Some(rc))
            .all(|w| w.skip_kind == Some(WallSkipKind::MissingResponse)));
        assert_eq!(WallCheckSummary::from_checks(&checks).max_ratio, None);
    }
    let response = [(0.0, [f64::NAN; 6])];
    let checks = collect_wall_design_checks(
        &model,
        &[(rc, &response)],
        LoadTerm::Short,
        Some(&index),
        "case:4",
    );
    assert!(checks
        .iter()
        .filter(|w| w.elem == Some(rc))
        .all(|w| w.skip_kind == Some(WallSkipKind::InvalidInput)));
    model
        .wall_attrs
        .iter_mut()
        .find(|a| a.elem == rc)
        .unwrap()
        .slit
        .beam_face[0] = true;
    let checks = collect_wall_design_checks(&model, &[], LoadTerm::Short, Some(&index), "case:4");
    assert!(checks
        .iter()
        .filter(|w| w.elem == Some(rc))
        .all(|w| !w.seismic_target && w.skip_kind == Some(WallSkipKind::NotApplicable)));
    let s = WallCheckSummary::from_checks(&checks);
    assert_eq!((s.n_walls, s.n_outside, s.n_skipped), (2, 2, 4));
    assert_eq!(s.max_ratio, None);
}

#[test]
fn wall_status_slit_stays_outside_with_or_without_section() {
    use crate::wall_check::{WallCheckSummary, WallSkipKind};
    let (mut source, _) = status_wall_model();
    source.elements.retain(|e| e.kind != ElementKind::Wall);
    source.wall_attrs.clear();
    source.wall_plates.truncate(1);
    source.wall_regions[0].wall_plate_ids.truncate(1);
    source.wall_plates[0].slit.beam_face[0] = true;
    for section in [Some(SectionId(0)), None] {
        source.wall_plates[0].section = section;
        let (model, index, _) = sepika_load::wall_expand::expand_wall_elements(&source);
        let checks =
            collect_wall_design_checks(&model, &[], LoadTerm::Short, Some(&index), "case:0");
        assert_eq!(checks.len(), 2);
        assert!(checks
            .iter()
            .all(|w| !w.seismic_target && w.skip_kind == Some(WallSkipKind::NotApplicable)));
        let summary = WallCheckSummary::from_checks(&checks);
        assert_eq!(
            (summary.n_walls, summary.n_skipped, summary.n_outside),
            (0, 0, 2)
        );
        assert_eq!(summary.max_ratio, None);
        assert!(checks.iter().all(|w| w.elem.is_some() == section.is_some()));
    }
}

#[test]
fn wall_status_slit_stays_outside_with_invalid_thickness_and_plate_or_attr() {
    use crate::wall_check::{WallCheckSummary, WallSkipKind};
    let (mut source, _) = status_wall_model();
    source.elements.retain(|e| e.kind != ElementKind::Wall);
    source.wall_attrs.clear();
    source.wall_plates.truncate(1);
    source.wall_regions[0].wall_plate_ids.truncate(1);
    source.wall_plates[0].slit.beam_face[0] = true;
    for thickness in [180.0, -1.0] {
        source.sections[0].thickness = Some(thickness);
        let (expanded, index, _) = sepika_load::wall_expand::expand_wall_elements(&source);
        let wall = expanded
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Wall)
            .unwrap();
        assert!(!sepika_element::wall::misc_wall::wall_is_seismic(
            wall, &expanded
        ));
        for slit_source in ["plate_and_attr", "plate", "attr"] {
            let mut model = expanded.clone();
            if slit_source == "plate" {
                model
                    .wall_attrs
                    .iter_mut()
                    .for_each(|a| a.slit = Default::default());
            } else if slit_source == "attr" {
                model.wall_plates.clear();
            }
            let checks = collect_wall_design_checks(
                &model,
                &[],
                LoadTerm::Short,
                (slit_source != "attr").then_some(&index),
                "case:0",
            );
            assert_eq!(checks.len(), 2);
            let expected_kind = if thickness > 0.0 {
                WallSkipKind::NotApplicable
            } else {
                WallSkipKind::InvalidInput
            };
            assert!(checks
                .iter()
                .all(|w| !w.seismic_target && w.skip_kind == Some(expected_kind)));
            assert!(checks.iter().all(|w| w.elem == Some(wall.id)
                && w.plate == (slit_source != "attr").then_some(source.wall_plates[0].id)
                && matches!(w.outcome, CheckOutcome::Skipped { .. })));
            let summary = WallCheckSummary::from_checks(&checks);
            assert_eq!(
                (summary.n_walls, summary.n_skipped, summary.n_outside),
                (0, 0, 2)
            );
            assert_eq!(summary.max_ratio, None);
            if thickness < 0.0 {
                assert!(checks.iter().all(|w| matches!(&w.outcome,
                    CheckOutcome::Skipped { reason } if reason.contains("板厚が不正"))));
            }
        }
    }
}

#[test]
fn wall_status_ungenerated_missing_section_and_attached_weight_only_remain_visible() {
    use crate::wall_check::{WallCheckSummary, WallSkipKind};
    use sepika_core::model::{LoadTransfer, RegionAnchor, WallPlateShape};
    let (mut expanded, _) = status_wall_model();
    expanded.elements.retain(|e| e.kind != ElementKind::Wall);
    expanded.wall_attrs.clear();
    expanded.wall_plates[0].section = None;
    expanded.wall_plates[1].shape = WallPlateShape::Attached {
        anchor: RegionAnchor::Line {
            nodes: [NodeId(0), NodeId(1)],
            span: [0.0, 1.0],
            transfer: LoadTransfer::Anchor,
        },
        extent: Some([500.0, 500.0]),
    };
    let (model, index, _) = sepika_load::wall_expand::expand_wall_elements(&expanded);
    let checks = collect_wall_design_checks(&model, &[], LoadTerm::Long, Some(&index), "case:0");
    assert!(checks
        .iter()
        .filter(|w| w.plate.unwrap().0 == 0)
        .all(|w| w.seismic_target
            && w.elem.is_none()
            && w.skip_kind == Some(WallSkipKind::MissingInput)));
    assert!(checks
        .iter()
        .filter(|w| w.plate.unwrap().0 == 1)
        .all(|w| !w.seismic_target && w.elem.is_none()));
    assert_eq!(WallCheckSummary::from_checks(&checks).max_ratio, None);
}

#[test]
fn wall_status_missing_invalid_fc_src_and_shape_applicability_are_distinct() {
    use crate::wall_check::WallSkipKind;
    let (base, index) = status_wall_model();
    let response = [(0.0, [0.0; 6])];
    let rc = index
        .generated_elem_ids()
        .find(|id| index.plate_of(*id).unwrap().0 == 0)
        .unwrap();
    for (fc, expected) in [
        (None, WallSkipKind::MissingInput),
        (Some(f64::NAN), WallSkipKind::InvalidInput),
        (Some(-1.0), WallSkipKind::InvalidInput),
    ] {
        let mut model = base.clone();
        model.materials[0].fc = fc;
        let checks = collect_wall_design_checks(
            &model,
            &[(rc, &response)],
            LoadTerm::Long,
            Some(&index),
            "case:0",
        );
        assert!(checks
            .iter()
            .filter(|w| w.elem == Some(rc))
            .all(|w| w.seismic_target && w.skip_kind == Some(expected)));
    }
    for (fy, expected) in [
        (None, WallSkipKind::MissingInput),
        (Some(f64::NAN), WallSkipKind::InvalidInput),
        (Some(-1.0), WallSkipKind::InvalidInput),
    ] {
        let mut model = base.clone();
        model.materials[1].fy = fy;
        let checks = collect_wall_design_checks(
            &model,
            &[(rc, &response)],
            LoadTerm::Short,
            Some(&index),
            "case:0",
        );
        assert!(checks
            .iter()
            .filter(|w| w.elem == Some(rc))
            .all(|w| w.skip_kind == Some(expected)));
    }
    let mut model = base.clone();
    model.sections[0].steel_material = Some(MaterialId(2));
    let checks = collect_wall_design_checks(
        &model,
        &[(rc, &response)],
        LoadTerm::Long,
        Some(&index),
        "case:0",
    );
    assert!(checks
        .iter()
        .filter(|w| w.elem == Some(rc))
        .all(|w| matches!(&w.outcome, CheckOutcome::Skipped { reason } if reason.contains("SRC"))));
    let mut model = base.clone();
    model.sections[0].shape = Some(SectionShape::RcSlab { thickness: 180.0 });
    let checks = collect_wall_design_checks(
        &model,
        &[(rc, &response)],
        LoadTerm::Long,
        Some(&index),
        "case:0",
    );
    assert!(checks
        .iter()
        .filter(|w| w.elem == Some(rc))
        .all(|w| w.skip_kind == Some(WallSkipKind::NotApplicable)));
    let mut model = base;
    model.sections[0].material = Some(MaterialId(2));
    let checks = collect_wall_design_checks(
        &model,
        &[(rc, &response)],
        LoadTerm::Long,
        Some(&index),
        "case:0",
    );
    assert!(checks
        .iter()
        .filter(|w| w.elem == Some(rc))
        .all(|w| w.skip_kind == Some(WallSkipKind::NotImplemented)));
}
