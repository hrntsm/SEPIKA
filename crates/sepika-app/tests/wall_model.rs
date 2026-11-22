//! 壁を含む最小モデルの統合テスト（壁領域再設計 Step 7+8 着手前の安全網）。
//!
//! `cargo test -p sepika-app --test wall_model` で実行する。壁関連の型を変更した場合も確認する。
//!
//! # 目的
//!
//! 実フィクスチャ（`tests/fixtures/full_model.ovika`、`full_model.rs`）には壁要素が 0 件で、
//! 壁関連の機能（`WallAttr` の開口低減・自重、取り付く壁版の自重配分）は
//! いずれも単体テストでしか検証されていない。`Model.floor_regions`（26 件）を
//! 巻き込まない**独立した壁専用フィクスチャ**として、耐震壁 1 パネル＋フレーム外雑壁
//! 1 本を含む最小の立体架構を用意し、`App` の全解析入口を通した代表スカラを
//! ピン止めする。`WallRegion`/`WallAttr`/`WallPlate` の型を作り替える Step 7+8 の
//! 着手時、このテストの差分が「型変更が計算結果を変えていないか」の一次判定になる。
//!
//! # モデル
//!
//! 4m(X)×3m(Y)×3m(Z) の 1 スパン・1 層。柱 4 本（うち柱 0・1〔壁の側柱〕は RC、
//! 柱 2・3 は Box-300x300x10 鋼）・梁 8 本（うち id 4〔頂部 4-5〕・id 8〔柱脚間 0-1、
//! いずれも壁の上下大梁〕は RC、残り 6 本は H-400x200 鋼。柱脚どうし・柱頭どうしの
//! 両方で閉路。id 4-7 が頂部、id 8-11 が柱脚〔基礎大梁想定〕）・耐震壁 1 枚
//! （Y=0 面、RC t=150、開口 1 つ）・フレーム外雑壁 1 本（Y=3000 面の梁上端に沿う想定、
//! height=900・Column 伝達）。柱脚 4 節点は固定支点。荷重はテストモデルに直接設定するため
//! `App::run_preparation` の自動同期（`sync_auto_load_cases_action`）が
//! DL・LL・EX・EY を生成する。
//! 床板を持たない壁専用架構であり、水平な割当領域は版なしと明示する。
//!
//! **柱脚どうしの梁（基礎大梁）を持つ。** 2026-08 に新設した当初は柱頭側の梁のみで、
//! `region_gen::wall::scan_wall_region_boundaries` が境界を 1 つも検出できなかった
//! （各鉛直構面が「柱 2 本＋頂部の梁 1 本」の開いた U 字にしかならないため）。
//! 壁側 `region_gen` の実データ検証は実フィクスチャ（`full_model.rs`）の
//! 軸組で代替していたが、`WallRegion`/`WallPlate` の型実装（Step 7+8 本体）は
//! 本フィクスチャでの境界検出を前提にするため、柱脚どうしの梁を追加して 4 面とも
//! 閉じた矩形（4 節点ループ）にした（`test_region_gen_wall_finds_all_four_faces` で確認）。
//!
//! **柱脚間の梁を足したことで、耐震壁の側柱・上下大梁を RC にする必要が生じた。**
//! 壁エレメントは壁と周辺架構を一体の耐震要素としてモデル化するため、耐震壁の
//! 四周（上下大梁・左右側柱）の構造種別が壁自身と一致しない場合はエラーになる
//! （`wall_frame_category_issue`）。柱脚間の梁がなかった当初は壁が「四周を持つ」と
//! 判定されず（`wall_is_framed` が false）、このチェック自体が働いていなかった
//! （＝もともと本フィクスチャは、この検定の観点では気づかれずに不正な混合構造
//! だった）。柱 0・1・大梁 4・8 の材料を RC（`MaterialId(1)`）へ差し替えて是正した。
//!
//! **側柱・壁上下大梁は本物の実配筋矩形（300x300・300x400、
//! 主筋 3-D22・せん断補強筋 D10@100）を使う。** 以前は「鋼断面（H 形）の断面性能を
//! そのまま流用し材料だけ RC へ差し替える」トリックを使っていたが、これは
//! 実在しない断面（形状は鋼、材料は RC）で、増分解析（プッシュオーバー）・
//! 保有水平耐力の Ds 群回帰テスト（`snapshot_wall_response_and_ds_hold`）を
//! 追加しようとした際に破綻した（ファイバー断面が `shape` だけで鋼材と判定し
//! 材料の `fy` を要求してエラーになる、側柱の主筋がないと壁の等価引張鉄筋比 pte が
//! 算定できない）。本物の RC 矩形に差し替えたことで、線形の代表スカラ・
//! 固有周期も現実の RC フレームの値に変わった（後述）。
//!
//! 追加した基礎大梁は柱脚（固定支点）どうしをつなぐため、線形静解析・固有値解析の
//! いずれにも力・変位としては現れない（両端固定の部材は固定端間で力を伝達しない
//! ため）。ただし RC への材料差し替え（ヤング係数・密度が鋼と異なる）は側柱・
//! 上下大梁の剛性・自重を実際に変えるため、代表スカラは総じて動く（実測して
//! スナップショットを更新した。値を予想で決め打ちしていない）。
//!
//! 壁有無の周期比較は、X 方向に同じ節点質量を置いた別条件で行う。
//! 立体架構の代表周期はスナップショットで照合する。

use sepika_app::app::{App, StaticCaseKey};
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, WallPlateId};
use sepika_core::model::{
    AreaLoad, ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LoadTransfer,
    LocalAxis, Material, MaterialCategory, Model, NodalLoad, Node, RegionAnchor, RigidZone,
    Section, WallOpening, WallPlate, WallPlateShape,
};
use sepika_core::section_shape::{RcBeamRebar, RcRectColumnRebar, SectionShape};
use sepika_core::wall_region_rebuild::rebuild_wall_regions;
use sepika_solver::statics::analysis::SeismicDir;

#[path = "support/frame_gravity.rs"]
mod frame_gravity;
use frame_gravity::select_frame_gravity;

/// 有効数字 4 桁の指数表記（`full_model.rs::sig4` と同じ丸め規則。両ファイルの
/// スナップショットを比較できるよう合わせている）。
fn sig4(v: f64) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v:.3e}")
}

#[test]
fn rc_girder_self_weight_uses_columns_not_orthogonal_steel_girders() {
    let mut model = wall_bay_model();
    model.stories =
        sepika_load::story_gen::generate_stories_with_opts(&model, &[], false, model.mass_method)
            .unwrap()
            .stories;
    let (_, loads) =
        sepika_load::self_weight::self_weight_case_content(&model, &Default::default()).unwrap();
    for id in [ElemId(4), ElemId(8)] {
        let element = model.element(id).unwrap();
        let section = model.element_section(element).unwrap();
        let material = model.element_material(element).unwrap();
        let total: f64 = loads
            .iter()
            .filter(|load| {
                load.elem == id && load.source != sepika_core::model::LoadSource::WallSelfWeight
            })
            .map(|load| match load.kind {
                sepika_core::model::MemberLoadKind::Distributed { a, b, w1, w2 } => {
                    (b - a) * (w1 + w2) / 2.0
                }
                sepika_core::model::MemberLoadKind::Point { p, .. } => p,
            })
            .sum();
        let expected = material.design_unit_weight_n_per_mm3() * section.area * 3700.0;
        assert!(
            (total - expected).abs() < 1e-9,
            "{id:?}: {total} != {expected}"
        );
    }
}

/// 指定した静的結果を取り出す（`full_model.rs::static_of` と同じ規則）。
fn static_res_for(app: &App, key: StaticCaseKey) -> &sepika_solver::statics::linear::StaticOnce {
    &app.core
        .scoped
        .results
        .as_ref()
        .expect("解析結果が格納されているはず")
        .statics
        .iter()
        .find(|(k, _)| *k == key)
        .expect("指定した荷重ケースの静的結果")
        .1
}

/// 壁 1 パネル＋雑壁 1 本を含む、柱 4 本・梁 8 本の 1 スパン立体架構。
fn wall_bay_model() -> Model {
    let mut model = Model {
        stories: [0.0, 3000.0, 6000.0]
            .into_iter()
            .enumerate()
            .map(|(i, fl)| {
                sepika_core::model::Story::new(
                    sepika_core::ids::StoryId(i as u32),
                    format!("{}F", i + 1),
                    Some(fl),
                    Some(0.0),
                )
            })
            .collect(),
        ..Default::default()
    };

    let base = [
        [0.0, 0.0, 0.0],
        [4000.0, 0.0, 0.0],
        [4000.0, 3000.0, 0.0],
        [0.0, 3000.0, 0.0],
    ];
    let top = [
        [0.0, 0.0, 3000.0],
        [4000.0, 0.0, 3000.0],
        [4000.0, 3000.0, 3000.0],
        [0.0, 3000.0, 3000.0],
    ];
    for (i, c) in base.iter().enumerate() {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord: *c,
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, c) in top.iter().enumerate() {
        model.nodes.push(Node {
            id: NodeId(4 + i as u32),
            coord: *c,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
    }

    let col_shape = SectionShape::SteelBox {
        height: 300.0,
        width: 300.0,
        thick: 10.0,
        horizontal_thick: 10.0,
        corner_r: Some(13.0),
    };
    let beam_shape = SectionShape::SteelH {
        root_r: Some(13.0),
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
    };
    let mut col_section = col_shape.to_section(SectionId(0), "柱 Box-300x300x10 R13".into());
    col_section.frame_use = Some(FrameSectionUse::Column);
    model.sections.push(col_section);
    let mut beam_section = beam_shape.to_section(SectionId(1), "梁 H-400x200x8x13".into());
    beam_section.frame_use = Some(FrameSectionUse::Girder);
    model.sections.push(beam_section);
    // shapeがなければ壁ランク判定がスキップされ、壁固有のDs経路を検証できない。
    // 壁の面内剛性は板厚・配筋比・節点座標で組み立てるため、線材の断面性能値は使わない。
    model.sections.push(Section {
        frame_use: None,
        id: SectionId(2),
        name: "耐震壁 t150".into(),
        area: 150.0 * 3000.0,
        iy: 1.0e9,
        iz: 1.0e9,
        j: 1.0e9,
        depth: 3000.0,
        width: 150.0,
        as_y: 1.0e5,
        as_z: 1.0e5,
        floor: None,
        panel_thickness: None,
        thickness: Some(150.0),
        shape: Some(SectionShape::RcWall {
            thickness: 150.0,
            pwh_ratio: Some(0.0025),
            pwv_ratio: Some(0.0025),
            ps: 0.0025,
        }),
        material: Some(MaterialId(1)),
        rebar_material: Some(MaterialId(2)),
        shear_rebar_material: Some(MaterialId(2)),
        steel_material: None,
        property_basis: Default::default(),
    });
    // 壁と四周の構造種別が一致しなければ耐震要素として扱えないため、周辺架構もRCとする。
    // 側柱の主筋がなければ壁の等価引張鉄筋比を算定できないため、RC実配筋断面を使う。
    let side_column_rebar = RcRectColumnRebar {
        main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
        x: vec![3],
        y: vec![2],
        cover: 40.0,
        hoop: sepika_core::section_shape::RectColumnHoop {
            dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    let wall_girder_rebar = RcBeamRebar {
        main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
        top: vec![3],
        bottom: vec![3],
        cover: 40.0,
        stirrup: sepika_core::section_shape::BeamStirrup {
            dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
            pitch: 100.0,
            legs: 2,
        },
    };
    let mut side_column_section = SectionShape::RcColumnRect {
        b: 300.0,
        d: 300.0,
        rebar: side_column_rebar,
    }
    .to_section(SectionId(3), "側柱 RC 300x300".into());
    side_column_section.frame_use = Some(FrameSectionUse::Column);
    side_column_section.material = Some(MaterialId(1));
    side_column_section.rebar_material = Some(MaterialId(2));
    side_column_section.shear_rebar_material = Some(MaterialId(2));
    model.sections.push(side_column_section);
    let mut wall_girder_section = SectionShape::RcBeamRect {
        b: 300.0,
        d: 400.0,
        rebar: wall_girder_rebar,
    }
    .to_section(SectionId(4), "壁上下大梁 RC 300x400".into());
    wall_girder_section.frame_use = Some(FrameSectionUse::Girder);
    wall_girder_section.material = Some(MaterialId(1));
    wall_girder_section.rebar_material = Some(MaterialId(2));
    wall_girder_section.shear_rebar_material = Some(MaterialId(2));
    model.sections.push(wall_girder_section);

    model.materials.push(Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
    });
    model.materials.push(Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(1),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 23000.0,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.0),
        fy: None,
    });
    model.materials.push(Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(2),
        name: "SD345".into(),
        category: MaterialCategory::Rebar,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(345.0),
    });
    model.sections.push(Section {
        frame_use: None,
        id: SectionId(5),
        name: "パラペット t120".into(),
        area: 120.0 * 900.0,
        iy: 1.0e8,
        iz: 1.0e8,
        j: 1.0e8,
        depth: 900.0,
        width: 120.0,
        as_y: 1.0e4,
        as_z: 1.0e4,
        floor: None,
        panel_thickness: None,
        thickness: Some(120.0),
        shape: Some(SectionShape::RcWall {
            thickness: 120.0,
            pwh_ratio: Some(0.0025),
            pwv_ratio: Some(0.0025),
            ps: 0.0025,
        }),
        material: Some(MaterialId(1)),
        rebar_material: Some(MaterialId(2)),
        shear_rebar_material: Some(MaterialId(2)),
        steel_material: None,
        property_basis: Default::default(),
    });
    for sec in model.sections.iter_mut().take(2) {
        sec.material = Some(MaterialId(0));
    }

    let beam = |id: u32, i: u32, j: u32, sec: u32, ref_v: [f64; 3]| ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: [NodeId(i), NodeId(j)].into_iter().collect(),
        section: Some(SectionId(sec)),
        local_axis: LocalAxis { ref_vector: ref_v },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone::default(),
        end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
        plastic_zone: None,
        spring: None,
    };
    for i in 0..4u32 {
        let sec = if i < 2 { 3 } else { 0 };
        model.elements.push(beam(i, i, 4 + i, sec, [1.0, 0.0, 0.0]));
    }
    let beam_pairs = [(4u32, 5u32), (5, 6), (6, 7), (7, 4)];
    for (k, (i, j)) in beam_pairs.iter().enumerate() {
        let sec = if k == 0 { 4 } else { 1 };
        model
            .elements
            .push(beam(4 + k as u32, *i, *j, sec, [0.0, 0.0, 1.0]));
    }
    // 柱頭梁だけでは開いたU字となり壁境界を検出できないため、固定柱脚間にも梁を置く。
    let base_beam_pairs = [(0u32, 1u32), (1, 2), (2, 3), (3, 0)];
    for (k, (i, j)) in base_beam_pairs.iter().enumerate() {
        let sec = if k == 0 { 4 } else { 1 };
        model
            .elements
            .push(beam(8 + k as u32, *i, *j, sec, [0.0, 0.0, 1.0]));
    }

    // 壁解析要素のIDは先に生成した仕口パネル数に依存するため、固定値では参照できない。
    model.add_enclosed_wall_plate_from_nodes(
        &[NodeId(0), NodeId(1), NodeId(5), NodeId(4)],
        WallPlate {
            dl_support: Some(sepika_core::model::WallDlSupport::HeightMidpoint),
            self_weight_shares: Vec::new(),
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(2)),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![WallOpening {
                width: 900.0,
                height: 1200.0,
                offset: Some([1550.0, 500.0]),
            }],
            loads: vec![],
            slit: Default::default(),
        },
    );

    // 屋上パラペットの階帯上端を覆う階レベルを明示する（未定義端帯は推測しない）。
    model.nodes.push(Node {
        id: NodeId(model.nodes.len() as u32),
        coord: [0.0, 0.0, 6000.0],
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    });
    // 面荷重ゼロだけの入力では仕上げ・増打ちの自重算入漏れを検出できない。
    model.wall_plates.push(WallPlate {
        dl_support: None,
        self_weight_shares: Vec::new(),
        id: WallPlateId(1),
        shape: WallPlateShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(6), NodeId(7)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Columns,
            },
            extent: Some([900.0, 900.0]),
        },
        section: Some(SectionId(5)),
        opening_area: 0.0,
        opening_weight: 0.0,
        openings: vec![],
        loads: vec![AreaLoad {
            kind: "仕上げ".into(),
            value: 3.0e-4,
        }],
        slit: Default::default(),
    });

    let report = rebuild_wall_regions(&mut model);
    assert_eq!(report.regions, 4, "1 スパンの 4 鉛直構面すべてが検出される");
    assert_eq!(
        report.wall_plates_assigned, 1,
        "壁版が Y=0 面の壁領域へ帰属する"
    );
    assert_eq!(report.unassigned_wall_plates, 0);

    // 壁の応答・Dsを検証する周辺梁は、分布荷重を扱う独立の標準型骨格を使う。
    for id in [ElemId(4), ElemId(8)] {
        model.set_member_hysteresis(id, sepika_core::model::HysteresisModel::Standard);
    }

    let mut vertical = model.materials[2].clone();
    vertical.id = MaterialId(3);
    vertical.strength_factor = Some(1.0);
    let mut horizontal = vertical.clone();
    horizontal.id = MaterialId(4);
    horizontal.name = "SD295A".into();
    horizontal.fy = Some(295.0);
    model.materials.extend([vertical, horizontal]);
    for section in &mut model.sections {
        if matches!(section.shape, Some(SectionShape::RcWall { .. })) {
            section.rebar_material = Some(MaterialId(3));
            section.shear_rebar_material = Some(MaterialId(4));
        }
    }
    model.rebuild_floor_assignment_regions();
    for region in &mut model.floor_assignment_regions.regions {
        region.assignment = sepika_core::model::PlateAssignment::NoPlate;
    }
    model
}

fn wall_bay_app() -> App {
    let mut app = App::default();
    app.core.analysis_cfg.threads = 1;
    app.core.model = wall_bay_model();
    // 単層の構造解析例には未定義上端帯の屋上突出壁を含めない。
    // Attachedの実帯・質量は独立多層fixtureで照合する。
    app.core
        .model
        .wall_plates
        .retain(|p| p.id != WallPlateId(1));
    app.core.model.nodes.retain(|n| n.coord[2] != 6000.0);
    app.core.model.stories.truncate(2);
    // 既定のSteelFrameではRC壁のDs表を通らないため、架構種別を明示する。
    app.core.design_frame = sepika_design_jp::secondary::holding_capacity::FrameType::RcWall;
    app
}

fn unperforated_wall_bay_app() -> App {
    let mut app = wall_bay_app();
    app.core.model.panel_zone = sepika_core::model::PanelZoneMode::None;
    let plate = &mut app.core.model.wall_plates[0];
    plate.openings.clear();
    assert_eq!(plate.opening_area, 0.0);
    assert_eq!(plate.opening_weight, 0.0);
    app
}

fn balance_ex_floor_rotation(model: &mut Model) {
    let story = model.layers()[0].top;
    let master = model.diaphragms_of(story).next().unwrap().master;
    let ex = model
        .load_cases
        .iter()
        .find(|case| case.name == sepika_core::model::EX_CASE_NAME)
        .unwrap()
        .id;
    let x = sepika_job::compute::compute_linear_static(model.clone(), ex).unwrap();
    let mut torsion_model = sepika_core::wall_expand::expand_wall_elements(model).0;
    let moment_nmm = 1_000_000.0;
    let case = &mut torsion_model.load_cases[ex.index()];
    case.member.clear();
    case.nodal = vec![NodalLoad::manual(
        master,
        [0.0, 0.0, 0.0, 0.0, 0.0, moment_nmm],
    )];
    let torsion = sepika_solver::statics::linear::linear_static_once(&torsion_model, ex).unwrap();
    let balance_nmm = -moment_nmm * x.disp[master.index()][5] / torsion.disp[master.index()][5];
    model.load_cases[ex.index()].nodal.push(NodalLoad::manual(
        master,
        [0.0, 0.0, 0.0, 0.0, 0.0, balance_nmm],
    ));
}

#[test]
fn nonlinear_opening_wall_refuses_job_without_publishing_partial_results() {
    for time_history in [false, true] {
        let mut app = wall_bay_app();
        app.core.model.panel_zone = sepika_core::model::PanelZoneMode::None;
        app.run_preparation();
        select_frame_gravity(&mut app.core.model);
        app.run_static_all();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let before = serde_json::to_vec(&app.core.scoped.results).unwrap();
        let (expanded, index, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
        let wall = expanded
            .elements
            .iter()
            .find(|element| index.plate_of(element.id) == Some(WallPlateId(0)))
            .unwrap()
            .id
            .0;
        if time_history {
            app.core.analysis_cfg.th_nonlinear = true;
            app.core.analysis_cfg.th_duration = 0.04;
            app.run_time_history_sample();
        } else {
            app.run_pushover();
        }
        let error = app.core.scoped.last_error.as_ref().unwrap();
        if time_history {
            assert!(
                error.contains(&format!("壁 ID [{wall}]")) && error.contains("従属回転の慣性"),
                "{error}"
            );
        } else {
            assert!(
                error.contains(&format!("壁 ID {wall}")) && error.contains("開口壁は未対応"),
                "{error}"
            );
        }
        assert_eq!(
            serde_json::to_vec(&app.core.scoped.results).unwrap(),
            before
        );
    }
}

/// GUI診断（`App::run_diagnostics`）が壁展開モデル（D5・dig Q4）を見ていることの回帰
/// テスト。壁要素は `run_diagnostics` の内部で `expand_wall_elements` により初めて
/// `model.elements` へ現れるため、この展開が壊れる（または元の `self.model` を渡す
/// 実装に後退する）と、`model.elements` に壁要素が 0 件のまま
/// `wall_frame_category_issue`（耐震壁と周辺架構の構造種別食い違い）が一度も
/// 呼ばれず、壁の不備が診断タブから静かに消える（解析実行時に初めて気づく劣化になる。
/// §5.14 dig Q4 参照）。
#[test]
fn test_wall_frame_mismatch_appears_in_gui_diagnostics() {
    let mut model = wall_bay_model();
    model.sections[3].material = Some(MaterialId(0));

    let mut app = App::default();
    app.core.analysis_cfg.threads = 1;
    app.core.model = model;
    app.run_diagnostics();

    assert!(
        app.core
            .scoped
            .diagnostics
            .iter()
            .any(|d| d.message.contains("耐震壁") && d.message.contains("構造種別")),
        "壁展開モデルを経由しないと、model.elements の壁要素が 0 件のため \
         この構造種別食い違いを検知できないはず: {:?}",
        app.core
            .scoped
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn undefined_side_column_shape_is_an_input_error() {
    let mut app = wall_bay_app();
    app.core.model.sections[3].shape = None;
    app.core.model.sections[3].property_basis.area = sepika_core::model::PropertyBasis::Supplied;
    app.core.model.sections[3].property_basis.j = sepika_core::model::PropertyBasis::Supplied;
    app.run_diagnostics();
    assert!(app
        .core
        .scoped
        .diagnostics
        .iter()
        .any(|d| d.message.contains("側柱") && d.message.contains("断面形状が未定義")));
}

#[test]
fn wall_analysis_preserves_side_column_shapes_with_long_axial_cut() {
    let mut app = wall_bay_app();
    app.core.model.stress_cfg.no_long_axial_column = true;
    app.run_preparation();
    app.run_static_all();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.core.scoped.results.is_some());
    assert!(app.core.model.sections[3].shape.is_some());
}

#[test]
fn circular_side_column_shear_section_runs_analysis() {
    let mut app = wall_bay_app();
    let rebar = sepika_core::section_shape::RcCircleColumnRebar {
        main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
        count: 8,
        cover: 40.0,
        hoop: sepika_core::section_shape::CircleColumnHoop {
            dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
            pitch: 100.0,
        },
    };
    let old = &app.core.model.sections[3];
    let mut section =
        SectionShape::RcColumnCircle { d: 600.0, rebar }.to_section(old.id, old.name.clone());
    section.floor = old.floor.clone();
    section.frame_use = old.frame_use;
    section.material = old.material;
    section.rebar_material = old.rebar_material;
    section.shear_rebar_material = old.shear_rebar_material;
    app.core.model.sections[3] = section;
    app.run_preparation();
    app.run_static_all();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    assert!(app.core.scoped.results.is_some());
}

#[test]
fn rotated_src_and_cft_side_columns_run_linear_analysis() {
    for cft in [false, true] {
        let mut app = wall_bay_app();
        let rebar = app.core.model.sections[3]
            .shape
            .as_ref()
            .unwrap()
            .rect_column_rebar()
            .unwrap()
            .clone();
        let shape = if cft {
            SectionShape::CftBox {
                corner_r: Some(32.0),
                height: 700.0,
                width: 500.0,
                thick: 20.0,
                horizontal_thick: 20.0,
            }
        } else {
            SectionShape::SrcColumnRect {
                b: 500.0,
                d: 700.0,
                rebar,
                steel_height: 400.0,
                steel_width: 250.0,
                steel_web_thick: 12.0,
                steel_flange_thick: 20.0,
            }
        };
        let old = app.core.model.sections[3].clone();
        let mut section = shape.to_section(old.id, old.name.clone());
        section.frame_use = old.frame_use;
        section.floor = old.floor;
        section.material = old.material;
        section.rebar_material = old.rebar_material;
        section.shear_rebar_material = old.shear_rebar_material;
        section.steel_material = Some(MaterialId(0));
        app.core.model.sections[3] = section;
        if cft {
            let mut material = app.core.model.materials[0].clone();
            material.id = MaterialId(app.core.model.materials.len() as u32);
            material.fc = Some(30.0);
            material.category = MaterialCategory::Concrete;
            material.name = "Fc30".into();
            material.young = sepika_core::section_shape::concrete_young_modulus(30.0);
            material.density = 2.4e-9;
            app.core.model.sections[3].material = Some(material.id);
            app.core.model.materials.push(material);
        }
        for e in &mut app.core.model.elements {
            if e.section == Some(SectionId(3)) {
                e.local_axis.ref_vector = [0.8, 0.6, 0.0];
            }
        }
        app.run_preparation();
        app.run_static_all();
        assert!(
            app.core.scoped.last_error.is_none(),
            "CFT={cft}: {:?}",
            app.core.scoped.last_error
        );
        assert!(app.core.scoped.results.is_some());
    }
}

#[test]
fn wall_fixture_weight_change_is_the_independent_fillet_area_increment() {
    let mut actual = wall_bay_app();
    let mut baseline = wall_bay_app();
    let mut area_deltas = std::collections::HashMap::new();
    for section in &mut baseline.core.model.sections {
        if let Some(SectionShape::SteelH {
            height,
            width,
            web_thick,
            flange_thick,
            root_r: Some(r),
        }) = section.shape
        {
            let old_area = 2.0 * width * flange_thick + (height - 2.0 * flange_thick) * web_thick;
            let delta = (4.0 - std::f64::consts::PI) * r * r;
            assert!((section.area - old_area - delta).abs() < 1.0e-8);
            area_deltas.insert(section.id, delta);
            section.area = old_area;
            section.property_basis.area = sepika_core::model::PropertyBasis::Supplied;
        }
    }
    let expected: f64 = actual
        .core
        .model
        .elements
        .iter()
        .filter_map(|elem| {
            let share = elem
                .nodes
                .iter()
                .filter(|id| actual.core.model.nodes[id.index()].coord[2] > 0.0)
                .count() as f64
                / elem.nodes.len() as f64;
            Some(
                area_deltas.get(&elem.section?)?
                    * actual.core.model.member_length(elem)
                    * 78.5e-6
                    * share,
            )
        })
        .sum();
    actual.run_preparation();
    baseline.run_preparation();
    assert!(actual.core.scoped.last_error.is_none());
    assert!(baseline.core.scoped.last_error.is_none());
    let new_weight = actual
        .core
        .scoped
        .preparation
        .as_ref()
        .unwrap()
        .summary
        .total_seismic_weight;
    let old_weight = baseline
        .core
        .scoped
        .preparation
        .as_ref()
        .unwrap()
        .summary
        .total_seismic_weight;
    assert!(
        (new_weight - old_weight - expected).abs() < 1.0e-7,
        "角丸={new_weight}, 直角={old_weight}, 実差={}, 独立差={expected}, cfg={:?}",
        new_weight - old_weight,
        actual.core.model.load_cfg
    );
    eprintln!("壁モデルの総重量: 角丸={new_weight:.12e} N, 直角面積={old_weight:.12e} N, 独立差={expected:.12e} N");
}

#[test]
fn wall_fixture_closed_columns_keep_real_properties_and_four_panel_connections() {
    let mut model = wall_bay_model();
    let section = &model.sections[0];
    let expected_area_mm2 = 300.0_f64.powi(2)
        - 280.0_f64.powi(2)
        - (4.0 - std::f64::consts::PI) * (13.0_f64.powi(2) - 3.0_f64.powi(2));
    assert!((section.area - expected_area_mm2).abs() < 1e-8);
    let mut reference_iy_mm4 = 0.0;
    let mut reference_iz_mm4 = 0.0;
    for (lo_mm, hi_mm) in [(0.0, 137.0), (137.0, 140.0), (140.0, 150.0)] {
        let dz_mm = (hi_mm - lo_mm) / 20_000.0;
        for slice in 0..20_000 {
            let z_mm = lo_mm + (f64::from(slice) + 0.5) * dz_mm;
            let outer_width_mm = if z_mm <= 137.0 {
                300.0
            } else {
                2.0 * (137.0 + (169.0 - (z_mm - 137.0).powi(2)).sqrt())
            };
            let inner_width_mm = if z_mm >= 140.0 {
                0.0
            } else if z_mm <= 137.0 {
                280.0
            } else {
                2.0 * (137.0 + (9.0 - (z_mm - 137.0).powi(2)).sqrt())
            };
            reference_iy_mm4 += 2.0 * (outer_width_mm - inner_width_mm) * z_mm.powi(2) * dz_mm;
            reference_iz_mm4 +=
                2.0 * (outer_width_mm.powi(3) - inner_width_mm.powi(3)) / 12.0 * dz_mm;
        }
    }
    assert!((section.iy / reference_iy_mm4 - 1.0).abs() < 2e-6);
    assert!((section.iz / reference_iz_mm4 - 1.0).abs() < 2e-6);
    let panels = sepika_element::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
    assert_eq!(
        panels.iter().map(|panel| panel.node).collect::<Vec<_>>(),
        vec![NodeId(2), NodeId(3), NodeId(6), NodeId(7)]
    );
    for panel in &panels {
        assert_eq!(panel.ve, [2_244_600.0; 2]);
    }
    for id in [2, 3] {
        assert_eq!(model.elements[id].rigid_zone.panel_offset_i, 200.0);
        assert_eq!(model.elements[id].rigid_zone.panel_offset_j, 200.0);
    }
    for (id, ends) in [
        (5, [0.0, 150.0]),
        (6, [150.0, 150.0]),
        (7, [150.0, 0.0]),
        (9, [0.0, 150.0]),
        (10, [150.0, 150.0]),
        (11, [150.0, 0.0]),
    ] {
        assert_eq!(
            [
                model.elements[id].rigid_zone.panel_offset_i,
                model.elements[id].rigid_zone.panel_offset_j
            ],
            ends
        );
    }
    assert_eq!(model.wall_plates.len(), 2);
    assert_eq!(
        model
            .elements
            .iter()
            .filter(|element| element.kind == ElementKind::Beam)
            .count(),
        12
    );
}

/// `App::run_design_check` が壁展開モデルを見ていることの回帰テスト。
/// `run_member_design_checks`（内部の `joint_wiring::wall::check_walls`）は `results.member_forces` の壁 `ElemId` を `model.element` で引き直すため、
/// `self.model`（壁展開前）をそのまま渡すと該当 `ElemId` が見つからず
/// `continue` し、耐震壁のせん断断面検定が常にスキップされる。
#[test]
fn test_wall_shear_check_appears_after_run_design_check() {
    let mut app = wall_bay_app();
    let wall_section = &mut app.core.model.sections[2];
    wall_section.rebar_material = Some(MaterialId(2));
    wall_section.shear_rebar_material = Some(MaterialId(2));
    if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut wall_section.shape {
        *pwh_ratio = Some(0.0025);
    }
    app.run_preparation();
    assert!(
        app.core.scoped.last_error.is_none(),
        "準備計算: {:?}",
        app.core.scoped.last_error
    );
    let live_id = sepika_core::ids::LoadCaseId(
        app.core
            .model
            .load_cases
            .iter()
            .map(|c| c.id.0)
            .max()
            .unwrap()
            + 1,
    );
    app.core
        .model
        .load_cases
        .push(sepika_core::model::LoadCase {
            origin: Default::default(),
            id: live_id,
            name: "無載荷の架構用P".into(),
            kind: sepika_core::model::LoadCaseKind::Live,
            nodal: vec![],
            member: vec![],
        });
    app.auto_generate_combinations_action();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.run_static_all();
    assert!(
        app.core.scoped.last_error.is_none(),
        "静的解析: {:?}",
        app.core.scoped.last_error
    );
    app.select_displayed_result(sepika_app::app::StaticKey::Combo(1));
    assert_eq!(
        app.core.scoped.last_static,
        Some(sepika_app::app::StaticKey::Combo(1))
    );
    app.run_design_check();

    let results = app
        .core
        .scoped
        .results
        .as_ref()
        .expect("解析結果が格納されているはず");
    assert!(
        results
            .wall_checks
            .iter()
            .any(|w| w.plate == Some(WallPlateId(0))
                && w.seismic_target
                && w.elem.is_some()
                && !w.case.is_empty()
                && w.kind == sepika_design_jp::wall_check::WallCheckKind::AllowableShear
                && matches!(w.outcome, sepika_design_jp::CheckOutcome::Checked(_))),
        "{:?}",
        results.wall_checks
    );
}

/// 代表スカラのスナップショット（有効数字 4 桁）。
///
/// `full_model.rs::snapshot_key_scalars` と同じ趣旨。壁関連の型（`WallRegion`・
/// `WallAttr`・`WallPlate`）を作り替える際、この値の変化を V&V の対象とすること。
#[test]
fn snapshot_wall_bay_scalars() {
    let mut app = wall_bay_app();
    app.run_preparation();
    app.core.scoped.last_error = None;
    app.run_static_all();
    app.run_eigen(app.core.analysis_cfg.n_modes);

    let mut out = String::new();
    let mut line = |k: &str, v: String| {
        out.push_str(k);
        out.push_str(" = ");
        out.push_str(&v);
        out.push('\n');
    };

    let prep = app
        .core
        .scoped
        .preparation
        .as_ref()
        .expect("準備計算の結果");
    line(
        "prep.total_seismic_weight",
        sig4(prep.summary.total_seismic_weight),
    );

    let modal = app
        .core
        .scoped
        .results
        .as_ref()
        .expect("解析結果")
        .modal
        .as_ref()
        .expect("固有値");
    for (i, t) in modal.period.iter().enumerate() {
        line(&format!("eigen.T[{i}]"), sig4(*t));
    }

    // 壁要素IDは仕口パネル生成後の要素数に依存するため、実際に壁展開してIDを解決する。
    let (_expanded_for_id, wall_index, wall_expand_report) =
        sepika_core::wall_expand::expand_wall_elements(&app.core.model);
    assert_eq!(wall_expand_report.generated, 1, "壁要素が1件生成されるはず");
    let wall_elem_id = wall_index
        .generated_elem_ids()
        .next()
        .expect("生成された壁要素の ElemId");
    let wall_member_forces = |res: &sepika_solver::statics::linear::StaticOnce| {
        res.member_forces
            .iter()
            .find(|(id, _)| *id == wall_elem_id)
            .expect("耐震壁要素の材端力")
            .1
            .at
            .clone()
    };

    // 対称な架構の鉛直載荷では壁内力がほぼゼロとなるため、面内せん断・開口低減の代表値はEXで確認する。
    let dl_key = app
        .core
        .model
        .load_cases
        .iter()
        .find(|c| c.kind == sepika_core::model::LoadCaseKind::Dead)
        .map(|c| c.id)
        .expect("DL ケース");
    let dl_res = static_res_for(&app, StaticCaseKey::User(dl_key));
    let wall_forces = wall_member_forces(dl_res);
    line("static.DL.wall_forces_len", wall_forces.len().to_string());
    let dl_force_sum: f64 = wall_forces
        .iter()
        .flat_map(|(_, f)| f.iter())
        .map(|v| v.abs())
        .sum();
    line("static.DL.wall_forces_abs_sum", sig4(dl_force_sum));

    let ex_res = static_res_for(&app, StaticCaseKey::Seismic(SeismicDir::X));
    let ex_wall_forces = wall_member_forces(ex_res);
    let ex_force_sum: f64 = ex_wall_forces
        .iter()
        .flat_map(|(_, f)| f.iter())
        .map(|v| v.abs())
        .sum();
    line("static.EX.wall_forces_abs_sum", sig4(ex_force_sum));
    line("static.EX.top_disp_x[node4]", sig4(ex_res.disp[4][0]));

    insta::assert_snapshot!(out);
}

#[test]
fn enclosed_wall_shortens_period_with_equal_x_mass() {
    use sepika_core::dof::{Dof, DofMap, DOF_PER_NODE};
    use sepika_core::ids::StoryId;
    use sepika_core::model::{
        Constraint, LoadCase, LoadCaseKind, MassMethod, NodalLoad, PanelZoneMode, PlateAssignment,
    };
    use sepika_element::behavior::MassOption;
    use sepika_solver::common::{assemble::assemble_global_m, constraint::Reducer};

    const TOP: NodeId = NodeId(4);
    const TOP_SLAVES: [NodeId; 3] = [NodeId(5), NodeId(6), NodeId(7)];
    const LOAD: LoadCaseId = LoadCaseId(0);
    const MASS_T: f64 = 2.0;
    const FORCE_X_N: f64 = 1000.0;

    let mut with_wall = wall_bay_app();
    with_wall.core.analysis_cfg.mass_method = MassMethod::LumpedOnly;
    let model = &mut with_wall.core.model;
    model.mass_method = MassMethod::LumpedOnly;
    model.panel_zone = PanelZoneMode::None;
    for node in &mut model.nodes {
        node.restraint = Dof6Mask::FIXED;
    }
    for node in [TOP].into_iter().chain(TOP_SLAVES) {
        let restraint = &mut model.nodes[node.index()].restraint;
        restraint.set_free(Dof::Ux);
        // 階地震力を伝える剛床では、直交方向の水平拘束も解析前チェックで拒否される。
        restraint.set_free(Dof::Uy);
    }
    // 自動生成した剛床代表節点には壁重量を含む質量が入るため、代表節点を直接指定する。
    model.constraints.push(Constraint::rigid_diaphragm(
        StoryId(1),
        TOP,
        TOP_SLAVES.to_vec(),
    ));
    model.nodes[TOP.index()].mass = Some([MASS_T, 0.0, 0.0, 0.0, 0.0, 0.0]);
    model.load_cases.push(LoadCase {
        id: LOAD,
        name: "周期比較用のX方向節点力".into(),
        // 長期荷重では設定により基準の軸剛性が変わるため、短期の荷重区分を使う。
        kind: LoadCaseKind::Wind,
        nodal: vec![NodalLoad {
            node: TOP,
            values: [FORCE_X_N, 0.0, 0.0, 0.0, 0.0, 0.0],
            name: String::new(),
            source: Default::default(),
        }],
        member: vec![],
        origin: Default::default(),
    });
    assert_eq!(model.wall_plates.len(), 1);
    assert_eq!(model.wall_plates[0].id, WallPlateId(0));
    assert_eq!(model.wall_plates[0].shape, WallPlateShape::Enclosed);

    let mut without_wall = wall_bay_app();
    without_wall.core.analysis_cfg.mass_method = MassMethod::LumpedOnly;
    without_wall.core.model = model.clone();
    without_wall
        .core
        .model
        .retain_wall_plates(|plate| plate.id != WallPlateId(0));
    assert!(without_wall.core.model.wall_plates.is_empty());
    let mut non_wall_input = model.clone();
    non_wall_input.wall_plates.clear();
    for region in &mut non_wall_input.wall_regions {
        region.wall_plate_ids.retain(|&id| id != WallPlateId(0));
    }
    for region in &mut non_wall_input.wall_assignment_regions.regions {
        if region.assignment == PlateAssignment::Plate(WallPlateId(0)) {
            region.assignment = PlateAssignment::Unset;
        }
    }
    assert!(
        non_wall_input.eq_ignoring_dofmap(&without_wall.core.model),
        "耐震壁版とその参照を除いたモデル入力は一致する"
    );

    let observe = |app: &mut App| {
        app.run_preparation();
        assert_eq!(app.core.scoped.last_error, None, "準備計算");
        assert_eq!(app.core.model.mass_method, MassMethod::LumpedOnly);

        let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
        let dofmap = DofMap::build(&expanded);
        let reducer = Reducer::build(&expanded, &dofmap);
        assert_eq!(reducer.n_indep, 2);
        let [x, y] = [Dof::Ux, Dof::Uy].map(|dof| {
            let active = dofmap
                .active(TOP.index() * DOF_PER_NODE + dof as usize)
                .unwrap();
            let [(independent, coefficient)] = reducer.t_rows[active as usize].as_slice() else {
                panic!("剛床代表節点の {dof:?} が独立していない");
            };
            assert_eq!(*coefficient, 1.0);
            *independent
        });
        assert_ne!(x, y);
        let mass = assemble_global_m(&expanded, &dofmap, MassOption::Consistent).unwrap();
        let mass = reducer.reduce_k(&mass);
        let mass_at = |i, j| mass.get(i, j).copied().unwrap_or(0.0);
        assert_eq!(
            [mass_at(x, x), mass_at(x, y), mass_at(y, x), mass_at(y, y)],
            [MASS_T, 0.0, 0.0, 0.0]
        );

        app.run_linear_static(LOAD);
        assert_eq!(app.core.scoped.last_error, None, "静的解析");
        let disp = &static_res_for(app, StaticCaseKey::User(LOAD)).disp;
        let ux_mm = disp[TOP.index()][0];
        for node in TOP_SLAVES {
            assert_eq!(&disp[node.index()][..2], &disp[TOP.index()][..2]);
        }
        assert!(ux_mm.is_finite() && ux_mm > 0.0, "X方向変位 {ux_mm}");

        app.run_eigen(1);
        assert_eq!(app.core.scoped.last_error, None, "固有値解析");
        let modal = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .modal
            .as_ref()
            .unwrap();
        let [period_s] = modal.period.as_slice() else {
            panic!("X方向の1モードが必要: {:?}", modal.period);
        };
        let [[mass_x_t, mass_y_t, mass_z_t]] = modal.effective_mass.as_slice() else {
            panic!("X方向の1モードが必要: {:?}", modal.effective_mass);
        };
        assert!(
            (mass_x_t / MASS_T - 1.0).abs() < 1e-6,
            "X方向の解析質量 {mass_x_t} t"
        );
        assert_eq!([*mass_y_t, *mass_z_t], [0.0; 2]);

        let stiffness_n_per_mm = FORCE_X_N / ux_mm;
        let expected_period_s = std::f64::consts::TAU * (MASS_T / stiffness_n_per_mm).sqrt();
        assert!(
            (period_s / expected_period_s - 1.0).abs() < 1e-6,
            "周期 {period_s} s、静的柔性と節点質量からの期待値 {expected_period_s} s"
        );
        (stiffness_n_per_mm, *period_s)
    };

    let (k_with, t_with) = observe(&mut with_wall);
    let (k_without, t_without) = observe(&mut without_wall);
    assert!(
        k_with > k_without,
        "X方向剛性の増加: {k_with} > {k_without} N/mm"
    );
    assert!(
        t_with < t_without,
        "X方向周期の短縮: {t_with} < {t_without} s"
    );
}

/// 本フィクスチャの 4 つの鉛直構面（X=0・X=4000・Y=0〔壁面〕・Y=3000〔雑壁面〕）が、
/// いずれも `region_gen::wall` で閉じた矩形として検出できることの確認。
///
/// 柱脚どうしの梁を追加する前は「柱 2 本＋頂部の梁 1 本」の開いた U 字にしかならず
/// 境界を 1 つも検出できなかった（モジュール doc 参照）。`WallRegion`/`WallPlate` の
/// 型実装（Step 7+8 本体）は本フィクスチャでの境界検出を前提にするため、退行しないよう
/// 面数・面積を固定する。
#[test]
fn test_region_gen_wall_finds_all_four_faces() {
    use sepika_core::region_gen::scan_wall_region_boundaries;

    let model = wall_bay_model();
    assert!(model.validate().is_ok(), "{:?}", model.validate());
    assert!(
        model.elements.iter().all(|e| e.kind != ElementKind::Wall),
        "壁要素は準備計算・出力前の生成物（D5）で、入力モデルには含まれない"
    );
    let scan = scan_wall_region_boundaries(&model);
    assert_eq!(scan.unclosed, 0, "半辺の後続は一意に定まるはず");
    assert_eq!(
        scan.boundaries.len(),
        4,
        "4 つの鉛直構面すべてが閉じた矩形になるはず"
    );

    let mut areas: Vec<f64> = scan.boundaries.iter().map(|b| b.area(&model)).collect();
    areas.sort_by(f64::total_cmp);
    // X=0・X=4000 面（3m×3m）が 2 面、Y=0・Y=3000 面（4m×3m）が 2 面。
    let expected = [
        3000.0 * 3000.0,
        3000.0 * 3000.0,
        4000.0 * 3000.0,
        4000.0 * 3000.0,
    ];
    for (a, e) in areas.iter().zip(expected.iter()) {
        assert!(
            (a - e).abs() < 1.0,
            "境界面積 {areas:?}（期待値 {expected:?}）"
        );
    }
}

#[test]
fn wall_bay_uses_rigid_floor_rigidity_when_column_drift_partly_cancels() {
    use sepika_app::summary::seismic_story_metrics;
    use sepika_design_jp::secondary::eccentricity_analysis::{
        center_of_gravity_from_axial, column_horizontal_stiffnesses,
    };
    use sepika_element::factory::build_behavior;

    let mut app = unperforated_wall_bay_app();
    app.run_preparation();
    select_frame_gravity(&mut app.core.model);
    app.run_static_all();
    app.run_pushover();
    app.run_seismic(SeismicDir::Y);
    select_holding_points(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );

    let model = &app.core.model;
    let story = model.layers()[0].top;
    let master = model.diaphragms_of(story).next().unwrap().master;
    let x = static_res_for(&app, StaticCaseKey::Seismic(SeismicDir::X));
    let wall_side_mm = x.disp[4][0];
    let far_side_mm = x.disp[7][0];
    let floor_center_mm = (wall_side_mm + far_side_mm) / 2.0;
    let angle_rad = x.disp[master.index()][5];
    let wall_side_rotation_mm = 1500.0 * angle_rad;
    assert!((wall_side_mm - floor_center_mm - wall_side_rotation_mm).abs() < 1e-12);
    assert!(floor_center_mm > 0.0 && wall_side_rotation_mm < 0.0);
    assert!(wall_side_mm.abs() < floor_center_mm.abs().min(wall_side_rotation_mm.abs()));

    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(model);
    let wall_side_forces = |response: &sepika_solver::statics::linear::StaticOnce| {
        [ElemId(0), ElemId(1)].map(|id| {
            let column = expanded.element(id).unwrap();
            let mut disp = [0.0; 12];
            for (six, id) in disp.as_chunks_mut::<6>().0.iter_mut().zip(&column.nodes) {
                *six = response.disp[id.index()];
            }
            let element = build_behavior(column, &expanded)
                .frame_load_response(&disp, &[])
                .unwrap()
                .unwrap();
            let force_n = response
                .column_end_horizontal_forces
                .iter()
                .find(|(id, _)| *id == column.id)
                .unwrap()
                .1
                .as_ref()
                .unwrap()[1][0];
            assert!((force_n - element.nodal_residual.data[6]).abs() < 1e-6);
            force_n
        })
    };
    let original_forces_n = wall_side_forces(x);
    assert!(original_forces_n.iter().all(|q| q.abs() < 1e-6));
    let mut comparison = model.clone();
    balance_ex_floor_rotation(&mut comparison);
    let ex = comparison
        .load_cases
        .iter()
        .find(|case| case.name == sepika_core::model::EX_CASE_NAME)
        .unwrap()
        .id;
    let balanced = sepika_job::compute::compute_linear_static(comparison, ex).unwrap();
    assert!(balanced.disp[master.index()][5].abs() < 1e-12);
    let horizontal_force_n = |result: &sepika_solver::statics::linear::StaticOnce| {
        result.story_shears.as_ref().unwrap()[0][0]
    };
    assert!((horizontal_force_n(x) - horizontal_force_n(&balanced)).abs() < 1e-6);
    let balanced_forces_n = wall_side_forces(&balanced);
    assert!(balanced_forces_n.iter().all(|q| q.abs() < 1e-6));
    println!("壁側X変位={wall_side_mm}, 反対側X変位={far_side_mm}, 図心X変位={floor_center_mm}, 壁側回転変位={wall_side_rotation_mm}, Rz={angle_rad}, Qx={original_forces_n:?}, 並進対照のQx={balanced_forces_n:?}, 層水平力={}", horizontal_force_n(x));

    let reason = column_horizontal_stiffnesses(model, story, x, SeismicDir::X).unwrap_err();
    for expected in ["階1", "X方向", "柱0", "ねじれ", "相殺"] {
        assert!(reason.contains(expected), "{reason}");
    }
    let [(_, Ok(Some(rigidity)))] = x.story_rigidity.as_slice() else {
        panic!("壁展開後の層剛性が記録されていません");
    };
    let mut probe = expanded.clone();
    let mut solve = |load: [f64; 6]| {
        let case = &mut probe.load_cases[ex.index()];
        case.member.clear();
        case.nodal = vec![NodalLoad::manual(master, load)];
        sepika_solver::statics::linear::linear_static_once(&probe, ex)
            .unwrap()
            .disp[master.index()]
    };
    let moment_nmm = 1_000_000.0;
    let [u, v, _, _, _, theta] = solve([0.0, 0.0, 0.0, 0.0, 0.0, moment_nmm]);
    let [origin_x, origin_y, _] = expanded.nodes[master.index()].coord;
    let cor = [origin_x - v / theta, origin_y + u / theta];
    let kr = moment_nmm / theta;
    for (actual, expected) in rigidity.center_xy_mm.iter().zip(cor) {
        assert!((actual - expected).abs() < 1e-8);
    }
    assert!((rigidity.torsional_stiffness_nmm / kr - 1.0).abs() < 1e-10);
    let force_n = 1000.0;
    let [ux, uy, _, _, _, rx] =
        solve([force_n, 0.0, 0.0, 0.0, 0.0, -force_n * (cor[1] - origin_y)]);
    let [vx, vy, _, _, _, ry] = solve([0.0, force_n, 0.0, 0.0, 0.0, force_n * (cor[0] - origin_x)]);
    assert!(uy.abs() < 1e-9 && vx.abs() < 1e-9);
    assert!(rx.abs() < 1e-12 && ry.abs() < 1e-12);
    let [kx, kxy, ky] = rigidity.translational_stiffness_n_per_mm;
    assert!(kxy.abs() < 1e-7);
    assert!((kx * ux / force_n - 1.0).abs() < 1e-10);
    assert!((ky * vy / force_n - 1.0).abs() < 1e-10);

    let gravity = &app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .combos
        .iter()
        .find(|(name, _)| sepika_core::load_combo::is_gravity_combination(name, model))
        .unwrap()
        .1;
    let cog = center_of_gravity_from_axial(model, story, gravity)
        .unwrap()
        .unwrap();
    let expected_re = (cog[1] - cor[1]).abs() * (force_n / ux / kr).sqrt();
    let metric = seismic_story_metrics(&app).remove(0);
    let re = metric.re.unwrap();
    assert!((re - expected_re).abs() < 1e-10);
    assert_eq!(metric.re_ok, Some(false));
    assert_eq!(metric.fes, Ok(1.5));
    println!("剛心={cor:?}, KR={kr}, Kxx={kx}, Kxy={kxy}, Kyy={ky}, X偏心率={re}");
    let reason = app.compute_holding_capacity().err().unwrap();
    assert!(
        reason.contains("Ds判定保留") && reason.contains("RC壁版 ID 0"),
        "{reason}"
    );
    assert!(app.core.scoped.holding_capacity_source.is_none());
}

/// 壁の実応答・重量・Qud・Fesを維持し、Ds条件不足では確定判定を保留する。
#[test]
fn snapshot_wall_response_and_ds_hold() {
    let mut app = unperforated_wall_bay_app();
    app.run_preparation();
    assert!(
        app.core.scoped.last_error.is_none(),
        "準備計算: {:?}",
        app.core.scoped.last_error
    );
    balance_ex_floor_rotation(&mut app.core.model);
    select_frame_gravity(&mut app.core.model);
    app.run_static_all();
    assert!(
        app.core.scoped.last_error.is_none(),
        "静的解析: {:?}",
        app.core.scoped.last_error
    );
    let x = static_res_for(&app, StaticCaseKey::Seismic(SeismicDir::X));
    let model = &app.core.model;
    let floor = model.diaphragms_of(model.layers()[0].top).next().unwrap();
    assert!(x.disp[floor.master.index()][5].abs() < 1e-12);
    assert!((x.disp[4][0] - x.disp[7][0]).abs() < 1e-10);
    app.run_eigen(app.core.analysis_cfg.n_modes);
    assert!(
        app.core.scoped.last_error.is_none(),
        "固有値解析: {:?}",
        app.core.scoped.last_error
    );
    app.run_pushover();
    app.run_seismic(SeismicDir::Y);
    select_holding_points(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "増分解析: {:?}",
        app.core.scoped.last_error
    );

    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    let reason = app.compute_holding_capacity().err().unwrap();
    assert!(
        reason.contains("Ds判定保留") && reason.contains("RC壁版 ID 0"),
        "{reason}"
    );
    assert!(app.core.scoped.holding_capacity_source.is_none());
    let mut out = String::new();
    let mut line = |k: &str, v: String| out.push_str(&format!("{k} = {v}\n"));
    let fillet_area_m2 = (4.0 - std::f64::consts::PI) * 0.013_f64.powi(2);
    let wall_upper_weight_kn = 3.7 * 1.3 * 0.15 * 23.53596;
    let columns_upper_weight_kn = 2.0 * 0.3 * 0.3 * 3.0 * 23.53596 / 2.0
        + (0.3_f64.powi(2)
            - 0.28_f64.powi(2)
            - (4.0 - std::f64::consts::PI) * (0.013_f64.powi(2) - 0.003_f64.powi(2)))
            * 3.0
            * 78.5;
    let roof_girders_weight_kn =
        0.3 * 0.4 * 3.7 * 23.53596 + (0.008192 + fillet_area_m2) * 10.0 * 78.5;
    let expected_weight_n =
        (wall_upper_weight_kn + columns_upper_weight_kn + roof_girders_weight_kn) * 1000.0;
    assert!(
        (app.core
            .scoped
            .preparation
            .as_ref()
            .unwrap()
            .summary
            .total_seismic_weight
            - expected_weight_n)
            .abs()
            < 1e-6
    );
    let po = app.displayed_pushover().unwrap();
    assert_eq!(
        po.rc_wall_shear_method,
        sepika_core::model::RcWallShearMethod::ArakawaMean
    );
    assert_eq!(po.wall_capacities.len(), 1);
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
    let wall = expanded.element(po.wall_capacities[0].0).unwrap();
    let face = sepika_element::wall::static_face::StaticWallFace::new(wall, &expanded).unwrap();
    let gravity_name =
        &app.core.model.combinations[app.core.model.ultimate_gravity_combination.unwrap()].name;
    let (_, gravity_result) = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .combos
        .iter()
        .find(|(name, _)| name == gravity_name)
        .unwrap();
    let batch: Vec<_> = gravity_result
        .member_forces
        .iter()
        .map(|(id, forces)| (*id, forces.at.as_slice()))
        .collect();
    let axial_at = |id: ElemId, position: f64| {
        gravity_result
            .member_forces
            .iter()
            .find(|(member, _)| *member == id)
            .unwrap()
            .1
            .at
            .iter()
            .find(|(s, _)| (*s - position).abs() < 1e-12)
            .unwrap()
            .1[0]
    };
    let expected_compression_n = -axial_at(wall.id, 0.0)
        - axial_at(ElemId(0), 200.0 / 3000.0)
        - axial_at(ElemId(1), 200.0 / 3000.0);
    assert!(expected_compression_n > 0.0);
    assert!((face.recover(&batch).unwrap().compression_n - expected_compression_n).abs() < 1e-6);
    // A=735000、D=4300、te=A/D、j=3631.25より軸力項は0.1·N·j/D。
    let expected_wall_capacity_n =
        1_714_717.190_814_330_4 + 0.1 * expected_compression_n * 3631.25 / 4300.0;
    for capacity_n in po.wall_capacities[0].1 {
        assert!((capacity_n - expected_wall_capacity_n).abs() < 1e-6);
    }
    let capacity_forces = po
        .evaluate_stories(
            po.capacity_evaluation.as_ref().unwrap(),
            EvaluationPurpose::HoldingCapacity,
        )
        .unwrap();
    assert_eq!(capacity_forces.len(), 1);
    let capacity_force = &capacity_forces[0];
    let point = po.capacity_evaluation.as_ref().unwrap();
    let step = &po.steps[point.step as usize];
    assert!((capacity_force.reference_n - 0.2 * expected_weight_n).abs() < 1e-6);
    assert!((capacity_force.external_n - step.load_factor * 0.2 * expected_weight_n).abs() < 1e-6);
    let response = po
        .wall_response_at(po.wall_capacities[0].0, point.step)
        .unwrap();

    assert!((capacity_force.wall_n - response.qdir_n).abs() < 1e-6);
    assert!(capacity_force.wall_n > 0.0);
    assert!(capacity_force.wall_n <= expected_wall_capacity_n + 1e-6);
    assert_eq!(capacity_force.brace_n, 0.0);
    assert!(capacity_force.frame_n > 0.0);
    assert!((capacity_force.qu_n - capacity_force.wall_n - capacity_force.frame_n).abs() < 1e-6);
    assert!((capacity_force.qu_n - capacity_force.external_n).abs() <= capacity_force.tolerance_n);
    let ds_point = po.ds_evaluation.as_ref().unwrap();
    let ds_forces = po
        .evaluate_stories(ds_point, EvaluationPurpose::Ds)
        .unwrap();
    assert!(ds_forces[0].beta_u > 0.7);
    let expanded = sepika_core::wall_expand::expand_wall_elements(&app.core.model).0;
    assert!(matches!(
        po.mechanism_at(ds_point, &expanded).unwrap(),
        sepika_solver::nonlinear::pushover::MechanismType::Partial
    ));
    let metrics = sepika_app::summary::compute_story_metrics_with(
        &app.core.model,
        SeismicDir::X,
        &sepika_app::summary::metrics_ctx_from_results(
            &app.core.model,
            app.core.scoped.results.as_ref(),
        ),
    );
    assert_eq!(metrics[0].fes, Ok(1.5));
    let analysis = sepika_solver::statics::analysis::Analysis::prepare(&app.core.model).unwrap();
    let t = analysis
        .seismic_period(app.core.analysis_cfg.ai_mode)
        .unwrap();
    let qud = sepika_solver::statics::analysis::seismic_distribution_for_model(
        &app.core.model,
        sepika_solver::statics::analysis::SeismicCfg {
            dir: SeismicDir::X,
            mode: app.core.analysis_cfg.ai_mode,
            z: app.core.analysis_cfg.z,
            soil: app.core.analysis_cfg.soil,
            c0: 1.0,
        },
        t,
    )
    .unwrap();
    assert!((qud.qi[0] - expected_weight_n).abs() < 1e-6);
    assert!(capacity_force.qu_n > 0.0 && capacity_force.qu_n.is_finite());
    line("analysis.qu", sig4(capacity_force.qu_n));
    line("analysis.fes", sig4(*metrics[0].fes.as_ref().unwrap()));
    line("ds.status", "保留".into());

    insta::assert_snapshot!(out);
}

/// 自動ランクは壁応答を認識しても、破壊形式・接合部等の条件不足を保留する。
#[test]
fn test_holding_capacity_auto_rank_detects_wall_before_ds_hold() {
    let mut app = unperforated_wall_bay_app();
    app.run_preparation();
    assert!(
        app.core.scoped.last_error.is_none(),
        "準備計算: {:?}",
        app.core.scoped.last_error
    );
    balance_ex_floor_rotation(&mut app.core.model);
    select_frame_gravity(&mut app.core.model);
    app.run_static_all();
    assert!(
        app.core.scoped.last_error.is_none(),
        "静的解析: {:?}",
        app.core.scoped.last_error
    );
    app.run_eigen(app.core.analysis_cfg.n_modes);
    assert!(
        app.core.scoped.last_error.is_none(),
        "固有値解析: {:?}",
        app.core.scoped.last_error
    );
    app.run_pushover();
    app.run_seismic(SeismicDir::Y);
    select_holding_points(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "増分解析: {:?}",
        app.core.scoped.last_error
    );

    app.core.design_rank_auto = true;
    let reason = app.compute_holding_capacity().err().unwrap();
    assert!(reason.contains("Ds判定保留"), "{reason}");
    assert!(reason.contains("壁せん断破壊時のβuは採用保留"), "{reason}");
    assert!(reason.contains("RC壁版 ID 0"), "{reason}");

    assert!(
        !app.core.scoped.ds_beta_u_unavailable,
        "壁専用応答を利用して壁を検出し βu を算定する"
    );
}

#[test]
fn wall_committed_results_reach_csv_save_schema_and_reject_unrecorded_design_input() {
    use sepika_solver::nonlinear::pushover::wall_response::WallUnavailableReason;
    let mut app = unperforated_wall_bay_app();
    app.run_preparation();
    balance_ex_floor_rotation(&mut app.core.model);
    select_frame_gravity(&mut app.core.model);
    app.run_static_all();
    app.run_pushover();
    app.run_seismic(SeismicDir::Y);
    select_holding_points(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.core.design_rank_auto = true;
    let po = app.displayed_pushover().unwrap();
    let records = po.wall_history.as_ref().unwrap();
    assert_eq!(records.len(), po.steps.len());
    assert!(records.iter().all(|r| r.plate == Some(WallPlateId(0))));
    let elem = records[0].elem;
    assert!(po.member_response.iter().all(|r| r.elem != elem));
    assert!(po.wall_run.as_ref().unwrap().input_generation.is_some());
    let bytes = rmp_serde::to_vec_named(po).unwrap();
    let saved: sepika_solver::nonlinear::pushover::PushoverResult =
        rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(
        saved.wall_run.unwrap().run_id,
        po.wall_run.as_ref().unwrap().run_id
    );
    assert_eq!(saved.wall_history.unwrap().len(), records.len());
    let csv = sepika_app::summary::build_report_csv(&app);
    for label in [
        "Qw[N]",
        "Qdir[N]",
        "R_wall[rad]",
        "O[mm]/F[N]/M_O[Nmm]",
        "仮想壁柱",
        "剛体回転・曲げを除いた材料せん断ひずみを復元していません",
        "線材ヒンジ・せん断降伏イベント",
    ] {
        assert!(csv.contains(label), "{label}");
    }
    assert!(app
        .compute_holding_capacity()
        .err()
        .unwrap()
        .contains("Ds判定保留"));
    let original = app.core.scoped.results.clone();
    for reason in [
        None,
        Some(WallUnavailableReason::InvalidForce),
        Some(WallUnavailableReason::UnsupportedGeometry),
    ] {
        app.core.scoped.results = original.clone();
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let po = bundle.pushover_x.as_mut().unwrap();
        if let Some(reason) = reason {
            let record = po.wall_history.as_mut().unwrap().last_mut().unwrap();
            record.response = None;
            record.unavailable = Some(reason);
        } else {
            po.wall_history = None;
        }
        let error = app
            .compute_holding_capacity()
            .err()
            .expect("壁未記録は設計を拒否");
        assert!(
            error.contains(
                reason
                    .unwrap_or(WallUnavailableReason::LegacyNotRecorded)
                    .description()
            ),
            "{error}"
        );
    }
    app.core.scoped.results = original.clone();
    let bundle = app.core.scoped.results.as_mut().unwrap();
    let po = bundle.pushover_x.as_mut().unwrap();
    po.wall_run = None;
    assert!(app
        .compute_holding_capacity()
        .err()
        .expect("壁未記録は設計を拒否")
        .contains("入力識別情報"));
    for corrupt in 0..3 {
        app.core.scoped.results = original.clone();
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let po = bundle.pushover_x.as_mut().unwrap();
        match corrupt {
            0 => po.wall_run.as_mut().unwrap().input_generation = Some(vec![0]),
            1 => {
                po.wall_history
                    .as_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .response
                    .as_mut()
                    .unwrap()
                    .qw_n = f64::NAN
            }
            _ => {
                po.wall_history
                    .as_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .response
                    .as_mut()
                    .unwrap()
                    .equilibrium_residual
                    .force_n[0] = 1e12
            }
        }
        let error = app
            .compute_holding_capacity()
            .err()
            .expect("不正壁入力は設計を拒否");
        let expected = match corrupt {
            0 => "生成入力が現在の計算入力と一致しません",
            1 => WallUnavailableReason::InvalidForce.description(),
            _ => WallUnavailableReason::UnbalancedForces.description(),
        };
        assert!(error.contains(expected), "{error}");
        if corrupt > 0 {
            assert!(sepika_app::summary::build_report_csv(&app).contains(expected));
        }
    }
    app.core.scoped.results = original;
    for po in [&mut app.core.scoped.results.as_mut().unwrap().pushover_x]
        .into_iter()
        .flatten()
    {
        let last = po.wall_history.as_mut().unwrap().last_mut().unwrap();
        let wall = last.response.as_mut().unwrap();
        wall.qw_n = 0.0;
        wall.qdir_n = 0.0;
        for resultant in [
            &mut wall.bottom,
            &mut wall.top,
            &mut wall.equilibrium_residual,
        ] {
            resultant.force_n = [0.0; 3];
            resultant.moment_nmm = [0.0; 3];
        }
        let step = last.step;
        assert_eq!(po.wall_response_at(elem, step).unwrap().qdir_n, 0.0);
    }
    let csv = sepika_app::summary::build_report_csv(&app);
    assert!(csv
        .lines()
        .any(|line| line.contains("Qdir[N]") && line.ends_with(",\"0\"")));
}

#[test]
fn diagonal_wall_response_is_preserved_when_directional_coupling_holds_required_capacity() {
    let mut app = unperforated_wall_bay_app();
    app.core.analysis_cfg.push_use_drift_angle = false;
    app.core.analysis_cfg.push_use_max_disp = true;
    app.core.analysis_cfg.push_max_disp = 1.0;
    app.core.analysis_cfg.push_steps = 4;
    let section = &mut app.core.model.sections[2];
    section.shape = Some(SectionShape::RcWall {
        thickness: 150.0,
        ps: 0.012,
        pwh_ratio: Some(0.012),
        pwv_ratio: Some(0.012),
    });
    let c = std::f64::consts::FRAC_1_SQRT_2;
    for node in &mut app.core.model.nodes {
        let [x, y, z] = node.coord;
        node.coord = [c * (x - y), c * (x + y), z];
    }
    for elem in &mut app.core.model.elements {
        let [x, y, z] = elem.local_axis.ref_vector;
        elem.local_axis.ref_vector = [c * (x - y), c * (x + y), z];
    }
    app.run_preparation();
    select_frame_gravity(&mut app.core.model);
    app.run_static_all();
    app.run_pushover();
    app.run_seismic(SeismicDir::Y);
    select_holding_points(&mut app);
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    let po = app.displayed_pushover().unwrap();
    let last = po.wall_history.as_ref().unwrap().last().unwrap();
    let wall = po.wall_response_at(last.elem, last.step).unwrap();
    assert!(
        wall.qw_n.abs() > 100.0,
        "実壁の局所せん断は有限非ゼロ: Qw={}, Qdir={}, steps={}",
        wall.qw_n,
        wall.qdir_n,
        po.steps.len()
    );
    assert!((wall.qw_n.abs() - wall.qdir_n.abs()).abs() > 10.0);
    assert!((wall.inplane_axis[0].abs() - c).abs() < 1e-12);
    assert!((wall.inplane_axis[1].abs() - c).abs() < 1e-12);
    let elem_id = last.elem;
    let (expanded, _, _) = sepika_core::wall_expand::expand_wall_elements(&app.core.model);
    let elem = expanded.element(elem_id).unwrap();
    let qu = po
        .wall_capacities
        .iter()
        .find(|(id, _)| *id == elem_id)
        .unwrap()
        .1;
    let geom = sepika_core::model::wall_element_geometry(elem, &expanded).unwrap();
    let r2 = sepika_element::wall::wall_element::WallElement::opening_strength_reduction(
        elem, &expanded,
    );
    let fc = expanded.element_material(elem).unwrap().fc.unwrap();
    let tau_case_qw = 0.105 * fc * 150.0 * geom.lw * r2;
    assert!(
        tau_case_qw < 0.99 * qu[0],
        "τu単独の試験で頭打ちに達しない: Qw={tau_case_qw}, Qu={qu:?}"
    );
    app.core.design_rank_auto = true;
    app.core.wall_structure = true;
    let reason = app.compute_holding_capacity().err().unwrap();
    assert!(
        reason.contains("階1") && reason.contains("方向間連成"),
        "{reason}"
    );
    assert!(app.core.scoped.holding_capacity_source.is_none());

    // Dsの適用条件を独立に照合するため、層評価は成立済みの入力を与える。
    for (key, response) in &mut app.core.scoped.results.as_mut().unwrap().statics {
        if matches!(key, sepika_app::app::StaticCaseKey::Seismic(_)) {
            let [(story, Ok(Some(mut rigidity)))] = response.story_rigidity.as_slice() else {
                panic!("剛床の層評価が記録されていません");
            };
            rigidity.translational_stiffness_n_per_mm[1] = 0.0;
            response.story_rigidity = vec![(*story, Ok(Some(rigidity)))];
        }
    }
    let reason = app.compute_holding_capacity().err().unwrap();
    assert!(reason.contains("Ds判定保留"), "{reason}");
    assert!(reason.contains("壁せん断破壊時のβuは採用保留"), "{reason}");
    assert!(app.core.scoped.holding_capacity_source.is_none());
}

fn select_holding_points(app: &mut sepika_app::app::App) {
    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    let dir = app.core.scoped.pushover_view_dir;
    let Some(bundle) = &mut app.core.scoped.results else {
        return;
    };
    for po in [&mut bundle.pushover_x, &mut bundle.pushover_y]
        .into_iter()
        .flatten()
    {
        let Some(step) = po.capacity_curve.last().map(|p| p.step) else {
            continue;
        };
        if let Ok(mut ds) = po.evaluation_point(
            EvaluationPurpose::Ds,
            dir,
            step,
            "テストの明示採用点".into(),
        ) {
            let mut ids: std::collections::BTreeSet<_> =
                po.member_response.iter().map(|r| r.elem).collect();
            if let Some(walls) = &po.wall_history {
                ids.extend(walls.iter().map(|r| r.elem));
            }
            ds.member_capacities_n = ids.into_iter().map(|id| (id, 1_000_000.0)).collect();
            po.ds_evaluation = Some(ds);
            po.capacity_evaluation = po
                .evaluation_point(
                    EvaluationPurpose::HoldingCapacity,
                    dir,
                    step,
                    "テストの明示採用点".into(),
                )
                .ok();
        }
    }
}
