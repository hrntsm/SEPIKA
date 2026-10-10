//! 実建物モデル（ST-Bridge）の対応解析と入力診断、および鋼構造サンプルの非線形解析の統合テスト。
//!
//! # 目的
//!
//! 既存のテストはいずれも手組みの小規模モデル（門型ラーメン・1 層立体フレーム）を
//! 対象としており、実建物特有の構成（剛床・二次部材・混構造・多数のスラブ）が
//! 揃って初めて現れる不具合を検出できない。本テストは実際の設計モデルを 1 つ
//! フィクスチャとして固定し、GUI のボタンが呼ぶのと同じ入口（`App` の
//! `run_*` / `compute_*`）を通して実行する。スラブ協力付きRC梁の非線形解析は
//! 必要情報不足を診断し、完走・収束は独立した既存の鋼構造サンプルで検証する。
//!
//! # モデル（`tests/fixtures/model.stb`）
//!
//! 4 層＋PH の S 造（一部 RC）。節点 166・解析要素 115（柱 40・大梁 75）・
//! 二次部材 56（小梁）・床領域 26（大梁1床領域単位）・階 5（Z=200/4700/8700/12700/16500）。
//! 荷重は ST-Bridge に含まれないため、取り込み時に標準荷重ケース
//! （DL・LL(架構用)・LL(地震用)・EX・EY）が自動生成される。支点情報も
//! 含まれないため、最下レベルの柱脚 12 箇所がピン支点として自動設定される。
//!
//! # 検証の三層構造
//!
//! 1. **煙テスト** — エラーなく完走し、結果が空でない
//! 2. **構造的不変量** — 「エラーは出ないが結果が静かに劣化する」退行を捕まえる
//!    （全部材に断面力がある／柱脚が圧縮／固有周期が正で降順／層せん断が上階ほど
//!    小さい 等）。過去に発生した「剛床上の梁の応力欠落」「サイレントにゼロ変位」
//!    の類はここで落ちる
//! 3. **代表スカラのスナップショット**（[`snapshot_key_scalars`]） — 値そのものの
//!    変化を可視化する。CI（Linux）と手元（Windows）の浮動小数差で偽陽性に
//!    ならないよう、有効数字 4 桁の指数表記に丸めてから記録する
//!
//! # 解析の再現性
//!
//! `analysis_cfg.threads = 1`（単一スレッド）を全テストで指定する。既定の 0
//! （全コア）では並列リダクションの加算順が実行ごとに変わりうるため、
//! スナップショットの比較が安定しない。
//!
//! # 新しい解析を追加したとき
//!
//! `App` に解析エントリを追加したら、本ファイルにもテストを追加すること
//! 追加を怠ると、その機能だけが回帰検出の対象外になる。
//!
//! # 実行
//!
//! `cargo test -p sepika-app --test full_model` で実行する。
//! `#[ignore]` のテストは末尾に `-- --ignored` を付けて実行する。
//! スナップショットの差分は `cargo insta review` で確認・承認する。

use sepika_app::app::{App, StaticCaseKey, ThDampingModel, ThDir, DL_CASE_NAME};
use sepika_core::dof::Dof6Mask;
use sepika_core::model::ElementKind;
use sepika_solver::statics::analysis::SeismicDir;

// ===================== フィクスチャと共通ヘルパー =====================

/// 固定フィクスチャ（実建物の ST-Bridge）のパス。
fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("model.stb")
}

#[test]
fn rounded_fixture_properties_and_weight_are_independently_integrated() {
    use sepika_core::section_shape::SectionShape;
    use std::f64::consts::PI;
    fn integrate(h: f64, breaks: Vec<f64>, width: impl Fn(f64) -> f64) -> [f64; 4] {
        let mut bounds = breaks;
        bounds.extend([-h / 2.0, 0.0, h / 2.0]);
        bounds.sort_by(f64::total_cmp);
        bounds.dedup();
        let mut result = [0.0; 4];
        for pair in bounds.windows(2) {
            let mid = (pair[0] + pair[1]) / 2.0;
            let half = (pair[1] - pair[0]) / 2.0;
            let dt = PI / 16384.0;
            for i in 0..16384 {
                let theta = -PI / 2.0 + (i as f64 + 0.5) * dt;
                let z = mid + half * theta.sin();
                let dz = half * theta.cos() * dt;
                let b = width(z);
                result[0] += b * dz;
                result[1] += b * z * z * dz;
                result[2] += b.powi(3) / 12.0 * dz;
                result[3] += b * z.abs() * dz;
            }
        }
        result
    }
    fn rectangle(h: f64, b: f64, r: f64) -> [f64; 4] {
        integrate(h, vec![-h / 2.0 + r, h / 2.0 - r], |z| {
            let d = z.abs() - h / 2.0 + r;
            if d <= 0.0 {
                b
            } else {
                b - 2.0 * r + 2.0 * (r * r - d * d).max(0.0).sqrt()
            }
        })
    }
    fn reference(shape: &SectionShape) -> Option<([f64; 4], f64)> {
        match *shape {
            SectionShape::SteelH {
                height: h,
                width: b,
                web_thick: tw,
                flange_thick: tf,
                root_r: Some(r),
            } => {
                let hw = h - 2.0 * tf;
                Some((
                    integrate(
                        h,
                        vec![-hw / 2.0, hw / 2.0, -hw / 2.0 + r, hw / 2.0 - r],
                        |z| {
                            let d = hw / 2.0 - z.abs();
                            if d <= 0.0 {
                                b
                            } else if d >= r {
                                tw
                            } else {
                                tw + 2.0 * (r - (r * r - (r - d).powi(2)).max(0.0).sqrt())
                            }
                        },
                    ),
                    2.0 * b * tf + hw * tw,
                ))
            }
            SectionShape::SteelBox {
                height: h,
                width: b,
                thick: t,
                corner_r: Some(r),
            } => {
                let outer = rectangle(h, b, r);
                let inner = rectangle(h - 2.0 * t, b - 2.0 * t, (r - t).max(0.0));
                Some((
                    std::array::from_fn(|i| outer[i] - inner[i]),
                    h * b - (h - 2.0 * t) * (b - 2.0 * t),
                ))
            }
            _ => None,
        }
    }
    let mut app = imported();
    let mut references = std::collections::HashMap::new();
    let mut max_error = 0.0_f64;
    for section in &app.core.model.sections {
        let Some(shape) = &section.shape else {
            continue;
        };
        let Some((p, old_area)) = reference(shape) else {
            continue;
        };
        for (actual, expected) in [section.area, section.iy, section.iz].into_iter().zip(p) {
            let error = (actual / expected - 1.0).abs();
            max_error = max_error.max(error);
            assert!(error < 1.0e-7, "{}: {actual}/{expected}", section.name);
        }
        let zp = shape.plastic_modulus_strong().unwrap();
        assert!((zp / p[3] - 1.0).abs() < 1.0e-7);
        references.insert(section.id, (p[0], old_area));
    }
    assert!(!references.is_empty());
    let mut model = app.core.model.clone();
    model
        .elements
        .retain(|e| e.section.is_some_and(|id| references.contains_key(&id)));
    model.wall_plates.clear();
    let mut expected = 0.0;
    let mut old = 0.0;
    for (i, elem) in model.elements.iter_mut().enumerate() {
        elem.id = sepika_core::ids::ElemId(i as u32);
    }
    for elem in &model.elements {
        let (area, old_area) = references[&elem.section.unwrap()];
        let length = model.member_length(elem);
        expected += area * length * 78.5e-6;
        old += old_area * length * 78.5e-6;
    }
    let (nodal, member) =
        sepika_load::self_weight::self_weight_case_content(&model, &Default::default()).unwrap();
    let mut actual: f64 = nodal.iter().map(|n| -n.values[2]).sum();
    for load in member {
        if let sepika_core::model::MemberLoadKind::Distributed { a, b, w1, w2 } = load.kind {
            actual += (b - a) * (w1 + w2) / 2.0;
        }
    }
    assert!((actual / expected - 1.0).abs() < 1.0e-7);
    eprintln!("実モデル断面数={}, 独立 A/I 最大差={max_error:.12e}, 対象線材DL={actual:.12e} N, 直角モデルDL={old:.12e} N, 差={:.12e} N", references.len(), actual - old);
    let mut baseline = imported();
    for section in &mut baseline.core.model.sections {
        if let Some((_, old_area)) = references.get(&section.id) {
            section.area = *old_area;
            section.property_basis.area = sepika_core::model::PropertyBasis::Supplied;
        }
    }
    app.run_preparation();
    baseline.run_preparation();
    assert_no_error(&app, "角丸モデルの準備計算");
    assert_no_error(&baseline, "独立直角面積の比較用準備計算");
    let new_weight = app
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
        (old_weight - 3.195e6).abs() < 500.0,
        "旧スナップショットの重量と不整合: {old_weight}"
    );
    assert!(
        (new_weight - 3.192e6).abs() < 500.0,
        "新スナップショットの重量と不整合: {new_weight}"
    );
    eprintln!("小梁・床荷重を含む総地震用重量: 角丸={new_weight:.12e} N, 独立直角面積={old_weight:.12e} N, 差={:.12e} N", new_weight - old_weight);
}

/// テストが書き込む一時ディレクトリ（プロセス ID 入り）。
/// `std::env::temp_dir()` 直下へ固定名で書き込むと、同一マシンで並行する
/// 別プロセスのテスト実行と衝突するため、プロセスごとに一意なサブディレクトリを
/// 介する（同一プロセス内はテストごとの固有ファイル名で分離する）。
fn test_tmp() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sepika-full-model-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// 直前の操作でエラーが出ていないことを確認する。
///
/// `App::last_error` はステータスバー共用の単一スロットで、前の操作の内容が
/// 残るため、各ステップの前に [`clear_error`] でクリアしてから使う。
fn assert_no_error(app: &App, what: &str) {
    assert!(
        app.core.scoped.last_error.is_none(),
        "{what} でエラー: {}",
        app.core.scoped.last_error.as_deref().unwrap_or("")
    );
}

/// `last_error` をクリアする（次のステップの判定に前の内容を持ち越さない）。
fn clear_error(app: &mut App) {
    app.core.scoped.last_error = None;
}

/// フィクスチャを取り込んだ `App`。
///
/// ST-Bridge 取り込みは、欠落属性の要約や支点の自動設定といった**注意**も
/// `report_error` 経由で `last_error` に載せる（`app/actions.rs` の
/// `import_stbridge_from`。ステータスバーで見落とされないようにするための仕様）。
/// そのため「注意（先頭が ⚠️）以外のエラーが出ていないこと」を確認したうえで
/// クリアする。
fn imported() -> App {
    let mut app = App::default();
    // 解析結果の完全再現性を確保する（スナップショット比較の前提）。
    app.core.analysis_cfg.threads = 1;
    app.import_stbridge_from(fixture_path());
    if let Some(e) = &app.core.scoped.last_error {
        assert!(
            e.starts_with('⚠'),
            "ST-Bridge 取り込みが失敗した（注意ではなくエラー）: {e}"
        );
    }
    clear_error(&mut app);
    app
}

/// 取り込み＋準備計算まで済ませた `App`（階・剛域・仕口パネル・地震力が確定した状態）。
fn prepared() -> App {
    let mut app = imported();
    app.run_preparation();
    assert_no_error(&app, "準備計算");
    app
}

/// 準備計算＋静的解析（全荷重ケース・全組合せ）＋固有値解析まで済ませた `App`。
fn analyzed() -> App {
    let mut app = prepared();
    app.run_static_all();
    assert_no_error(&app, "静的解析（一括）");
    app.run_eigen(app.core.analysis_cfg.n_modes);
    assert_no_error(&app, "固有値解析");
    app
}

/// 既存鋼門型の断面・材料を使う床なし4層（各3500 mm）・1スパン6000 mmの独立fixture。
/// 各層梁に既存の鉛直10 N/mm、各層両節点に水平20 kNを明示して与える。
fn four_story_steel_portal() -> sepika_core::model::Model {
    use sepika_core::ids::{ElemId, NodeId};
    let base = sepika_app::sample::portal_frame();
    let mut model = base.clone();
    model.nodes.clear();
    model.elements.clear();
    model.load_cases[0].member.clear();
    model.load_cases[1].nodal.clear();
    for level in 0..=4u32 {
        for side in 0..2u32 {
            let mut node = base.nodes[if level == 0 { side } else { side + 2 } as usize].clone();
            node.id = NodeId(2 * level + side);
            node.coord[2] = 3500.0 * f64::from(level);
            model.nodes.push(node);
        }
    }
    for level in 0..4u32 {
        for (offset, source) in base.elements.iter().enumerate() {
            let mut element = source.clone();
            element.id = ElemId(3 * level + offset as u32);
            element.nodes = source
                .nodes
                .iter()
                .map(|node| NodeId(node.0 + 2 * level))
                .collect();
            model.elements.push(element);
        }
        let mut gravity = base.load_cases[0].member[0].clone();
        gravity.elem = ElemId(3 * level + 2);
        model.load_cases[0].member.push(gravity);
        for source in &base.load_cases[1].nodal {
            let mut load = source.clone();
            load.node = NodeId(source.node.0 + 2 * level);
            model.load_cases[1].nodal.push(load);
        }
    }
    model.validate().expect("4層鋼門型のモデル契約");
    model
}

/// 元STBとは独立した、床を持たない鋼構造の非線形解析用モデル。
fn prepared_steel_portal(four_story: bool) -> App {
    let mut app = App::default();
    app.core.analysis_cfg.threads = 1;
    app.load_model(if four_story {
        four_story_steel_portal()
    } else {
        sepika_app::sample::portal_frame()
    });
    app.generate_stories_action();
    app.run_preparation();
    assert_no_error(&app, "鋼構造サンプルの準備計算");
    assert!(app.core.model.slabs.is_empty());
    sepika_element::factory::ensure_nonlinear_input(&app.core.model)
        .expect("鋼構造サンプルの非線形入力");
    app
}

/// 床なし4層RC矩形。柱600角・梁400×700、D25主筋、Fc24/SD345を明示した独立fixture。
fn prepared_rectangular_rc_portal() -> App {
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_core::model::{FrameSectionUse, Material, MaterialCategory};
    use sepika_core::section_shape::{
        BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
    };
    let mut model = four_story_steel_portal();
    let shapes = [
        SectionShape::RcColumnRect {
            b: 600.0,
            d: 600.0,
            rebar: RcRectColumnRebar {
                main_dia: 25.0,
                x: vec![8],
                y: vec![8],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        },
        SectionShape::RcBeamRect {
            b: 400.0,
            d: 700.0,
            rebar: RcBeamRebar {
                main_dia: 25.0,
                top: vec![4],
                bottom: vec![4],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: 10.0,
                    pitch: 100.0,
                    legs: 2,
                },
            },
        },
    ];
    model.sections = shapes
        .into_iter()
        .enumerate()
        .map(|(i, shape)| {
            let mut sec = shape.to_section(SectionId(i as u32), format!("RC矩形{i}"));
            sec.frame_use = Some(if i == 0 {
                FrameSectionUse::Column
            } else {
                FrameSectionUse::Girder
            });
            sec.material = Some(MaterialId(0));
            sec.rebar_material = Some(MaterialId(1));
            sec.shear_rebar_material = Some(MaterialId(1));
            sec
        })
        .collect();
    model.materials = vec![
        Material {
            id: MaterialId(0),
            name: "Fc24".into(),
            category: MaterialCategory::Concrete,
            young: 23000.0,
            poisson: 0.2,
            density: 2.4e-9,
            shear: None,
            fc: Some(24.0),
            fy: None,
            strength_factor: None,
            concrete_class: Default::default(),
        },
        Material {
            id: MaterialId(1),
            name: "SD345".into(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(345.0),
            strength_factor: None,
            concrete_class: Default::default(),
        },
    ];
    let mut app = App::default();
    app.core.analysis_cfg.threads = 1;
    app.load_model(model);
    app.generate_stories_action();
    app.run_preparation();
    assert_no_error(&app, "RC矩形準備");
    sepika_element::factory::ensure_nonlinear_input(&app.core.model).expect("RC矩形入力");
    app
}

fn assert_t_beam_diagnostic(app: &App) {
    let error = app.core.scoped.last_error.as_deref().expect("T形入力診断");
    for id in [40, 41, 48, 49, 56] {
        assert!(
            error.contains(&format!("部材 ID {id} はスラブ協力付きRC梁")),
            "{error}"
        );
    }
    assert!(error.contains("スラブ引張筋面積と正負別骨格"), "{error}");
    assert!(
        error.contains("矩形梁への代用は行わず解析を停止"),
        "{error}"
    );
    assert!(error.contains("他 12 件"), "{error}");
}

#[test]
fn imported_t_beams_stop_pushover_and_nonlinear_time_history() {
    let mut app = prepared();
    app.run_pushover();
    assert_t_beam_diagnostic(&app);
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .is_none_or(|r| r.pushover.is_none()));
    clear_error(&mut app);
    app.core.analysis_cfg.th_nonlinear = true;
    app.run_time_history_sample();
    assert_t_beam_diagnostic(&app);
    assert!(app
        .core
        .scoped
        .results
        .as_ref()
        .is_none_or(|r| r.time_history.is_none()));
}

/// 解析対象の梁要素（断面力が必ず得られる部材）の本数。
/// 準備計算が自動生成する仕口パネル要素（`PanelZone`）は断面力の対象外のため除く。
fn frame_elem_count(app: &App) -> usize {
    app.core
        .model
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Beam && e.nodes.len() == 2)
        .count()
}

/// 荷重ケースの解析結果が格納されるキー。
///
/// 標準の水平力ケース（EX/EY）は Ai 分布から水平力を組み立て直して解かれ、
/// 方向別の [`StaticCaseKey::Seismic`] に格納される（`App::standard_lateral_case`。
/// `pub(crate)` のためテストからは呼べず、同じ判定をここに置く）。それ以外は
/// [`StaticCaseKey::User`]。
fn static_case_key(app: &App, lc: sepika_core::ids::LoadCaseId) -> StaticCaseKey {
    use sepika_core::model::{LoadCaseKind, EX_CASE_NAME, EY_CASE_NAME};
    let case = app
        .core
        .model
        .load_cases
        .iter()
        .find(|c| c.id == lc)
        .unwrap_or_else(|| panic!("荷重ケース {lc:?} が見つからない"));
    match (case.name.as_str(), case.kind) {
        (EX_CASE_NAME, LoadCaseKind::Seismic) => StaticCaseKey::Seismic(SeismicDir::X),
        (EY_CASE_NAME, LoadCaseKind::Seismic) => StaticCaseKey::Seismic(SeismicDir::Y),
        _ => StaticCaseKey::User(lc),
    }
}

/// 指定した静的結果を取り出す。
fn static_of(app: &App, key: StaticCaseKey) -> &sepika_solver::statics::linear::StaticOnce {
    app.core
        .scoped
        .results
        .as_ref()
        .expect("解析結果が格納されているはず")
        .statics
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .unwrap_or_else(|| panic!("{key:?} の静的結果が見つからない"))
}

/// DL（固定荷重）の荷重ケース ID。
fn dl_case_id(app: &App) -> sepika_core::ids::LoadCaseId {
    app.core
        .model
        .load_cases
        .iter()
        .find(|lc| lc.name == DL_CASE_NAME)
        .expect("標準荷重ケース DL が自動生成されるはず")
        .id
}

/// 名前で荷重ケースを取り出す（内容の比較用）。
fn auto_case(app: &App, name: &str) -> sepika_core::model::LoadCase {
    app.core
        .model
        .load_cases
        .iter()
        .find(|lc| lc.name == name)
        .unwrap_or_else(|| panic!("荷重ケース「{name}」が見つからない"))
        .clone()
}

/// 柱脚（支点に取り付く鉛直部材の支点側材端）の軸力 [N] を集める。
///
/// 鉛直荷重の伝達経路が壊れた（剛床・二次部材の CMQ 変換・スラブ分配のいずれかが
/// 荷重を落とした）場合、ここの合計が変化する回帰検出用の指標であり、**総反力
/// （＝総荷重）の代わりにはならない**（このフィクスチャでは合計 [`base_column_axials`]
/// が総荷重の半分未満）。垂直部材（柱）自身の材端軸力だけを見ており、同じ支点節点に
/// 取り付く基礎梁（水平部材）が負担する分は含まない。荷重の分配経路が変わると
/// （床領域単位で1枚に畳んで配るか、床板ごとに個別へ配るか等）、柱の軸力と基礎梁の
/// せん断のどちらへどれだけ載るかの配分が変わるため、この合計も変わりうる
/// （床領域の荷重分配作り替え〔Step 4〕の際に実際に約 7.7% 動いた。総荷重は
/// `dev_docs/v_and_v/床領域の再設計_荷重分配とSlabFloorRegion分離_2026-08.md`
/// の追記のとおりビット単位で一致しており、荷重が失われたのではない）。
fn base_column_axials(app: &App, res: &sepika_solver::statics::linear::StaticOnce) -> Vec<f64> {
    let mut out = Vec::new();
    for (eid, mf) in &res.member_forces {
        let Some(e) = app.core.model.elements.get(eid.index()) else {
            continue;
        };
        if e.kind != ElementKind::Beam || e.nodes.len() != 2 {
            continue;
        }
        let (Some(na), Some(nb)) = (
            app.core.model.nodes.get(e.nodes[0].index()),
            app.core.model.nodes.get(e.nodes[1].index()),
        ) else {
            continue;
        };
        // 鉛直部材（柱）のみ対象。
        if (nb.coord[2] - na.coord[2]).abs() <= 1e-6 {
            continue;
        }
        let i_is_base = na.coord[2] < nb.coord[2];
        let base_node = if i_is_base { na } else { nb };
        if base_node.restraint == Dof6Mask::FREE {
            continue;
        }
        let (Some((_, fi)), Some((_, fj))) = (mf.at.first(), mf.at.last()) else {
            continue;
        };
        out.push(if i_is_base { fi[0] } else { fj[0] });
    }
    out
}

/// 有効数字 4 桁の指数表記へ丸める（スナップショット用）。
///
/// 手元（Windows）で生成したスナップショットを CI（Linux）で照合するため、
/// 浮動小数の環境差（ベクトル化の差・`faer` の並べ替え・libm の実装差。
/// 通常 1e-12 オーダー）が結果に出ないところまで桁を落とす。4 桁あれば
/// 設計上意味のある変化（例: T1 が 0.4676→0.4931）は確実に捕まる。
fn sig4(v: f64) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    // -0.0 が "-0.000e0" と "0.000e0" で揺れないよう正規化する。
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v:.3e}")
}

// ===================== 1. 取り込み =====================

/// ST-Bridge の取り込みが、想定どおりのモデル構成（部材・二次部材・床・階・
/// 荷重ケース・支点）を組み立てる。
///
/// 取り込み側の分類が変わると（例: 小梁を解析要素として取り込むようになる）、
/// 以降のすべての解析の前提が変わるため、まず構成を固定する。
#[test]
fn import_builds_expected_model() {
    let app = imported();
    let m = &app.core.model;

    assert_eq!(m.nodes.len(), 166, "節点数");
    assert_eq!(m.elements.len(), 115, "解析要素数（柱 40・大梁 75）");
    assert_eq!(m.beams().count(), 56, "二次部材（小梁）");
    assert_eq!(m.floor_regions.len(), 26, "床領域（大梁1床領域単位）");
    assert_eq!(m.stories.len(), 5, "階（1FL/2FL/3FL/RFL/PHRFL）");

    use sepika_core::region_gen::generate_region_boundaries;
    assert!(
        m.slabs
            .iter()
            .all(|s| !s.is_attached() && s.section().is_some()),
        "すべて Enclosed かつ版あり"
    );
    let boundaries = generate_region_boundaries(m);
    for r in &m.floor_regions {
        for sm in &r.secondary_beams {
            let coords = r.boundary_coords(m).expect("領域境界");
            let n = coords.len() as f64;
            let centroid = [
                coords.iter().map(|p| p[0]).sum::<f64>() / n,
                coords.iter().map(|p| p[1]).sum::<f64>() / n,
            ];
            let z = coords[0][2];
            let boundary = boundaries
                .iter()
                .find(|b| b.is_same_level(z) && b.contains(m, centroid))
                .expect("領域が大梁の境界に載る");
            let Some((a, b)) = m.secondary_member_end_points(sm) else {
                continue;
            };
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            assert!(
                boundary.contains(m, mid),
                "小梁 {:?} の中点が所属領域に入らない",
                sm.id
            );
        }
    }

    // 荷重は ST-Bridge に含まれないため標準荷重ケースが自動生成される。
    let names: Vec<&str> = m.load_cases.iter().map(|lc| lc.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["DL", "LL(架構用)", "LL(地震用)", "EX", "EY"],
        "標準荷重ケースが自動生成される"
    );
    assert!(!m.combinations.is_empty(), "標準荷重組合せも用意される");

    // 支点情報も含まれないため、最下レベルの柱脚がピン支点として自動設定される。
    let supports = m
        .nodes
        .iter()
        .filter(|n| n.restraint != Dof6Mask::FREE)
        .count();
    assert_eq!(supports, 12, "自動設定される支点の数");

    // 断面・材料が全部材へ割り当たっている（未割当があると解析前チェックで落ちる）。
    for e in &m.elements {
        assert!(
            e.section.is_some(),
            "部材 {:?} に断面が割り当たっていない",
            e.id
        );
    }
}

/// 取り込み時に、二次部材の支持端が支持部材アンカーへ解決されることを固定する。
#[test]
fn import_anchorizes_secondary_members() {
    let app = imported();
    let m = &app.core.model;
    let total = m.beams().count() + m.posts().count();
    let anchored = m
        .beams()
        .chain(m.posts())
        .filter(|sm| !sm.is_detached())
        .count();
    assert_eq!(
        anchored, total,
        "実フィクスチャの二次部材は全端をアンカーへ解決できる"
    );
}

/// ST-Bridge フィクスチャの `<StbNode>` 座標 [mm] を id で引く（テスト専用の簡易走査）。
/// 本番のパーサは `sepika-io` にあり、ここは元スラブの面積を独立に検算するためだけに使う。
fn fixture_node_coords(xml: &str) -> std::collections::HashMap<u32, [f64; 2]> {
    let mut nodes = std::collections::HashMap::new();
    for chunk in xml.split("<StbNode ").skip(1) {
        let head = chunk.split('>').next().unwrap_or("");
        let (Some(id), Some(x), Some(y)) = (
            stb_attr(head, "id").and_then(|v| v.parse::<u32>().ok()),
            stb_attr(head, "X").and_then(|v| v.parse::<f64>().ok()),
            stb_attr(head, "Y").and_then(|v| v.parse::<f64>().ok()),
        ) else {
            continue;
        };
        nodes.insert(id, [x, y]);
    }
    nodes
}

/// タグの属性値 `name="value"` を取り出す。
fn stb_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    Some(&rest[..rest.find('"')?])
}

/// ST-Bridge フィクスチャの元スラブ総面積 [mm²] と、板厚を掛けた総和 [mm³] を返す。
/// 板厚は `StbSecSlab_RC` の `depth`（無ければ `thickness`）を使う。
fn fixture_source_slab_measures(xml: &str) -> (f64, f64) {
    use sepika_core::geom::polygon;
    let nodes = fixture_node_coords(xml);
    let mut thickness = std::collections::HashMap::new();
    for chunk in xml.split("<StbSecSlab_RC ").skip(1) {
        let block = chunk.split("</StbSecSlab_RC>").next().unwrap_or("");
        let (Some(id), Some(t)) = (
            stb_attr(block, "id").and_then(|v| v.parse::<u32>().ok()),
            stb_attr(block, "depth")
                .or_else(|| stb_attr(block, "thickness"))
                .and_then(|v| v.parse::<f64>().ok()),
        ) else {
            continue;
        };
        thickness.insert(id, t);
    }
    let mut area_sum = 0.0;
    let mut thickness_sum = 0.0;
    for chunk in xml.split("<StbSlab ").skip(1) {
        let block = chunk.split("</StbSlab>").next().unwrap_or("");
        let head = block.split('>').next().unwrap_or("");
        let section = stb_attr(head, "id_section").and_then(|v| v.parse::<u32>().ok());
        let order = block
            .split("<StbNodeIdOrder>")
            .nth(1)
            .and_then(|rest| rest.split("</StbNodeIdOrder>").next());
        let Some(order) = order else {
            continue;
        };
        let poly: Vec<[f64; 2]> = order
            .split_whitespace()
            .filter_map(|v| v.parse::<u32>().ok())
            .filter_map(|id| nodes.get(&id).copied())
            .collect();
        if poly.len() < 3 {
            continue;
        }
        let area = polygon::area(&poly);
        area_sum += area;
        if let Some(t) = section.and_then(|s| thickness.get(&s)) {
            thickness_sum += area * t;
        }
    }
    (area_sum, thickness_sum)
}

/// 取り込み後も床板の総面積と、総重量に比例する「面積×板厚」が元 StbSlab と一致すること。
///
/// 割当領域と境界が一致しない版を領域ごとに切り分けるとき、面積を領域面積へ置き換えると
/// 総重量が動く。ここでは元フィクスチャの境界から独立に面積を求め、取り込み結果と比べる。
/// フィクスチャの床板はすべて同じコンクリート（Fc21）なので、面積×板厚の一致は総重量の一致を意味する。
#[test]
fn import_preserves_stbridge_slab_area_and_weight() {
    let app = imported();
    let m = &app.core.model;

    let xml = std::fs::read_to_string(fixture_path()).expect("フィクスチャを読む");
    let (source_area, source_area_thickness) = fixture_source_slab_measures(&xml);

    let mut slab_area = 0.0;
    let mut slab_area_thickness = 0.0;
    for slab in &m.slabs {
        let coords = slab
            .boundary_coords(m)
            .unwrap_or_else(|| panic!("床板 {:?} の境界が解決できない", slab.id));
        let area = sepika_core::geom::polygon::area_xy(&coords);
        slab_area += area;
        if let Some(t) = m.slab_plate_thickness(slab) {
            slab_area_thickness += area * t;
        }
    }

    let rel = |a: f64, b: f64| {
        if b == 0.0 {
            (a - b).abs()
        } else {
            (a - b).abs() / b.abs()
        }
    };
    assert!(
        rel(slab_area, source_area) < 1e-6,
        "床板総面積: 取り込み {slab_area} / 元 {source_area}"
    );
    assert!(
        rel(slab_area_thickness, source_area_thickness) < 1e-6,
        "床板の総重量（面積×板厚）: 取り込み {slab_area_thickness} / 元 {source_area_thickness}"
    );
}

// ===================== 2. 準備計算 =====================

/// 準備計算が階・地震重量・Ai 分布・剛域・仕口パネルを算定する。
///
/// 地震力については「ΣW から Q1 が令 88 条どおり求まる」ことを式で検算する
/// （`Q1 = Z·Rt·C0·ΣW`。Ai は最下層で 1.0）。準備計算の内部でどう組み立てても、
/// この恒等式は成り立たなければならない。
#[test]
fn preparation_computes_stories_and_seismic_forces() {
    let app = prepared();
    let prep = app
        .core
        .scoped
        .preparation
        .as_ref()
        .expect("準備計算の結果が入るはず");

    assert!(prep.is_ready(), "整合性チェックにエラーがない");
    assert_eq!(
        app.core.model.stories.len(),
        5,
        "階（1FL/2FL/3FL/RFL/PHRFL）"
    );
    // 準備計算の階の表は「層」（階と階の間）を並べるため、階数より 1 少ない。
    assert_eq!(prep.stories.len(), 4, "階の分布の行数");
    assert_eq!(prep.summary.n_supports, 12, "支点数");
    assert_eq!(prep.summary.n_diaphragms, 5, "剛床数");
    assert!(
        prep.summary.total_seismic_weight > 0.0,
        "地震用重量の総和が正"
    );

    // 階は下から上へ、床レベルが単調増加する。
    let elevations: Vec<f64> = prep.stories.iter().map(|s| s.elevation).collect();
    assert!(
        elevations.windows(2).all(|w| w[0] < w[1]),
        "階は下階→上階の順（床レベル単調増加）: {elevations:?}"
    );

    for s in &prep.stories {
        assert!(s.height > 0.0, "階 {} の階高が正", s.name);
        assert!(s.weight > 0.0, "階 {} の地震用重量が正", s.name);
    }

    let seismic = prep.seismic.as_ref().expect("地震力が算定されるはず");
    assert_eq!(seismic.rows.len(), prep.stories.len(), "Ai 分布の層数");
    assert!(
        !seismic.clamped_negative_pi,
        "Pi に負値クランプが発生しない"
    );
    assert!(seismic.t > 0.0, "設計用一次固有周期 T が正");
    assert!(
        seismic.rt > 0.0 && seismic.rt <= 1.0,
        "振動特性係数 Rt は 0<Rt≤1"
    );

    // 令88条: Q1 = Z·Rt·C0·ΣW（最下層は Ai=1.0）。
    let expected_q1 = seismic.z * seismic.rt * seismic.c0 * prep.summary.total_seismic_weight;
    assert!(
        (seismic.base_shear - expected_q1).abs() <= expected_q1.abs() * 1e-9,
        "基部せん断力 Q1={} が Z·Rt·C0·ΣW={} と一致しない",
        seismic.base_shear,
        expected_q1
    );

    // 層せん断力 Qi は上階ほど小さい（Ai 分布の性質）。
    let qi: Vec<f64> = seismic.rows.iter().map(|r| r.qi).collect();
    assert!(
        qi.windows(2).all(|w| w[0] >= w[1]),
        "層せん断力 Qi が上階ほど小さくない: {qi:?}"
    );

    // 剛域・仕口パネル・断面性能が算定されている。
    assert!(
        prep.rigid_zone_candidates > 0,
        "剛域の算定対象となる梁がある"
    );
    assert!(!prep.sections.is_empty(), "断面性能が算定される");
    assert_eq!(
        prep.load_cases.len(),
        app.core.model.load_cases.len(),
        "荷重ケースの集計行数"
    );
}

/// 準備計算は 1 回目から冪等である（何度実行しても階・地震用重量・固定荷重が同じ）。
///
/// RC/SRC 梁の自重は柱面間の内法長（節点間長 − 両端の柱フェース距離
/// `RigidZone::face_i/face_j`）で算定するため、自重の同期は
/// フェース距離の算定より後に行わなければならない。かつては
/// `generate_stories_action` が同期を先に行っており、1 回目の準備計算だけ
/// フェース距離が未算定（0）のまま節点間距離で算定した過大な自重が DL に入り、
/// 2 回目の実行で初めて正しい値へ変わっていた。準備計算は各解析の実行前にも
/// 自動で走るため、「準備計算を実行した回数」で柱脚軸力・断面検定が変わる
/// 状態だった。
#[test]
fn preparation_is_idempotent() {
    let mut app = prepared();
    assert_eq!(
        app.core.model.floor_regions.len(),
        26,
        "準備計算後も床領域は 26"
    );
    let stories = app.core.model.stories.len();
    let weights: Vec<Option<f64>> = app
        .core
        .model
        .stories
        .iter()
        .map(|s| s.seismic_weight)
        .collect();
    let dl = auto_case(&app, DL_CASE_NAME);

    for n in 2..=3 {
        app.run_preparation();
        assert_no_error(&app, &format!("準備計算（{n} 回目）"));
        assert_eq!(
            app.core.model.stories.len(),
            stories,
            "{n} 回目で階数が変わった"
        );
        assert_eq!(
            app.core
                .model
                .stories
                .iter()
                .map(|s| s.seismic_weight)
                .collect::<Vec<_>>(),
            weights,
            "{n} 回目で地震用重量が変わった"
        );
        assert_eq!(
            auto_case(&app, DL_CASE_NAME),
            dl,
            "{n} 回目で固定荷重 DL の内容が変わった"
        );
    }
}

// ===================== 3. 診断 =====================

/// 実建物モデルの整合性チェックがエラー・警告ともに 0 件である。
///
/// 診断が誤検知を出すようになると、利用者は解析前に赤い表示を見ることになる。
/// 逆に検出漏れが起きると解析が謎のエラーで落ちる。実モデルで 0 件を固定しておく。
#[test]
fn diagnostics_are_clean() {
    let mut app = prepared();
    app.run_diagnostics();
    assert_no_error(&app, "診断");
    assert_eq!(
        app.diagnostics_counts(),
        (0, 0),
        "実建物モデルの診断は (エラー, 警告) = (0, 0) のはず"
    );
}

// ===================== 4. 静的解析（全荷重ケース・全組合せ） =====================

/// 全荷重ケース・全荷重組合せが解け、すべての解析対象部材に断面力が入る。
///
/// 「解析は成功したのに一部の部材だけ断面力が空」という静かな劣化
/// （剛床上の梁・二次部材が絡む経路で過去に発生）を検出する。
#[test]
fn static_all_solves_every_case_and_combination() {
    let app = analyzed();
    let results = app.core.scoped.results.as_ref().expect("解析結果");
    let n_elems = frame_elem_count(&app);

    assert_eq!(results.statics.len(), 5, "荷重ケース単体の結果数");
    assert_eq!(
        results.combos.len(),
        app.core.model.combinations.len(),
        "荷重組合せの結果数"
    );

    for (key, once) in &results.statics {
        assert_eq!(
            once.disp.len(),
            app.core.model.nodes.len(),
            "{key:?}: 変位が全節点分ない"
        );
        assert!(
            once.disp.iter().flatten().all(|v| v.is_finite()),
            "{key:?}: 変位に非有限値がある"
        );
        assert_eq!(
            once.member_forces.len(),
            n_elems,
            "{key:?}: 断面力が全部材分ない（応力の欠落）"
        );
        for (eid, mf) in &once.member_forces {
            assert!(!mf.at.is_empty(), "{key:?}: 部材 {eid:?} の評価断面が空");
            assert!(
                mf.at.iter().all(|(_, f)| f.iter().all(|v| v.is_finite())),
                "{key:?}: 部材 {eid:?} の断面力に非有限値がある"
            );
        }
    }

    for (name, once) in &results.combos {
        assert_eq!(
            once.member_forces.len(),
            n_elems,
            "組合せ「{name}」: 断面力が全部材分ない"
        );
    }
}

/// 固定荷重（DL）の応答が鉛直荷重の伝達経路として妥当である。
///
/// - すべての節点が下向き（または不動）に変位する
/// - すべての柱脚が圧縮（軸力が負）
///
/// スラブ分配・小梁の CMQ 変換・剛床のいずれかが荷重を落とすと、柱脚軸力の合計が
/// 変わる。合計値そのものは [`snapshot_key_scalars`] で固定する。
#[test]
fn dead_load_transfers_to_column_bases() {
    let app = analyzed();
    let dl = static_of(&app, StaticCaseKey::User(dl_case_id(&app)));

    let max_uz = dl
        .disp
        .iter()
        .map(|d| d[2])
        .fold(f64::NEG_INFINITY, f64::max);
    let min_uz = dl.disp.iter().map(|d| d[2]).fold(f64::INFINITY, f64::min);
    assert!(
        max_uz <= 1e-9,
        "固定荷重で上向きに変位する節点がある（最大 uz={max_uz}）"
    );
    assert!(min_uz < 0.0, "固定荷重で誰も沈まない（荷重が載っていない）");

    let axials = base_column_axials(&app, dl);
    assert_eq!(axials.len(), 12, "柱脚の本数（支点に取り付く柱）");
    for (i, n) in axials.iter().enumerate() {
        assert!(*n < 0.0, "柱脚 {i} が固定荷重で圧縮になっていない（N={n}）");
    }
}

/// 荷重組合せの結果が、参照する荷重ケース単体の線形和と一致する（重ね合わせの原理）。
///
/// 組合せの求解は荷重ケース単体の線形和として組み立てられるため
/// （`Analysis::linear_combination`）、実モデルでもこの関係が保たれる。
/// **全組合せ**を対象に検算する（1 件だけだと、組合せの構成が変わったときに
/// 検証していない組合せが増えても気づけない）。
#[test]
fn combination_is_linear_sum_of_load_cases() {
    let app = analyzed();
    let results = app.core.scoped.results.as_ref().expect("解析結果");
    assert!(!results.combos.is_empty(), "荷重組合せの結果が空");

    for (name, combo_res) in &results.combos {
        let combo = app
            .core
            .model
            .combinations
            .iter()
            .find(|c| c.name == *name)
            .unwrap_or_else(|| panic!("組合せ「{name}」の定義が見つからない"));

        let mut expected = vec![[0.0_f64; 6]; app.core.model.nodes.len()];
        for (case, factor) in &combo.terms {
            let once = static_of(&app, static_case_key(&app, *case));
            for (dst, src) in expected.iter_mut().zip(once.disp.iter()) {
                for k in 0..6 {
                    dst[k] += factor * src[k];
                }
            }
        }
        for (i, (got, want)) in combo_res.disp.iter().zip(expected.iter()).enumerate() {
            for k in 0..6 {
                let tol = want[k].abs() * 1e-9 + 1e-9;
                assert!(
                    (got[k] - want[k]).abs() <= tol,
                    "組合せ「{name}」節点 {i} 成分 {k}: {} != {}（線形和と不一致）",
                    got[k],
                    want[k]
                );
            }
        }
    }
}

// ===================== 5. 固有値解析 =====================

/// 固有値解析が指定モード数の正の固有周期を降順で返し、モード形状が全節点分ある。
#[test]
fn eigen_returns_descending_positive_periods() {
    let app = analyzed();
    let modal = app
        .core
        .scoped
        .results
        .as_ref()
        .expect("解析結果")
        .modal
        .as_ref()
        .expect("固有値解析の結果");

    assert_eq!(modal.period.len(), 3, "モード数（既定 3）");
    assert!(
        modal.period.iter().all(|t| t.is_finite() && *t > 0.0),
        "固有周期に非正・非有限がある: {:?}",
        modal.period
    );
    assert!(
        modal.period.windows(2).all(|w| w[0] > w[1]),
        "固有周期が降順でない（1 次が最長のはず）: {:?}",
        modal.period
    );
    assert_eq!(modal.node_shapes.len(), 3, "モード形状の本数");
    for (i, shape) in modal.node_shapes.iter().enumerate() {
        assert_eq!(
            shape.len(),
            app.core.model.nodes.len(),
            "{i} 次モードの形状が全節点分ない"
        );
        assert!(
            shape.iter().flatten().all(|v| v.is_finite()),
            "{i} 次モードの形状に非有限値がある"
        );
        assert!(
            shape.iter().flatten().any(|v| v.abs() > 1e-12),
            "{i} 次モードの形状が全ゼロ（サイレント失敗）"
        );
    }
}

// ===================== 6. 地震静的解析（Ai 分布） =====================

/// 地震静的解析が X・Y 両方向で解け、上階ほど水平変位が大きくなる。
///
/// 剛床が効いていない・水平力が一部の階に載っていないといった配線の破壊は、
/// 「階の水平変位が上階へ向かって単調増加しない」形で現れる。
#[test]
fn seismic_static_produces_monotonic_story_displacement() {
    for dir in [SeismicDir::X, SeismicDir::Y] {
        let mut app = prepared();
        app.run_seismic(dir);
        assert_no_error(&app, &format!("地震静的解析 {dir:?}"));

        let res = static_of(&app, StaticCaseKey::Seismic(dir));
        let comp = match dir {
            SeismicDir::X => 0,
            SeismicDir::Y => 1,
        };

        let mut prev = -1.0_f64;
        for s in &app.core.model.stories {
            let mx = s
                .node_ids
                .iter()
                .filter_map(|n| res.disp.get(n.index()))
                .map(|d| d[comp].abs())
                .fold(0.0_f64, f64::max);
            assert!(
                mx >= prev,
                "{dir:?}: 階 {} の水平変位 {mx} が下階の {prev} より小さい",
                s.name
            );
            prev = mx;
        }
        assert!(
            prev > 0.0,
            "{dir:?}: 最上階が全く動いていない（水平力が載っていない）"
        );
    }
}

// ===================== 7. 一次設計（断面検定） =====================

/// 断面検定が全部材・全接合部・全スラブを対象に実施され、検定比が有限かつ非負。
#[test]
fn design_check_covers_every_member() {
    let mut app = analyzed();
    app.run_design_check();
    assert_no_error(&app, "断面検定");

    let results = app.core.scoped.results.as_ref().expect("解析結果");
    assert_eq!(
        results.member_checks.len(),
        frame_elem_count(&app),
        "検定された部材が全部材分ない"
    );
    assert!(!results.joint_checks.is_empty(), "接合部の検定結果が空");
    assert!(!results.slab_checks.is_empty(), "スラブの検定結果が空");

    let mut checked = 0usize;
    let mut skipped = 0usize;
    let mut rc_src_positions = 0usize;
    let mut rc_src_checked = 0usize;
    for mc in &results.member_checks {
        assert!(
            !mc.positions.is_empty(),
            "部材 {:?} の検定位置が空",
            mc.elem
        );
        let is_rc_src = app
            .core
            .model
            .elements
            .get(mc.elem.index())
            .and_then(|e| e.section)
            .and_then(|sid| app.core.model.sections.get(sid.index()))
            .and_then(|sec| sec.shape.as_ref())
            .is_some_and(|shape| {
                matches!(
                    shape,
                    sepika_core::section_shape::SectionShape::RcBeamRect { .. }
                        | sepika_core::section_shape::SectionShape::RcColumnRect { .. }
                        | sepika_core::section_shape::SectionShape::RcColumnCircle { .. }
                        | sepika_core::section_shape::SectionShape::SrcBeamRect { .. }
                        | sepika_core::section_shape::SectionShape::SrcColumnRect { .. }
                )
            });
        for p in &mc.positions {
            if let sepika_design_jp::CheckOutcome::Checked(r) = &p.outcome {
                let ratio = r.ratio();
                assert!(
                    ratio.is_finite() && ratio >= 0.0,
                    "部材 {:?} 位置 {} の検定比が異常: {ratio}",
                    mc.elem,
                    p.xi
                );
                checked += 1;
                if is_rc_src {
                    rc_src_checked += 1;
                }
            } else if let sepika_design_jp::CheckOutcome::Skipped { reason } = &p.outcome {
                assert!(
                    !reason.trim().is_empty(),
                    "Skipped の理由が空: 部材 {:?} 位置 {}",
                    mc.elem,
                    p.xi
                );
                skipped += 1;
            }
            if is_rc_src {
                rc_src_positions += 1;
            }
        }
    }
    assert!(checked > 0, "検定が 1 件も実施されていない（全件 Skipped）");
    assert_eq!(skipped, 0, "Skipped がある（全位置 Checked のはず）");
    assert_eq!(checked, 345, "Checked 位置数");
    assert!(
        rc_src_positions > 0,
        "RC/SRC 部材の検定位置がない（フィクスチャに RC 部材があるはず）"
    );
    assert_eq!(
        rc_src_checked, rc_src_positions,
        "RC/SRC 部材に Skipped がある"
    );
    eprintln!("full_model member check positions: Checked={checked}, Skipped={skipped}");
}

// ===================== 8. 二次設計（層指標） =====================

/// 層間変形角・剛性率 Rs・偏心率 Re が全層で算定される。
#[test]
fn story_metrics_computed_for_every_layer() {
    let app = analyzed();
    let results = app.core.scoped.results.as_ref().expect("解析結果");
    let ex = static_of(&app, StaticCaseKey::Seismic(SeismicDir::X));
    let ctx = sepika_app::summary::metrics_ctx_from_results(Some(results));
    let metrics = sepika_app::summary::compute_story_metrics_with(
        &app.core.model,
        &ex.disp,
        SeismicDir::X,
        &ctx,
    );

    assert_eq!(metrics.len(), 4, "層指標の層数（階数 5 − 1）");
    for m in &metrics {
        assert!(m.height > 0.0, "層 {} の階高が正でない", m.name);
        assert!(
            m.drift > 0.0 && m.drift.is_finite(),
            "層 {} の層間変位が異常: {}",
            m.name,
            m.drift
        );
        assert!(
            m.rs > 0.0 && m.rs.is_finite(),
            "層 {} の剛性率が異常: {}",
            m.name,
            m.rs
        );
        assert!(
            m.re >= 0.0 && m.re.is_finite(),
            "層 {} の偏心率が異常: {}",
            m.name,
            m.re
        );
        assert!(m.fes >= 1.0, "層 {} の Fes が 1.0 未満: {}", m.name, m.fes);
    }
}

// ===================== 9. 保有水平耐力・Ds・終局検定 =====================

/// 鋼構造サンプルの保有水平耐力・Ds・ランク、および元STBの静的結果によるRC終局検定。
#[test]
fn steel_portal_holding_capacity_and_imported_rc_ultimate_checks() {
    let mut app = prepared_steel_portal(true);
    app.run_static_all();
    assert_no_error(&app, "鋼構造サンプルの静的解析");
    app.run_eigen(app.core.analysis_cfg.n_modes);
    assert_no_error(&app, "鋼構造サンプルの固有値解析");
    app.run_pushover();
    assert_no_error(&app, "増分解析");

    let (holding, ranks) = app
        .compute_holding_capacity()
        .expect("保有水平耐力が算定できるはず");
    assert_eq!(holding.stories.len(), 4, "保有水平耐力の層数");
    assert_eq!(ranks.len(), 4, "層ごとの部材ランク");
    for s in &holding.stories {
        assert!(
            s.qu > 0.0 && s.qu.is_finite(),
            "保有水平耐力 Qu が異常: {}",
            s.qu
        );
        assert!(
            (0.25..=0.55).contains(&s.ds),
            "構造特性係数 Ds が規定の範囲外: {}",
            s.ds
        );
        assert!(
            s.qun > 0.0 && s.qun.is_finite(),
            "必要保有水平耐力 Qun が異常: {}",
            s.qun
        );
        assert!(s.fes >= 1.0, "Fes が 1.0 未満: {}", s.fes);
    }

    insta::assert_snapshot!(
        "four_story_steel_portal_holding_capacity",
        holding
            .stories
            .iter()
            .enumerate()
            .map(|(i, story)| format!(
                "story[{i}].Qu={}\nstory[{i}].Qun={}\nstory[{i}].Ds={}\nstory[{i}].Fes={}",
                sig4(story.qu),
                sig4(story.qun),
                sig4(story.ds),
                sig4(story.fes),
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut app = analyzed();
    let ultimate = app
        .compute_ultimate_checks()
        .expect("終局検定が算定できるはず");
    assert!(!ultimate.is_empty(), "RC 部材の終局検定が 1 件もない");
    for u in &ultimate {
        assert!(u.mu.is_finite() && u.mu > 0.0, "Mu が異常: {}", u.mu);
        assert!(u.qsu.is_finite() && u.qsu > 0.0, "Qsu が異常: {}", u.qsu);
        assert!(
            u.shear_margin.is_finite() && u.shear_margin > 0.0,
            "せん断余裕度が異常: {}",
            u.shear_margin
        );
    }
}

// ===================== 10. 増分解析（プッシュオーバー） =====================

/// 鋼構造サンプルの増分解析で性能曲線・層せん断が力学的に整合する。
#[test]
fn pushover_reaches_ultimate_state() {
    let mut app = prepared_steel_portal(true);
    app.run_pushover();
    assert_no_error(&app, "増分解析");

    let push = app
        .core
        .scoped
        .results
        .as_ref()
        .expect("解析結果")
        .pushover
        .as_ref()
        .expect("増分解析の結果");

    assert!(
        push.steps.len() > 1,
        "確定ステップが 1 つ以下（即座に発散）"
    );
    assert!(!push.capacity_curve.is_empty(), "性能曲線が空");
    assert!(
        push.qu > 0.0 && push.qu.is_finite(),
        "保有水平耐力 Qu が異常: {}",
        push.qu
    );

    insta::assert_snapshot!(
        "four_story_steel_portal_pushover",
        format!(
            "steps={}\nQu={}\nmechanism={:?}\nhinges={}",
            push.steps.len(),
            sig4(push.qu),
            push.mechanism,
            push.hinges.len(),
        )
    );

    // 屋根変位は増分とともに単調増加する。
    let roof: Vec<f64> = push.capacity_curve.iter().map(|c| c.roof_disp).collect();
    assert!(
        roof.windows(2).all(|w| w[1] >= w[0]),
        "性能曲線の屋根変位が単調増加でない: {roof:?}"
    );

    let last = push.capacity_curve.last().expect("最終ステップ");
    assert!(
        (last.base_shear - push.qu).abs() <= push.qu.abs() * 1e-9,
        "最終ステップのベースシア {} が Qu {} と一致しない",
        last.base_shear,
        push.qu
    );
    assert_eq!(last.story_shear.len(), 4, "4層の層せん断");
    assert!(last.story_shear.windows(2).next().is_some());
    // 層せん断はベースシアから始まり、上階へ向かって減少する（水平力の累積）。
    assert!(
        (last.story_shear[0] - last.base_shear).abs() <= last.base_shear.abs() * 1e-9,
        "最下層のせん断力がベースシアと一致しない: {:?}",
        last.story_shear
    );
    assert!(
        last.story_shear.windows(2).all(|w| w[0] >= w[1]),
        "層せん断力が上階ほど小さくない: {:?}",
        last.story_shear
    );
    // 最終ステップの部材別応答が記録される（保有水平耐力・部材ランクの入力になる）。
    assert!(
        !push.member_response.is_empty(),
        "最終確定ステップの部材別応答が空"
    );
}

// ===================== 11. 時刻歴応答解析 =====================

/// 線形時刻歴応答解析（サンプル波）が完走し、応答が有限で層応答が記録される。
#[test]
fn time_history_linear_runs() {
    let mut app = prepared();
    app.core.analysis_cfg.th_dir = ThDir::X;
    app.core.analysis_cfg.th_nonlinear = false;
    app.run_time_history_sample();
    assert_no_error(&app, "線形時刻歴応答解析");

    let th = app
        .core
        .scoped
        .results
        .as_ref()
        .expect("解析結果")
        .time_history
        .as_ref()
        .expect("時刻歴の結果");

    let expected_frames =
        (app.core.analysis_cfg.th_duration / app.core.analysis_cfg.th_dt).round() as usize + 1;
    assert_eq!(th.time.len(), expected_frames, "時刻ステップ数");
    assert!(!th.nonlinear, "線形として記録される");
    assert_eq!(
        th.peak_disp.len(),
        app.core.model.nodes.len(),
        "ピーク変位が全節点分ない"
    );
    assert!(
        th.peak_disp.iter().flatten().all(|v| v.is_finite()),
        "ピーク変位に非有限値がある（発散）"
    );
    assert!(
        th.peak_disp.iter().any(|d| d[0].abs() > 1e-6),
        "X 加振なのに X 方向の応答がゼロ"
    );
    assert_eq!(th.story_drift_angle.len(), 4, "層間変形角の層数");
    assert!(
        th.story_drift_angle
            .iter()
            .all(|a| a.is_finite() && *a > 0.0),
        "層間変形角が異常: {:?}",
        th.story_drift_angle
    );
}

/// 鋼構造サンプルの120秒波形で、減衰末尾を含め非収束を生じない。
///
/// 元STBで同定された収束不具合の条件はT形骨格未対応のため現在は完走検証できない。
/// 有効な別モデルでも長期荷重初期化なし・dt=.05の長時間検証を維持する。
#[test]
fn time_history_nonlinear_long_duration_has_no_false_non_convergence() {
    let mut app = prepared_steel_portal(false);
    app.core.analysis_cfg.th_dir = ThDir::X;
    app.core.analysis_cfg.th_nonlinear = true;
    app.core.analysis_cfg.th_apply_long_term = false;
    app.core.analysis_cfg.th_duration = 120.0;
    // 刻みは既定より粗くする（この不具合は応答の減衰で決まり、刻みには依らない）。
    // 既定の 0.01 秒では 12000 ステップになり、テスト時間が 5 倍以上に伸びる。
    app.core.analysis_cfg.th_dt = 0.05;
    app.run_time_history_sample();
    assert_no_error(&app, "非線形時刻歴応答解析（120 秒）");

    let th = app
        .core
        .scoped
        .results
        .as_ref()
        .expect("解析結果")
        .time_history
        .as_ref()
        .expect("時刻歴の結果");
    assert!(th.nonlinear);
    assert!(!th.applied_long_term);
    assert!((th.time.last().unwrap() - 120.0).abs() < 1e-9);
    assert!(th.peak_disp.iter().flatten().any(|v| v.abs() > 0.0));
    assert_eq!(
        th.non_converged_steps, 0,
        "減衰しきった末尾で偽の非収束が出ている"
    );
    assert!(
        th.peak_disp.iter().flatten().all(|v| v.is_finite()),
        "ピーク変位に非有限値がある"
    );
}

/// 明示RC矩形フレームの減衰末尾で、ピーク力下限を欠く旧判定による偽非収束を検出する。
#[test]
fn rectangular_rc_decay_tail_has_no_false_non_convergence() {
    let mut app = prepared_rectangular_rc_portal();
    app.core.analysis_cfg.th_dir = ThDir::X;
    app.core.analysis_cfg.th_nonlinear = true;
    app.core.analysis_cfg.th_apply_long_term = false;
    app.core.analysis_cfg.th_duration = 120.0;
    app.core.analysis_cfg.th_dt = 0.05;
    app.core.analysis_cfg.th_amp = 10000.0;
    app.core.analysis_cfg.th_period = 1.0;
    app.run_time_history_sample();
    assert_no_error(&app, "RC矩形の減衰末尾回帰");
    let th = app
        .core
        .scoped
        .results
        .as_ref()
        .unwrap()
        .time_history
        .as_ref()
        .unwrap();
    assert!(th.nonlinear);
    assert!(!th.applied_long_term);
    assert_eq!(th.time.len(), 2401);
    assert!((th.time.last().unwrap() - 120.0).abs() < 1e-9);
    assert_eq!(th.non_converged_steps, 0, "減衰末尾で偽の非収束が出ている");
    assert!(th.peak_disp.iter().flatten().all(|v| v.is_finite()));
    assert!(th.peak_disp.iter().flatten().any(|v| v.abs() > 0.0));
}

/// 既存1層鋼サンプルの非線形時刻歴が長期荷重初期化を含め完走する。
#[test]
fn time_history_nonlinear_runs() {
    let mut app = prepared_steel_portal(false);
    app.core.analysis_cfg.th_dir = ThDir::X;
    app.core.analysis_cfg.th_nonlinear = true;
    app.run_time_history_sample();
    assert_no_error(&app, "非線形時刻歴応答解析");

    let th = app
        .core
        .scoped
        .results
        .as_ref()
        .expect("解析結果")
        .time_history
        .as_ref()
        .expect("時刻歴の結果");
    insta::assert_snapshot!(
        "steel_portal_nonlinear_time_history",
        format!(
            "frames={}\npeak_ux={}\ndrift_angle={}",
            th.time.len(),
            sig4(th.peak_disp.iter().map(|d| d[0].abs()).fold(0.0, f64::max)),
            sig4(th.story_drift_angle[0]),
        )
    );
    assert!(th.nonlinear, "非線形として記録される");
    assert!(
        th.applied_long_term,
        "既定の長期荷重初期化が解析まで反映される"
    );
    assert!(
        th.peak_disp.iter().flatten().all(|v| v.is_finite()),
        "ピーク変位に非有限値がある（発散）"
    );
}

// ===================== 12. 保存・読込の往復 =====================

/// OVIKA へ保存し、読み直してもモデル・準備計算・結果・解析条件が保たれる。
#[test]
fn ovika_roundtrip_preserves_model_and_results() {
    let mut app = analyzed();
    app.run_design_check();
    clear_error(&mut app);
    // 既定値から変えておき、往復で「既定値に戻ってしまう」誤りを検出できるようにする。
    app.core.analysis_cfg.th_damping = 0.037;
    app.core.analysis_cfg.th_damping_model = ThDampingModel::Rayleigh;
    app.core.analysis_cfg.n_modes = 5;

    let path = test_tmp().join("full_model_roundtrip.ovika");
    app.save_project_to(path.clone());
    assert_no_error(&app, "プロジェクト保存");

    let contents = sepika_io::ovika::load_ovika(&path).unwrap();
    contents.model.validate().unwrap();
    assert!(app.core.model.eq_ignoring_dofmap(&contents.model));
    for bytes in [
        contents.preparation.as_ref().unwrap(),
        contents.results.as_ref().unwrap(),
        contents.analysis_settings.as_ref().unwrap(),
    ] {
        let fields: std::collections::BTreeMap<String, serde::de::IgnoredAny> =
            rmp_serde::from_slice(bytes).unwrap();
        assert!(!fields.is_empty(), "任意 payload のトップレベルは map");
    }
    #[derive(serde::Deserialize)]
    struct ReorderedPreparation {
        diag_warnings: usize,
        #[serde(default)]
        added_field: bool,
        computed_at: std::time::SystemTime,
    }
    let prep: ReorderedPreparation =
        rmp_serde::from_slice(contents.preparation.as_ref().unwrap()).unwrap();
    let original_prep = app.core.scoped.preparation.as_ref().unwrap();
    assert_eq!(prep.computed_at, original_prep.computed_at);
    assert_eq!(prep.diag_warnings, original_prep.diag_warnings);
    assert!(!prep.added_field);

    #[derive(serde::Deserialize)]
    struct ReorderedResults {
        last_run: Option<std::time::SystemTime>,
        #[serde(default)]
        added_field: bool,
        bundle: sepika_app::app::ResultsBundle,
    }
    let results: ReorderedResults =
        rmp_serde::from_slice(contents.results.as_ref().unwrap()).unwrap();
    assert_eq!(results.last_run, app.core.scoped.staleness.last_run);
    assert!(!results.added_field);
    assert_eq!(
        rmp_serde::to_vec_named(&results.bundle).unwrap(),
        rmp_serde::to_vec_named(app.core.scoped.results.as_ref().unwrap()).unwrap()
    );

    #[derive(serde::Deserialize)]
    struct ReorderedSettings {
        wave_name: Option<String>,
        #[serde(default)]
        added_field: bool,
        cfg: ReorderedConfig,
    }
    #[derive(serde::Deserialize)]
    struct ReorderedConfig {
        th_damping: f64,
        n_modes: usize,
    }
    let settings: ReorderedSettings =
        rmp_serde::from_slice(contents.analysis_settings.as_ref().unwrap()).unwrap();
    assert_eq!(settings.wave_name, app.core.scoped.wave_library_selection);
    assert_eq!(settings.cfg.th_damping, app.core.analysis_cfg.th_damping);
    assert_eq!(settings.cfg.n_modes, app.core.analysis_cfg.n_modes);
    assert!(!settings.added_field);

    let mut reopened = App::default();
    reopened.core.analysis_cfg.threads = 1;
    reopened.open_project_from(path.clone());
    assert_no_error(&reopened, "プロジェクト読込");

    reopened.core.model.validate().unwrap();
    assert!(app.core.model.eq_ignoring_dofmap(&reopened.core.model));
    assert_eq!(
        rmp_serde::to_vec_named(reopened.core.scoped.results.as_ref().unwrap()).unwrap(),
        rmp_serde::to_vec_named(&results.bundle).unwrap()
    );
    assert_eq!(
        rmp_serde::to_vec_named(reopened.core.scoped.preparation.as_ref().unwrap()).unwrap(),
        rmp_serde::to_vec_named(original_prep).unwrap()
    );
    assert_eq!(
        rmp_serde::to_vec_named(&reopened.core.analysis_cfg).unwrap(),
        rmp_serde::to_vec_named(&app.core.analysis_cfg).unwrap()
    );

    assert_eq!(
        reopened.core.model.nodes.len(),
        app.core.model.nodes.len(),
        "節点数"
    );
    assert_eq!(
        reopened.core.model.elements.len(),
        app.core.model.elements.len(),
        "要素数"
    );
    assert_eq!(
        reopened.core.model.beams().count(),
        app.core.model.beams().count(),
        "二次部材数"
    );
    assert_eq!(
        reopened.core.model.floor_regions.len(),
        app.core.model.floor_regions.len(),
        "スラブ数"
    );
    assert_eq!(
        reopened.core.model.stories.len(),
        app.core.model.stories.len(),
        "階数"
    );

    let before = app.core.scoped.results.as_ref().expect("保存前の結果");
    let after = reopened
        .core
        .scoped
        .results
        .as_ref()
        .expect("読込後に結果が復元されるはず");
    assert_eq!(after.statics.len(), before.statics.len(), "静的結果の件数");
    assert_eq!(after.combos.len(), before.combos.len(), "組合せ結果の件数");
    assert!(after.modal.is_some(), "固有値解析の結果が復元される");

    // 解析タブの設定値（結果を生成した条件）も往復で保たれる。既定値と異なる値に
    // しておいたので、既定値へ戻ってしまう回帰（設定が保存されない）を検出できる。
    assert_eq!(
        reopened.core.analysis_cfg.th_damping, app.core.analysis_cfg.th_damping,
        "時刻歴の減衰比"
    );
    assert_eq!(
        reopened.core.analysis_cfg.th_damping_model, app.core.analysis_cfg.th_damping_model,
        "時刻歴の減衰モデル"
    );
    assert_eq!(
        reopened.core.analysis_cfg.n_modes, app.core.analysis_cfg.n_modes,
        "固有値解析のモード数"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn ovika_reads_defaulted_settings_fields() {
    #[derive(serde::Serialize)]
    struct SettingsWithoutLumpedSelection {
        wave_sha256: Option<String>,
        wave_name: Option<String>,
        cfg: sepika_app::app::AnalysisSettings,
    }
    let mut app = imported();
    app.core.analysis_cfg.th_damping = 0.043;
    let bytes = rmp_serde::to_vec_named(&SettingsWithoutLumpedSelection {
        wave_sha256: None,
        wave_name: None,
        cfg: app.core.analysis_cfg,
    })
    .unwrap();
    let settings: sepika_app::app::SavedAnalysisSettings = rmp_serde::from_slice(&bytes).unwrap();
    assert!(settings.lumped_wave_name.is_none());
    assert!(settings.lumped_wave_sha256.is_none());
    let path = test_tmp().join("defaulted_settings.ovika");
    sepika_io::ovika::save_ovika(
        &path,
        &app.core.model,
        sepika_io::ovika::OvikaExtras {
            analysis_settings: Some(&bytes),
            ..Default::default()
        },
    )
    .unwrap();
    let mut reopened = App::default();
    reopened.open_project_from(path.clone());
    assert_no_error(&reopened, "default フィールドを持つ設定の復元");
    assert_eq!(reopened.core.analysis_cfg.th_damping, 0.043);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ovika_corrupt_optional_payloads_report_notices() {
    let app = imported();
    let path = test_tmp().join("corrupt_optional.ovika");
    let corrupt = [0xc1];
    sepika_io::ovika::save_ovika(
        &path,
        &app.core.model,
        sepika_io::ovika::OvikaExtras {
            preparation: Some(&corrupt),
            results: Some(&corrupt),
            analysis_settings: Some(&corrupt),
        },
    )
    .unwrap();
    let mut reopened = App::default();
    let damping = reopened.core.analysis_cfg.th_damping;
    reopened.open_project_from(path.clone());
    assert_no_error(&reopened, "破損した任意 payload はモデル読込を妨げない");
    assert!(reopened.core.scoped.preparation.is_none());
    assert!(reopened.core.scoped.results.is_none());
    assert_eq!(reopened.core.analysis_cfg.th_damping, damping);
    for label in ["準備計算の結果", "解析結果", "解析タブの設定値"] {
        assert!(reopened
            .core
            .log
            .entries
            .iter()
            .any(|e| e.message.contains(label) && e.message.contains("読み込めませんでした")));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ovika_invalid_restored_model_is_not_installed() {
    let mut app = imported();
    app.core.model.nodes.push(app.core.model.nodes[0].clone());
    assert!(app.core.model.validate().is_err());
    let path = test_tmp().join("invalid_restored_model.ovika");
    app.save_project_to(path.clone());
    assert_no_error(&app, "検証前のモデル保存");
    let mut reopened = App::default();
    let original = reopened.core.model.clone();
    reopened.open_project_from(path.clone());
    assert!(reopened
        .core
        .scoped
        .last_error
        .as_ref()
        .unwrap()
        .contains("読込モデルの検証エラー"));
    assert!(original.eq_ignoring_dofmap(&reopened.core.model));
    assert!(reopened.core.scoped.project_path.is_none());
    std::fs::remove_file(path).unwrap();
}

/// ST-Bridge へ書き出し、読み直してもモデル構成が保たれ、そのまま再解析できる。
///
/// 「読める → 書ける → また読める → 解ける」の往復は、取り込み・書き出しの
/// どちらが壊れても落ちる最も安価な回帰テストになる。
///
/// 書き出しは**準備計算の前**（取り込み直後）の状態から行う。準備計算は仕口パネル
/// 要素を `Model::elements` へ追加するが、これは解析用の生成物であって
/// ST-Bridge の部材ではないため書き出されない。準備計算後のモデルと比べると
/// 要素数が一致せず、往復の検証にならない。
#[test]
fn stbridge_roundtrip_is_reanalyzable() {
    let mut app = imported();
    let path = test_tmp().join("full_model_roundtrip.stb");
    app.export_stbridge_to(path.clone());
    assert_no_error(&app, "ST-Bridge 書き出し");

    let mut reimported = App::default();
    reimported.core.analysis_cfg.threads = 1;
    reimported.import_stbridge_from(path.clone());
    if let Some(e) = &reimported.core.scoped.last_error {
        assert!(
            e.starts_with('⚠'),
            "書き出したファイルの再取り込みが失敗: {e}"
        );
    }
    clear_error(&mut reimported);

    assert_eq!(
        reimported.core.model.nodes.len(),
        app.core.model.nodes.len(),
        "往復で節点数が変わる"
    );
    assert_eq!(
        reimported.core.model.elements.len(),
        app.core.model.elements.len(),
        "往復で要素数が変わる"
    );
    assert_eq!(
        reimported.core.model.beams().count(),
        app.core.model.beams().count(),
        "往復で二次部材数が変わる"
    );
    assert_eq!(
        reimported.core.model.floor_regions.len(),
        app.core.model.floor_regions.len(),
        "往復でスラブ数が変わる"
    );
    assert_eq!(
        app.core.model.floor_regions.len(),
        26,
        "STB 再取り込みで小片 82 に戻らない"
    );

    reimported.run_preparation();
    assert_eq!(
        reimported.core.model.floor_regions.len(),
        26,
        "準備計算で 26 のまま"
    );
    assert_no_error(&reimported, "往復後の準備計算");
    reimported.run_static_all();
    assert_no_error(&reimported, "往復後の静的解析");

    std::fs::remove_file(&path).ok();
}

// ===================== 既知の欠落 =====================

/// ST-Bridge から取り込んだ小梁が、床の小梁設計で検定される。
///
/// あわせて、各小梁が**自分と同じレベルのスラブ**で検定されていることを確認する。
/// スラブの内包判定は XY 平面へ投影して行うため、レベルを見ないと上下階のスラブが
/// すべて該当し、別階の板厚・室用途・境界寸法で検定されてしまう（エラーは出ない）。
#[test]
fn beam_design_checks_cover_imported_secondary_members() {
    let mut app = analyzed();
    app.run_design_check();
    assert_no_error(&app, "断面検定");

    let results = app.core.scoped.results.as_ref().expect("解析結果");
    let n_beams = app.core.model.beams().count();
    assert!(
        !results.beam_checks.is_empty(),
        "小梁 {n_beams} 本が 1 件も検定されていない"
    );

    let mut checked = 0;
    for (slab_id, target, jr) in &results.beam_checks {
        let sepika_app::app::BeamCheckTarget::SecondaryBeam { member } = target else {
            continue; // 間柱は検定対象外（軸力・面外曲げが未対応）。
        };
        checked += 1;
        if jr.unchecked {
            continue;
        }
        let sm = app.core.model.secondary_member(*member).expect("小梁");
        let z_beam = app
            .core
            .model
            .secondary_member_end_points(sm)
            .map(|(a, b)| (a[2] + b[2]) / 2.0)
            .unwrap_or(0.0);
        let Some(sid) = slab_id else {
            continue;
        };
        let slab = app
            .core
            .model
            .slabs
            .iter()
            .find(|s| s.id == *sid)
            .expect("検定結果の床板が実在する");
        let z_slab = slab.level(&app.core.model).expect("床板のレベル");
        assert!(
            (z_slab - z_beam).abs() <= 1.0,
            "小梁 {}（Z={z_beam}）が別レベルのスラブ {:?}（Z={z_slab}）で検定されている",
            member.0,
            slab_id
        );
    }
    assert_eq!(
        checked, n_beams,
        "取り込んだ小梁がすべて検定されていない（{checked}/{n_beams}）"
    );
}

/// 主架構の面走査（`region_gen`）が大梁の囲む区画をレベルごとに検出し、
/// 取り込んだ床板がその区画へ過不足なく収まる（D1）。
///
/// 床領域は「大梁で囲まれた領域ごとに 1 つ」と定めるため（D1）、その検出が実建物で
/// 期待どおりの数になることを固定する。期待値は Euler の公式（内部面数 `F = E − V + C`）
/// で独立に検算した値である。床板（小梁でさらに細分された打設単位）は重複・欠落なく、
/// ちょうど 1 つの床領域へ割り当たる。
#[test]
fn floor_regions_and_slabs_are_consistent() {
    use sepika_core::region_gen::scan_region_boundaries;
    use std::collections::BTreeMap;

    let app = imported();
    let scan = scan_region_boundaries(&app.core.model);
    assert_eq!(scan.unclosed, 0, "閉じない面走査はない");
    assert!(
        scan.crossings.is_empty(),
        "節点を共有せずに交差する大梁がある: {:?}",
        scan.crossings
    );

    let mut per_level: BTreeMap<i64, (usize, f64)> = BTreeMap::new();
    for b in &scan.boundaries {
        let e = per_level.entry(b.level.round() as i64).or_insert((0, 0.0));
        e.0 += 1;
        e.1 += b.area(&app.core.model);
    }
    let counts: Vec<(i64, usize)> = per_level.iter().map(|(z, (n, _))| (*z, *n)).collect();
    assert_eq!(
        counts,
        vec![(200, 6), (4700, 6), (8700, 6), (12700, 7), (16500, 1)],
        "レベル別の床領域数（Euler の公式による検算値と一致すること）"
    );
    assert_eq!(scan.boundaries.len(), 26, "床領域総数");
    assert_eq!(
        app.core.model.floor_regions.len(),
        scan.boundaries.len(),
        "取り込み後の床領域数は床領域数 26"
    );

    // 大梁の区画の面積の合計は、そのレベルの床板面積の合計と一致する
    // （床板は小梁で細分されているが、覆う範囲は大梁の区画と同じ）。
    let mut slab_area: BTreeMap<i64, f64> = BTreeMap::new();
    for s in &app.core.model.slabs {
        let Some(coords) = s.boundary_coords(&app.core.model) else {
            continue;
        };
        if coords.len() < 3 {
            continue;
        }
        *slab_area.entry(coords[0][2].round() as i64).or_default() +=
            sepika_core::geom::polygon::area_xy(&coords);
    }
    for (z, (_, area)) in &per_level {
        let s = slab_area.get(z).copied().unwrap_or(0.0);
        assert!(
            (area - s).abs() / s < 1e-6,
            "Z={z}: 床領域の面積 {area} と床板面積 {s} が一致しない"
        );
    }

    // 各床板の重心が、ちょうど 1 つの床領域へ収まる（未割当・床板なし領域なし）。
    let mut by_region: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut unassigned = Vec::new();
    for (si, slab) in app.core.model.slabs.iter().enumerate() {
        let Some(coords) = slab.boundary_coords(&app.core.model) else {
            continue;
        };
        if coords.len() < 3 {
            continue;
        }
        let n = coords.len() as f64;
        let centroid = [
            coords.iter().map(|p| p[0]).sum::<f64>() / n,
            coords.iter().map(|p| p[1]).sum::<f64>() / n,
        ];
        let z = coords[0][2];
        match scan
            .boundaries
            .iter()
            .position(|b| b.is_same_level(z) && b.contains(&app.core.model, centroid))
        {
            Some(bi) => by_region.entry(bi).or_default().push(si),
            None => unassigned.push(si),
        }
    }
    assert!(
        unassigned.is_empty(),
        "どの床領域にも収まらない床板: {unassigned:?}"
    );
    assert_eq!(
        by_region.len(),
        scan.boundaries.len(),
        "床板を持たない床領域はない"
    );
}

/// 柱・梁が実建物データで壁側の鉛直構面をどれだけ検出できるかを実測して固定する。
///
/// `crates/sepika-app/tests/wall_model.rs`（壁 1 パネル・雑壁 1 本の最小フィクスチャ）は
/// 頂部にしか梁がなく（`柱脚 4 節点は固定支点`、梁は「頂部で閉路」の 4 本のみ）、
/// 各鉛直構面が「柱 2 本＋頂部の梁 1 本」という開いた U 字にしかならないため、
/// `region_gen::wall` の境界検出を一切通らない（面が 0 件になる）。壁領域検出の
/// 実データによる検証は、本テストが唯一の経路である（`region_gen::wall` は
/// `ElementKind::Wall` の有無を見ず、柱・梁の幾何だけで構面を検出するため、
/// 壁要素が 0 件のこの実フィクスチャでも検出は成立する）。
#[test]
fn region_gen_finds_wall_bounded_regions() {
    use sepika_core::region_gen::scan_wall_region_boundaries;
    use std::collections::HashSet;

    let app = imported();
    let scan = scan_wall_region_boundaries(&app.core.model);
    assert_eq!(scan.unclosed, 0, "半辺の後続は一意に定まるはず");

    // 単一の観測値（境界数・合計面積）だけでは、直線の重複統合に不具合があって
    // 同じ構面を 2 回検出していても気づけない。境界どうしが節点集合として
    // 重複していないこと（構面の重複統合ミスの検出）と、面積が必ず正であること
    // （外周面の判別・ニューエル面積算定の破綻の検出）を独立の不変条件として確認する。
    // 本フィクスチャは直交グリッドの S 造建物であり（`import_builds_expected_model` 等
    // 参照）、正の面積を持つ壁境界の構面はすべて軸方向（X または Y）のはずである。
    // 斜め方向の構面に正の面積を持つ境界が現れた場合、`wall_planes` が実際には
    // つながっていない柱の組を誤って直線候補として拾い、構造的に無意味な面を
    // 検出している疑いが強い（列挙した候補直線ごとに面走査をかけるため、
    // 実在しない斜めの「構面」でも部材がたまたま条件を満たせば面ができうる）。
    let mut seen: HashSet<Vec<u32>> = HashSet::new();
    for b in &scan.boundaries {
        let area = b.area(&app.core.model);
        assert!(area > 0.0, "境界の面積は必ず正: {area}");
        let axis_aligned = b.plane_direction[0].abs() < 1e-6 || b.plane_direction[1].abs() < 1e-6;
        assert!(
            axis_aligned,
            "直交グリッド建物のはずが斜め構面に正の面積を持つ境界がある（構造的に無意味な面の疑い）: {:?}",
            b.plane_direction
        );
        let mut nodes: Vec<u32> = b.boundary.iter().map(|n| n.0).collect();
        nodes.sort_unstable();
        assert!(
            seen.insert(nodes.clone()),
            "同じ節点集合を持つ境界が重複している（構面の重複統合ミスの疑い）: {nodes:?}"
        );
    }

    let total_area: f64 = scan
        .boundaries
        .iter()
        .map(|b| b.area(&app.core.model))
        .sum();
    assert_eq!(scan.boundaries.len(), 55, "壁側の鉛直構面の境界数");
    assert!(
        (total_area - 1_427_640_000.0).abs() / total_area < 1e-6,
        "境界の合計面積（ニューエルの公式） {total_area}"
    );
}

/// ST-Bridge 取り込み（`assemble.rs`）が `rebuild_wall_regions` を実際に呼び、
/// `model.wall_regions` が `region_gen::wall` の検出結果と一致する件数で
/// 埋まっていること（§5.9 で結線した経路の end-to-end 確認）。
///
/// 本フィクスチャは壁版（`WallPlate`）を 1 枚も持たないため、検出した壁領域は
/// すべて `wall_plate_ids` が空のまま（幾何の検出だけが先に結線され、壁版の取り込み
/// はまだ Step 7+8 本体で行うため）。
#[test]
fn import_populates_wall_regions_from_region_gen() {
    let app = imported();
    assert_eq!(
        app.core.model.wall_regions.len(),
        55,
        "region_gen_finds_wall_bounded_regions と同じ件数のはず"
    );
    assert!(
        app.core
            .model
            .wall_regions
            .iter()
            .all(|r| r.wall_plate_ids.is_empty()),
        "本フィクスチャは壁版を持たないため、壁版の割当は 0 件のはず"
    );
    for (i, r) in app.core.model.wall_regions.iter().enumerate() {
        assert_eq!(r.id.0, i as u32, "id は配列添字と一致するはず");
    }
    assert!(
        app.core.model.validate().is_ok(),
        "{:?}",
        app.core.model.validate()
    );
}

/// 壁領域は「保存 → 読込 → 再度準備計算」を経ても ID・境界が変わらない。
///
/// `WallRegion` の ID は `scan_wall_region_boundaries` の走査順（`model.elements`
/// の並びに依存。D10）で割り当たる。保存・再読込を経て `model.elements` の並びが
/// 保たれなければ、再準備計算のたびに壁領域 ID が振り直され、UI・保存済み結果の
/// 対応付けが壊れる。1 回の準備計算だけでは検出できない回帰のため、
/// 「読込直後の状態」ではなく「再準備計算後の状態」を比較する。
#[test]
fn wall_regions_survive_save_reopen_reprepare() {
    let app = prepared();
    let before = app.core.model.wall_regions.clone();
    assert!(!before.is_empty(), "本フィクスチャは壁領域を持つはず");

    let path = test_tmp().join("wall_regions_reprepare_roundtrip.ovika");
    let mut app = app;
    app.save_project_to(path.clone());
    assert_no_error(&app, "プロジェクト保存");

    let mut reopened = App::default();
    reopened.core.analysis_cfg.threads = 1;
    reopened.open_project_from(path.clone());
    assert_no_error(&reopened, "プロジェクト読込");

    reopened.run_preparation();
    assert_no_error(&reopened, "再度の準備計算");

    assert_eq!(
        reopened.core.model.wall_regions, before,
        "保存→読込→再準備計算を経ても壁領域（ID・境界）は変わらないはず"
    );

    std::fs::remove_file(&path).ok();
}

// ===================== スナップショット =====================

/// 全解析の代表スカラをスナップショットで固定する。
///
/// 不変量のアサートは「壊れ方」を捕まえるが、「値が静かに変わったこと」自体は
/// 捕まえない。ソルバー・断面性能・荷重分配のいずれかに手を入れて結果が動いた
/// 場合、ここが差分として現れる。意図した変更なら `cargo insta review` で承認する。
///
/// 値は有効数字 4 桁の指数表記へ丸めてある（[`sig4`]。手元と CI の浮動小数差で
/// 偽陽性にならないようにするため）。
#[test]
fn snapshot_key_scalars() {
    let mut app = analyzed();
    app.run_design_check();
    clear_error(&mut app);

    let mut out = String::new();
    let mut line = |k: &str, v: String| {
        out.push_str(k);
        out.push_str(" = ");
        out.push_str(&v);
        out.push('\n');
    };

    // --- モデル構成 ---
    line("model.nodes", app.core.model.nodes.len().to_string());
    line("model.elements", app.core.model.elements.len().to_string());
    line("model.beams()", app.core.model.beams().count().to_string());
    line(
        "model.floor_regions",
        app.core.model.floor_regions.len().to_string(),
    );
    line("model.stories", app.core.model.stories.len().to_string());

    // --- 準備計算 ---
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
    line("prep.height", sig4(prep.summary.height_mm));
    for s in &prep.stories {
        line(
            &format!("prep.story[{}].seismic_weight", s.name),
            sig4(s.weight),
        );
    }
    let seismic = prep.seismic.as_ref().expect("地震力");
    line("prep.seismic.T", sig4(seismic.t));
    line("prep.seismic.Rt", sig4(seismic.rt));
    line("prep.seismic.base_shear", sig4(seismic.base_shear));
    for r in &seismic.rows {
        line(&format!("prep.seismic[{}].Ai", r.name), sig4(r.ai));
        line(&format!("prep.seismic[{}].Qi", r.name), sig4(r.qi));
    }

    // --- 固有値 ---
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

    // --- 静的（DL・EX） ---
    let dl = static_of(&app, StaticCaseKey::User(dl_case_id(&app)));
    line(
        "static.DL.min_uz",
        sig4(dl.disp.iter().map(|d| d[2]).fold(f64::INFINITY, f64::min)),
    );
    line(
        "static.DL.sum_base_axial",
        sig4(base_column_axials(&app, dl).iter().sum::<f64>()),
    );
    let ex = static_of(&app, StaticCaseKey::Seismic(SeismicDir::X));
    for s in &app.core.model.stories {
        let mx = s
            .node_ids
            .iter()
            .filter_map(|n| ex.disp.get(n.index()))
            .map(|d| d[0].abs())
            .fold(0.0_f64, f64::max);
        line(&format!("static.EX.story[{}].max_ux", s.name), sig4(mx));
    }

    // --- 断面検定 ---
    let results = app.core.scoped.results.as_ref().expect("解析結果");
    line(
        "design.member_checks",
        results.member_checks.len().to_string(),
    );
    line(
        "design.joint_checks",
        results.joint_checks.len().to_string(),
    );
    line("design.beam_checks", results.beam_checks.len().to_string());
    line("design.slab_checks", results.slab_checks.len().to_string());
    let max_ratio = results
        .member_checks
        .iter()
        .flat_map(|mc| mc.positions.iter())
        .filter_map(|p| match &p.outcome {
            sepika_design_jp::CheckOutcome::Checked(r) => Some(r.ratio()),
            sepika_design_jp::CheckOutcome::Skipped { .. } => None,
        })
        .fold(0.0_f64, f64::max);
    line("design.max_ratio", sig4(max_ratio));
    let (checked_positions, skipped_positions) = results
        .member_checks
        .iter()
        .flat_map(|mc| mc.positions.iter())
        .fold((0usize, 0usize), |(checked, skipped), p| match &p.outcome {
            sepika_design_jp::CheckOutcome::Checked(_) => (checked + 1, skipped),
            sepika_design_jp::CheckOutcome::Skipped { reason } => {
                assert!(
                    !reason.trim().is_empty(),
                    "Skipped の理由が空: 位置 {}",
                    p.xi
                );
                (checked, skipped + 1)
            }
        });
    assert_eq!(
        skipped_positions, 0,
        "Skipped がある（全位置 Checked のはず）"
    );
    assert_eq!(checked_positions, 345, "Checked 位置数");
    line("design.checked_positions", checked_positions.to_string());
    line("design.skipped_positions", skipped_positions.to_string());
    // 小梁の最大検定比。件数だけでは「どのスラブで検定したか」の変化を捉えられないため、
    // 値そのものも固定する（負担幅・床荷重強度の取り違えはここに現れる）。
    let beam_max_ratio = results
        .beam_checks
        .iter()
        .filter(|(_, _, r)| !r.unchecked)
        .map(|(_, _, r)| r.ratio)
        .fold(0.0_f64, f64::max);
    line("design.beam_max_ratio", sig4(beam_max_ratio));

    // --- 層指標 ---
    let ctx = sepika_app::summary::metrics_ctx_from_results(Some(results));
    let metrics = sepika_app::summary::compute_story_metrics_with(
        &app.core.model,
        &ex.disp,
        SeismicDir::X,
        &ctx,
    );
    for m in &metrics {
        line(
            &format!("metrics[{}].drift_angle", m.name),
            sig4(m.drift_angle),
        );
        line(&format!("metrics[{}].Rs", m.name), sig4(m.rs));
        line(&format!("metrics[{}].Re", m.name), sig4(m.re));
    }

    app.run_pushover();
    assert_t_beam_diagnostic(&app);
    assert!(app.core.scoped.results.as_ref().unwrap().pushover.is_none());
    line("pushover.input", "T形梁の情報不足により停止".into());
    clear_error(&mut app);
    line(
        "ultimate.checks",
        app.compute_ultimate_checks()
            .expect("終局検定")
            .len()
            .to_string(),
    );

    // --- 時刻歴（線形） ---
    app.core.analysis_cfg.th_dir = ThDir::X;
    app.core.analysis_cfg.th_nonlinear = false;
    app.run_time_history_sample();
    clear_error(&mut app);
    let th = app
        .core
        .scoped
        .results
        .as_ref()
        .expect("解析結果")
        .time_history
        .as_ref()
        .expect("時刻歴");
    line("th.frames", th.time.len().to_string());
    line(
        "th.peak_ux",
        sig4(th.peak_disp.iter().map(|d| d[0].abs()).fold(0.0, f64::max)),
    );
    for (i, a) in th.story_drift_angle.iter().enumerate() {
        line(&format!("th.drift_angle[{i}]"), sig4(*a));
    }

    app.core.analysis_cfg.th_nonlinear = true;
    app.run_time_history_sample();
    assert_t_beam_diagnostic(&app);
    assert!(
        !app.core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .time_history
            .as_ref()
            .unwrap()
            .nonlinear
    );
    line("th_nl.input", "T形梁の情報不足により停止".into());

    insta::assert_snapshot!(out);
}

/// 床板の面荷重（固定荷重）が、二次部材を経由しても失われずに主架構へ届く。
///
/// 二次部材は解析要素ではないため、その反力は節点荷重として出され
/// `resolve_nodal_to_primary` が主架構の梁へ変換する。変換できない節点への荷重は
/// `DofMap::build` が非構造節点として無視するため、**荷重が黙って消える**
/// （申し送り §3.4 F10）。小梁の途中に別の小梁が取り付くモデルでこれが起こりうる。
#[test]
fn slab_floor_load_reaches_primary_frame() {
    use sepika_load::secondary::{node_connected_flags, resolve_nodal_to_primary, SPAN_TOL_MM};

    let app = prepared();
    let model = &app.core.model;

    // 期待値: 全床板の固定荷重 × XY 投影面積（`compute_dl_beam_loads` と同じ強度）。
    let extra = sepika_load::wall_attached::floor_region_wall_extra_intensity(model);
    let w_of = |slab: &sepika_core::model::Slab| {
        model.slab_dead_intensity(slab) + extra.get(&slab.id).copied().unwrap_or(0.0)
    };
    let mut expected = 0.0_f64;
    for slab in &model.slabs {
        let Some(coords) = slab.boundary_coords(model) else {
            continue;
        };
        let mut area2 = 0.0;
        let n = coords.len();
        for k in 0..n {
            let (p, q) = (coords[k], coords[(k + 1) % n]);
            area2 += p[0] * q[1] - q[0] * p[1];
        }
        expected += w_of(slab) * (area2 / 2.0).abs();
    }
    assert!(expected > 0.0, "床板の固定荷重が 0");

    // 二次部材の自重も DL へ逐次伝達で載る（`self_weight_case_content` は扱わない）。
    // 期待値は `enumerate_self_weight` と同じ ρ·A·L·g·鉄骨割増を独立に組み立てる。
    let steel_factor = model
        .load_cfg
        .as_ref()
        .map(|c| c.effective_steel_factor())
        .unwrap_or(1.0);
    for sm in model.beams().chain(model.posts()) {
        if model.secondary_member_materialized(sm) {
            continue;
        }
        let (Some(sec), Some(mat)) = (
            sm.section.and_then(|id| model.sections.get(id.index())),
            model.secondary_material(sm),
        ) else {
            continue;
        };
        let [a, b] = sepika_core::face_distance::secondary_self_weight_interval(model, sm)
            .expect("自重フェース間区間");
        let len = b - a;
        let factor = if mat.fc.is_some() { 1.0 } else { steel_factor };
        expected += mat.design_unit_weight_n_per_mm3() * sec.area * len * factor;
    }

    // 実際に主架構へ届く鉛直荷重（非構造節点で捨てられるぶんを除く）。
    let beam_loads = sepika_job::auto_loads::compute_dl_beam_loads(model).expect("DL 分配");
    let (nodal, mut member) = sepika_job::auto_loads::slab_load_case_content(model, &beam_loads);
    let (nodal, extra_member) = resolve_nodal_to_primary(model, nodal, SPAN_TOL_MM);
    member.extend(extra_member);

    let connected = node_connected_flags(model);
    let mut delivered = 0.0_f64;
    let mut dropped = 0.0_f64;
    for nl in &nodal {
        let w = -nl.values[2];
        if connected.get(nl.node.index()).copied().unwrap_or(false) {
            delivered += w;
        } else {
            dropped += w;
        }
    }
    for ml in &member {
        let total = match ml.kind {
            sepika_core::model::MemberLoadKind::Distributed { a, b, w1, w2 } => {
                (w1 + w2) / 2.0 * (b - a)
            }
            sepika_core::model::MemberLoadKind::Point { p, .. } => p,
        };
        delivered += total * -ml.dir[2];
    }

    let ratio = delivered / expected;
    assert!(
        dropped.abs() <= expected * 1e-9,
        "非構造節点で捨てられる床荷重が {dropped:.1} N ある（総床荷重 {expected:.1} N）"
    );
    assert!(
        (ratio - 1.0).abs() < 1e-6,
        "主架構へ届いた床荷重 {delivered:.1} N / 期待 {expected:.1} N = {ratio:.6}"
    );
}
