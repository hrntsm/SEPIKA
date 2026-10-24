//! 仕口パネル（柱梁接合部パネル）の検証（V&V）。
//!
//! 資料には数値例がないため、次の 3 本を軸に検証する。
//!
//! 1. **回帰**: パネルを設けないモデルの解析結果が、パネル導入前と完全に一致する
//!    （追加自由度が 1 つも払い出されないこと）。
//! 2. **解析解照合**: 単一接合部で、パネルのせん断変形角が
//!    `γ = pM / (G・Ve)` の解析解と一致する。
//! 3. **釣り合い**: 資料 (2.10.3-3) の節点せん断モーメント釣り合い
//!    `M^p_S + Σ M^i_S = 0` の残差が数値誤差の範囲に収まる。
//!
//! さらに、パネルを入れると接合部がせん断変形する分だけ架構が柔らかくなる
//! （変位が増える）という定性的な整合も確認する。

use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LoadCase, LocalAxis,
    Material, MaterialCategory, Model, NodalLoad, Node, RigidZone, Section,
};
use sepika_core::section_shape::SectionShape;

const COL_H: f64 = 400.0;
const COL_B: f64 = 400.0;
const COL_TW: f64 = 13.0;
const COL_TF: f64 = 21.0;
const BEAM_H: f64 = 600.0;
const BEAM_TF: f64 = 17.0;
/// 柱フェース距離（＝柱せい/2）。梁端のパネル分オフセット。
const FACE_BEAM: f64 = COL_H / 2.0;
/// 梁フェース距離（＝梁せい/2）。柱端のパネル分オフセット。
const FACE_COL: f64 = BEAM_H / 2.0;

fn node(id: u32, coord: [f64; 3], restraint: Dof6Mask) -> Node {
    Node {
        id: NodeId(id),
        coord,
        restraint,
        mass: None,
        story: None,
        support_spring: None,
    }
}

fn steel_section(id: u32, shape: SectionShape, depth: f64, width: f64, area: f64) -> Section {
    Section {
        frame_use: Some(if id == 0 {
            FrameSectionUse::Girder
        } else {
            FrameSectionUse::Column
        }),
        id: SectionId(id),
        name: String::new(),
        area,
        iy: 2.0e8,
        iz: 2.0e8,
        j: 1.0e7,
        depth,
        width,
        as_y: area * 0.4,
        as_z: area * 0.4,
        floor: None,
        panel_thickness: None,
        thickness: None,
        shape: Some(shape),
        material: Some(MaterialId(0)),
        rebar_material: None,
        shear_rebar_material: None,
        steel_material: None,
        property_basis: Default::default(),
    }
}

fn member(id: u32, n0: u32, n1: u32, sec: u32, rigid: RigidZone) -> ElementData {
    ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(n0), NodeId(n1)],
        section: Some(SectionId(sec)),
        local_axis: LocalAxis {
            ref_vector: if sec == 1 {
                [0.0, 1.0, 0.0]
            } else {
                [0.0, 0.0, 1.0]
            },
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: rigid,
        plastic_zone: None,
        spring: None,
    }
}

/// 面外自由度（Uy・Rx・Rz）の拘束マスク。
///
/// 部材 2 本だけの最小モデルは面外方向に機構を持つ（柱の材軸まわり回転＝Rz は
/// 既定のねじり解放で拘束が外れ、片持ちの梁はそれを接地できない）。検証したいのは
/// パネルの面内挙動なので、平面架構として面外を拘束する。
fn out_of_plane_fixed() -> Dof6Mask {
    let mut m = Dof6Mask::FREE;
    m.set_fixed(sepika_core::dof::Dof::Uy);
    m.set_fixed(sepika_core::dof::Dof::Rx);
    m.set_fixed(sepika_core::dof::Dof::Rz);
    m
}

/// L 型の 1 節点フレーム（柱脚固定・梁先端自由）。X-Z 面内の平面架構とする。
///
/// - 節点 0: 接合部（柱頭 ＝ 梁の i 端）
/// - 節点 1: 梁の先端（鉛直荷重を載荷）
/// - 節点 2: 柱脚（固定）
///
/// `with_panel` が真なら接合部へ仕口パネルを設ける。
fn l_frame(with_panel: bool) -> Model {
    l_frame_with(with_panel, false)
}

/// `rigid_joint` が真なら、パネルを設けずに接合部の有限寸法だけを剛域として
/// 与える。パネル有りモデルと剛域条件が揃うため、両者の差は
/// **パネルのせん断変形のみ**に由来する。
///
/// パネル有りのモデルは準備計算と同じ経路
/// （[`sepika_element::springs::panel_gen::apply_auto_panel_zones`]）で生成する。
/// パネル要素の追加とオフセットの剛域長への書き込みが一体の処理であり、
/// 手組みすると実際のモデル化と食い違うためである。
fn l_frame_with(with_panel: bool, rigid_joint: bool) -> Model {
    let rigid = RigidZone {
        face_i: Some(FACE_BEAM),
        face_j: Some(0.0),
        length_i: if rigid_joint { FACE_BEAM } else { 0.0 },
        ..Default::default()
    };
    let col_rigid = RigidZone {
        face_i: Some(0.0),
        face_j: Some(FACE_COL),
        length_j: if rigid_joint { FACE_COL } else { 0.0 },
        ..Default::default()
    };

    let elements = vec![
        member(0, 0, 1, 0, rigid),     // 梁（水平材）: i 端が接合部
        member(1, 2, 0, 1, col_rigid), // 柱（鉛直材）: j 端が接合部
    ];

    let mut model = Model {
        nodes: vec![
            node(0, [0.0, 0.0, 3000.0], out_of_plane_fixed()),
            node(1, [6000.0, 0.0, 3000.0], out_of_plane_fixed()),
            node(2, [0.0, 0.0, 0.0], Dof6Mask::FIXED),
        ],
        sections: vec![
            steel_section(
                0,
                SectionShape::SteelH {
                    root_r: Some(0.0),
                    height: BEAM_H,
                    width: 200.0,
                    web_thick: 11.0,
                    flange_thick: BEAM_TF,
                },
                BEAM_H,
                200.0,
                1.34e4,
            ),
            steel_section(
                1,
                SectionShape::SteelH {
                    root_r: Some(0.0),
                    height: COL_H,
                    width: COL_B,
                    web_thick: COL_TW,
                    flange_thick: COL_TF,
                },
                COL_H,
                COL_B,
                2.187e4,
            ),
        ],
        materials: vec![Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "SN400B".into(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }],
        elements,
        load_cases: vec![LoadCase {
            kind: Default::default(),
            id: LoadCaseId(1),
            name: "P".into(),
            // 梁先端に鉛直下向き荷重（接合部に曲げを与える）
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -50_000.0, 0.0, 0.0, 0.0],
            )],
            member: Vec::new(),
        }],
        ..Default::default()
    };
    if with_panel {
        let panels =
            sepika_element::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
        assert_eq!(panels.len(), 1, "接合部にパネルが 1 つ生成される");
    }
    model
}

/// 柱の強軸・弱軸の実効体積 Ve とせん断剛性 G·Ve。
fn panel_stiffness(model: &Model) -> ([f64; 2], [f64; 2]) {
    let db = BEAM_H - BEAM_TF;
    let ve = [(COL_H - COL_TF) * db * COL_TW, 2.0 * COL_B * db * COL_TF];
    let mat = &model.materials[0];
    (ve, ve.map(|volume| mat.shear_modulus() * volume))
}

fn panel_757_space_frame(column_rotation: f64, building_rotation: f64) -> Model {
    let mut model = l_frame(false);
    model.girder_torsion = sepika_core::model::GirderTorsionMode::Keep;
    model.materials[0].shear = Some(80_000.0);
    model.nodes[0].restraint = Dof6Mask::FREE;
    model.nodes[1].restraint = Dof6Mask::FREE;
    model
        .nodes
        .push(node(3, [0.0, 4500.0, 3000.0], Dof6Mask::FREE));
    let mut other_beam = model.elements[0].clone();
    other_beam.id = ElemId(2);
    other_beam.nodes = smallvec::smallvec![NodeId(0), NodeId(3)];
    model.elements.push(other_beam);
    let shape = SectionShape::SteelH {
        height: 500.0,
        width: 300.0,
        web_thick: 12.0,
        flange_thick: 24.0,
        root_r: Some(0.0),
    };
    let mut column = shape.to_section(SectionId(1), "H-500x300x12x24".into());
    column.frame_use = Some(FrameSectionUse::Column);
    column.material = Some(MaterialId(0));
    model.sections[1] = column;
    let (sin, cos) = column_rotation.sin_cos();
    model.elements[1].local_axis.ref_vector = [cos, sin, 0.0];
    let (sin, cos) = building_rotation.sin_cos();
    let rotate = |v: [f64; 3]| [cos * v[0] - sin * v[1], sin * v[0] + cos * v[1], v[2]];
    for node in &mut model.nodes {
        node.coord = rotate(node.coord);
    }
    for element in &mut model.elements {
        element.local_axis.ref_vector = rotate(element.local_axis.ref_vector);
    }
    model.load_cases = vec![
        LoadCase {
            id: LoadCaseId(1),
            name: "二方向同時".into(),
            kind: Default::default(),
            nodal: vec![
                NodalLoad::manual(NodeId(1), [0.0, 0.0, -50_000.0, 0.0, 0.0, 0.0]),
                NodalLoad::manual(NodeId(3), [0.0, 0.0, -30_000.0, 0.0, 0.0, 0.0]),
            ],
            member: Vec::new(),
        },
        LoadCase {
            id: LoadCaseId(2),
            name: "X梁単独".into(),
            kind: Default::default(),
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -50_000.0, 0.0, 0.0, 0.0],
            )],
            member: Vec::new(),
        },
        LoadCase {
            id: LoadCaseId(3),
            name: "Y梁単独".into(),
            kind: Default::default(),
            nodal: vec![NodalLoad::manual(
                NodeId(3),
                [0.0, 0.0, -30_000.0, 0.0, 0.0, 0.0],
            )],
            member: Vec::new(),
        },
    ];
    let panels = sepika_element::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
    assert_eq!(panels.len(), 1);
    assert_eq!(panels[0].dc, [476.0, 300.0]);
    assert_eq!(panels[0].db, [583.0; 2]);
    assert_eq!(panels[0].tp, [12.0, 24.0]);
    assert_eq!(panels[0].ve, [3_330_096.0, 8_395_200.0]);
    assert_eq!(panels[0].k_panel, [266_407_680_000.0, 671_616_000_000.0]);
    model
}

fn panel_757_rigid_comparison(model: &Model) -> Model {
    let mut rigid = model.clone();
    rigid.elements.retain(|e| e.kind != ElementKind::PanelZone);
    rigid.panel_zone = sepika_core::model::PanelZoneMode::None;
    rigid
}

#[test]
fn panel_757_public_solver_rejects_invalid_column_reference_directions() {
    let model = panel_757_space_frame(0.0, 0.0);
    for reference in [
        [f64::NAN, 0.0, 0.0],
        [f64::INFINITY, 0.0, 0.0],
        [0.0, 0.0, f64::NEG_INFINITY],
        [1e308, 0.0, 0.0],
    ] {
        let mut invalid = model.clone();
        invalid.elements[1].local_axis.ref_vector = reference;
        let prepared = sepika_solver::statics::analysis::Analysis::prepare(&invalid).map(|_| ());
        let direct =
            sepika_solver::statics::linear::linear_static_once(&invalid, LoadCaseId(1)).map(|_| ());
        for (entry, result) in [("prepare", prepared), ("linear_static_once", direct)] {
            let error = result.unwrap_err().to_string();
            for expected in ["接合部 ID 0", "柱 ID 1", "参照方向", "有限", "単位直交"]
            {
                assert!(error.contains(expected), "{entry}, {reference:?}: {error}");
            }
        }
    }
}

#[test]
fn panel_757_public_solver_preserves_finite_zero_and_parallel_reference_fallbacks() {
    let model = panel_757_space_frame(0.0, 0.0);
    let expected =
        sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(1)).unwrap();
    for reference in [[0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, -2.0]] {
        let mut fallback = model.clone();
        fallback.elements[1].local_axis.ref_vector = reference;
        let direct =
            sepika_solver::statics::linear::linear_static_once(&fallback, LoadCaseId(1)).unwrap();
        let prepared = sepika_solver::statics::analysis::Analysis::prepare(&fallback)
            .unwrap()
            .linear_static(LoadCaseId(1))
            .unwrap();
        for actual in [direct, prepared] {
            assert_eq!(actual.panel_moments, expected.panel_moments);
            assert_eq!(actual.disp, expected.disp);
        }
    }
}

#[test]
fn panel_757_public_solver_column_axes_match_load_equilibrium_and_independent_work() {
    // 静定系の MS=(PyLy,-PxLx)、追加荷重仕事=ms²/Ks+mw²/Kw の独立値。
    for (angle, expected_work_n_mm) in [
        (0.0, 364_964.140_673_422),
        (std::f64::consts::FRAC_PI_2, 202_415.335_774_103_85),
        (0.37, 405_554.238_011_519_1),
    ] {
        let model = panel_757_space_frame(angle, 0.0);
        let response =
            sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(1)).unwrap();
        let facade = sepika_solver::statics::analysis::Analysis::prepare(&model)
            .unwrap()
            .linear_static(LoadCaseId(1))
            .unwrap();
        assert_eq!(response.panel_moments.len(), 1);
        assert_eq!(response.panel_moments[0].0, NodeId(0));
        for (actual, expected) in response.panel_moments[0]
            .1
            .into_iter()
            .zip([135_000_000.0, -300_000_000.0])
        {
            assert!(
                (actual - expected).abs() < 1e-9 * expected.abs(),
                "angle={angle}, moment={actual}, expected={expected}"
            );
        }
        for (actual, expected) in facade.panel_moments[0]
            .1
            .into_iter()
            .zip(response.panel_moments[0].1)
        {
            assert!((actual - expected).abs() < 1e-9 * expected.abs());
        }
        for (actual, expected) in facade
            .disp
            .iter()
            .flatten()
            .zip(response.disp.iter().flatten())
        {
            assert!((actual - expected).abs() < 1e-9 * expected.abs().max(1.0));
        }
        let rigid = panel_757_rigid_comparison(&model);
        let rigid_response =
            sepika_solver::statics::linear::linear_static_once(&rigid, LoadCaseId(1)).unwrap();
        let extra_work_n_mm = -50_000.0 * (response.disp[1][2] - rigid_response.disp[1][2])
            - 30_000.0 * (response.disp[3][2] - rigid_response.disp[3][2]);
        assert!(
            (extra_work_n_mm - expected_work_n_mm).abs() < 1e-8 * expected_work_n_mm,
            "angle={angle}, work={extra_work_n_mm}, expected={expected_work_n_mm}"
        );
    }
}

#[test]
fn panel_757_public_solver_whole_rotation_and_simultaneous_loading_preserve_response() {
    let model = panel_757_space_frame(0.37, 0.0);
    let rotated = panel_757_space_frame(0.37, 0.61);
    let analysis = sepika_solver::statics::analysis::Analysis::prepare(&model).unwrap();
    let together = analysis.linear_static(LoadCaseId(1)).unwrap();
    let x = analysis.linear_static(LoadCaseId(2)).unwrap();
    let y = analysis.linear_static(LoadCaseId(3)).unwrap();
    let separate = sepika_solver::statics::linear::superpose_static(&[(&x, 1.0), (&y, 1.0)]);
    let rotated_response = sepika_solver::statics::analysis::Analysis::prepare(&rotated)
        .unwrap()
        .linear_static(LoadCaseId(1))
        .unwrap();
    let (sin, cos) = 0.61_f64.sin_cos();
    for (node, (before, after)) in together.disp.iter().zip(&rotated_response.disp).enumerate() {
        for start in [0, 3] {
            let expected = [
                cos * before[start] - sin * before[start + 1],
                sin * before[start] + cos * before[start + 1],
                before[start + 2],
            ];
            for (actual, expected) in after[start..start + 3].iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-8 * expected.abs().max(1.0),
                    "node={node}"
                );
            }
        }
        for (actual, expected) in before.iter().zip(separate.disp[node]) {
            assert!((actual - expected).abs() < 1e-9 * expected.abs().max(1.0));
        }
    }
    let [mx, my] = together.panel_moments[0].1;
    for (actual, expected) in rotated_response.panel_moments[0]
        .1
        .into_iter()
        .zip([cos * mx - sin * my, sin * mx + cos * my])
    {
        assert!((actual - expected).abs() < 1e-9 * expected.abs().max(1.0));
    }
    for (actual, expected) in together.panel_moments[0]
        .1
        .into_iter()
        .zip(separate.panel_moments[0].1)
    {
        assert!((actual - expected).abs() < 1e-9 * expected.abs().max(1.0));
    }
}

/// 1: パネルを設けないモデルでは追加自由度が 1 つも払い出されず、
///    接合部へパネルを設けると当該節点にだけ γX・γY の 2 自由度が増える。
#[test]
fn test_panel_adds_exactly_two_dofs() {
    let base_model = l_frame(false);
    let base_map = DofMap::build(&base_model);
    // 節点 0・1 は面内 3 成分（Ux・Uz・Ry）のみ自由、節点 2 は固定。
    assert_eq!(base_map.n_active(), 6);
    for ni in 0..base_model.nodes.len() {
        assert!(
            !base_map.has_panel_dof(ni),
            "パネル無しで追加自由度は出ない"
        );
    }

    let model = l_frame(true);
    let dofmap = DofMap::build(&model);
    assert_eq!(dofmap.n_active(), base_map.n_active() + 2);
    assert!(dofmap.has_panel_dof(0), "接合部節点にパネル自由度");
    assert!(!dofmap.has_panel_dof(1));
}

/// `K·u = f` を直接解き、独立自由度の解ベクトルを返す（追加自由度を含む）。
///
/// `linear_static_once` は節点 6 成分の変位しか返さないため、パネルの
/// せん断変形角を取り出すには全体系を自前で解く必要がある。
fn solve_free(model: &Model, dofmap: &DofMap) -> Vec<f64> {
    use sepika_math::solver::LinearSolver;
    let k = sepika_solver::common::assemble::assemble_global_k(model, dofmap);
    let f = sepika_solver::common::assemble::assemble_global_f(model, dofmap, LoadCaseId(1));
    let mut solver = sepika_math::lu::LuSolver::default();
    solver.factorize(&k).expect("分解できる");
    solver.solve(&f).expect("解ける")
}

/// 疎行列とベクトルの積 `K·u`。
fn spmv(k: &faer::sparse::SparseColMat<usize, f64>, u: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; k.nrows()];
    let col_ptr = k.col_ptr();
    let row_idx = k.row_idx();
    let val = k.val();
    for c in 0..k.ncols() {
        for i in col_ptr[c]..col_ptr[c + 1] {
            out[row_idx[i]] += val[i] * u[c];
        }
    }
    out
}

/// 2: 解析解照合。
///
/// パネルのせん断変形角 `γ` と、パネルが負担するせん断モーメント `M` の間には
/// `M = K・γ`（`K = G・Ve`）が成り立たなければならない。解析で得た `γ` から
/// パネル要素の内力を評価し、この関係を確認する。
///
/// あわせて、パネルの内力が「部材がパネル自由度へ寄せるモーメント」と符号反転で
/// 釣り合う（資料 (2.10.3-3)）ことも確認する。
#[test]
fn test_panel_shear_angle_matches_closed_form() {
    let model = l_frame(true);
    let (ve, k_panel) = panel_stiffness(&model);
    assert!(ve.iter().all(|v| *v > 0.0) && k_panel.iter().all(|k| *k > 0.0));

    let dofmap = DofMap::build(&model);
    let u = solve_free(&model, &dofmap);

    let gx = dofmap.panel_dof(0, 0).expect("γX") as usize;
    let gy = dofmap.panel_dof(0, 1).expect("γY") as usize;
    let gamma = [u[gx], u[gy]];

    // X-Z 面の載荷では全体 Y 軸まわりのせん断変形 γY が生じる。
    assert!(
        gamma[1].abs() > 1e-12,
        "パネルがせん断変形しているはず: γ={gamma:?}"
    );

    // パネル要素へ解を与えて内力（＝せん断モーメント）を取り出す。
    use sepika_element::behavior::{Ctx, ElementBehavior, LocalVec};
    let panel_data = model
        .elements
        .iter()
        .find(|e| matches!(e.kind, ElementKind::PanelZone))
        .expect("パネル要素");
    let mut panel = sepika_element::springs::panel::PanelZone::new(panel_data, &model);
    let ctx = Ctx { model: &model };
    let du = LocalVec {
        data: smallvec::smallvec![gamma[0], gamma[1]],
    };
    panel.update_state(&du, true, &ctx).unwrap();
    let m = panel.panel_moments();

    // 柱主軸と全体軸が揃うため、各成分で M = K・γ が成り立つ。
    for (i, (&mi, &gi)) in m.iter().zip(gamma.iter()).enumerate() {
        let expected = k_panel[i] * gi;
        assert!(
            (mi - expected).abs() <= 1e-9 * expected.abs().max(1.0),
            "成分 {i}: M={mi} は K·γ={expected} と一致すべき"
        );
    }
}

/// 3: 釣り合い（資料 (2.10.3-3)）。
///
/// パネル自由度には外力が加わらないため、平衡方程式の当該行の残差
/// `(K·u − f)` は数値誤差の範囲で 0 でなければならない。これは
/// 「パネル要素の `M^p_S` ＋ 各部材が寄せる `M^i_S` の総和が 0」と同値である。
#[test]
fn test_panel_dof_equilibrium_residual_is_zero() {
    let model = l_frame(true);
    let dofmap = DofMap::build(&model);
    let k = sepika_solver::common::assemble::assemble_global_k(&model, &dofmap);
    let f = sepika_solver::common::assemble::assemble_global_f(&model, &dofmap, LoadCaseId(1));
    let u = solve_free(&model, &dofmap);
    let ku = spmv(&k, &u);

    let gx = dofmap.panel_dof(0, 0).expect("γX") as usize;
    let gy = dofmap.panel_dof(0, 1).expect("γY") as usize;

    // パネル自由度に外力は加わらない。
    assert_eq!(f[gx], 0.0);
    assert_eq!(f[gy], 0.0);

    // 残差の尺度は系全体の内力スケールで正規化する。
    let scale = ku.iter().fold(0.0_f64, |m, v| m.max(v.abs())).max(1.0);
    for (label, g) in [("γX", gx), ("γY", gy)] {
        let residual = (ku[g] - f[g]).abs() / scale;
        assert!(
            residual < 1e-9,
            "{label} の釣り合い残差が大きい: {residual}"
        );
    }
}

/// パネル導入が架構剛性へ与える 2 つの効果を、同一のパネル有りモデルの求解 1 回で確認する。
///
/// - 「同じ剛域（接合部の有限寸法）を持ち、パネルが剛（せん断変形しない）」モデル
///   との比較: パネル有りはせん断変形の分だけ柔らかくなる。剛域の有無で比べると、
///   パネル導入に伴う剛域の追加で逆に硬くなるため、せん断変形の効果だけを取り出せない。
/// - 「接合部を無視した（剛域なし・パネルなし）」モデルとの比較: 剛域が付いて硬く
///   なる効果が勝り、パネル有りの方が硬くなる。この向きが逆転した場合は剛域の
///   折り込み（`panel_offset::resolve`）が効いていないことを意味するため、回帰として押さえる。
#[test]
fn test_panel_stiffness_effects() {
    let with_panel =
        sepika_solver::statics::linear::linear_static_once(&l_frame(true), LoadCaseId(1))
            .expect("パネル有りの解析");
    let rigid_joint = sepika_solver::statics::linear::linear_static_once(
        &l_frame_with(false, true),
        LoadCaseId(1),
    )
    .expect("剛パネル（剛域のみ）の解析");
    let plain = sepika_solver::statics::linear::linear_static_once(&l_frame(false), LoadCaseId(1))
        .expect("接合部無視モデルの解析");

    // 荷重点（節点 1）の鉛直変位（下向き荷重なので負）。
    let d_panel = with_panel.disp[1][2].abs();
    let d_rigid = rigid_joint.disp[1][2].abs();
    let d_plain = plain.disp[1][2].abs();
    assert!(d_rigid > 0.0 && d_panel > 0.0, "変位が生じている");
    assert!(
        d_panel > d_rigid,
        "パネルのせん断変形で柔らかくなるはず: 剛パネル {d_rigid}, パネル有り {d_panel}"
    );
    assert!(
        d_panel < d_plain,
        "剛域が折り込まれていない: 接合部無視 {d_plain}, パネル有り {d_panel}"
    );
}

#[test]
fn panel_504_standard_solver_rejects_unsupported_manual_panel_with_reason() {
    let mut model = l_frame(true);
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    assert!(sepika_solver::statics::analysis::Analysis::prepare(&model).is_ok());
    model.sections[0].shape = Some(SectionShape::SteelPipe {
        outer_dia: 600.0,
        thick: 20.0,
    });
    let expected = sepika_core::panel_zone::resolve_panel_joint(&model, NodeId(0), &model.elements)
        .unwrap_err()
        .to_string();
    let error = sepika_solver::statics::analysis::Analysis::prepare(&model)
        .err()
        .expect("未対応の手動パネルを剛性算定成功と扱わない");
    assert!(error.to_string().contains(&expected));
}

#[test]
fn panel_505_public_solver_rejects_old_or_manual_staggered_panel() {
    let mut model = l_frame(true);
    let mut section = model.sections[0].clone();
    section.id = SectionId(2);
    section.shape = Some(SectionShape::SteelH {
        height: 500.0,
        width: 200.0,
        web_thick: 11.0,
        flange_thick: 17.0,
        root_r: Some(0.0),
    });
    model.sections.push(section);
    let mut far = model.nodes[1].clone();
    far.id = NodeId(model.nodes.len() as u32);
    far.coord[0] = -6000.0;
    model.nodes.push(far.clone());
    let mut other = model.elements[0].clone();
    other.id = ElemId(model.elements.len() as u32);
    other.section = Some(SectionId(2));
    other.nodes = smallvec::smallvec![far.id, NodeId(0)];
    model.elements.push(other);
    let reason = sepika_core::panel_zone::resolve_panel_joint(&model, NodeId(0), &model.elements)
        .unwrap_err()
        .to_string();
    assert!(reason.contains("上位置差=50 mm"));
    for mode in [
        sepika_core::model::PanelZoneMode::Model,
        sepika_core::model::PanelZoneMode::None,
    ] {
        model.panel_zone = mode;
        let error = sepika_solver::statics::analysis::Analysis::prepare(&model)
            .err()
            .expect("旧/手動パネルは解析不可");
        assert!(error.to_string().contains(&reason));
    }
}

#[test]
fn panel_585_public_linear_and_nonlinear_reject_invalid_lower_with_joint_reason() {
    let mut model = l_frame(true);
    let mut upper = model.elements[1].clone();
    upper.id = ElemId(model.elements.len() as u32);
    let mut upper_node = model.nodes[2].clone();
    upper_node.id = NodeId(3);
    upper_node.coord[2] = 6000.0;
    model.nodes.push(upper_node);
    let mut upper_section = model.sections[1].clone();
    upper_section.id = SectionId(2);
    model.sections.push(upper_section);
    upper.nodes = smallvec::smallvec![NodeId(0), NodeId(3)];
    upper.section = Some(SectionId(2));
    model.elements.push(upper);
    model.sections[1].shape = None;
    let reason = sepika_core::panel_zone::resolve_panel_joint(&model, NodeId(0), &model.elements)
        .unwrap_err()
        .to_string();
    assert!(reason.contains("接合部 ID 0"));
    assert!(reason.contains("柱 ID 1"));
    let linear = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(1))
        .err()
        .unwrap();
    assert!(linear.to_string().contains(&reason), "{linear}");
    let dof = DofMap::build(&model);
    let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
    let nonlinear = sepika_solver::nonlinear::pushover::pushover_analysis(
        &model,
        &dof,
        &reducer,
        sepika_solver::statics::analysis::SeismicDir::X,
        1,
        10.0,
        false,
    )
    .err()
    .unwrap();
    assert!(nonlinear.contains(&reason), "{nonlinear}");
}

#[test]
fn panel_585_public_inputs_reject_invalid_adopted_column_area_and_axial_stiffness() {
    for (area, young) in [
        (0.0, 205_000.0),
        (-1.0, 205_000.0),
        (f64::NAN, 205_000.0),
        (f64::INFINITY, 205_000.0),
        (21_870.0, 0.0),
        (21_870.0, f64::INFINITY),
    ] {
        let mut model = l_frame(true);
        model.sections[1].area = area;
        model.materials[0].young = young;
        model.materials[0].shear = Some(80_000.0);
        let linear = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(1))
            .expect_err("不正な採用柱A/軸剛性は解析入力エラー")
            .to_string();
        assert!(
            linear.contains("接合部 ID 0") && linear.contains("柱 ID 1"),
            "{linear}"
        );
        let error = model.validate().unwrap_err().to_string();
        assert!(
            error.contains("接合部 ID 0") && error.contains("柱 ID 1"),
            "{error}"
        );
        let dof = DofMap::build(&model);
        let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
        let nonlinear = sepika_solver::nonlinear::pushover::pushover_analysis(
            &model,
            &dof,
            &reducer,
            sepika_solver::statics::analysis::SeismicDir::X,
            1,
            10.0,
            false,
        )
        .err()
        .unwrap();
        assert!(
            nonlinear.contains("接合部 ID 0") && nonlinear.contains("柱 ID 1"),
            "{nonlinear}"
        );
    }
}

#[test]
fn panel_585_explicit_panel_without_column_is_not_a_successful_rigid_panel() {
    let mut model = l_frame(true);
    model.elements.remove(1);
    for (i, element) in model.elements.iter_mut().enumerate() {
        element.id = ElemId(i as u32);
    }
    let reason = model
        .validate()
        .expect_err("明示パネルの参照柱が必要")
        .to_string();
    assert!(
        reason.contains("接合部 ID 0") && reason.contains("柱"),
        "{reason}"
    );
    let linear = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(1))
        .err()
        .unwrap()
        .to_string();
    assert!(
        linear.contains("接合部 ID 0") && linear.contains("柱"),
        "{linear}"
    );
    let dof = DofMap::build(&model);
    let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
    let nonlinear = sepika_solver::nonlinear::pushover::pushover_analysis(
        &model,
        &dof,
        &reducer,
        sepika_solver::statics::analysis::SeismicDir::X,
        1,
        10.0,
        false,
    )
    .err()
    .unwrap();
    assert!(
        nonlinear.contains("接合部 ID 0") && nonlinear.contains("柱"),
        "{nonlinear}"
    );
    let data = model
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::PanelZone)
        .unwrap();
    for panel in [
        sepika_element::springs::panel::PanelZone::new(data, &model),
        sepika_element::springs::panel::PanelZone::new_nonlinear(data, &model),
    ] {
        assert!(panel.resolution_error.is_some());
        assert_eq!(panel.k_panel, [0.0; 2]);
    }
    model.elements.retain(|e| e.kind != ElementKind::PanelZone);
    assert!(
        sepika_core::panel_zone::resolve_panel_joint(&model, NodeId(0), &model.elements)
            .unwrap()
            .is_none()
    );
}

#[test]
fn panel_728_public_entry_matrix() {
    for (label, shear, poisson) in [
        ("derived_normal", None, 0.3),
        ("explicit_normal", Some(80_000.0), 0.3),
        (
            "explicit_overrides_invalid_poisson",
            Some(80_000.0),
            f64::NAN,
        ),
        ("nan", Some(f64::NAN), 0.3),
        ("positive_infinity", Some(f64::INFINITY), 0.3),
        ("negative_infinity", Some(f64::NEG_INFINITY), 0.3),
        ("negative", Some(-1.0), 0.3),
        ("zero", Some(0.0), 0.3),
        ("derived_nan", None, f64::NAN),
        ("derived_infinite", None, -1.0),
        ("derived_negative", None, -2.0),
        ("derived_zero", None, f64::INFINITY),
        ("finite_g_overflow_stiffness", Some(f64::MAX), 0.3),
        ("finite_g_underflow_stiffness", Some(1.0e-200), 0.3),
    ] {
        let mut model = l_frame(true);
        model.materials[0].fy = Some(235.0);
        model.stories = [(0.0, vec![NodeId(2)]), (3000.0, vec![NodeId(0), NodeId(1)])]
            .into_iter()
            .enumerate()
            .map(|(i, (elevation, node_ids))| sepika_core::model::Story {
                wall_weights: vec![],
                level_kind: Default::default(),
                structure: Default::default(),
                id: sepika_core::ids::StoryId(i as u32),
                name: format!("{i}F"),
                elevation: Some(elevation),
                sl_offset_mm: Some(0.0),
                source_story: None,
                node_ids,
                seismic_weight: Some(10_000.0),
                weight_override: None,
                dynamic_mass: None,
                standard_floor_load: None,
                column_finish_area_weight: 0.0,
                fireproof: Default::default(),
            })
            .collect();
        let mut column_material = model.materials[0].clone();
        column_material.id = MaterialId(1);
        column_material.shear = shear;
        column_material.poisson = poisson;
        model.materials.push(column_material);
        model.sections[1].material = Some(MaterialId(1));
        if label == "finite_g_underflow_stiffness" {
            model.sections[1].panel_thickness = Some(1.0e-200);
        }
        let mut auto = model.clone();
        let generated = sepika_element::springs::panel_gen::apply_auto_panel_zones(&mut auto);
        let validated = model.validate();
        let prepared = sepika_solver::statics::analysis::Analysis::prepare(&model);
        let linear = sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(1));
        let dof = DofMap::build(&model);
        let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
        let nonlinear = sepika_solver::nonlinear::pushover::pushover_analysis_recording(
            &model,
            &dof,
            &reducer,
            sepika_solver::statics::analysis::SeismicDir::X,
            1,
            sepika_solver::nonlinear::pushover::PushoverTarget::from_max_disp(10.0),
            Default::default(),
            false,
            false,
            Default::default(),
        );
        let data = model
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::PanelZone)
            .unwrap();
        let panels = [
            sepika_element::springs::panel::PanelZone::new(data, &model),
            sepika_element::springs::panel::PanelZone::new_nonlinear(data, &model),
        ];
        println!("{label}: auto={:?}; validate={:?}; prepare={:?}; linear={:?}; nonlinear={:?}; panel={:?}",
            generated.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
            validated.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
            prepared.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
            linear.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
            nonlinear.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
            panels.iter().map(|p| (&p.resolution_error, p.g, p.k_panel)).collect::<Vec<_>>()
        );
        let valid = matches!(
            label,
            "derived_normal" | "explicit_normal" | "explicit_overrides_invalid_poisson"
        );
        if valid {
            assert!(generated.is_ok(), "{label}");
            assert!(validated.is_ok(), "{label}");
            assert!(prepared.is_ok(), "{label}");
            assert!(linear.is_ok(), "{label}");
            assert!(nonlinear.is_ok(), "{label}");
            for panel in panels {
                assert!(panel.resolution_error.is_none(), "{label}");
                assert!(panel.g.is_finite() && panel.g > 0.0, "{label}");
                assert!(
                    panel.k_panel.iter().all(|k| k.is_finite() && *k > 0.0),
                    "{label}"
                );
                if shear.is_some() {
                    assert_eq!(panel.g, 80_000.0);
                }
            }
        } else {
            let automatic = generated.unwrap_err();
            assert!(
                automatic.contains("接合部 ID 0") && automatic.contains("柱 ID 1"),
                "{label}: {automatic}"
            );
            let reason = format!(
                "接合部 ID 0 のパネル未算定: 柱 ID 1: {} は有限の正値が必要です",
                if label.starts_with("finite_g_") {
                    "強軸のせん断剛性 G・Ve"
                } else {
                    "せん断弾性係数 G"
                }
            );
            for error in [
                validated.err().map(|e| e.to_string()),
                prepared.err().map(|e| e.to_string()),
                linear.err().map(|e| e.to_string()),
                nonlinear.err(),
            ] {
                assert!(
                    error.as_ref().is_some_and(|e| e.contains(&reason)),
                    "{label}: {error:?}"
                );
            }
            for mut panel in panels {
                assert_eq!(
                    panel.resolution_error.as_deref(),
                    Some(reason.as_str()),
                    "{label}"
                );
                assert_eq!(panel.k_panel, [0.0; 2], "{label}");
                use sepika_element::behavior::{Ctx, ElementBehavior, LocalVec};
                let increment = LocalVec {
                    data: smallvec::SmallVec::from_elem(0.0, panel.n_dof()),
                };
                assert_eq!(
                    panel
                        .update_state(&increment, false, &Ctx { model: &model })
                        .unwrap_err()
                        .0,
                    reason
                );
            }
        }
    }
}

#[test]
fn panel_728_public_dynamic_entries() {
    use sepika_solver::dynamic::damping::Damping;
    use sepika_solver::dynamic::timehistory::{
        linear_time_history_analysis, GroundMotion, NewmarkCfg,
    };
    for (label, g) in [
        ("normal", 80_000.0),
        ("nan", f64::NAN),
        ("overflow", f64::MAX),
        ("underflow", 1e-200),
    ] {
        let mut model = l_frame(true);
        model.materials[0].density = 7.85e-9;
        model.materials[0].shear = Some(80_000.0);
        let dof = DofMap::build(&model);
        let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
        let stiffness = sepika_solver::common::assemble::assemble_global_k(&model, &dof);
        let mut solver = sepika_math::solver::make_solver(
            sepika_math::solver::SolverBackend::DirectSparseCholesky,
        );
        solver.factorize(&reducer.reduce_k(&stiffness)).unwrap();
        model.materials[0].shear = Some(g);
        if label == "underflow" {
            model.sections[1].panel_thickness = Some(1e-200);
        }
        let dof = DofMap::build(&model);
        let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
        let eigen = sepika_solver::dynamic::eigen::solve_eigen(&model, &dof, &reducer, 1);
        let eigen_with_solver = sepika_solver::dynamic::eigen::solve_eigen_with_solver(
            &model,
            &dof,
            &reducer,
            1,
            solver.as_ref(),
        );
        let wave = GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0, 1.0, 0.0],
            accel_y: None,
            accel_theta: None,
        };
        let history = linear_time_history_analysis(
            &model,
            &dof,
            &reducer,
            &wave,
            &NewmarkCfg::average_accel(),
            &Damping::MassProportional { h: 0.0, omega: 1.0 },
            &[],
            &[],
            false,
            None,
        );
        let state = sepika_solver::dynamic::timehistory::TimeStepState {
            step: 0,
            time: 0.0,
            disp_red: vec![0.0; reducer.n_indep],
            vel_red: vec![0.0; reducer.n_indep],
            accel_red: vec![0.0; reducer.n_indep],
        };
        let resumed = sepika_solver::dynamic::timehistory::linear_time_history_from_state(
            &model,
            &dof,
            &reducer,
            &wave,
            &NewmarkCfg::average_accel(),
            &Damping::MassProportional { h: 0.0, omega: 1.0 },
            &state,
            false,
            None,
        );
        println!(
            "{label}: eigen_with_solver={:?}; resumed={:?}",
            eigen_with_solver
                .as_ref()
                .map(|_| "ok")
                .map_err(|e| e.to_string()),
            resumed.as_ref().map(|_| "ok").map_err(|e| e.to_string())
        );
        println!(
            "{label}: eigen={:?}; linear_time_history={:?}",
            eigen.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
            history.as_ref().map(|_| "ok").map_err(|e| e.to_string())
        );
        if label == "normal" {
            assert!(eigen.is_ok());
            assert!(history.is_ok());
            assert!(eigen_with_solver.is_ok());
            assert!(resumed.is_ok());
        } else {
            let reason = format!(
                "接合部 ID 0 のパネル未算定: 柱 ID 1: {} は有限の正値が必要です",
                if label == "nan" {
                    "せん断弾性係数 G"
                } else {
                    "強軸のせん断剛性 G・Ve"
                }
            );
            assert!(eigen.unwrap_err().to_string().contains(&reason));
            assert!(history.err().unwrap().to_string().contains(&reason));
            assert!(eigen_with_solver.unwrap_err().to_string().contains(&reason));
            assert!(resumed.err().unwrap().to_string().contains(&reason));
        }
    }
}

#[test]
fn beam_736_public_dynamic_entries() {
    use sepika_solver::dynamic::damping::Damping;
    use sepika_solver::dynamic::timehistory::{
        linear_time_history_analysis, GroundMotion, NewmarkCfg,
    };
    for with_panel in [false, true] {
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            for g in [80_000.0, f64::MAX] {
                let mut model = l_frame(with_panel);
                model.elements[0].kind = kind;
                model.materials[0].density = 7.85e-9;
                model.materials[0].shear = Some(80_000.0);
                let dof = DofMap::build(&model);
                let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
                let stiffness = sepika_solver::common::assemble::assemble_global_k(&model, &dof);
                let mut solver = sepika_math::solver::make_solver(
                    sepika_math::solver::SolverBackend::DirectSparseCholesky,
                );
                solver.factorize(&reducer.reduce_k(&stiffness)).unwrap();
                model.materials[0].shear = Some(g);
                let eigen = sepika_solver::dynamic::eigen::solve_eigen(&model, &dof, &reducer, 1);
                let eigen_with_solver = sepika_solver::dynamic::eigen::solve_eigen_with_solver(
                    &model,
                    &dof,
                    &reducer,
                    1,
                    solver.as_ref(),
                );
                let wave = GroundMotion {
                    dt: 0.01,
                    accel_x: vec![0.0, 1.0, 0.0],
                    accel_y: None,
                    accel_theta: None,
                };
                let damping = Damping::MassProportional { h: 0.0, omega: 1.0 };
                let history = linear_time_history_analysis(
                    &model,
                    &dof,
                    &reducer,
                    &wave,
                    &NewmarkCfg::average_accel(),
                    &damping,
                    &[],
                    &[],
                    false,
                    None,
                );
                let state = sepika_solver::dynamic::timehistory::TimeStepState {
                    step: 0,
                    time: 0.0,
                    disp_red: vec![0.0; reducer.n_indep],
                    vel_red: vec![0.0; reducer.n_indep],
                    accel_red: vec![0.0; reducer.n_indep],
                };
                let resumed = sepika_solver::dynamic::timehistory::linear_time_history_from_state(
                    &model,
                    &dof,
                    &reducer,
                    &wave,
                    &NewmarkCfg::average_accel(),
                    &damping,
                    &state,
                    false,
                    None,
                );
                let linear =
                    sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(1));
                let mass = sepika_solver::common::assemble::assemble_global_m(
                    &model,
                    &dof,
                    sepika_element::behavior::MassOption::Consistent,
                );
                let results = [
                    eigen.map(|_| ()),
                    eigen_with_solver.map(|_| ()),
                    history.map(|_| ()),
                    resumed.map(|_| ()),
                    linear.map(|_| ()),
                    mass.map(|_| ()),
                ];
                for (entry, result) in [
                    "eigen",
                    "eigen_with_solver",
                    "history",
                    "resumed",
                    "linear",
                    "mass",
                ]
                .into_iter()
                .zip(results)
                {
                    println!("panel={with_panel}, kind={kind:?}, G={g:e}, {entry}: {result:?}");
                    if g == 80_000.0 {
                        result.unwrap();
                    } else {
                        let error = result.unwrap_err().to_string();
                        let reason = if with_panel {
                            "接合部 ID 0 のパネル未算定: 柱 ID 1: 強軸のせん断剛性 G・Ve"
                        } else {
                            "部材 ID 0 の弾性梁"
                        };
                        assert!(error.contains(reason), "{entry}: {error}");
                        if !with_panel {
                            assert!(
                                error.contains("GJ/L") && error.contains("非有限"),
                                "{error}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn beam_736_preparation_and_nonlinear_entries_return_stiffness_diagnostics() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for g in [80_000.0, f64::MAX] {
            let mut model = l_frame(false);
            model.elements[0].kind = kind;
            model.materials[0].fy = Some(235.0);
            model.materials[0].shear = Some(g);
            model.stories = [(0.0, vec![NodeId(2)]), (3000.0, vec![NodeId(0), NodeId(1)])]
                .into_iter()
                .enumerate()
                .map(|(i, (elevation, node_ids))| sepika_core::model::Story {
                    wall_weights: vec![],
                    level_kind: Default::default(),
                    structure: Default::default(),
                    id: sepika_core::ids::StoryId(i as u32),
                    name: format!("{i}F"),
                    elevation: Some(elevation),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids,
                    seismic_weight: Some(10_000.0),
                    weight_override: None,
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                })
                .collect();
            let prepared = sepika_solver::statics::analysis::Analysis::prepare(&model)
                .map(|_| ())
                .map_err(|e| e.to_string());
            let dof = DofMap::build(&model);
            let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
            let nonlinear = sepika_solver::nonlinear::pushover::pushover_analysis_recording(
                &model,
                &dof,
                &reducer,
                sepika_solver::statics::analysis::SeismicDir::X,
                1,
                sepika_solver::nonlinear::pushover::PushoverTarget::from_max_disp(10.0),
                Default::default(),
                false,
                false,
                Default::default(),
            )
            .map(|_| ());
            for (entry, result) in [("prepare", prepared), ("pushover", nonlinear)] {
                if g == 80_000.0 {
                    result.unwrap_or_else(|e| panic!("{kind:?} {entry}: {e}"));
                } else {
                    let error = result.unwrap_err();
                    assert!(
                        error.contains("部材 ID 0")
                            && error.contains("GJ/L")
                            && error.contains("非有限"),
                        "{kind:?} {entry}: {error}"
                    );
                }
            }
        }
    }
}

#[test]
fn panel_753_public_nonlinear_time_history_propagates_axial_limit() {
    use sepika_core::dof::Dof;
    use sepika_solver::dynamic::damping::{Damping, DampingAccumulation};
    use sepika_solver::dynamic::timehistory::{
        nonlinear_time_history_analysis, GroundMotion, NewmarkCfg, NonlinearThCfg,
    };
    for (n, accepted, moving, scale, singular) in [
        (0.0, true, false, 1.0, false),
        (0.6, true, false, 1.0, false),
        (0.9999, true, false, 1.0, false),
        (1.0, false, false, 1.0, false),
        (1.1, false, false, 1.0, false),
        (0.6, false, true, 1.0, false),
        (0.5, true, false, 0.0001, false),
        (1.0, false, false, 0.0001, false),
        (1.1, false, false, 0.0001, false),
        (0.5, true, false, 0.001, false),
        (0.5, true, false, 0.00001, false),
        (0.5, false, false, 0.0001, true),
    ] {
        let mut model = l_frame(true);
        if scale != 1.0 {
            for node in &mut model.nodes {
                node.coord = node.coord.map(|v| v * scale);
            }
            for section in &mut model.sections {
                if let Some(SectionShape::SteelH {
                    height,
                    width,
                    web_thick,
                    flange_thick,
                    ..
                }) = &mut section.shape
                {
                    *height *= scale;
                    *width *= scale;
                    *web_thick *= scale;
                    *flange_thick *= scale;
                }
                section.depth *= scale;
                section.width *= scale;
                section.area *= scale * scale;
                section.as_y *= scale * scale;
                section.as_z *= scale * scale;
                section.iy *= scale.powi(4);
                section.iz *= scale.powi(4);
                section.j *= scale.powi(4);
            }
            for member in &mut model.elements {
                member.rigid_zone.length_i *= scale;
                member.rigid_zone.length_j *= scale;
                member.rigid_zone.face_i = member.rigid_zone.face_i.map(|v| v * scale);
                member.rigid_zone.face_j = member.rigid_zone.face_j.map(|v| v * scale);
            }
        }
        if singular {
            model.panel_zone = sepika_core::model::PanelZoneMode::None;
            model
                .elements
                .retain(|element| element.kind != ElementKind::PanelZone);
            model.sections[0].shape = None;
            model.sections[0].depth = 0.0;
            model.sections[0].width = 0.0;
        }
        model.materials[0].fy = Some(235.0);
        model.materials[0].density = 7.85e-9;
        let dof = DofMap::build(&model);
        let reducer = sepika_solver::common::constraint::Reducer::build(&model, &dof);
        assert_eq!(reducer.n_indep, dof.n_active());
        let mut displacement = vec![0.0; dof.n_active()];
        displacement[dof.active(Dof::Uz as usize).unwrap() as usize] =
            -n * 235.0 * 3000.0 * scale / 205000.0;
        let mut velocity = vec![0.0; dof.n_active()];
        if moving {
            velocity[dof.active(Dof::Uz as usize).unwrap() as usize] = -1_000_000.0;
        }
        let result = nonlinear_time_history_analysis(
            &model,
            &dof,
            &reducer,
            &GroundMotion {
                dt: 0.01,
                accel_x: vec![0.0; if moving { 2 } else { 1 }],
                accel_y: None,
                accel_theta: None,
            },
            &NewmarkCfg::average_accel(),
            &Damping::MassProportional { h: 0.0, omega: 1.0 },
            DampingAccumulation::NonCumulative,
            &displacement,
            &velocity,
            NonlinearThCfg {
                apply_long_term: false,
                ..Default::default()
            },
        );
        println!(
            "panel753 public dynamic: n={n}, moving={moving}, scale={scale}, result={:?}",
            result.as_ref().map(|_| "ok").map_err(|e| e.to_string())
        );
        assert_eq!(result.is_ok(), accepted);
        if accepted {
            let response = result.unwrap();
            assert!(response.nonlinear);
            assert_eq!(response.time, vec![0.0]);
            assert!(
                (response.recording.unwrap().node_disp[0][0][2]
                    + n * 235.0 * 3000.0 * scale / 205000.0)
                    .abs()
                    < 1e-12
            );
        } else {
            let error = result.unwrap_err().to_string();
            if singular {
                assert!(
                    error.contains("部材 ID 0") && error.contains("初期断面剛性"),
                    "{error}"
                );
                continue;
            }
            if moving {
                assert!(
                    error.contains("step 1") && error.contains("時刻 0.01 s"),
                    "{error}"
                );
            }
            assert!(
                error.contains("接合部 ID 0") && error.contains("軸耐力到達・超過"),
                "{error}"
            );
        }
    }
}
