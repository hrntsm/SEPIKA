//! 剛域を持つ門形フレームの解析機構・可撓長のVerification。Dsの規準適合は検証しない。

use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    Constraint, ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material,
    MaterialCategory, Model, Node, RigidZone, Section, Story,
};
use sepika_solver::common::constraint::Reducer;
use sepika_solver::nonlinear::pushover::{
    pushover_analysis, HingeLevel, MechanismType, PushoverResult,
};
use sepika_solver::statics::analysis::SeismicDir;

/// 節点（座標・拘束）を作る補助。
fn node(id: u32, coord: [f64; 3], restraint: Dof6Mask, story: Option<StoryId>) -> Node {
    Node {
        id: NodeId(id),
        coord,
        restraint,
        mass: None,
        story,
        support_spring: None,
    }
}

/// ファイバー要素（2 節点）を作る補助。
fn fiber_elem(id: u32, i: u32, j: u32, section: u32, rigid: f64) -> ElementData {
    ElementData {
        id: ElemId(id),
        kind: ElementKind::Fiber,
        nodes: smallvec::smallvec![NodeId(i), NodeId(j)],
        section: Some(SectionId(section)),
        local_axis: LocalAxis {
            ref_vector: [1.0, 0.0, 0.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone {
            length_i: rigid,
            length_j: rigid,
            face_i: Some(rigid),
            face_j: Some(rigid),
            ..Default::default()
        },
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    }
}

/// 1 層 1 スパンの門形フレーム（柱 100×100・はり 300×300）。
/// `rigid` に柱の剛域長 λ [mm] を与える。はりは十分強くして柱の崩壊機構に固定する。
fn portal_frame(rigid: f64, seismic_weight: f64) -> Model {
    let sec = |id: u32, b: f64, d: f64| -> Section {
        Section {
            frame_use: None,
            id: SectionId(id),
            name: format!("s{id}"),
            area: b * d,
            iy: b * d.powi(3) / 12.0,
            iz: d * b.powi(3) / 12.0,
            j: 1.0e6,
            depth: d,
            width: b,
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
        }
    };
    Model {
        load_cases: sepika_core::model::default_load_cases(),
        combinations: sepika_core::model::default_combinations(),
        ultimate_gravity_combination: Some(0),
        nodes: vec![
            node(0, [0.0, 0.0, 0.0], Dof6Mask::FIXED, None),
            // ファイバー要素は Z 軸柱の頂部ねじり DOF（Rz）に剛性を持たないため拘束する。
            node(1, [0.0, 0.0, 3000.0], Dof6Mask(0b100000), Some(StoryId(0))),
            node(
                2,
                [5000.0, 0.0, 3000.0],
                Dof6Mask(0b100000),
                Some(StoryId(0)),
            ),
            node(3, [5000.0, 0.0, 0.0], Dof6Mask::FIXED, None),
        ],
        elements: vec![
            fiber_elem(0, 0, 1, 0, rigid), // 左柱
            fiber_elem(1, 1, 2, 1, 0.0),   // はり（強い断面・剛域なし）
            fiber_elem(2, 3, 2, 0, rigid), // 右柱
        ],
        sections: vec![sec(0, 100.0, 100.0), sec(1, 300.0, 300.0)],
        materials: vec![Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "steel".to_string(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
        }],
        stories: vec![
            // 階は床であり、先頭は基部の床（`Model::layers` の不変条件）。
            Story {
                wall_weights: Vec::new(),
                level_kind: Default::default(),
                structure: Default::default(),
                id: StoryId(0),
                name: "1F".to_string(),
                elevation: Some(0.0),
                sl_offset_mm: Some(0.0),
                source_story: None,
                node_ids: vec![],
                seismic_weight: None,
                weight_override: None,
                dynamic_mass: None,
                standard_floor_load: None,
                column_finish_area_weight: 0.0,
                fireproof: Default::default(),
            },
            Story {
                wall_weights: Vec::new(),
                level_kind: Default::default(),
                structure: Default::default(),
                id: StoryId(1),
                name: "2F".to_string(),
                elevation: Some(3000.0),
                sl_offset_mm: Some(0.0),
                source_story: None,
                node_ids: vec![NodeId(1), NodeId(2)],
                seismic_weight: Some(seismic_weight),
                weight_override: None,
                dynamic_mass: None,
                standard_floor_load: None,
                column_finish_area_weight: 0.0,
                fireproof: Default::default(),
            },
        ],
        constraints: vec![Constraint::rigid_diaphragm(
            StoryId(1),
            NodeId(1),
            vec![NodeId(2)],
        )],
        ..Default::default()
    }
}

fn run_pushover(rigid: f64, seismic_weight: f64, steps: usize) -> PushoverResult {
    let model = portal_frame(rigid, seismic_weight);
    let dofmap = DofMap::build(&model);
    let reducer = Reducer::build(&model, &dofmap);
    pushover_analysis(&model, &dofmap, &reducer, SeismicDir::X, steps, 0.0, false)
        .expect("pushover should run end-to-end")
}

#[test]
fn 剛域つきプッシュオーバーが解析上の機構に達する() {
    let result = run_pushover(300.0, 600_000.0, 400);

    // 柱両端の 4 ヒンジで運動学的機構が成立している。
    let yielded = result
        .hinges
        .iter()
        .filter(|h| !matches!(h.level, HingeLevel::Crack))
        .count();
    assert!(yielded >= 4, "降伏ヒンジが不足: {yielded}");
    assert!(
        !matches!(result.mechanism, MechanismType::Partial),
        "剛域つきで崩壊機構が成立しない（Partial）"
    );
}

#[test]
fn 未降伏のフレームを解析上の機構成立と扱わない() {
    let result = run_pushover(300.0, 1_000.0, 20);
    assert!(
        matches!(result.mechanism, MechanismType::Partial),
        "降伏させていないのに機構が成立した"
    );
}

/// 剛域は保有水平耐力（崩壊荷重）を可撓長基準へ引き上げること。
/// 4材端が降伏した点のせん断力比を可撓長の独立比と比較する。
#[test]
fn 剛域は保有水平耐力を可撓長基準へ引き上げる() {
    let collapse_shear = |r: &PushoverResult| -> Option<f64> {
        let mut seen: std::collections::BTreeSet<(u32, u8)> = std::collections::BTreeSet::new();
        let mut events: Vec<_> = r
            .hinges
            .iter()
            .filter(|h| !matches!(h.level, HingeLevel::Crack))
            .collect();
        events.sort_by_key(|h| h.step);
        for h in events {
            seen.insert((h.elem.index() as u32, u8::from(h.pos >= 0.5)));
            if seen.len() >= 4 {
                return r
                    .capacity_curve
                    .iter()
                    .find(|c| c.step == h.step)
                    .map(|c| c.base_shear);
            }
        }
        None
    };
    let q0 = collapse_shear(&run_pushover(0.0, 600_000.0, 400)).expect("剛域なしで機構不成立");
    let q1 = collapse_shear(&run_pushover(300.0, 600_000.0, 400)).expect("剛域ありで機構不成立");
    let theory = 3000.0 / (3000.0 - 2.0 * 300.0);
    let ratio = q1 / q0;
    assert!(
        (ratio / theory - 1.0).abs() < 0.05,
        "保有水平耐力の比が可撓長基準から外れている: 実測 {ratio:.4}, 理論 {theory:.4}"
    );
}
