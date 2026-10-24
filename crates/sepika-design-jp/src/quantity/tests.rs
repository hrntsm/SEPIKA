//! 数量積算のモデル走査テスト。

use sepika_core::model::{FloorRegion, Slab, SlabPlate, SlabShape};
use smallvec::SmallVec;

use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, FloorRegionId, MaterialId, NodeId, SectionId, SlabId};
use sepika_core::model::{
    DistributionMethod, ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse,
    LocalAxis, Material, MaterialCategory, Model, Node, RigidZone, Section,
};
use sepika_core::section_shape::{
    BeamStirrup, CircleColumnHoop, RcBeamRebar, RcCircleColumnRebar, RcRectColumnRebar,
    RectColumnHoop, SectionShape,
};

use super::*;

fn node(id: u32, x: f64, y: f64, z: f64) -> Node {
    Node {
        id: NodeId(id),
        coord: [x, y, z],
        restraint: Dof6Mask::FREE,
        mass: None,
        story: None,
        support_spring: None,
    }
}

fn line_elem(id: u32, n0: u32, n1: u32, sec: u32) -> ElementData {
    ElementData {
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
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: RigidZone::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    }
}

fn rc_beam_rebar() -> RcBeamRebar {
    RcBeamRebar {
        main_dia: 25.0,
        top: vec![4],
        bottom: vec![4],
        cover: 40.0,
        stirrup: BeamStirrup {
            dia: 10.0,
            pitch: 200.0,
            legs: 2,
        },
    }
}

fn rc_girder_section(id: u32) -> Section {
    let shape = SectionShape::RcBeamRect {
        b: 400.0,
        d: 800.0,
        rebar: rc_beam_rebar(),
    };
    with_rc_materials(shape.to_section(SectionId(id), format!("G{id}")))
}

/// 材料は断面が持つ。RC 断面へ主材料（コンクリート 0）と鉄筋（1）を割り当てる。
fn with_rc_materials(mut sec: Section) -> Section {
    sec.frame_use = Some(
        if matches!(
            sec.shape,
            Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. })
        ) {
            FrameSectionUse::Column
        } else {
            FrameSectionUse::Girder
        },
    );
    sec.material = Some(MaterialId(0));
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(1));
    sec
}

/// 鉄筋の材料（材料名がグレード名、`fy` が降伏点）。
fn rebar_material(id: u32) -> Material {
    Material {
        strength_factor: None,
        id: MaterialId(id),
        name: "SD345".to_string(),
        category: MaterialCategory::Rebar,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(345.0),
        concrete_class: Default::default(),
    }
}

fn rc_column_section(id: u32) -> Section {
    let rebar = RcRectColumnRebar {
        main_dia: 25.0,
        x: vec![4],
        y: vec![3],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: 13.0,
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    let shape = SectionShape::RcColumnRect {
        b: 700.0,
        d: 700.0,
        rebar,
    };
    with_rc_materials(shape.to_section(SectionId(id), format!("C{id}")))
}

fn rc_material(id: u32) -> Material {
    Material {
        strength_factor: None,
        id: MaterialId(id),
        name: "Fc24".to_string(),
        category: MaterialCategory::Concrete,
        young: 22_700.0,
        poisson: 0.2,
        density: 2.4e-9,
        shear: None,
        fc: Some(24.0),
        fy: None,
        concrete_class: Default::default(),
    }
}

fn steel_material(id: u32) -> Material {
    Material {
        strength_factor: None,
        id: MaterialId(id),
        name: "SN400B".to_string(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
    }
}

/// 2 柱＋上部大梁＋基礎梁の 1 スパン RC ラーメン。
fn rc_portal_model() -> Model {
    let nodes = vec![
        node(0, 0.0, 0.0, 0.0),
        node(1, 6_000.0, 0.0, 0.0),
        node(2, 0.0, 0.0, 3_500.0),
        node(3, 6_000.0, 0.0, 3_500.0),
    ];
    let mut girder = line_elem(2, 2, 3, 0);
    girder.rigid_zone.face_i = Some(350.0);
    girder.rigid_zone.face_j = Some(350.0);
    let mut fg = line_elem(3, 0, 1, 0);
    fg.rigid_zone.face_i = Some(350.0);
    fg.rigid_zone.face_j = Some(350.0);
    let elements = vec![
        line_elem(0, 0, 2, 1), // 柱
        line_elem(1, 1, 3, 1), // 柱
        girder,                // 大梁
        fg,                    // 基礎梁
    ];
    Model {
        nodes,
        elements,
        sections: vec![rc_girder_section(0), rc_column_section(1)],
        materials: vec![rc_material(0), rebar_material(1)],
        ..Default::default()
    }
}

#[test]
fn test_rc_portal_categories_and_concrete() {
    let model = rc_portal_model();
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert_eq!(q.items.len(), 4);

    let cols: Vec<_> = q
        .items
        .iter()
        .filter(|i| i.category == MemberCategory::Column)
        .collect();
    let girders: Vec<_> = q
        .items
        .iter()
        .filter(|i| i.category == MemberCategory::Girder)
        .collect();
    let fgs: Vec<_> = q
        .items
        .iter()
        .filter(|i| i.category == MemberCategory::FoundationGirder)
        .collect();
    assert_eq!(cols.len(), 2);
    assert_eq!(girders.len(), 1);
    assert_eq!(fgs.len(), 1);

    // 柱: 0.7×0.7×3.5 = 1.715 m³、型枠 2×(0.7+0.7)×3.5 = 9.8 m²
    assert!((cols[0].concrete_m3 - 1.715).abs() < 1e-9);
    assert!((cols[0].formwork_m2 - 9.8).abs() < 1e-9);

    // 大梁: 内法 L=6000−350×2=5300 → 0.4×0.8×5.3 = 1.696 m³
    assert!((girders[0].concrete_m3 - 1.696).abs() < 1e-9);
    // 型枠（スラブなし）: 側面 0.8×2×5.3 + 底面 0.4×5.3 = 10.6 m²
    assert!((girders[0].formwork_m2 - (0.8 * 2.0 + 0.4) * 5.3).abs() < 1e-9);

    // 基礎梁の型枠: 側面 1 面＋底面 = (0.8+0.4)×5.3 = 6.36 m²
    assert!((fgs[0].formwork_m2 - 1.2 * 5.3).abs() < 1e-9);

    // 合計（QuantityTotals）は明細の全フィールドの和と一致する。
    let totals = q.totals();
    let concrete_sum: f64 = q.items.iter().map(|i| i.concrete_m3).sum();
    let formwork_sum: f64 = q.items.iter().map(|i| i.formwork_m2).sum();
    let rebar_sum: f64 = q.items.iter().map(|i| i.rebar_weight_t()).sum();
    let steel_sum: f64 = q.items.iter().map(|i| i.steel_weight_t()).sum();
    let joints_sum: f64 = q.items.iter().map(|i| i.rebar_joints).sum();
    assert!((totals.concrete_m3 - concrete_sum).abs() < 1e-12);
    assert!((totals.formwork_m2 - formwork_sum).abs() < 1e-12);
    assert!((totals.rebar_t - rebar_sum).abs() < 1e-12);
    assert!((totals.steel_t - steel_sum).abs() < 1e-12);
    assert!((totals.rebar_joints - joints_sum).abs() < 1e-12);

    // 部位別小計はカテゴリ明細の全フィールドの和と一致する
    // （区分は MemberCategory の定義順で基礎梁・柱・大梁の 3 つ）。
    let by_cat = q.totals_by_category();
    assert_eq!(
        by_cat.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![
            MemberCategory::FoundationGirder,
            MemberCategory::Column,
            MemberCategory::Girder
        ]
    );
    for (category, sub) in &by_cat {
        let items: Vec<_> = q.items.iter().filter(|i| i.category == *category).collect();
        assert!(!items.is_empty(), "category={category:?}");
        let concrete: f64 = items.iter().map(|i| i.concrete_m3).sum();
        let formwork: f64 = items.iter().map(|i| i.formwork_m2).sum();
        let rebar: f64 = items.iter().map(|i| i.rebar_weight_t()).sum();
        let steel: f64 = items.iter().map(|i| i.steel_weight_t()).sum();
        let joints: f64 = items.iter().map(|i| i.rebar_joints).sum();
        assert!(
            (sub.concrete_m3 - concrete).abs() < 1e-12,
            "category={category:?}: concrete sub={}, items={concrete}",
            sub.concrete_m3
        );
        assert!(
            (sub.formwork_m2 - formwork).abs() < 1e-12,
            "category={category:?}: formwork sub={}, items={formwork}",
            sub.formwork_m2
        );
        assert!(
            (sub.rebar_t - rebar).abs() < 1e-12,
            "category={category:?}: rebar sub={}, items={rebar}",
            sub.rebar_t
        );
        assert!(
            (sub.steel_t - steel).abs() < 1e-12,
            "category={category:?}: steel sub={}, items={steel}",
            sub.steel_t
        );
        assert!(
            (sub.rebar_joints - joints).abs() < 1e-12,
            "category={category:?}: joints sub={}, items={joints}",
            sub.rebar_joints
        );
    }
}

#[test]
fn test_unset_frame_use_of_supplied_section_is_not_counted_as_beam() {
    let mut model = rc_portal_model();
    model.sections[0].shape = None;
    model.sections[0].frame_use = None;

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert_eq!(
        q.items
            .iter()
            .filter(|item| item.category == MemberCategory::Column)
            .count(),
        2
    );
    assert!(!q.items.iter().any(|item| matches!(
        item.category,
        MemberCategory::Girder | MemberCategory::FoundationGirder
    )));
}

#[test]
fn rc_primary_missing_frame_use_rejects_quantity_with_section_reason() {
    let mut model = rc_portal_model();
    model.sections[0].frame_use = None;
    let before = model.clone();
    let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
    assert!(
        error.contains("SectionId(0)") && error.contains("RC 梁用断面"),
        "{error}"
    );
    assert!(model.eq_ignoring_dofmap(&before));
}

#[test]
fn test_rc_girder_main_bars_and_stirrups() {
    let model = rc_portal_model();
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let girder = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Girder)
        .unwrap();

    // 主筋: 両端とも連続梁なし → 両外端（タイプ7a）
    // 1 本長さ = 5300 + 35×25×2 = 7050mm、8 本 → 56.4 m
    let main = girder
        .rebar
        .iter()
        .find(|r| r.usage == RebarUsage::MainBar)
        .unwrap();
    assert!((main.total_length_m - 8.0 * 7.05).abs() < 1e-9);
    // D25 = 3.98 kg/m → 56.4 m = 224.472 kg
    assert!((main.weight_t - 8.0 * 7.05 * 3.98 / 1_000.0).abs() < 1e-9);

    // スターラップ: 一組 2×400+2×800 = 2400、本数 5300/200 = 26.5
    let st = girder
        .rebar
        .iter()
        .find(|r| r.usage == RebarUsage::Stirrup)
        .unwrap();
    assert!((st.total_length_m - 2.4 * 26.5).abs() < 1e-9);

    // 継手: 8 本 × (0.5 + 0.5×floor(5.3/5)) = 8 × 1.0
    assert!((girder.rebar_joints - 8.0).abs() < 1e-9);
}

#[test]
fn test_rc_column_rebar() {
    let model = rc_portal_model();
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let col = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Column)
        .unwrap();

    // 主筋: 固有本数 2·4+2·3−4=10 本、各 H=3.5m
    let total_main_len: f64 = col
        .rebar
        .iter()
        .filter(|r| r.usage == RebarUsage::MainBar)
        .map(|r| r.total_length_m)
        .sum();
    assert!((total_main_len - 10.0 * 3.5).abs() < 1e-9);

    // フープ: 一組 2×700+2×700 = 2800、本数 3500/100 = 35 → 98 m
    let hoop = col
        .rebar
        .iter()
        .find(|r| r.usage == RebarUsage::Hoop)
        .unwrap();
    assert!((hoop.total_length_m - 2.8 * 35.0).abs() < 1e-9);

    // 継手: 10 本 × 1 個所（階高 < 7m）
    assert!((col.rebar_joints - 10.0).abs() < 1e-9);
}

#[test]
fn test_girder_formwork_slab_deduction() {
    // 大梁の両側にスラブが取り付く場合、側面せいからスラブ厚を控除する。
    let mut model = rc_portal_model();
    model.slab_thickness = 150.0;
    let mut slab_sec =
        SectionShape::RcSlab { thickness: 150.0 }.to_section(SectionId(2), "S15".into());
    slab_sec.material = Some(MaterialId(0));
    model.sections.push(slab_sec);
    // 大梁（節点 2-3）の両側（y=+5000 と y=−5000）に床板を配置する。
    model.nodes.push(node(4, 0.0, 5_000.0, 3_500.0));
    model.nodes.push(node(5, 6_000.0, 5_000.0, 3_500.0));
    model.nodes.push(node(6, 0.0, -5_000.0, 3_500.0));
    model.nodes.push(node(7, 6_000.0, -5_000.0, 3_500.0));
    for (a, b) in [(5u32, 4u32), (7u32, 6u32)] {
        model.add_enclosed_slab_from_nodes(
            &[NodeId(2), NodeId(3), NodeId(a), NodeId(b)],
            SlabPlate {
                section: Some(SectionId(2)),
                loads: vec![],
                usage: None,
                method: DistributionMethod::TriTrapezoid,
                one_way: None,
            },
        );
        set_test_slab_placements(&mut model);
    }

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let girder = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Girder)
        .unwrap();
    // 側面 (0.8−0.15)×2×5.3 + 底面 0.4×5.3
    let expected = ((0.8 - 0.15) * 2.0 + 0.4) * 5.3;
    assert!((girder.formwork_m2 - expected).abs() < 1e-9);
}

#[test]
fn test_beam_by_ratio() {
    // 柱に取り付かない水平梁は小梁として鉄筋比で概算する。
    let mut model = rc_portal_model();
    model.nodes.push(node(4, 2_000.0, 0.0, 3_500.0));
    model.nodes.push(node(5, 2_000.0, 3_000.0, 3_500.0));
    model.elements.push(line_elem(4, 4, 5, 0));

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let beam = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Beam)
        .unwrap();
    // 0.4×0.8×3.0 = 0.96 m³、型枠 (0.4+1.6)×3.0 = 6.0 m²
    assert!((beam.concrete_m3 - 0.96).abs() < 1e-9);
    assert!((beam.formwork_m2 - 6.0).abs() < 1e-9);
    // 主筋 0.8% → 0.96×0.008×7.85 t
    let main = beam
        .rebar
        .iter()
        .find(|r| r.usage == RebarUsage::BeamMain)
        .unwrap();
    assert!((main.weight_t - 0.96 * 0.008 * 7.85).abs() < 1e-9);
}

#[test]
fn test_steel_member_weight() {
    // S 造: W = L×A×7.85（t/m³）。
    let mut model = rc_portal_model();
    let shape = SectionShape::SteelH {
        root_r: Some(0.0),
        height: 400.0,
        width: 200.0,
        web_thick: 8.0,
        flange_thick: 13.0,
    };
    let a = shape.calc_area();
    // 材料は断面が持つ。
    let mut sec = shape.to_section(SectionId(2), "H-400x200x8x13".to_string());
    sec.frame_use = Some(FrameSectionUse::Girder);
    sec.material = Some(MaterialId(2));
    model.sections.push(sec);
    model.materials.push(steel_material(2));
    // 大梁を S 断面へ差し替え。
    model.elements[2].section = Some(SectionId(2));

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let girder = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Girder)
        .unwrap();
    assert_eq!(girder.structure, StructureKind::S);
    assert!(girder.concrete_m3 == 0.0 && girder.formwork_m2 == 0.0);
    let steel = girder.steel.as_ref().unwrap();
    // 鉄骨は節点間距離（6.0m）で算定。
    assert!((steel.length_m - 6.0).abs() < 1e-9);
    assert!((steel.weight_t - a * 6_000.0 * 7.85e-9).abs() < 1e-12);
}

#[test]
fn test_brace_length_and_weight() {
    // ブレース長さは節点間距離 LB=√(L²+H²)。
    let mut model = rc_portal_model();
    let shape = SectionShape::SteelAngle {
        leg_a: 90.0,
        leg_b: 90.0,
        thick: 7.0,
    };
    let a = shape.calc_area();
    // 材料は断面が持つ。
    let mut sec = shape.to_section(SectionId(2), "L-90x90x7".to_string());
    sec.material = Some(MaterialId(2));
    model.sections.push(sec);
    model.materials.push(steel_material(2));
    let mut brace = line_elem(4, 0, 3, 2);
    brace.kind = ElementKind::Brace {
        tension_only: false,
    };
    model.elements.push(brace);

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let br = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Brace)
        .unwrap();
    let lb = (6_000.0_f64.powi(2) + 3_500.0_f64.powi(2)).sqrt();
    let steel = br.steel.as_ref().unwrap();
    assert!((steel.length_m - lb / 1_000.0).abs() < 1e-9);
    assert!((steel.weight_t - a * lb * 7.85e-9).abs() < 1e-12);
}

#[test]
fn test_slab_quantity() {
    let mut model = rc_portal_model();
    // 6m×5m の床板。板厚はスラブ断面が持つ（建物一律の `slab_thickness` は
    // 剛性計算に見込む厚さであり、数量には使わない）。
    model.nodes.push(node(4, 0.0, 5_000.0, 3_500.0));
    model.nodes.push(node(5, 6_000.0, 5_000.0, 3_500.0));
    let slab_sec = sepika_core::ids::SectionId(model.sections.len() as u32);
    model.sections.push(
        sepika_core::section_shape::SectionShape::RcSlab { thickness: 150.0 }
            .to_section(slab_sec, "S15".into()),
    );
    model.add_enclosed_slab_from_nodes(
        &[NodeId(2), NodeId(3), NodeId(5), NodeId(4)],
        SlabPlate {
            section: Some(slab_sec),
            loads: vec![],
            usage: None,
            method: DistributionMethod::TriTrapezoid,
            one_way: None,
        },
    );
    set_test_slab_placements(&mut model);

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let slab = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Slab)
        .unwrap();
    // 面積 30 m²、体積 30×0.15 = 4.5 m³、型枠 30 m²
    assert!((slab.formwork_m2 - 30.0).abs() < 1e-6);
    assert!((slab.concrete_m3 - 4.5).abs() < 1e-6);
    // 床筋: 4.5×1.0%×7.85 t
    let bar = slab.rebar.first().unwrap();
    assert!((bar.weight_t - 4.5 * 0.01 * 7.85).abs() < 1e-9);
}

#[test]
fn test_wall_quantity_with_opening() {
    let mut model = rc_portal_model();
    // 柱・梁で囲まれた 4 節点壁（節点 0-1-3-2）。
    let shape = SectionShape::RcWall {
        thickness: 200.0,
        pwh_ratio: Some(0.0025),
        pwv_ratio: Some(0.0025),
        ps: 0.0025,
    };
    let sec = shape.to_section(SectionId(2), "W20".to_string());
    model.sections.push(sec);
    let mut wall = line_elem(4, 0, 1, 2);
    wall.kind = ElementKind::Wall;
    wall.nodes = {
        let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
        for n in [0, 1, 3, 2] {
            v.push(NodeId(n));
        }
        v
    };
    model.elements.push(wall);
    model.wall_attrs.push(sepika_core::model::WallAttr {
        elem: ElemId(4),
        opening_area: 2.0e6, // 2 m²
        opening_weight: 0.0,
        slit: Default::default(),
        openings: vec![],
        finish_intensity: 0.0,
    });

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let w = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Wall)
        .unwrap();
    // 内法: L = 6000 − 700/2×2 = 5300（柱 700 角）、H = 3500 − 800/2×2 = 2700（梁せい 800）
    let net = 5.3_f64 * 2.7 - 2.0;
    assert!((w.concrete_m3 - net * 0.2).abs() < 1e-6);
    assert!((w.formwork_m2 - net * 2.0).abs() < 1e-6);
    // 壁筋: 横筋・縦筋の 2 件が計上される。
    assert_eq!(w.rebar.len(), 2);
    assert!(w.rebar_weight_t() > 0.0);
}

/// `try_compute_quantity_takeoff` が壁展開モデルを見ていることの回帰テスト
/// （申し送り §5.15）。壁の解析要素（`ElementKind::Wall`）は入力の正である
/// `Model`（`WallPlate`＋`WallRegion` 経由）には存在しない生成物（D5）のため、
/// 内部で壁展開しないと `MemberCategory::Wall` の数量が一切集計されない
/// （壁の数量が積算から静かに消える）。
#[test]
fn test_wall_quantity_via_wall_plate_is_included() {
    use sepika_core::ids::{WallPlateId, WallRegionId};
    use sepika_core::model::{WallPlate, WallPlateShape, WallRegion};

    let mut model = rc_portal_model();
    let shape = SectionShape::RcWall {
        thickness: 200.0,
        pwh_ratio: None,
        pwv_ratio: None,
        ps: 0.0025,
    };
    let sec = shape.to_section(SectionId(2), "W20".to_string());
    model.sections.push(sec);
    // 柱・梁で囲まれた4節点壁（節点 0-1-3-2）を壁版として構築する
    // （`test_wall_quantity_with_opening` の直接 `ElementData` 構築と同じ幾何）。
    model.add_enclosed_wall_plate_from_nodes(
        &[NodeId(0), NodeId(1), NodeId(3), NodeId(2)],
        WallPlate {
            dl_support: None,
            self_weight_shares: Vec::new(),
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(2)),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: vec![],
            slit: Default::default(),
        },
    );
    model.wall_regions.push(WallRegion {
        id: WallRegionId(0),
        name: String::new(),
        boundary: vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)],
        wall_plate_ids: vec![WallPlateId(0)],
        posts: Vec::new(),
    });
    // 入力モデル自体には壁の解析要素は一切ない（D5）。
    assert!(model.elements.iter().all(|e| e.kind != ElementKind::Wall));

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let w = q.items.iter().find(|i| i.category == MemberCategory::Wall);
    assert!(
        w.is_some(),
        "壁展開モデルを経由しないと、model.elements に壁要素が 0 件のため \
         壁の数量が積算から欠落するはず: {:?}",
        q.items.iter().map(|i| i.category).collect::<Vec<_>>()
    );
    let w = w.unwrap();
    assert!(w.concrete_m3 > 0.0, "壁のコンクリート数量が正であるはず");
    assert!(w.formwork_m2 > 0.0, "壁の型枠数量が正であるはず");
}

/// 取り付く壁版（`Attached`。パラペット等）は解析要素を持たない（D5）ため、
/// `wall_quantity`（エレメント経由）ではなく `attached_wall_plate_quantity`
/// （`WallPlate` から直接算定）が数量へ算入する。カテゴリは
/// 雑壁の区分として `MemberCategory::MiscWall` を使う。
#[test]
fn test_attached_wall_plate_quantity_is_included_as_misc_wall() {
    use sepika_core::ids::WallPlateId;
    use sepika_core::model::{RegionAnchor, WallPlate, WallPlateShape};

    let mut model = rc_portal_model();
    let shape = SectionShape::RcWall {
        thickness: 150.0,
        pwh_ratio: None,
        pwv_ratio: None,
        ps: 0.0,
    };
    let sec = shape.to_section(SectionId(2), "W15".to_string());
    model.sections.push(sec);
    // 頂部の大梁（節点2-3）に載るパラペット（立ち上がり高さ1000mm、全長）。
    model.wall_plates.push(WallPlate {
        dl_support: None,
        self_weight_shares: Vec::new(),
        id: WallPlateId(0),
        shape: WallPlateShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(2), NodeId(3)],
                span: [0.0, 1.0],
                transfer: sepika_core::model::LoadTransfer::Anchor,
            },
            extent: Some([1000.0, 1000.0]),
        },
        section: Some(SectionId(2)),
        opening_area: 0.0,
        opening_weight: 0.0,
        openings: Vec::new(),
        loads: vec![],
        slit: Default::default(),
    });
    assert!(model.elements.iter().all(|e| e.kind != ElementKind::Wall));

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let items: Vec<_> = q
        .items
        .iter()
        .filter(|i| i.category == MemberCategory::MiscWall)
        .collect();
    assert_eq!(items.len(), 1, "取り付く壁版1件が雑壁として数量に入るはず");
    // ラベルは壁版に割り当てた断面の符号（slab_quantity が SlabId を使うのと同じく、
    // ループ位置ではなく壁版自身に紐づく安定した値を使う）。
    assert_eq!(items[0].label, "W15");
    let expected_m3 = 6_000.0 * 1_000.0 * 150.0 * 1e-9;
    assert!(
        (items[0].concrete_m3 - expected_m3).abs() / expected_m3 < 1e-9,
        "concrete_m3={} expected={}",
        items[0].concrete_m3,
        expected_m3
    );
    assert_eq!(items[0].rebar_status(), QuantityStatus::Incomplete);
    assert_eq!(items[0].rebar_issues.len(), 2);
    assert!(items[0]
        .rebar_issues
        .iter()
        .all(|i| i.kind == RebarIssueKind::Unsupported && i.wall_plate == Some(WallPlateId(0))));
    assert_eq!(q.totals().rebar_status(), QuantityStatus::Incomplete);
}

/// 大梁（i 端ハンチ）・基礎梁（両端ハンチ）のハンチが部材付帯情報から
/// 平均断面で体積・型枠へ加算される。
#[test]
fn test_haunch_from_member_detail() {
    use sepika_core::model::{Haunch as CoreHaunch, MemberDetailAttr};

    // 大梁（ElemId(2)）の i 端にハンチ: 長さ 1000、せい増分 200、幅増分 200。
    let mut model = rc_portal_model();
    model.member_detail_attrs.push(MemberDetailAttr {
        elem: ElemId(2),
        haunch_i: Some(CoreHaunch {
            length: 1_000.0,
            depth_increase: 200.0,
            width_increase: 200.0,
        }),
        haunch_j: None,
        joints: vec![],
    });
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let g = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Girder)
        .unwrap();

    // 体積: 基準 0.4×0.8×5.3 に平均断面 (400+600)(800+1000)/4×1000 を加算。
    let base_m3 = 0.4 * 0.8 * 5.3;
    let haunch_m3 = (400.0 + 600.0) * (800.0 + 1_000.0) / 4.0 * 1_000.0 * 1e-9;
    assert!((g.concrete_m3 - (base_m3 + haunch_m3)).abs() < 1e-9);

    // 型枠（スラブなし）: 基準 (0.8×2+0.4)×5.3 に側面 (Di−D)/2×Li×2 と
    // 底面 (Bi−B)/2×Li を加算。
    let base_m2 = (0.8 * 2.0 + 0.4) * 5.3;
    let haunch_m2 =
        ((1_000.0 - 800.0) / 2.0 * 1_000.0 * 2.0 + (600.0 - 400.0) / 2.0 * 1_000.0) * 1e-6;
    assert!((g.formwork_m2 - (base_m2 + haunch_m2)).abs() < 1e-9);

    // 付帯情報を持たない基礎梁はハンチなし。
    let fg = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::FoundationGirder)
        .unwrap();
    assert!((fg.formwork_m2 - 1.2 * 5.3).abs() < 1e-9);

    // 基礎梁（ElemId(3)）の両端にハンチ: 長さ 800、せい増分 300（幅増分なし）。
    let mut model = rc_portal_model();
    let haunch = CoreHaunch {
        length: 800.0,
        depth_increase: 300.0,
        width_increase: 0.0,
    };
    model.member_detail_attrs.push(MemberDetailAttr {
        elem: ElemId(3),
        haunch_i: Some(haunch),
        haunch_j: Some(haunch),
        joints: vec![],
    });
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let fg = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::FoundationGirder)
        .unwrap();

    // 体積: 基準 0.4×0.8×5.3 に (400+400)(800+1100)/4×800 を両端分加算。
    let base_m3 = 0.4 * 0.8 * 5.3;
    let haunch_m3 = (400.0 + 400.0) * (800.0 + 1_100.0) / 4.0 * 800.0 * 2.0 * 1e-9;
    assert!((fg.concrete_m3 - (base_m3 + haunch_m3)).abs() < 1e-9);

    // 型枠: 側面 1 面＋底面 (0.8+0.4)×5.3 に側面 (Di−D)/2×Li を両端分加算
    // （幅増分 0 のため底面の加算なし）。
    let base_m2 = 1.2 * 5.3;
    let haunch_m2 = (1_100.0 - 800.0) / 2.0 * 800.0 * 2.0 * 1e-6;
    assert!((fg.formwork_m2 - (base_m2 + haunch_m2)).abs() < 1e-9);
}

fn girder_formwork(model: &Model) -> f64 {
    try_compute_quantity_takeoff(model, &QuantityCfg::default())
        .unwrap()
        .items
        .iter()
        .find(|i| i.elem == Some(ElemId(2)) && i.category == MemberCategory::Girder)
        .map(|i| i.formwork_m2)
        .expect("大梁型枠")
}

/// 床板のない囲まれた床領域は型枠控除なし。取り付く床板ありは取付き線の大梁が n_adj≥1。
#[test]
fn test_formwork_plateless_no_deduction_attached_deducts() {
    use sepika_core::model::{LoadTransfer, RegionAnchor};
    use sepika_core::section_shape::SectionShape;

    let empty = rc_portal_model();
    let form_empty = girder_formwork(&empty);

    let mut plateless = rc_portal_model();
    plateless.slab_thickness = 150.0;
    plateless.nodes.push(node(4, 0.0, 5_000.0, 3_500.0));
    plateless.nodes.push(node(5, 6_000.0, 5_000.0, 3_500.0));
    plateless.floor_regions.push(FloorRegion::new(
        FloorRegionId(0),
        vec![NodeId(2), NodeId(3), NodeId(5), NodeId(4)],
    ));
    let form_plateless = girder_formwork(&plateless);
    assert!(
        (form_plateless - form_empty).abs() < 1e-9,
        "床板のない囲まれは控除なし empty={form_empty} plateless={form_plateless}"
    );

    let mut attached = rc_portal_model();
    let mut slab_sec =
        SectionShape::RcSlab { thickness: 150.0 }.to_section(SectionId(2), "S15".into());
    slab_sec.material = Some(MaterialId(0));
    attached.sections.push(slab_sec);
    attached.slabs.push(Slab {
        id: SlabId(0),
        shape: SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(2), NodeId(3)],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: [-1500.0, -1500.0],
        },
        plate: SlabPlate {
            section: Some(SectionId(2)),
            ..Default::default()
        },
        reference_story: None,
        vertical_eccentricity_mm: None,
        tip_loads: Vec::new(),
    });
    set_test_slab_placements(&mut attached);
    let form_att = girder_formwork(&attached);
    assert!(
        form_att < form_empty - 1e-6,
        "取り付く床板ありは型枠が減る empty={form_empty} att={form_att}"
    );
}

/// 新型 `RcBeamRect`（上 4+2 / 下 3+2）で主筋本数・質量・あばら筋が
/// 段別本数の合計（11 本）どおりに算定される。
#[test]
fn test_new_rc_beam_rebar_uses_layer_counts() {
    let mut model = rc_portal_model();
    model.sections[0] = with_rc_materials(
        SectionShape::RcBeamRect {
            b: 400.0,
            d: 800.0,
            rebar: RcBeamRebar {
                main_dia: 25.0,
                top: vec![4, 2],
                bottom: vec![3, 2],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: 10.0,
                    pitch: 200.0,
                    legs: 2,
                },
            },
        }
        .to_section(SectionId(0), "RCB-400x800".to_string()),
    );

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let girder = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Girder)
        .unwrap();

    // 主筋本数は段別合計 4+2+3+2=11（総本数÷段数の推定は使わない）。
    // 両外端 Lo=5300、L2=35×25=875 → 1 本 7050mm。
    let mains: Vec<_> = girder
        .rebar
        .iter()
        .filter(|r| r.usage == RebarUsage::MainBar)
        .collect();
    assert_eq!(mains.len(), 1);
    assert_eq!(mains[0].dia, Some(25.0));
    assert!((mains[0].total_length_m - 11.0 * 7.05).abs() < 1e-9);
    assert!((mains[0].weight_t - 11.0 * 7.05 * 3.98 / 1_000.0).abs() < 1e-9);

    // あばら筋: 一組 2×400+2×800=2400、本数 5300/200=26.5。
    let stirrup = girder
        .rebar
        .iter()
        .find(|r| r.usage == RebarUsage::Stirrup)
        .unwrap();
    assert_eq!(stirrup.dia, Some(10.0));
    assert!((stirrup.total_length_m - 2.4 * 26.5).abs() < 1e-9);
    assert!((stirrup.weight_t - 2.4 * 26.5 * 0.560 / 1_000.0).abs() < 1e-9);

    // 継手: 11 本 × (0.5 + 0.5×floor(5.3/5)) = 11 × 1.0。
    assert!((girder.rebar_joints - 11.0).abs() < 1e-9);
}

/// 新型 `RcColumnRect`（x:[3] / y:[3]）で、主筋は固有本数 8 本、
/// 帯筋は X/Y 脚数（各 2）で算定される。
#[test]
fn test_new_rc_rect_column_rebar_uses_layer_counts() {
    let mut model = rc_portal_model();
    model.sections[1] = with_rc_materials(
        SectionShape::RcColumnRect {
            b: 700.0,
            d: 700.0,
            rebar: RcRectColumnRebar {
                main_dia: 25.0,
                x: vec![3],
                y: vec![3],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 13.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        }
        .to_section(SectionId(1), "RCC-700x700".to_string()),
    );

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let col = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Column)
        .unwrap();

    // 固有本数 = 2·Σx + 2·Σy − 4·nx·ny = 2×3 + 2×3 − 4×1×1 = 8 本。柱高 3.5m。
    let main_len: f64 = col
        .rebar
        .iter()
        .filter(|r| r.usage == RebarUsage::MainBar)
        .map(|r| r.total_length_m)
        .sum();
    assert!((main_len - 8.0 * 3.5).abs() < 1e-9);

    // 帯筋: 一組 legs_x×700 + legs_y×700 = 2800、本数 3500/100=35 → 98m。
    let hoop = col
        .rebar
        .iter()
        .find(|r| r.usage == RebarUsage::Hoop)
        .unwrap();
    assert!((hoop.total_length_m - 2.8 * 35.0).abs() < 1e-9);

    // 継手: 8 本 × 1 個所。
    assert!((col.rebar_joints - 8.0).abs() < 1e-9);
}

/// 新型 `RcColumnCircle`（count=8）で、主筋は総本数 8 本、
/// 帯筋は 1 リング 1 周分（πd）として算定される。
#[test]
fn test_new_rc_circle_column_rebar_uses_total_count() {
    let mut model = rc_portal_model();
    model.sections[1] = with_rc_materials(
        SectionShape::RcColumnCircle {
            d: 700.0,
            rebar: RcCircleColumnRebar {
                main_dia: 25.0,
                count: 8,
                cover: 40.0,
                hoop: CircleColumnHoop {
                    dia: 13.0,
                    pitch: 100.0,
                },
            },
        }
        .to_section(SectionId(1), "RD-700".to_string()),
    );

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let col = q
        .items
        .iter()
        .find(|i| i.category == MemberCategory::Column)
        .unwrap();

    // 主筋: 総本数 8 × 柱高 3.5m。
    let main_len: f64 = col
        .rebar
        .iter()
        .filter(|r| r.usage == RebarUsage::MainBar)
        .map(|r| r.total_length_m)
        .sum();
    assert!((main_len - 8.0 * 3.5).abs() < 1e-9);

    // 帯筋: 1 リング 1 周分 = π×700、本数 3500/100=35。
    let hoop = col
        .rebar
        .iter()
        .find(|r| r.usage == RebarUsage::Hoop)
        .unwrap();
    let expected = std::f64::consts::PI * 700.0 * 35.0 / 1_000.0;
    assert!((hoop.total_length_m - expected).abs() < 1e-9);
    assert!((hoop.weight_t - expected * 0.995 / 1_000.0).abs() < 1e-9);

    // 継手: 8 本 × 1 個所。
    assert!((col.rebar_joints - 8.0).abs() < 1e-9);
}

/// 配筋未入力（`is_unset`）の新型断面は数量 0 とし、panic しない。
#[test]
fn test_new_rc_rebar_unset_yields_zero_without_panic() {
    let mut model = rc_portal_model();
    model.sections[0] = with_rc_materials(
        SectionShape::RcBeamRect {
            b: 400.0,
            d: 800.0,
            rebar: RcBeamRebar {
                main_dia: 25.0,
                top: vec![],
                bottom: vec![],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: 10.0,
                    pitch: 200.0,
                    legs: 2,
                },
            },
        }
        .to_section(SectionId(0), "RCB".to_string()),
    );
    model.sections[1] = with_rc_materials(
        SectionShape::RcColumnRect {
            b: 700.0,
            d: 700.0,
            rebar: RcRectColumnRebar {
                main_dia: 25.0,
                x: vec![],
                y: vec![],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 13.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        }
        .to_section(SectionId(1), "RCC".to_string()),
    );

    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert!(
        q.items.iter().all(|i| i.rebar_weight_t() == 0.0),
        "未入力の新型は鉄筋重量 0"
    );
    assert!(q.items.iter().all(|i| i.rebar_joints == 0.0));
    // コンクリート・型枠は形状から算定される（未入力でも panic しない）。
    assert!(q.items.iter().any(|i| i.concrete_m3 > 0.0));

    // 円形柱 count=0 も同様に 0。
    let mut model = rc_portal_model();
    model.sections[1] = with_rc_materials(
        SectionShape::RcColumnCircle {
            d: 700.0,
            rebar: RcCircleColumnRebar {
                main_dia: 25.0,
                count: 0,
                cover: 40.0,
                hoop: CircleColumnHoop {
                    dia: 13.0,
                    pitch: 100.0,
                },
            },
        }
        .to_section(SectionId(1), "RD".to_string()),
    );
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert!(q
        .items
        .iter()
        .filter(|i| i.category == MemberCategory::Column)
        .all(|i| i.rebar_weight_t() == 0.0));
}

fn circular_rc_post_model(end_mm: [f64; 3]) -> Model {
    use sepika_core::ids::{SecondaryMemberId, StoryId};
    use sepika_core::model::{SecondaryMemberEnds, Story};
    let section = with_rc_materials(
        SectionShape::RcColumnCircle {
            d: 400.0,
            rebar: RcCircleColumnRebar {
                main_dia: 25.0,
                count: 0,
                cover: 40.0,
                hoop: CircleColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                },
            },
        }
        .to_section(SectionId(0), "円形断面".into()),
    );
    let mut lower = node(0, 0.0, 0.0, 0.0);
    lower.story = Some(StoryId(0));
    Model {
        nodes: vec![lower, node(1, end_mm[0], end_mm[1], end_mm[2])],
        sections: vec![section],
        materials: vec![rc_material(0), rebar_material(1)],
        unassigned_posts: vec![SecondaryMember {
            id: SecondaryMemberId(448),
            kind: SecondaryMemberKind::Post,
            ends: SecondaryMemberEnds::Detached([[0.0; 3], end_mm]),
            section: Some(SectionId(0)),
            name: "間柱符号".into(),
            vertical_eccentricity: Some(0.0),
            gravity_end_shares: None,
        }],
        stories: vec![Story {
            wall_weights: Vec::new(),
            id: StoryId(0),
            name: "1F".into(),
            elevation: Some(0.0),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(0)],
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        }],
        ..Default::default()
    }
}

fn assert_circular_post_quantity(q: &QuantityTakeoff) {
    assert_eq!(q.items.len(), 1);
    let item = &q.items[0];
    assert!((item.concrete_m3 - 0.3769911184).abs() < 1e-9);
    assert!((item.formwork_m2 - 3.7699111843).abs() < 1e-9);
    assert_eq!(item.category, MemberCategory::Column);
    assert_eq!(item.structure, StructureKind::Rc);
    assert_eq!(item.story, "1F");
    assert!(item.rebar.is_empty());
    assert!(item.steel.is_none());
    let totals = q.totals();
    assert!((totals.concrete_m3 - 0.3769911184).abs() < 1e-9);
    assert!((totals.formwork_m2 - 3.7699111843).abs() < 1e-9);
    assert_eq!(q.totals_by_story()[0].0, "1F");
    assert_eq!(q.totals_by_category()[0].0, MemberCategory::Column);
    assert_eq!(q.totals_by_story()[0].1.concrete_m3, totals.concrete_m3);
    assert_eq!(q.totals_by_category()[0].1.formwork_m2, totals.formwork_m2);
}

#[test]
fn circular_rc_post_common_entry_uses_diameter_and_actual_length() {
    for end_mm in [[0.0, 0.0, 3000.0], [1800.0, 0.0, 2400.0]] {
        let mut model = circular_rc_post_model(end_mm);
        model.sections[0].width = 900.0;
        model.sections[0].depth = 700.0;
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert_circular_post_quantity(&q);
        assert_eq!(q.items[0].label, "間柱符号");
        assert_eq!(q.items[0].elem, None);
        assert_eq!(q.items[0].slab, None);
        model.unassigned_posts[0].name.clear();
        assert_eq!(
            try_compute_quantity_takeoff(&model, &QuantityCfg::default())
                .unwrap()
                .items[0]
                .label,
            "円形断面"
        );
        model.unassigned_posts.clear();
        model.elements.push(line_elem(0, 0, 1, 0));
        let primary = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert_circular_post_quantity(&primary);
        assert_eq!(q.items[0].concrete_m3, primary.items[0].concrete_m3);
        assert_eq!(q.items[0].formwork_m2, primary.items[0].formwork_m2);
    }
}

#[test]
fn circular_rc_post_materialized_is_counted_once_in_both_directions() {
    for (start, end) in [(0, 1), (1, 0)] {
        let mut model = circular_rc_post_model([0.0, 0.0, 3000.0]);
        model.elements.push(line_elem(0, start, end, 0));
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert_circular_post_quantity(&q);
        assert_eq!(q.items[0].elem, Some(ElemId(0)));
    }
}

#[test]
fn circular_rc_post_invalid_geometry_reports_id_and_cause() {
    for diameter_mm in [0.0, -400.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut model = circular_rc_post_model([0.0, 0.0, 3000.0]);
        if let Some(SectionShape::RcColumnCircle { d, .. }) = &mut model.sections[0].shape {
            *d = diameter_mm;
        }
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(error.contains("SecondaryMemberId(448)"), "{error}");
        assert!(error.contains("直径 D"), "{error}");
    }
    for end_mm in [
        [0.0; 3],
        [f64::NAN, 0.0, 3000.0],
        [0.0, 0.0, f64::INFINITY],
        [0.0, 0.0, f64::MAX],
    ] {
        let model = circular_rc_post_model(end_mm);
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(error.contains("SecondaryMemberId(448)"), "{error}");
        assert!(error.contains("実長 L"), "{error}");
    }
}

#[test]
fn secondary_rectangular_and_steel_paths_are_preserved() {
    let mut model = circular_rc_post_model([0.0, 0.0, 3000.0]);
    let SectionShape::RcColumnRect { rebar, .. } = rc_column_section(0).shape.unwrap() else {
        unreachable!()
    };
    for shape in [
        SectionShape::RcColumnRect {
            b: 400.0,
            d: 400.0,
            rebar: rebar.clone(),
        },
        SectionShape::SrcColumnRect {
            b: 400.0,
            d: 400.0,
            rebar,
            steel_height: 200.0,
            steel_width: 100.0,
            steel_web_thick: 8.0,
            steel_flange_thick: 10.0,
        },
    ] {
        model.sections[0] = with_rc_materials(shape.to_section(SectionId(0), "矩形".into()));
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert!((q.items[0].concrete_m3 - 0.48).abs() < 1e-9);
        assert!((q.items[0].formwork_m2 - 4.8).abs() < 1e-9);
    }
    model.unassigned_beams = std::mem::take(&mut model.unassigned_posts);
    model.unassigned_beams[0].kind = SecondaryMemberKind::Beam;
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert_eq!(q.items[0].category, MemberCategory::Beam);
    assert!((q.items[0].concrete_m3 - 0.48).abs() < 1e-9);
    assert!((q.items[0].formwork_m2 - 3.6).abs() < 1e-9);
    model.elements.push(line_elem(0, 0, 1, 0));
    assert_eq!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap()
            .items
            .len(),
        1
    );
    model.elements.clear();
    for kind in [SecondaryMemberKind::Beam, SecondaryMemberKind::Post] {
        if kind == SecondaryMemberKind::Post {
            model.unassigned_posts = std::mem::take(&mut model.unassigned_beams);
            model.unassigned_posts[0].kind = kind;
        }
        let shape = SectionShape::SteelH {
            root_r: Some(0.0),
            height: 400.0,
            width: 200.0,
            web_thick: 8.0,
            flange_thick: 13.0,
        };
        let mut section = shape.to_section(SectionId(0), "鋼材".into());
        section.material = Some(MaterialId(0));
        model.sections[0] = section;
        model.materials[0] = steel_material(0);
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert_eq!(q.items[0].structure, StructureKind::S);
        assert_eq!(q.items[0].concrete_m3, 0.0);
        assert_eq!(q.items[0].formwork_m2, 0.0);
        assert!((q.items[0].steel.as_ref().unwrap().length_m - 3.0).abs() < 1e-9);
        assert!((q.items[0].steel.as_ref().unwrap().weight_t - 0.1929216).abs() < 1e-9);
    }
}

#[test]
fn circular_rc_post_without_endpoint_nodes_keeps_quantity_and_reports_unknown_story() {
    let mut model = circular_rc_post_model([0.0, 0.0, 3000.0]);
    model.nodes.clear();
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert!((q.items[0].concrete_m3 - 0.376991118430775).abs() < 1e-12);
    assert!((q.items[0].formwork_m2 - 3.76991118430775).abs() < 1e-12);
    assert_eq!(q.items[0].secondary, Some(SecondaryMemberId(448)));
    assert_eq!(q.items[0].story, "-");
    assert!(q
        .notes
        .iter()
        .any(|n| n.contains("SecondaryMemberId(448)") && n.contains("階帰属未確定")));
    assert_eq!(q.totals_by_story()[0].0, "-");
}

fn contact_model() -> Model {
    let mut section = with_rc_materials(
        SectionShape::RcBeamRect {
            b: 300.0,
            d: 600.0,
            rebar: rc_beam_rebar(),
        }
        .to_section(SectionId(0), "G451".into()),
    );
    section.frame_use = Some(FrameSectionUse::Girder);
    Model {
        nodes: vec![node(0, 0.0, 0.0, 3000.0), node(1, 6000.0, 0.0, 3000.0)],
        elements: vec![ElementData {
            vertical_eccentricity: Some(-300.0),
            ..line_elem(0, 0, 1, 0)
        }],
        sections: vec![section],
        materials: vec![rc_material(0), rebar_material(1)],
        ..Default::default()
    }
}

fn contact_slab(model: &mut Model, span: [f64; 2], side: f64, t: f64, top: f64) {
    let sec = SectionId(model.sections.len() as u32);
    let mut section = SectionShape::RcSlab { thickness: t }.to_section(sec, "床".into());
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    let n = model.nodes.len() as u32;
    for (i, (x, y)) in [
        (span[0], 0.0),
        (span[1], 0.0),
        (span[1], side * 2000.0),
        (span[0], side * 2000.0),
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(node(n + i as u32, x, y, top));
    }
    model.add_enclosed_slab_from_nodes(
        &[NodeId(n), NodeId(n + 1), NodeId(n + 2), NodeId(n + 3)],
        SlabPlate {
            section: Some(sec),
            ..Default::default()
        },
    );
    set_test_slab_placements(model);
}

fn contact_secondary_ends(model: &mut Model) -> sepika_core::model::SecondaryMemberEnds {
    use sepika_core::model::{SecondaryMemberAnchor, SecondaryMemberEnds, SupportMemberId};
    let n = model.nodes.len() as u32;
    model.nodes.push(node(n, -1500.0, 0.0, 3000.0));
    model.nodes.push(node(n + 1, 7500.0, 0.0, 3000.0));
    let id = model.elements.len() as u32;
    let mut support = line_elem(id, n, n + 1, 0);
    support.section = None;
    model.elements.push(support);
    SecondaryMemberEnds::Supported(
        [1.0 / 6.0, 5.0 / 6.0].map(|position| SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(id)),
            position,
        }),
    )
}

fn contact_formwork(model: &Model) -> f64 {
    try_compute_quantity_takeoff(model, &QuantityCfg::default())
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.elem == Some(ElemId(0)))
        .unwrap()
        .formwork_m2
}

#[test]
fn beam_contact_union_independent_rectangles_and_invariances() {
    for (slabs, expected) in [
        (vec![], 9.0),
        (vec![(1.0, 150.0, 6000.0), (-1.0, 100.0, 6000.0)], 7.5),
        (vec![(1.0, 150.0, 6000.0)], 8.1),
        (vec![(1.0, 100.0, 6000.0), (1.0, 150.0, 6000.0)], 8.1),
        (vec![(1.0, 150.0, 3000.0)], 8.55),
        (vec![(1.0, 150.0, 3000.0), (1.0, 100.0, 3000.0)], 8.55),
    ] {
        for reflected in [1.0, -1.0] {
            let mut model = contact_model();
            for &(side, t, end) in &slabs {
                contact_slab(&mut model, [0.0, end], side * reflected, t, 3000.0);
            }
            assert!((contact_formwork(&model) - expected).abs() <= 1e-9);
            model.slabs.reverse();
            model.floor_assignment_regions.regions.reverse();
            assert!((contact_formwork(&model) - expected).abs() <= 1e-9);
            model.elements[0].nodes.reverse();
            assert!((contact_formwork(&model) - expected).abs() <= 1e-9);
            model.slab_thickness = 0.0;
            assert!((contact_formwork(&model) - expected).abs() <= 1e-9);
        }
    }
}

#[test]
fn beam_contact_height_intersection_opening_gap_and_clear_span() {
    for (top, t, expected) in [
        (3100.0, 150.0, 8.7),
        (2400.0, 150.0, 9.0),
        (2300.0, 150.0, 9.0),
        (3500.0, 150.0, 9.0),
        (3000.0, 900.0, 5.4),
    ] {
        let mut model = contact_model();
        contact_slab(&mut model, [0.0, 6000.0], 1.0, t, top);
        assert!(
            (contact_formwork(&model) - expected).abs() <= 1e-9,
            "top={top}"
        );
    }
    let mut model = contact_model();
    contact_slab(&mut model, [0.0, 2000.0], 1.0, 150.0, 3000.0);
    contact_slab(&mut model, [4000.0, 6000.0], 1.0, 150.0, 3000.0);
    assert!((contact_formwork(&model) - 8.4).abs() <= 1e-9);
    let mut model = contact_model();
    contact_slab(&mut model, [0.0, 3000.0], 1.0, 150.0, 3000.0);
    model.elements[0].rigid_zone.face_i = Some(500.0);
    model.elements[0].rigid_zone.face_j = Some(1000.0);
    assert!((contact_formwork(&model) - 6.375).abs() <= 1e-9);
    model.elements[0].nodes.reverse();
    model.elements[0].rigid_zone.face_i = Some(1000.0);
    model.elements[0].rigid_zone.face_j = Some(500.0);
    assert!((contact_formwork(&model) - 6.375).abs() <= 1e-9);
}

#[test]
fn beam_contact_rejects_unknown_invalid_and_unsupported_inputs() {
    use sepika_core::model::PlateAssignment;
    let mut base = contact_model();
    contact_slab(&mut base, [0.0, 3000.0], 1.0, 150.0, 3000.0);
    for t in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        let mut model = base.clone();
        model.sections[1].thickness = Some(t);
        let reason = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(reason.contains("実板厚"), "{reason}");
    }
    for assignment in [PlateAssignment::Unset, PlateAssignment::Plate(SlabId(999))] {
        let mut model = base.clone();
        model.floor_assignment_regions.regions[0].assignment = assignment;
        assert!(try_compute_quantity_takeoff(&model, &QuantityCfg::default()).is_err());
    }
    let mut model = base.clone();
    model.slabs.clear();
    model.floor_assignment_regions.regions[0].assignment = PlateAssignment::NoPlate;
    assert!((contact_formwork(&model) - 9.0).abs() <= 1e-9);
    let mut model = base.clone();
    model.slabs[0].plate.section = Some(SectionId(999));
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("断面")
    );
    let mut model = base.clone();
    model.floor_assignment_regions.regions[0].boundary[0].support =
        sepika_core::model::SupportMemberId::Primary(ElemId(999));
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("支持")
    );
    let mut model = base.clone();
    model.nodes[1].coord[2] += 100.0;
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("傾斜梁")
    );
    let mut model = base.clone();
    model.sections[0].shape = None;
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("実断面形状が未指定")
    );
    let mut model = contact_model();
    let sec = SectionId(1);
    model
        .sections
        .push(SectionShape::RcSlab { thickness: 150.0 }.to_section(sec, "床".into()));
    for (i, (x, y)) in [(0.0, 0.0), (3000.0, 2000.0), (0.0, 2000.0), (3000.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        model.nodes.push(node(i as u32 + 2, x, y, 3000.0));
    }
    model.add_enclosed_slab_from_nodes(
        &[NodeId(2), NodeId(3), NodeId(4), NodeId(5)],
        SlabPlate {
            section: Some(sec),
            ..Default::default()
        },
    );
    set_test_slab_placements(&mut model);
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("自己交差")
    );
}

#[test]
fn beam_contact_direct_support_span_secondary_and_vertical_union() {
    use sepika_core::ids::SecondaryMemberId;
    use sepika_core::model::SupportMemberId;
    let mut model = contact_model();
    contact_slab(&mut model, [0.0, 3000.0], 1.0, 150.0, 3000.0);
    model.floor_assignment_regions.regions[0].boundary[0].support =
        SupportMemberId::Primary(ElemId(0));
    model.floor_assignment_regions.regions[0].boundary[0].span = [0.0, 0.5];
    assert!((contact_formwork(&model) - 8.55).abs() <= 1e-9);
    model.elements[0].nodes.reverse();
    model.floor_assignment_regions.regions[0].boundary[0].span = [1.0, 0.5];
    assert!((contact_formwork(&model) - 8.55).abs() <= 1e-9);
    let mut model = contact_model();
    contact_slab(&mut model, [0.0, 6000.0], 1.0, 100.0, 3000.0);
    contact_slab(&mut model, [0.0, 6000.0], 1.0, 100.0, 2700.0);
    assert!((contact_formwork(&model) - 7.8).abs() <= 1e-9);
    let mut model = contact_model();
    model.elements.clear();
    let secondary_ends = contact_secondary_ends(&mut model);
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(451),
        kind: SecondaryMemberKind::Beam,
        ends: secondary_ends,
        section: Some(SectionId(0)),
        name: "二次小梁451".into(),
        vertical_eccentricity: Some(-300.0),
        gravity_end_shares: None,
    });
    contact_slab(&mut model, [0.0, 3000.0], 1.0, 150.0, 3000.0);
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let beam = q.items.iter().find(|i| i.label == "二次小梁451").unwrap();
    assert!((beam.formwork_m2 - 8.55).abs() <= 1e-9);
    let mut foundation = rc_portal_model();
    contact_slab(&mut foundation, [0.0, 6000.0], 1.0, 150.0, 0.0);
    assert!(
        try_compute_quantity_takeoff(&foundation, &QuantityCfg::default())
            .unwrap_err()
            .contains("基礎梁")
    );
    let mut model = contact_model();
    contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3000.0);
    let mut detail = sepika_core::model::MemberDetailAttr::new(ElemId(0));
    detail.haunch_i = Some(sepika_core::model::Haunch {
        length: 1000.0,
        depth_increase: 100.0,
        width_increase: 0.0,
    });
    model.member_detail_attrs.push(detail);
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("ハンチ")
    );
}

#[test]
fn beam_contact_nonconstant_height_within_tolerance_is_unavailable() {
    for difference in [0.5, 1.0] {
        let mut model = contact_model();
        model.nodes[1].coord[2] += difference;
        contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3100.0);
        for _ in 0..2 {
            let reason = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
            assert!(reason.contains("高さが一定でない梁"), "{reason}");
            model.elements[0].nodes.reverse();
        }
        for node in &mut model.nodes[2..] {
            node.coord[2] = 4000.0;
        }
        for slab in &mut model.slabs {
            slab.vertical_eccentricity_mm = Some(4000.);
        }
        let first = contact_formwork(&model);
        let expected = if difference == 0.5 {
            9.00000003125
        } else {
            9.000000125
        };
        assert!((first - expected).abs() <= 1e-9);
        model.elements[0].nodes.reverse();
        assert!((contact_formwork(&model) - first).abs() <= 1e-9);
    }
    let mut model = contact_model();
    contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3000.0);
    model.nodes[4].coord[2] += 0.5;
    assert!((contact_formwork(&model) - 8.1).abs() < 1e-9);
}

#[test]
fn beam_contact_narrow_one_sided_slab_does_not_create_opposite_contact() {
    for side in [-1.0, 1.0] {
        for (spans, expected) in [
            (vec![[0.0, 6000.0]], 8.1),
            (vec![[0.0, 6000.0], [0.0, 6000.0]], 8.1),
            (vec![[0.0, 3000.0], [0.0, 3000.0]], 8.55),
        ] {
            let mut model = contact_model();
            for span in spans {
                let n = model.nodes.len();
                let t = if model.slabs.is_empty() { 150.0 } else { 100.0 };
                contact_slab(&mut model, span, side, t, 3000.0);
                model.nodes[n + 2].coord[1] = side * 5.0;
                model.nodes[n + 3].coord[1] = side * 5.0;
            }
            for _ in 0..2 {
                assert!((contact_formwork(&model) - expected).abs() <= 1e-9);
                model.elements[0].nodes.reverse();
            }
        }
        for (offset, expected) in [(9.0, 8.1), (11.0, 9.0)] {
            let mut model = contact_model();
            contact_slab(&mut model, [0.0, 6000.0], side, 150.0, 3000.0);
            for (i, node) in model.nodes[2..].iter_mut().enumerate() {
                node.coord[1] = side * (offset + if i < 2 { 0.0 } else { 5.0 });
            }
            for _ in 0..2 {
                assert!((contact_formwork(&model) - expected).abs() <= 1e-9);
                model.elements[0].nodes.reverse();
            }
        }
    }
}

#[test]
fn beam_contact_nonconvex_slab_side_occupancy_is_local_to_contact_interval() {
    for (coords, expected) in [
        (
            vec![
                (0.0, 0.0),
                (3000.0, 0.0),
                (3000.0, -5.0),
                (6000.0, -5.0),
                (6000.0, 5.0),
                (0.0, 5.0),
            ],
            7.65,
        ),
        (
            vec![
                (0.0, 0.0),
                (6000.0, 0.0),
                (6000.0, -2000.0),
                (0.0, -2000.0),
                (0.0, -2005.0),
                (6005.0, -2005.0),
                (6005.0, 5.0),
                (0.0, 5.0),
            ],
            8.1,
        ),
    ] {
        let mut model = contact_model();
        model
            .sections
            .push(SectionShape::RcSlab { thickness: 150.0 }.to_section(SectionId(1), "床".into()));
        let mut boundary = Vec::new();
        for (x, y) in coords {
            let id = model.nodes.len() as u32;
            model.nodes.push(node(id, x, y, 3000.0));
            boundary.push(NodeId(id));
        }
        model.add_enclosed_slab_from_nodes(
            &boundary,
            SlabPlate {
                section: Some(SectionId(1)),
                ..Default::default()
            },
        );
        set_test_slab_placements(&mut model);
        for _ in 0..2 {
            assert!((contact_formwork(&model) - expected).abs() <= 1e-9);
            model.elements[0].nodes.reverse();
        }
    }
}

#[test]
fn beam_contact_three_node_primary_is_unavailable_without_negative_quantity() {
    use sepika_core::model::{LoadTransfer, RegionAnchor};
    let mut model = contact_model();
    model.nodes.push(node(2, 3000.0, 0.0, 3000.0));
    model.elements[0].nodes = [NodeId(0), NodeId(2), NodeId(1)].into_iter().collect();
    let mut boundary = line_elem(1, 0, 1, 0);
    boundary.section = None;
    model.elements.push(boundary);
    model
        .sections
        .push(SectionShape::RcSlab { thickness: 600.0 }.to_section(SectionId(1), "床".into()));
    for side in [-1.0, 1.0] {
        model.slabs.push(Slab {
            id: SlabId(model.slabs.len() as u32),
            shape: SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span: [0.0, 1.0],
                    transfer: LoadTransfer::Anchor,
                },
                extent: [side * 2000.0; 2],
            },
            plate: SlabPlate {
                section: Some(SectionId(1)),
                ..Default::default()
            },
            reference_story: None,
            vertical_eccentricity_mm: None,
            tip_loads: vec![],
        });
    }
    set_test_slab_placements(&mut model);
    assert!(model.validate().is_err());
    for _ in 0..2 {
        let reason = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(
            reason.contains("ElemId(0)") && reason.contains("実節点数=3"),
            "{reason}"
        );
        model.elements[0].nodes.reverse();
    }
    model.elements[0].nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    assert!((contact_formwork(&model) - 1.8).abs() <= 1e-9);
    model.slabs.clear();
    model.elements[0].nodes = [NodeId(0), NodeId(2), NodeId(1)].into_iter().collect();
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("実節点数=3")
    );
}

#[test]
fn beam_contact_unknown_beam_section_is_unavailable() {
    for support_secondary in [false, true] {
        let mut model = contact_model();
        contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3000.0);
        if support_secondary {
            model
                .unassigned_beams
                .push(sepika_core::model::SecondaryMember {
                    id: sepika_core::ids::SecondaryMemberId(451),
                    kind: sepika_core::model::SecondaryMemberKind::Beam,
                    ends: sepika_core::model::SecondaryMemberEnds::Detached([
                        [0.0, 0.0, 3000.0],
                        [6000.0, 0.0, 3000.0],
                    ]),
                    section: Some(SectionId(999)),
                    name: "断面不明451".into(),
                    vertical_eccentricity: Some(-300.0),
                    gravity_end_shares: None,
                });
        } else {
            model.elements[0].section = Some(SectionId(999));
        }
        let reason = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(
            reason.contains(if support_secondary {
                "SecondaryMemberId(451)"
            } else {
                "Primary(ElemId(0))"
            }),
            "{reason}"
        );
        assert!(
            reason.contains("SectionId(999)") && reason.contains("参照が不明"),
            "{reason}"
        );
    }
}

#[test]
fn beam_contact_sloped_beam_contact_is_direction_independent() {
    for span in [[0.0, 6000.0], [5700.0, 6000.0], [0.0, 3000.0]] {
        let mut model = contact_model();
        model.nodes[0].coord[2] = 0.0;
        contact_slab(&mut model, span, 1.0, 150.0, 3000.0);
        for _ in 0..2 {
            let result = try_compute_quantity_takeoff(&model, &QuantityCfg::default());
            if span[1] == 6000.0 {
                assert!(result.unwrap_err().contains("傾斜梁"));
            } else {
                let q = result.unwrap();
                // 非接触区間の床は控除せず、既存の梁長sqrt(6000²+3000²)を保持する。
                assert!((q.items[0].formwork_m2 - 10.062305898749054).abs() <= 1e-9);
            }
            model.elements[0].nodes.reverse();
        }
    }
    let mut model = contact_model();
    model.nodes[0].coord[2] = 0.0;
    contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3000.0);
    model.elements[0].rigid_zone.face_i = Some(0.0);
    model.elements[0].rigid_zone.face_j = Some(3354.1019662496847);
    for _ in 0..2 {
        assert!((contact_formwork(&model) - 5.031152949374527).abs() <= 1e-9);
        model.elements[0].nodes.reverse();
        let rz = &mut model.elements[0].rigid_zone;
        std::mem::swap(&mut rz.face_i, &mut rz.face_j);
    }
}

#[test]
fn beam_contact_uses_shared_mm_tolerance_and_ignores_unrelated_slope() {
    for (offset, expected) in [(9.0, 8.1), (11.0, 9.0)] {
        let mut model = contact_model();
        contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3000.0);
        for node in &mut model.nodes[2..] {
            node.coord[1] += offset;
        }
        assert!((contact_formwork(&model) - expected).abs() <= 1e-9);
    }
    let mut model = contact_model();
    contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3500.0);
    model.nodes[4].coord[2] += 100.0;
    assert!((contact_formwork(&model) - 9.0).abs() <= 1e-9);
    model.nodes[2].coord[2] = 3000.0;
    model.nodes[3].coord[2] = 3000.0;
    assert!((contact_formwork(&model) - 9.).abs() < 1e-9);
}

#[test]
fn beam_contact_non_nodal_secondary_is_counted_and_materialized_is_counted_once() {
    use sepika_core::ids::SecondaryMemberId;
    let mut model = contact_model();
    let secondary_ends = sepika_core::model::SecondaryMemberEnds::Detached([
        [0.0, 0.0, 3000.0],
        [6000.0, 0.0, 3000.0],
    ]);
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(451),
        kind: SecondaryMemberKind::Beam,
        ends: secondary_ends,
        section: Some(SectionId(0)),
        name: "二次小梁451".into(),
        vertical_eccentricity: Some(-300.0),
        gravity_end_shares: None,
    });
    contact_slab(&mut model, [0.0, 3000.0], 1.0, 150.0, 3000.0);
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert!(!q.items.iter().any(|i| i.label == "二次小梁451"));
    assert!((contact_formwork(&model) - 8.55).abs() <= 1e-9);
    let mut model = contact_model();
    model.elements.clear();
    model.nodes.clear();
    let secondary_ends = contact_secondary_ends(&mut model);
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(451),
        kind: SecondaryMemberKind::Beam,
        ends: secondary_ends,
        section: Some(SectionId(0)),
        name: "二次小梁451".into(),
        vertical_eccentricity: Some(-300.0),
        gravity_end_shares: None,
    });
    contact_slab(&mut model, [1000.0, 3000.0], 1.0, 150.0, 3000.0);
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let beam = q
        .items
        .iter()
        .find(|i| i.secondary == Some(SecondaryMemberId(451)))
        .unwrap();
    assert!((beam.concrete_m3 - 1.08).abs() < 1e-12);
    assert!((beam.formwork_m2 - 8.7).abs() < 1e-12);
}

#[test]
fn beam_contact_uses_shape_dimensions_and_rejects_nonfinite_formwork() {
    let mut base = contact_model();
    contact_slab(&mut base, [0.0, 6000.0], 1.0, 900.0, 3000.0);
    base.sections[0].width = 10000.0;
    base.sections[0].depth = 10000.0;
    assert!((contact_formwork(&base) - 5.4).abs() <= 1e-9);
    for bad in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        for width in [false, true] {
            let mut model = base.clone();
            if let Some(SectionShape::RcBeamRect { b, d, .. }) = &mut model.sections[0].shape {
                if width {
                    *b = bad;
                } else {
                    *d = bad;
                }
            }
            assert!(
                try_compute_quantity_takeoff(&model, &QuantityCfg::default()).is_err(),
                "bad={bad}, width={width}"
            );
        }
    }
    let mut model = base.clone();
    if let Some(SectionShape::RcBeamRect { b, .. }) = &mut model.sections[0].shape {
        *b = f64::MAX;
    }
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("有限範囲")
    );
    for face in [-1.0, f64::NAN, f64::INFINITY] {
        let mut model = base.clone();
        model.elements[0].rigid_zone.face_i = Some(face);
        model.elements[0].rigid_zone.face_j = Some(0.0);
        assert!(
            try_compute_quantity_takeoff(&model, &QuantityCfg::default())
                .unwrap_err()
                .contains("内法区間")
        );
    }
}

#[test]
fn frame_node_counts_reject_direct_quantity_and_normal_two_nodes_have_independent_totals() {
    let base = contact_model();
    let q = try_compute_quantity_takeoff(&base, &QuantityCfg::default()).unwrap();
    assert!((q.items[0].concrete_m3 - 1.08).abs() < 1e-9);
    assert!((q.items[0].formwork_m2 - 9.0).abs() < 1e-9);
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
        ElementKind::Brace {
            tension_only: false,
        },
    ] {
        for nodes in [
            vec![],
            vec![NodeId(0)],
            vec![NodeId(0), NodeId(2), NodeId(1)],
            vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            vec![NodeId(0), NodeId(0)],
        ] {
            let mut model = base.clone();
            model.elements[0].kind = kind;
            model.elements[0].nodes = nodes.into();
            assert_eq!(
                try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err(),
                model.validate_frame_node_counts().unwrap_err().to_string()
            );
        }
    }
}

#[test]
fn three_node_reproduction_is_rejected_with_or_without_floor_and_collinear_boundary() {
    for floor in [false, true] {
        for boundary in [false, true] {
            for reversed in [false, true] {
                let mut model = contact_model();
                model.nodes.push(node(2, 3000.0, 0.0, 3000.0));
                if floor {
                    contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3000.0);
                }
                if boundary {
                    let mut edge = line_elem(model.elements.len() as u32, 0, 1, 0);
                    edge.section = None;
                    model.elements.push(edge);
                }
                model.elements[0].nodes = if reversed {
                    vec![NodeId(1), NodeId(2), NodeId(0)]
                } else {
                    vec![NodeId(0), NodeId(2), NodeId(1)]
                }
                .into();
                let reason = model.validate_frame_node_counts().unwrap_err().to_string();
                assert_eq!(model.validate().unwrap_err().to_string(), reason);
                assert_eq!(
                    try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err(),
                    reason
                );
                assert!(model
                    .support_member_axis(sepika_core::model::SupportMemberId::Primary(ElemId(0)))
                    .is_none());
            }
        }
    }
}

fn directional_wall_quantity_fixture() -> Model {
    let mut model = rc_portal_model();
    model.nodes[1].coord[0] = 4700.0;
    model.nodes[3].coord[0] = 4700.0;
    model.nodes[2].coord[2] = 3800.0;
    model.nodes[3].coord[2] = 3800.0;
    let mut section = SectionShape::RcWall {
        thickness: 200.0,
        ps: 0.002,
        pwh_ratio: Some(0.006),
        pwv_ratio: Some(0.002),
    }
    .to_section(SectionId(2), "W20".into());
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    model.add_enclosed_wall_plate_from_nodes(
        &[NodeId(0), NodeId(1), NodeId(3), NodeId(2)],
        sepika_core::model::WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(2)),
            dl_support: None,
            self_weight_shares: vec![],
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
        },
    );
    model.wall_regions.push(sepika_core::model::WallRegion {
        id: sepika_core::ids::WallRegionId(0),
        name: "壁領域".into(),
        boundary: vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)],
        wall_plate_ids: vec![WallPlateId(0)],
        posts: vec![],
    });
    model
}
fn directional_wall(q: &QuantityTakeoff) -> &MemberQuantity {
    q.items
        .iter()
        .find(|it| it.category == MemberCategory::Wall)
        .unwrap()
}

#[test]
fn directional_wall_quantity_matches_independent_clear_dimension_arithmetic() {
    let mut model = directional_wall_quantity_fixture();
    let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
    let w = directional_wall(&q);
    assert_eq!(w.wall_plate, Some(WallPlateId(0)));
    assert_eq!(w.section, Some(SectionId(2)));
    assert_eq!(w.rebar_status(), QuantityStatus::Complete);
    assert!((w.concrete_m3 - 2.4).abs() < 1e-12);
    for (usage, length_m, weight_t) in [
        (RebarUsage::WallHorizontal, 215.432130969, 0.120641993343),
        (RebarUsage::WallVertical, 75.3757810483, 0.0422104373871),
    ] {
        let r = w.rebar.iter().find(|r| r.usage == usage).unwrap();
        assert!((r.total_length_m - length_m).abs() < 1e-9);
        assert!((r.weight_t - weight_t).abs() < 1e-12);
    }
    if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut model.sections[2].shape {
        *pwh_ratio = Some(0.002);
    }
    let equal = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
    let w = directional_wall(&equal);
    assert!((w.rebar[0].total_length_m - 71.8107103231).abs() < 1e-9);
    assert!((w.rebar[1].total_length_m - 75.3757810483).abs() < 1e-9);
    assert!((w.rebar[0].weight_t - 0.0402139977809).abs() < 1e-12);
}

#[test]
fn missing_wall_directions_keep_known_quantity_and_every_subtotal_reason() {
    let mut model = directional_wall_quantity_fixture();
    for (horizontal, vertical, known, missing) in [
        (Some(0.006), None, 1, 1),
        (None, Some(0.002), 1, 1),
        (None, None, 0, 2),
        (Some(0.0), Some(0.0), 2, 0),
    ] {
        model.sections[2].shape = Some(SectionShape::RcWall {
            thickness: 200.0,
            ps: 0.006,
            pwh_ratio: horizontal,
            pwv_ratio: vertical,
        });
        let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
        let w = directional_wall(&q);
        assert_eq!(w.rebar.len(), known);
        assert_eq!(w.rebar_issues.len(), missing);
        assert!((w.concrete_m3 - 2.4).abs() < 1e-12);
        if missing > 0 {
            for t in std::iter::once(q.totals())
                .chain(
                    q.totals_by_category()
                        .into_iter()
                        .filter(|(k, _)| *k == MemberCategory::Wall)
                        .map(|(_, t)| t),
                )
                .chain(q.totals_by_story().into_iter().map(|(_, t)| t))
                .chain(q.totals_by_structure().into_iter().map(|(_, t)| t))
            {
                assert_eq!(t.rebar_status(), QuantityStatus::Incomplete);
                assert_eq!(t.rebar_issues, w.rebar_issues);
            }
            for i in &w.rebar_issues {
                assert_eq!(i.kind, RebarIssueKind::Missing);
                assert_eq!(i.wall_plate, Some(WallPlateId(0)));
                assert_eq!(i.section, Some(SectionId(2)));
                assert!(i.elem.is_some());
            }
        } else {
            assert_eq!(w.rebar_status(), QuantityStatus::Complete);
            assert_eq!(w.rebar_weight_t(), 0.0);
        }
    }
    assert_eq!(
        QuantityTakeoff::default().totals().rebar_status(),
        QuantityStatus::Empty
    );
}

#[test]
fn invalid_wall_ratios_and_quantity_settings_are_unavailable_instead_of_zero() {
    for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut model = directional_wall_quantity_fixture();
        if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut model.sections[2].shape {
            *pwh_ratio = Some(invalid);
        }
        let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
        let w = directional_wall(&q);
        assert_eq!(w.rebar.len(), 1);
        assert_eq!(w.rebar[0].usage, RebarUsage::WallVertical);
        assert_eq!(w.rebar_issues[0].kind, RebarIssueKind::Invalid);
        assert_eq!(w.rebar_issues[0].usage, RebarUsage::WallHorizontal);
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut model = directional_wall_quantity_fixture();
        model.quantity_cfg.assumed_wall_bar_dia = invalid;
        let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
        let w = directional_wall(&q);
        assert!(w.rebar.is_empty());
        assert_eq!(w.rebar_issues.len(), 2);
        assert!(w.rebar_issues.iter().all(|i| i.reason.contains("仮定径")));
    }
    for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut model = directional_wall_quantity_fixture();
        model.quantity_cfg.anchorage_dia_factor = invalid;
        let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
        let w = directional_wall(&q);
        assert!(w.rebar.is_empty());
        assert_eq!(w.rebar_issues.len(), 2);
        assert!(w.rebar_issues.iter().all(|i| i.reason.contains("定着係数")));
    }
}

#[test]
fn directional_wall_quantity_uses_saved_assumptions_without_requiring_strength_materials() {
    let mut model = directional_wall_quantity_fixture();
    model.materials[0].fc = None;
    model.materials[1].fy = None;
    model.sections[2].rebar_material = None;
    model.sections[2].shear_rebar_material = None;
    model.quantity_cfg.assumed_wall_bar_dia = 13.0;
    model.quantity_cfg.anchorage_dia_factor = 40.0;
    let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
    let w = directional_wall(&q);
    assert_eq!(w.rebar_status(), QuantityStatus::Complete);
    assert!((w.rebar[0].total_length_m - 136.696202956656).abs() < 1e-9);
    assert!((w.rebar[0].weight_t - 0.136012721941872).abs() < 1e-12);
    assert!((w.rebar[1].total_length_m - 48.6995290956692).abs() < 1e-9);
    assert!((w.rebar[1].weight_t - 0.0484560314501908).abs() < 1e-12);
    assert!(q
        .notes
        .iter()
        .any(|n| n.contains("13 mm") && n.contains("定着係数 40")));
}

#[test]
fn invalid_wall_thickness_or_dimensions_keeps_reasons_without_ps_fallback() {
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut model = directional_wall_quantity_fixture();
        if let Some(SectionShape::RcWall { thickness, .. }) = &mut model.sections[2].shape {
            *thickness = invalid;
        }
        let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
        let w = directional_wall(&q);
        assert!(w.rebar.is_empty());
        assert_eq!(w.rebar_issues.len(), 2);
        assert!(w
            .rebar_issues
            .iter()
            .all(|i| i.kind == RebarIssueKind::Invalid && i.reason.contains("壁厚")));
    }
    let mut model = directional_wall_quantity_fixture();
    for node in &mut model.nodes {
        if node.coord[2] > 0.0 {
            node.coord[2] = 800.0;
        }
    }
    let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
    let w = directional_wall(&q);
    assert!(w.rebar.is_empty());
    assert!(w.rebar_issues.iter().all(|i| i.reason.contains("内法寸法")));
    let mut model = directional_wall_quantity_fixture();
    model.quantity_cfg.anchorage_dia_factor = 0.0;
    let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
    let w = directional_wall(&q);
    assert_eq!(w.rebar_status(), QuantityStatus::Complete);
    assert!((w.rebar[0].total_length_m - 183.346494441863).abs() < 1e-9);
}

#[test]
fn unresolved_wall_plate_geometry_is_incomplete_instead_of_disappearing() {
    let mut model = directional_wall_quantity_fixture();
    for node in &mut model.nodes {
        node.coord[2] = 0.0;
    }
    let q = try_compute_quantity_takeoff(&model, &model.quantity_cfg).unwrap();
    let item = q
        .items
        .iter()
        .find(|it| it.wall_plate == Some(WallPlateId(0)))
        .unwrap();
    assert_eq!(item.rebar_status(), QuantityStatus::Incomplete);
    assert_eq!(item.rebar_issues.len(), 2);
    assert!(item
        .rebar_issues
        .iter()
        .all(|i| i.reason.contains("境界") && i.kind == RebarIssueKind::Invalid));
    assert_eq!(q.totals().rebar_status(), QuantityStatus::Incomplete);
}

fn nonnodal_beam_model() -> Model {
    use sepika_core::model::{SecondaryMemberAnchor, SecondaryMemberEnds, SupportMemberId};
    let mut model = circular_rc_post_model([0.0, 0.0, 3000.0]);
    model.unassigned_posts.clear();
    model.nodes = vec![
        node(0, 0.0, 0.0, 3000.0),
        node(1, 3000.0, 0.0, 3000.0),
        node(2, 0.0, 3000.0, 3000.0),
        node(3, 3000.0, 3000.0, 3000.0),
    ];
    for n in &mut model.nodes {
        n.story = Some(StoryId(0));
    }
    model.sections[0] = with_rc_materials(
        SectionShape::RcBeamRect {
            b: 300.0,
            d: 600.0,
            rebar: rc_beam_rebar(),
        }
        .to_section(SectionId(0), "B300x600".into()),
    );
    model.elements = vec![line_elem(0, 0, 1, 0), line_elem(1, 2, 3, 0)];
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(557),
        kind: SecondaryMemberKind::Beam,
        ends: SecondaryMemberEnds::Supported([0, 1].map(|id| SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(id)),
            position: 0.5,
        })),
        section: Some(SectionId(0)),
        name: "小梁557".into(),
        vertical_eccentricity: Some(0.0),
        gravity_end_shares: None,
    });
    model
}

fn secondary_557(q: &QuantityTakeoff) -> &MemberQuantity {
    q.items
        .iter()
        .find(|i| i.secondary == Some(SecondaryMemberId(557)))
        .unwrap()
}

#[test]
fn nonnodal_beam_three_end_modes_and_node_addition_preserve_independent_quantities() {
    use sepika_core::model::SecondaryMemberEnds;
    let base = nonnodal_beam_model();
    let support = base.unassigned_beams[0].ends.anchors()[0];
    for ends in [
        base.unassigned_beams[0].ends,
        SecondaryMemberEnds::Cantilever {
            support,
            free_end_vector: [0.0, 3000.0],
        },
        SecondaryMemberEnds::Detached([[1500.0, 0.0, 3000.0], [1500.0, 3000.0, 3000.0]]),
        SecondaryMemberEnds::Detached([[1500.0, 3000.0, 3000.0], [1500.0, 0.0, 3000.0]]),
    ] {
        let mut model = base.clone();
        model.unassigned_beams[0].ends = ends;
        for node_count in 0..=2 {
            let before = format!("{model:?}");
            let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
            assert_eq!(format!("{model:?}"), before);
            let item = secondary_557(&q);
            assert!((item.concrete_m3 - 0.54).abs() < 1e-12);
            assert!((item.formwork_m2 - 4.5).abs() < 1e-12);
            assert_eq!(item.elem, None);
            assert_eq!(item.label, "小梁557");
            if node_count < 2 {
                let mut added = node(
                    4 + node_count,
                    1500.0,
                    3000.0 * f64::from(node_count),
                    3000.0,
                );
                added.story = Some(StoryId(0));
                model.nodes.push(added);
            }
        }
        model.unassigned_beams[0].name.clear();
        assert_eq!(
            secondary_557(&try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap())
                .label,
            "B300x600"
        );
    }
}

#[test]
fn nonnodal_rectangular_post_and_supplied_steel_area_use_actual_sloping_length() {
    use sepika_core::model::SecondaryMemberEnds;
    for end in [[0.0, 0.0, 3000.0], [1800.0, 0.0, 2400.0]] {
        let mut model = circular_rc_post_model(end);
        model.nodes.clear();
        let SectionShape::RcColumnRect { rebar, .. } = rc_column_section(0).shape.unwrap() else {
            unreachable!()
        };
        model.sections[0] = with_rc_materials(
            SectionShape::RcColumnRect {
                b: 300.0,
                d: 400.0,
                rebar,
            }
            .to_section(SectionId(0), "Post".into()),
        );
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert!((q.items[0].concrete_m3 - 0.36).abs() < 1e-12);
        assert!((q.items[0].formwork_m2 - 4.2).abs() < 1e-12);
    }
    let mut model = nonnodal_beam_model();
    model.sections[0].shape = None;
    model.sections[0].area = 10000.0;
    model.sections[0].property_basis.area = sepika_core::model::PropertyBasis::Supplied;
    model.materials[0] = steel_material(0);
    let support = model.unassigned_beams[0].ends.anchors()[0];
    for ends in [
        model.unassigned_beams[0].ends,
        SecondaryMemberEnds::Cantilever {
            support,
            free_end_vector: [0.0, 3000.0],
        },
        SecondaryMemberEnds::Detached([[1500.0, 0.0, 3000.0], [1500.0, 3000.0, 3000.0]]),
    ] {
        model.unassigned_beams[0].ends = ends;
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        let steel = secondary_557(&q).steel.as_ref().unwrap();
        assert_eq!(steel.length_m, 3.0);
        assert!((steel.weight_t - 0.2355).abs() < 1e-12);
    }
    model.sections[0].material = None;
    let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
    assert!(
        error.contains("SecondaryMemberId(557)") && error.contains("材料"),
        "{error}"
    );
}

#[test]
fn nonnodal_secondary_invalid_references_and_geometry_never_disappear() {
    use sepika_core::model::{SecondaryMemberEnds, SupportMemberId};
    let base = nonnodal_beam_model();
    for section in [None, Some(SectionId(999))] {
        let mut model = base.clone();
        model.unassigned_beams[0].section = section;
        assert!(
            try_compute_quantity_takeoff(&model, &QuantityCfg::default())
                .unwrap_err()
                .contains("SecondaryMemberId(557)")
        );
    }
    let mut model = base.clone();
    model.sections[0].material = Some(MaterialId(999));
    assert!(
        try_compute_quantity_takeoff(&model, &QuantityCfg::default())
            .unwrap_err()
            .contains("SecondaryMemberId(557)")
    );
    for coord in [
        [1500.0, 0.0, 3000.0],
        [f64::NAN, 0.0, 3000.0],
        [f64::INFINITY, 0.0, 3000.0],
    ] {
        let mut model = base.clone();
        model.unassigned_beams[0].ends =
            SecondaryMemberEnds::Detached([[1500.0, 0.0, 3000.0], coord]);
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(
            error.contains("SecondaryMemberId(557)") && error.contains("実長"),
            "{error}"
        );
    }
    for support in [
        SupportMemberId::Primary(ElemId(999)),
        SupportMemberId::Secondary(SecondaryMemberId(557)),
    ] {
        let mut model = base.clone();
        let mut anchors = [model.unassigned_beams[0].ends.anchors()[0]; 2];
        anchors[0].support = support;
        model.unassigned_beams[0].ends = SecondaryMemberEnds::Supported(anchors);
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(
            error.contains("SecondaryMemberId(557)") && error.contains("端点"),
            "{error}"
        );
    }
}

#[test]
fn nonnodal_secondary_story_priority_conflict_and_recursive_support_preserve_quantity() {
    use sepika_core::model::{SecondaryMemberAnchor, SecondaryMemberEnds, SupportMemberId};
    let mut model = nonnodal_beam_model();
    assert_eq!(
        secondary_557(&try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap())
            .story,
        "1F"
    );
    let mut story = model.stories[0].clone();
    story.id = StoryId(1);
    story.name = "2F".into();
    model.stories.push(story);
    let mut near = node(4, 1500.0, 0.0, 3000.0);
    near.story = Some(StoryId(1));
    model.nodes.push(near);
    assert_eq!(
        secondary_557(&try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap())
            .story,
        "2F"
    );
    let mut near = node(5, 1500.0, 0.0, 3000.0);
    near.story = Some(StoryId(0));
    model.nodes.push(near);
    assert_eq!(
        secondary_557(&try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap())
            .story,
        "1F"
    );
    model.nodes[1].story = Some(StoryId(1));
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert_eq!(secondary_557(&q).story, "-");
    assert!((secondary_557(&q).concrete_m3 - 0.54).abs() < 1e-12);
    assert!(q
        .notes
        .iter()
        .any(|n| n.contains("557") && n.contains("階帰属未確定")));
    model.nodes.truncate(4);
    model.nodes[1].story = Some(StoryId(0));
    let parent = model.unassigned_beams[0].clone();
    model.unassigned_beams[0].id = SecondaryMemberId(558);
    model.unassigned_beams.push(SecondaryMember {
        ends: SecondaryMemberEnds::Cantilever {
            support: SecondaryMemberAnchor {
                support: SupportMemberId::Secondary(SecondaryMemberId(558)),
                position: 0.5,
            },
            free_end_vector: [1500.0, 0.0],
        },
        ..parent
    });
    assert_eq!(
        secondary_557(&try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap())
            .story,
        "1F"
    );
}

#[test]
fn nonnodal_secondary_materialization_requires_unique_full_pair_section_use_and_material() {
    let base = nonnodal_beam_model();
    for reverse in [false, true] {
        let mut model = base.clone();
        model.nodes.extend([
            node(4, 1500.0, 0.0, 3000.0),
            node(5, 1500.0, 3000.0, 3000.0),
        ]);
        model.elements.push(if reverse {
            line_elem(2, 5, 4, 0)
        } else {
            line_elem(2, 4, 5, 0)
        });
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert_eq!(q.items.iter().filter(|i| i.secondary.is_some()).count(), 0);
        assert_eq!(q.items.len(), 3);
        model.elements.push(line_elem(3, 4, 5, 0));
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(error.contains("557") && error.contains("複数"), "{error}");
        model.elements.pop();
        let mut other = model.sections[0].clone();
        other.id = SectionId(1);
        other.name = "異断面".into();
        model.sections.push(other);
        model.elements[2].section = Some(SectionId(1));
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(error.contains("557") && error.contains("一致"), "{error}");
        model.elements[2].section = Some(SectionId(0));
        model.sections[0].shape = None;
        model.sections[0].frame_use = Some(FrameSectionUse::Column);
        assert!(
            try_compute_quantity_takeoff(&model, &QuantityCfg::default())
                .unwrap_err()
                .contains("557")
        );
    }
}

#[test]
fn secondary_overflow_is_id_error_before_and_after_materialization() {
    for materialized in [false, true] {
        let mut model = nonnodal_beam_model();
        model.sections[0].shape = None;
        model.sections[0].area = 1e306;
        model.sections[0].property_basis.area = sepika_core::model::PropertyBasis::Supplied;
        model.materials[0] = steel_material(0);
        model.materials[0].density = 0.0;
        if materialized {
            model.nodes.extend([
                node(4, 1500.0, 0.0, 3000.0),
                node(5, 1500.0, 3000.0, 3000.0),
            ]);
            model.elements.push(line_elem(2, 4, 5, 0));
        }
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(
            error.contains("SecondaryMemberId(557)") && error.contains("有限範囲"),
            "{error}"
        );
    }
}

#[test]
fn nonnodal_secondary_stb_member_strength_is_preserved_and_material_mismatch_is_rejected() {
    use sepika_core::model::{StbGradeMaterial, StbMemberStrength, StrengthTarget};
    let mut model = nonnodal_beam_model();
    let mut fc30 = rc_material(2);
    fc30.name = "Fc30".into();
    fc30.fc = Some(30.0);
    model.materials.push(fc30);
    model.stb_strengths.materials = vec![
        StbGradeMaterial {
            grade: "Fc24".into(),
            material: MaterialId(0),
        },
        StbGradeMaterial {
            grade: "Fc30".into(),
            material: MaterialId(2),
        },
    ];
    model.stb_strengths.members.push(StbMemberStrength {
        target: StrengthTarget::Secondary(SecondaryMemberId(557)),
        node: NodeId(0),
        node_order: vec![NodeId(0)],
        concrete: Some("Fc30".into()),
    });
    for nodes in [false, true] {
        if nodes {
            model.nodes.extend([
                node(4, 1500.0, 0.0, 3000.0),
                node(5, 1500.0, 3000.0, 3000.0),
            ]);
        }
        let before = format!("{model:?}");
        assert_eq!(
            model
                .secondary_material(&model.unassigned_beams[0])
                .unwrap()
                .fc,
            Some(30.0)
        );
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert!((secondary_557(&q).concrete_m3 - 0.54).abs() < 1e-12);
        assert!((secondary_557(&q).formwork_m2 - 4.5).abs() < 1e-12);
        assert_eq!(format!("{model:?}"), before);
    }
    model.elements.push(line_elem(2, 4, 5, 0));
    let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
    assert!(
        error.contains("557") && error.contains("解決材料"),
        "{error}"
    );
    model.stb_strengths.members.push(StbMemberStrength {
        target: StrengthTarget::Element(ElemId(2)),
        node: NodeId(0),
        node_order: vec![NodeId(0)],
        concrete: Some("Fc30".into()),
    });
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert_eq!(q.items.len(), 3);
    assert!(q.items.iter().all(|i| i.secondary.is_none()));
}

#[test]
fn materialized_secondary_contact_reversal_uses_same_union_as_primary() {
    for reverse in [false, true] {
        let mut model = contact_model();
        contact_slab(&mut model, [0.0, 3000.0], 1.0, 150.0, 3000.0);
        let before = contact_formwork(&model);
        let mut ends = [[0.0, 0.0, 3000.0], [6000.0, 0.0, 3000.0]];
        if reverse {
            ends.reverse();
        }
        model.unassigned_beams.push(SecondaryMember {
            id: SecondaryMemberId(557),
            kind: SecondaryMemberKind::Beam,
            ends: sepika_core::model::SecondaryMemberEnds::Detached(ends),
            section: Some(SectionId(0)),
            name: "contact557".into(),
            vertical_eccentricity: Some(-300.0),
            gravity_end_shares: None,
        });
        assert!((contact_formwork(&model) - before).abs() < 1e-12);
    }
}

#[test]
fn materialized_secondary_story_uses_unique_support_instead_of_first_nearby_node() {
    let mut model = nonnodal_beam_model();
    model.nodes.extend([
        node(4, 1500.0, 0.0, 3000.0),
        node(5, 1500.0, 3000.0, 3000.0),
    ]);
    model.elements.push(line_elem(2, 4, 5, 0));
    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    assert_eq!(
        q.items
            .iter()
            .find(|i| i.elem == Some(ElemId(2)))
            .unwrap()
            .story,
        "1F"
    );
    let mut story = model.stories[0].clone();
    story.id = StoryId(1);
    story.name = "2F".into();
    model.stories.push(story);
    for order in [[StoryId(0), StoryId(1)], [StoryId(1), StoryId(0)]] {
        let mut model = model.clone();
        model.nodes[4].story = Some(order[0]);
        let mut near = node(6, 1500.0, 0.0, 3000.0);
        near.story = Some(order[1]);
        model.nodes.push(near);
        model.nodes[1].story = Some(StoryId(1));
        let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
        assert_eq!(
            q.items
                .iter()
                .find(|i| i.elem == Some(ElemId(2)))
                .unwrap()
                .story,
            "-"
        );
        assert!(q
            .notes
            .iter()
            .any(|n| n.contains("557") && n.contains("階帰属未確定")));
    }
}

#[test]
fn secondary_invalid_lip_channel_reports_member_id_and_dimension_cause() {
    for thick in [0.0, -2.0, f64::NAN] {
        let mut model = nonnodal_beam_model();
        model.sections[0].shape = Some(SectionShape::SteelLipChannel {
            height: 200.0,
            width: 75.0,
            lip: 20.0,
            thick,
        });
        model.materials[0] = steel_material(0);
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(error.contains("SecondaryMemberId(557)"), "{error}");
        assert!(error.contains("H/B/C/t"), "{error}");
    }
}

#[test]
fn invalid_lip_channel_is_rejected_for_primary_and_unreferenced_sections() {
    for referenced in [false, true] {
        let mut model = nonnodal_beam_model();
        let section_id = if referenced {
            SectionId(0)
        } else {
            SectionId(1)
        };
        if referenced {
            model.unassigned_beams.clear();
        } else {
            let mut section = model.sections[0].clone();
            section.id = section_id;
            section.name = "未参照不正リップ溝形".into();
            model.sections.push(section);
        }
        model.sections[section_id.index()].shape = Some(SectionShape::SteelLipChannel {
            height: 200.0,
            width: 75.0,
            lip: 20.0,
            thick: 0.0,
        });
        let error = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(error.contains(&format!("断面 {}", section_id.0)), "{error}");
        assert!(error.contains("H/B/C/t"), "{error}");
    }
}

#[test]
fn automatic_wall_quantity_requires_the_immediate_floor_input() {
    use sepika_core::ids::{StoryId, WallPlateId};
    use sepika_core::model::{RegionAnchor, Story, WallPlate, WallPlateShape};
    let mut model = rc_portal_model();
    model.stories = [0.0, 3000.0, 6000.0]
        .into_iter()
        .enumerate()
        .map(|(i, fl)| Story::new(StoryId(i as u32), format!("{i}F"), Some(fl), Some(0.0)))
        .collect();
    model.sections.push(
        SectionShape::RcWall {
            thickness: 150.0,
            pwh_ratio: None,
            pwv_ratio: None,
            ps: 0.0,
        }
        .to_section(SectionId(2), "W15".into()),
    );
    model.wall_plates.push(WallPlate {
        id: WallPlateId(0),
        shape: WallPlateShape::Attached {
            anchor: RegionAnchor::FloorRegion {
                nodes: [NodeId(0), NodeId(1)],
            },
            extent: None,
        },
        section: Some(SectionId(2)),
        opening_area: 0.0,
        opening_weight: 0.0,
        openings: vec![],
        loads: vec![],
        slit: Default::default(),
        self_weight_shares: vec![],
        dl_support: None,
    });
    for field in ["FL", "ΔSL"] {
        let mut incomplete = model.clone();
        if field == "FL" {
            incomplete.stories[1].elevation = None;
        } else {
            incomplete.stories[1].sl_offset_mm = None;
        }
        let before = incomplete.clone();
        let error = try_compute_quantity_takeoff(&incomplete, &QuantityCfg::default()).unwrap_err();
        assert!(
            error.contains("壁版 0") && error.contains("階 1") && error.contains(field),
            "{error}"
        );
        assert!(incomplete.eq_ignoring_dofmap(&before));
        if let WallPlateShape::Attached { extent, .. } = &mut incomplete.wall_plates[0].shape {
            *extent = Some([1000.0; 2]);
        }
        let quantities =
            try_compute_quantity_takeoff(&incomplete, &QuantityCfg::default()).unwrap();
        let wall = quantities
            .items
            .iter()
            .find(|item| item.wall_plate == Some(WallPlateId(0)))
            .unwrap();
        assert!((wall.concrete_m3 - 0.9).abs() < 1e-12);
        assert!((wall.formwork_m2 - 12.0).abs() < 1e-12);
        incomplete.wall_plates.clear();
        assert!(try_compute_quantity_takeoff(&incomplete, &QuantityCfg::default()).is_ok());
    }
    let quantities = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
    let wall = quantities
        .items
        .iter()
        .find(|item| item.wall_plate == Some(WallPlateId(0)))
        .unwrap();
    assert!((wall.concrete_m3 - 2.7).abs() < 1e-12);
    assert!((wall.formwork_m2 - 36.0).abs() < 1e-12);
}

fn set_test_slab_placements(model: &mut Model) {
    use sepika_core::{ids::StoryId, model::Story};
    if model.stories.is_empty() {
        model
            .stories
            .push(Story::new(StoryId(0), "基準階".into(), Some(0.), Some(0.)));
    }
    let levels: Vec<_> = model
        .slabs
        .iter()
        .map(|s| {
            s.boundary_coords(model)
                .and_then(|c| c.first().map(|p| p[2]))
                .unwrap_or(0.)
        })
        .collect();
    for (slab, z) in model.slabs.iter_mut().zip(levels) {
        if slab.reference_story.is_none() {
            slab.reference_story = Some(StoryId(0));
            slab.vertical_eccentricity_mm = Some(z);
        }
    }
}

#[test]
fn 床の実配置変更は型枠接触へ反映し支持z変更は床面積体積を変えない() {
    let mut model = contact_model();
    contact_slab(&mut model, [0., 6000.], 1., 150., 3000.);
    assert!((contact_formwork(&model) - 8.1).abs() < 1e-9);
    model.slabs[0].vertical_eccentricity_mm = Some(3100.);
    assert!((contact_formwork(&model) - 8.7).abs() < 1e-9);
    model.stories[0].elevation = Some(200.);
    assert!((contact_formwork(&model) - 9.).abs() < 1e-9);
    model.stories[0].elevation = Some(0.);
    model.sections[1].thickness = Some(200.);
    assert!((contact_formwork(&model) - 8.4).abs() < 1e-9);
    let before = try_compute_quantity_takeoff(&model, &QuantityCfg::default())
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.slab == Some(SlabId(0)))
        .unwrap();
    assert!((before.formwork_m2 - 12.).abs() < 1e-9);
    assert!((before.concrete_m3 - 2.4).abs() < 1e-9);
    model.nodes[3].coord[2] = 6000.;
    model.rebuild_assignment_regions().unwrap();
    let after = try_compute_quantity_takeoff(&model, &QuantityCfg::default())
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.slab == Some(SlabId(0)))
        .unwrap();
    assert_eq!(
        (after.formwork_m2, after.concrete_m3),
        (before.formwork_m2, before.concrete_m3)
    );
    assert!((contact_formwork(&model) - 8.4).abs() < 1e-9);
    model.slabs[0].vertical_eccentricity_mm = None;
    let reason = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
    assert!(
        reason.contains("床板 0") && reason.contains("鉛直偏心"),
        "{reason}"
    );
}

#[test]
fn 梁の実材軸と床の独立した鉛直偏心から接触上下端を求める() {
    for src in [false, true] {
        for secondary in [false, true] {
            let mut model = contact_model();
            if src {
                model.sections[0] = with_rc_materials(
                    SectionShape::SrcBeamRect {
                        b: 300.0,
                        d: 600.0,
                        rebar: rc_beam_rebar(),
                        steel_height: 400.0,
                        steel_width: 200.0,
                        steel_web_thick: 10.0,
                        steel_flange_thick: 20.0,
                    }
                    .to_section(SectionId(0), "SRC梁".into()),
                );
                model.sections[0].steel_material = Some(MaterialId(2));
                model.materials.push(steel_material(2));
            }
            if secondary {
                model.elements.clear();
                let ends = contact_secondary_ends(&mut model);
                model.unassigned_beams.push(SecondaryMember {
                    id: SecondaryMemberId(789),
                    kind: SecondaryMemberKind::Beam,
                    ends,
                    section: Some(SectionId(0)),
                    name: "小梁789".into(),
                    vertical_eccentricity: Some(0.0),
                    gravity_end_shares: None,
                });
            }
            contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3400.0);
            let nodes = model.nodes.clone();
            let supports = format!("{:?}", model.floor_assignment_regions);
            let beam_ends = model.unassigned_beams.first().map(|m| m.ends);
            for (offset, upper_expected, lower_expected) in
                [(0.0, 8.7, 8.4), (100.0, 8.1, 9.0), (-100.0, 9.0, 8.1)]
            {
                if secondary {
                    model.unassigned_beams[0].vertical_eccentricity = Some(offset);
                } else {
                    model.elements[0].vertical_eccentricity = Some(offset);
                }
                for (floor_top, expected) in [(3400.0, upper_expected), (2800.0, lower_expected)] {
                    model.slabs[0].vertical_eccentricity_mm = Some(floor_top);
                    let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
                    let beam = q
                        .items
                        .iter()
                        .find(|i| {
                            if secondary {
                                i.secondary == Some(SecondaryMemberId(789))
                            } else {
                                i.elem == Some(ElemId(0))
                            }
                        })
                        .unwrap();
                    assert!(
                        (beam.formwork_m2 - expected).abs() < 1e-9,
                        "src={src}, secondary={secondary}, offset={offset}, floor={floor_top}: {}",
                        beam.formwork_m2
                    );
                }
            }
            if secondary {
                model.unassigned_beams[0].vertical_eccentricity = Some(0.0);
            } else {
                model.elements[0].vertical_eccentricity = Some(0.0);
            }
            model.slabs[0].vertical_eccentricity_mm = Some(3400.0);
            match model.sections[0].shape.as_mut().unwrap() {
                SectionShape::RcBeamRect { d, .. } | SectionShape::SrcBeamRect { d, .. } => {
                    *d = 800.0
                }
                _ => unreachable!(),
            }
            let q = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap();
            let beam = q
                .items
                .iter()
                .find(|i| {
                    if secondary {
                        i.secondary == Some(SecondaryMemberId(789))
                    } else {
                        i.elem == Some(ElemId(0))
                    }
                })
                .unwrap();
            assert!((beam.formwork_m2 - 10.5).abs() < 1e-9);
            assert_eq!(model.nodes, nodes);
            assert_eq!(format!("{:?}", model.floor_assignment_regions), supports);
            assert_eq!(model.unassigned_beams.first().map(|m| m.ends), beam_ends);
        }
    }
}

#[test]
fn 梁実配置の未解決理由と未対応断面姿勢を数量に残す() {
    for secondary in [false, true] {
        let mut model = contact_model();
        if secondary {
            model.elements.clear();
            let ends = contact_secondary_ends(&mut model);
            model.unassigned_beams.push(SecondaryMember {
                id: SecondaryMemberId(789),
                kind: SecondaryMemberKind::Beam,
                ends,
                section: Some(SectionId(0)),
                name: "小梁789".into(),
                vertical_eccentricity: Some(0.0),
                gravity_end_shares: None,
            });
        }
        contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3000.0);
        for (offset, expected) in [(None, "未指定"), (Some(f64::NAN), "非有限")] {
            if secondary {
                model.unassigned_beams[0].vertical_eccentricity = offset;
            } else {
                model.elements[0].vertical_eccentricity = offset;
            }
            let reason = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
            assert!(
                reason.contains(if secondary {
                    "小梁 ID 789"
                } else {
                    "大梁 ID 0"
                }) && reason.contains("実配置")
                    && reason.contains(expected),
                "{reason}"
            );
        }
    }
    let mut detached = contact_model();
    detached.elements.clear();
    detached.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(789),
        kind: SecondaryMemberKind::Beam,
        ends: sepika_core::model::SecondaryMemberEnds::Detached([
            [0.0, 0.0, 3000.0],
            [6000.0, 0.0, 3000.0],
        ]),
        section: Some(SectionId(0)),
        name: "未解決小梁789".into(),
        vertical_eccentricity: Some(0.0),
        gravity_end_shares: None,
    });
    contact_slab(&mut detached, [1000.0, 3000.0], 1.0, 150.0, 3000.0);
    let reason = try_compute_quantity_takeoff(&detached, &QuantityCfg::default()).unwrap_err();
    assert!(
        reason.contains("小梁 ID 789") && reason.contains("支持アンカー"),
        "{reason}"
    );
    let mut model = contact_model();
    contact_slab(&mut model, [0.0, 6000.0], 1.0, 150.0, 3000.0);
    model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
    let reason = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
    assert!(
        reason.contains("Primary(ElemId(0))") && reason.contains("断面せい方向が鉛直でない"),
        "{reason}"
    );
    let mut foundation = rc_portal_model();
    foundation.elements[3].vertical_eccentricity = Some(100.0);
    contact_slab(&mut foundation, [0.0, 6000.0], 1.0, 150.0, 0.0);
    let reason = try_compute_quantity_takeoff(&foundation, &QuantityCfg::default()).unwrap_err();
    assert!(reason.contains("基礎梁"), "{reason}");
}

#[test]
fn 傾斜した回転矩形梁の実側面に床が接すると未算定にする() {
    let mut model = contact_model();
    model.nodes[0].coord[2] = 0.0;
    model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
    model.sections[0] = with_rc_materials(
        SectionShape::RcBeamRect {
            b: 800.0,
            d: 200.0,
            rebar: rc_beam_rebar(),
        }
        .to_section(SectionId(0), "回転梁".into()),
    );
    contact_slab(&mut model, [5700.0, 6000.0], 1.0, 150.0, 3000.0);
    for _ in 0..2 {
        let reason = try_compute_quantity_takeoff(&model, &QuantityCfg::default()).unwrap_err();
        assert!(reason.contains("傾斜梁"), "{reason}");
        model.elements[0].nodes.reverse();
    }
}
