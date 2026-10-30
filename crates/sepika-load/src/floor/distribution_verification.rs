//! 床荷重の合力・対称性・頂点順序の不変性と、独立格子による矩形床・主架構反力の照合。

use super::polygon::{integrate_polygon, polygon_edge_areas, PolygonIntegrationOptions};
use super::tests::{make_rect_slab_model, make_square_slab_model, polygon_slab_model, total_load};
use super::*;
use sepika_core::geom::polygon as geom_polygon;
use sepika_core::geom::polygon::area_xy;
use sepika_core::ids::{ElemId, FloorRegionId, NodeId, SecondaryMemberId, SlabId};
use sepika_core::model::{
    AreaLoad, DistributionMethod, ElementData, ElementKind, EndCondition, FloorRegion, ForceRegime,
    LocalAxis, Node, SecondaryMember, SecondaryMemberAnchor, SecondaryMemberEnds,
    SecondaryMemberKind, SlabPlate, SupportMemberId,
};

/// 格子の最大辺長 [mm]。
const MAX_CELL_MM: f64 = 100.0;

/// 等距離とみなす許容差の、セル寸法に対する相対値。
const TIE_REL_TOL: f64 = 1e-9;

fn grid_edge_areas(coords: &[[f64; 3]]) -> Vec<f64> {
    let n = coords.len();
    let mut areas = vec![0.0_f64; n];
    if n < 3 {
        return areas;
    }
    let poly: Vec<[f64; 2]> = coords.iter().map(|c| [c[0], c[1]]).collect();
    let (lo, hi) = geom_polygon::bounding_box(&poly);
    let width = hi[0] - lo[0];
    let height = hi[1] - lo[1];
    if width <= 0.0 || height <= 0.0 {
        return areas;
    }
    let [nx, ny] = [
        (width / MAX_CELL_MM).ceil().max(1.0) as usize,
        (height / MAX_CELL_MM).ceil().max(1.0) as usize,
    ];
    let dx = width / nx as f64;
    let dy = height / ny as f64;
    let cell_area = dx * dy;
    let tie_tol = dx.max(dy) * TIE_REL_TOL;

    let mut dists = vec![0.0_f64; n];
    for iy in 0..ny {
        let y = lo[1] + (iy as f64 + 0.5) * dy;
        for ix in 0..nx {
            let x = lo[0] + (ix as f64 + 0.5) * dx;
            let p = [x, y];
            if !geom_polygon::contains_by_ray_crossing(&poly, p) {
                continue;
            }

            let mut d_min = f64::INFINITY;
            for (e, d) in dists.iter_mut().enumerate() {
                let (a, b) = (poly[e], poly[(e + 1) % n]);
                *d = geom_polygon::point_segment_dist(p, a, b);
                d_min = d_min.min(*d);
            }

            let ties: Vec<usize> = dists
                .iter()
                .enumerate()
                .filter(|(_, d)| **d - d_min <= tie_tol)
                .map(|(e, _)| e)
                .collect();
            let share = cell_area / ties.len() as f64;
            for e in ties {
                areas[e] += share;
            }
        }
    }
    areas
}

/// 荷重分配結果から、境界辺 `Edge(e)` ごとの総荷重 [N] を集計する。
fn edge_totals(loads: &[BeamLoad], n: usize) -> Vec<f64> {
    let mut out = vec![0.0_f64; n];
    for bl in loads {
        if let LoadTarget::Edge(e) = bl.target {
            if e < n {
                out[e] += bl.cmq.q_i + bl.cmq.q_j;
            }
        }
    }
    out
}

/// 総和が期待値（`w × 面積`）に相対 1e-9 以内で一致することを確認する（総和保存）。
fn assert_total(label: &str, values: &[f64], expected: f64) {
    let sum: f64 = values.iter().sum();
    let rel = (sum - expected).abs() / expected.abs().max(1e-12);
    assert!(
        rel < 1e-9,
        "{label}: 総和={sum} 期待={expected} 相対誤差={rel:e}"
    );
}

/// 辺負担がすべて等しいこと（対称性が崩れていないこと）を確認する。
fn assert_symmetric(values: &[f64], label: &str) {
    let max = values.iter().cloned().fold(f64::MIN, f64::max);
    let min = values.iter().cloned().fold(f64::MAX, f64::min);
    let rel = (max - min).abs() / max.abs().max(1e-12);
    assert!(
        rel < 1e-9,
        "{label}: 対称性が崩れた max={max} min={min} 相対差={rel:e}"
    );
}

/// `pairs` で指定した辺どうしの負担が等しいことを確認する（長方形の対辺対称性）。
fn assert_pairs_equal(values: &[f64], pairs: &[(usize, usize)], label: &str) {
    for &(a, b) in pairs {
        let (va, vb) = (values[a], values[b]);
        let rel = (va - vb).abs() / va.abs().max(vb.abs()).max(1e-12);
        assert!(
            rel < 1e-9,
            "{label}: 辺{a}={va} と辺{b}={vb} が等しくない 相対差={rel:e}"
        );
    }
}

/// 現行と有限線分拡張方式の辺負担を表と総和で出力する。`true_area` は多角形の真の面積 [mm²]。
fn print_comparison(label: &str, w: f64, true_area: f64, current: &[f64], baseline: &[f64]) {
    let cur_total: f64 = current.iter().sum();
    let base_total: f64 = baseline.iter().sum();
    println!("--- {label} ---");
    println!("辺   現行[N]          拡張方式[N]      差[%]      判定");
    for i in 0..current.len().max(baseline.len()) {
        let c = current.get(i).copied().unwrap_or(0.0);
        let b = baseline.get(i).copied().unwrap_or(0.0);
        let d = if b.abs() > 1e-12 {
            100.0 * (c - b) / b
        } else {
            0.0
        };
        let side = if (c - b).abs() <= b.abs() * 1e-9 {
            "一致"
        } else if c < b {
            "現行<拡張(過小=危険側)"
        } else {
            "現行>拡張(過大=安全側)"
        };
        println!("{i:<3} {c:15.3} {b:15.3} {d:9.4}  {side}");
    }
    let expected = w * true_area;
    println!(
        "総和: 現行={cur_total:.6} 拡張={base_total:.6} 真値={expected:.6} 現行誤差={:.3e} 拡張誤差={:.3e}",
        (cur_total - expected).abs() / expected.abs().max(1e-12),
        (base_total - expected).abs() / expected.abs().max(1e-12),
    );
}

/// ケース1: 正方形 6000×6000。TriTrapezoid と有限線分拡張方式の各辺負担を比較する。
#[test]
fn case1_square_6000_tritrapezoid_vs_reference() {
    let w = 0.005_f64;
    let side = 6000.0_f64;
    let (model, slab) = make_square_slab_model(side, DistributionMethod::TriTrapezoid, w);
    let loads = distribute_slab_w(&model, &slab, slab.plate.finish_intensity()).unwrap();
    let current = edge_totals(&loads, 4);
    let coords = slab.boundary_coords(&model).expect("境界座標");
    let baseline_areas = grid_edge_areas(&coords);
    let baseline: Vec<f64> = baseline_areas.iter().map(|a| w * a).collect();

    print_comparison(
        "ケース1: 正方形 6000x6000 TriTrapezoid vs 有限線分拡張方式",
        w,
        side * side,
        &current,
        &baseline,
    );

    assert_total("現行TriTrapezoid", &current, w * side * side);
    assert_symmetric(&current, "現行TriTrapezoid");
    assert_max_rel_diff_below(
        &current,
        &baseline,
        1e-9,
        "ケース1: 現行TriTrapezoid と有限線分拡張方式",
    );
}

/// ケース2: 長方形 6000×4000。TriTrapezoid と有限線分拡張方式の各辺負担を比較する。
#[test]
fn case2_rect_6000x4000_tritrapezoid_vs_reference() {
    let w = 0.005_f64;
    let (lx, ly) = (6000.0_f64, 4000.0_f64);
    let (model, slab) = make_rect_slab_model(lx, ly, DistributionMethod::TriTrapezoid, w);
    let loads = distribute_slab_w(&model, &slab, slab.plate.finish_intensity()).unwrap();
    let current = edge_totals(&loads, 4);
    let coords = slab.boundary_coords(&model).expect("境界座標");
    let baseline_areas = grid_edge_areas(&coords);
    let baseline: Vec<f64> = baseline_areas.iter().map(|a| w * a).collect();

    print_comparison(
        "ケース2: 長方形 6000x4000 TriTrapezoid vs 有限線分拡張方式",
        w,
        lx * ly,
        &current,
        &baseline,
    );

    assert_total("現行TriTrapezoid", &current, w * lx * ly);
    assert_pairs_equal(&current, &[(0, 2), (1, 3)], "現行TriTrapezoid");
    assert_max_rel_diff_below(
        &current,
        &baseline,
        1e-9,
        "ケース2: 現行TriTrapezoid と有限線分拡張方式",
    );
}

#[test]
fn large_floor_polygon_conserves_total_load() {
    let w = 0.002_f64;
    let (lx, ly) = (30000.0_f64, 20000.0_f64);
    let coords = [
        [0.0, 0.0, 0.0],
        [lx, 0.0, 0.0],
        [lx, ly, 0.0],
        [0.0, ly, 0.0],
    ];
    let areas = polygon_edge_areas(&coords, &[0, 1, 2, 3]);
    let current: Vec<f64> = areas.iter().map(|a| w * a).collect();
    assert_total("大きな床の分配荷重", &current, w * lx * ly);
}

fn run_nonrect_case(label: &str, pts: &[(f64, f64)], w: f64) {
    let (model, slab) = polygon_slab_model(pts, DistributionMethod::TriTrapezoid, w);
    let loads = distribute_slab_w(&model, &slab, slab.plate.finish_intensity()).unwrap();
    let current = edge_totals(&loads, pts.len());
    let coords: Vec<_> = pts.iter().map(|&(x, y)| [x, y, 0.0]).collect();
    assert_total(label, &current, w * area_xy(&coords));
}

/// 台形・L形床の分配で合力を保存する。
#[test]
fn case4_nonrect_trapezoid_and_lshape() {
    let w = 0.003_f64;
    run_nonrect_case(
        "ケース4a: 台形の合力保存",
        &[
            (0.0, 0.0),
            (6000.0, 0.0),
            (4000.0, 3000.0),
            (1000.0, 3000.0),
        ],
        w,
    );
    run_nonrect_case(
        "ケース4b: L形の合力保存",
        &[
            (0.0, 0.0),
            (12000.0, 0.0),
            (12000.0, 6000.0),
            (6000.0, 6000.0),
            (6000.0, 12000.0),
            (0.0, 12000.0),
        ],
        w,
    );
}

/// ケース5の床モデルの大梁（辺→節点）。
const GIRDER_ENDS: [(usize, usize); 4] = [(0, 1), (1, 2), (2, 3), (3, 0)];

/// ケース5の中央小梁の材軸（X=2500 の鉛直線）。
const JOIST_A: [f64; 2] = [2500.0, 0.0];
/// ケース5の中央小梁の材軸（上端）。
const JOIST_B: [f64; 2] = [2500.0, 4000.0];

/// ケース5の小梁が境界として載る辺の分類。
enum EdgeClass {
    Girder(usize),
    Beam,
}

/// 点がどの大梁（`GIRDER_ENDS` の順）の材軸上にあるかを返す。
fn girder_of_point(model: &Model, p: [f64; 2]) -> Option<usize> {
    for (idx, &(i, j)) in GIRDER_ENDS.iter().enumerate() {
        let a = model.nodes.get(i)?.coord;
        let b = model.nodes.get(j)?.coord;
        let d2 = geom_polygon::point_segment_dist_sq(p, [a[0], a[1]], [b[0], b[1]]);
        if d2 <= 1.0 {
            return Some(idx);
        }
    }
    None
}

/// 床板の辺 `e` が大梁か中央小梁かを、辺中点の位置から判定する。
fn classify_edge(coords: &[[f64; 3]], e: usize, model: &Model) -> Option<EdgeClass> {
    let a = coords[e];
    let b = coords[(e + 1) % coords.len()];
    let mid = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    if geom_polygon::point_segment_dist(mid, JOIST_A, JOIST_B) <= 1.0 {
        return Some(EdgeClass::Beam);
    }
    girder_of_point(model, mid).map(EdgeClass::Girder)
}

/// ケース5のモデル（6000×4000 の床領域と X=2500 の中央小梁）を作る。
fn beam_floor_model(w: f64) -> (Model, SlabId, SlabId) {
    let mk_node = |id: u32, x: f64, y: f64| Node {
        id: NodeId(id),
        coord: [x, y, 0.0],
        restraint: Default::default(),
        mass: None,
        story: None,
        support_spring: None,
    };
    let mk_beam = |id: u32, i: u32, j: u32| ElementData {
        id: ElemId(id),
        kind: ElementKind::Beam,
        nodes: [NodeId(i), NodeId(j)].into_iter().collect(),
        section: None,
        local_axis: LocalAxis {
            ref_vector: [0.0, 0.0, 1.0],
        },
        end_cond: [EndCondition::Fixed, EndCondition::Fixed],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    };
    let nodes = vec![
        mk_node(0, 0.0, 0.0),
        mk_node(1, 6000.0, 0.0),
        mk_node(2, 6000.0, 4000.0),
        mk_node(3, 0.0, 4000.0),
        mk_node(4, 2500.0, 0.0),
        mk_node(5, 2500.0, 4000.0),
    ];
    let elements = vec![
        mk_beam(0, 0, 1),
        mk_beam(1, 1, 2),
        mk_beam(2, 2, 3),
        mk_beam(3, 3, 0),
    ];
    let plate = SlabPlate {
        loads: vec![AreaLoad {
            kind: "DL".into(),
            value: w,
        }],
        method: DistributionMethod::TriTrapezoid,
        ..Default::default()
    };
    let mut region = FloorRegion::new(
        FloorRegionId(0),
        vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
    );
    region.secondary_beams = vec![SecondaryMember {
        id: SecondaryMemberId(4),
        vertical_eccentricity: Some(0.0),
        gravity_end_shares: None,
        kind: SecondaryMemberKind::Beam,
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(0)),
                position: 2500.0 / 6000.0,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(2)),
                position: 3500.0 / 6000.0,
            },
        ]),
        section: None,
        name: "J".into(),
    }];
    let mut model = Model {
        nodes,
        elements,
        floor_regions: vec![region],
        ..Default::default()
    };
    model.rebuild_floor_assignment_regions();
    let left = model
        .assign_enclosed_slab_to_matching_region(
            &[NodeId(0), NodeId(4), NodeId(5), NodeId(3)],
            plate.clone(),
        )
        .expect("左半分の割当領域");
    let right = model
        .assign_enclosed_slab_to_matching_region(
            &[NodeId(4), NodeId(1), NodeId(2), NodeId(5)],
            plate,
        )
        .expect("右半分の割当領域");
    model.floor_regions[0].slab_ids = vec![left, right];
    (model, left, right)
}

/// 現行カスケードの最終主架構反力 [N] を大梁 `GIRDER_ENDS` ごとに集計する。
fn current_primary_by_girder(
    model: &Model,
    transfer: &crate::cascade::SecondaryTransfer,
) -> [f64; 4] {
    let mut out = [0.0_f64; 4];
    for bl in &transfer.leftover_region_loads {
        add_to_girder(&mut out, bl.elem, bl.cmq.q_i + bl.cmq.q_j);
    }
    let (nodal, member) = transfer.primary_loads(model);
    for bl in &member {
        add_to_girder(&mut out, bl.elem, bl.cmq.q_i + bl.cmq.q_j);
    }
    for (node, r) in &nodal {
        let Some(n) = model.nodes.get(node.index()) else {
            continue;
        };
        if let Some(idx) = girder_of_point(model, [n.coord[0], n.coord[1]]) {
            out[idx] += *r;
        }
    }
    out
}

fn add_to_girder(out: &mut [f64; 4], elem: ElemId, total: f64) {
    let idx = elem.0 as usize;
    if idx < out.len() {
        out[idx] += total;
    }
}

/// 有限線分拡張方式の辺負担を小梁の単純梁反力（両端等分）に換算して主架構ごとに集計する。
fn segment_extension_primary_by_girder(model: &Model, slabs: &[SlabId], w: f64) -> [f64; 4] {
    let mut out = [0.0_f64; 4];
    let mut beam_total = 0.0_f64;
    for &sid in slabs {
        let Some(slab) = model.slab(sid) else {
            continue;
        };
        let Some(coords) = slab.boundary_coords(model) else {
            continue;
        };
        for (e, area) in grid_edge_areas(&coords).into_iter().enumerate() {
            let load = w * area;
            match classify_edge(&coords, e, model) {
                Some(EdgeClass::Girder(i)) => out[i] += load,
                Some(EdgeClass::Beam) => beam_total += load,
                None => {}
            }
        }
    }
    out[0] += 0.5 * beam_total;
    out[2] += 0.5 * beam_total;
    out
}

/// ケース5: 小梁を含む床の最終主架構反力を、現行カスケードと有限線分拡張方式で比較する。
#[test]
fn case5_beam_floor_primary_reactions_current_vs_reference() {
    let w = 0.005_f64;
    let area = 6000.0 * 4000.0;
    let (model, left, right) = beam_floor_model(w);

    let region_loads = distribute_region(&model, &model.floor_regions[0], |_| w).unwrap();
    assert_total("distribute_region", &[total_load(&region_loads)], w * area);
    let (beam_map, _) = secondary_beam_distribution_split(&model, |_| w).unwrap();
    let beam_load_count = beam_map
        .get(&SecondaryMemberId(4))
        .map(|e| e.member_loads.len())
        .unwrap_or(0);
    assert!(beam_load_count > 0, "小梁に分配荷重がある");

    let transfer = crate::cascade::solve(&model, |_| w, false).unwrap();
    assert!(transfer.unresolved.is_empty(), "{:?}", transfer.unresolved);
    let current = current_primary_by_girder(&model, &transfer);
    let baseline = segment_extension_primary_by_girder(&model, &[left, right], w);
    let current_total: f64 = current.iter().sum();
    let baseline_total: f64 = baseline.iter().sum();

    println!("--- ケース5: 小梁を含む床 6000x4000 (小梁 X=2500) ---");
    println!("主架構   現行[N]          有限線分拡張[N]  差[%]      判定");
    for (i, name) in ["G0(下)", "G1(右)", "G2(上)", "G3(左)"].iter().enumerate() {
        let c = current[i];
        let b = baseline[i];
        let d = if b.abs() > 1e-12 {
            100.0 * (c - b) / b
        } else {
            0.0
        };
        let side = if (c - b).abs() <= b.abs() * 1e-9 {
            "一致"
        } else if c < b {
            "現行<有限線分拡張(過小=危険側)"
        } else {
            "現行>有限線分拡張(過大=安全側)"
        };
        println!("{name:<8} {c:15.3} {b:15.3} {d:9.4}  {side}");
    }
    println!(
        "合計: 現行={current_total:.6} 有限線分拡張={baseline_total:.6} 真値={:.6}",
        w * area
    );

    assert_total("現行カスケード合計", &[current_total], w * area);
    assert_total("有限線分拡張方式合計", &[baseline_total], w * area);
    assert_max_rel_diff_below(
        &current,
        &baseline,
        0.01,
        "ケース5: 現行カスケード と 有限線分拡張方式（最終主架構反力）",
    );
}

/// `a` と `b` の辺ごとの相対差の最大値（比）が `limit` 未満であることを確認する。
fn assert_max_rel_diff_below(a: &[f64], b: &[f64], limit: f64, label: &str) {
    let m = a
        .iter()
        .zip(b.iter())
        .map(|(&x, &y)| {
            let denom = x.abs().max(y.abs());
            if denom > 1e-12 {
                (x - y).abs() / denom
            } else {
                0.0
            }
        })
        .fold(0.0_f64, f64::max);
    assert!(
        m < limit,
        "{label}: 最大相対差={:.4}% が閾値 {:.4}% 以上",
        100.0 * m,
        100.0 * limit
    );
}

fn run_split_case(label: &str, pts: &[(f64, f64)], w: f64, symmetry_pairs: &[(usize, usize)]) {
    let n = pts.len();
    let coords: Vec<[f64; 3]> = pts.iter().map(|(x, y)| [*x, *y, 0.0]).collect();
    let true_area = area_xy(&coords);
    let candidate: Vec<usize> = (0..n).collect();
    let diagnostics =
        integrate_polygon(&coords, &candidate, PolygonIntegrationOptions::default()).unwrap();
    let production_areas = &diagnostics.edge_areas_mm2;
    let production: Vec<_> = production_areas.iter().map(|area| w * area).collect();
    assert_total(label, &production, w * true_area);
    for &(a, b) in symmetry_pairs {
        assert!(
            (production_areas[a] - production_areas[b]).abs()
                <= diagnostics.edge_error_bounds_mm2[a] + diagnostics.edge_error_bounds_mm2[b]
        );
    }
}

/// L形・T形・十字形・凸五角形・正八角形・U字形状の合力と対称性を検証する。
#[test]
fn case7_split_ties_shape_matrix() {
    let w = 0.003_f64;

    // L形: 12000×12000 のうち右上 6000×6000 を欠く。対角線 y=x について対称。
    run_split_case(
        "ケース7a: L形 12000x12000（右上6000x6000欠）",
        &[
            (0.0, 0.0),
            (12000.0, 0.0),
            (12000.0, 6000.0),
            (6000.0, 6000.0),
            (6000.0, 12000.0),
            (0.0, 12000.0),
        ],
        w,
        &[(0, 5), (1, 4), (2, 3)],
    );

    // T形（凹形状）: 下辺 12000×6000 の上に 6000×6000 が中央に乗る。x=6000 について対称。
    run_split_case(
        "ケース7b: T形（凹形状）12000x12000",
        &[
            (0.0, 0.0),
            (12000.0, 0.0),
            (12000.0, 6000.0),
            (9000.0, 6000.0),
            (9000.0, 12000.0),
            (3000.0, 12000.0),
            (3000.0, 6000.0),
            (0.0, 6000.0),
        ],
        w,
        &[(1, 7), (2, 6), (3, 5)],
    );

    // 十字形: 12000×12000 から四隅 3000×3000 を欠く。凹角を4つ持つ。
    run_split_case(
        "ケース7c: 十字形 12000x12000（四隅3000x3000欠）",
        &[
            (3000.0, 0.0),
            (9000.0, 0.0),
            (9000.0, 3000.0),
            (12000.0, 3000.0),
            (12000.0, 9000.0),
            (9000.0, 9000.0),
            (9000.0, 12000.0),
            (3000.0, 12000.0),
            (3000.0, 9000.0),
            (0.0, 9000.0),
            (0.0, 3000.0),
            (3000.0, 3000.0),
        ],
        w,
        &[(1, 11), (2, 10), (3, 9), (4, 8), (5, 7)],
    );

    // 凸五角形: 既存 test_polygon_pentagon_conservation と同じ形状。対称性なし。
    run_split_case(
        "ケース7d: 凸五角形",
        &[
            (0.0, 0.0),
            (5000.0, 0.0),
            (6000.0, 3000.0),
            (2500.0, 5000.0),
            (-1000.0, 3000.0),
        ],
        w,
        &[],
    );

    // 正八角形: 半径 6000、頂点角 22.5°+45°k。45°回転対称は格子が保存しないため、
    // 90°回転で移る辺（0-2-4-6、1-3-5-7）のみ対称辺として検証する。
    let r = 6000.0_f64;
    let (c, s) = (22.5_f64.to_radians().cos(), 22.5_f64.to_radians().sin());
    let octagon = [
        (r * c, r * s),
        (r * s, r * c),
        (-r * s, r * c),
        (-r * c, r * s),
        (-r * c, -r * s),
        (-r * s, -r * c),
        (r * s, -r * c),
        (r * c, -r * s),
    ];
    run_split_case(
        "ケース7e: 正八角形（半径6000）",
        &octagon,
        w,
        &[(0, 2), (2, 4), (4, 6), (1, 3), (3, 5), (5, 7)],
    );

    // U字形状: 12000×12000 の上辺中央 6000×8000 を欠く。凹角を2つ持つ。
    run_split_case(
        "ケース7f: U字形状 12000x12000（上辺中央6000x8000欠）",
        &[
            (0.0, 0.0),
            (12000.0, 0.0),
            (12000.0, 12000.0),
            (9000.0, 12000.0),
            (9000.0, 4000.0),
            (3000.0, 4000.0),
            (3000.0, 12000.0),
            (0.0, 12000.0),
        ],
        w,
        &[(1, 7), (2, 6), (3, 5)],
    );
}

/// ケース7で使う凹形状のうち、頂点列の変換検査で使う代表形状。
const LSHAPE_PTS: [(f64, f64); 6] = [
    (0.0, 0.0),
    (12000.0, 0.0),
    (12000.0, 6000.0),
    (6000.0, 6000.0),
    (6000.0, 12000.0),
    (0.0, 12000.0),
];
const CROSS_PTS: [(f64, f64); 12] = [
    (3000.0, 0.0),
    (9000.0, 0.0),
    (9000.0, 3000.0),
    (12000.0, 3000.0),
    (12000.0, 9000.0),
    (9000.0, 9000.0),
    (9000.0, 12000.0),
    (3000.0, 12000.0),
    (3000.0, 9000.0),
    (0.0, 9000.0),
    (0.0, 3000.0),
    (3000.0, 3000.0),
];

/// 頂点列を `shift` 個循環移動し、`reverse` なら時計回りへ反転した座標列を返す。
fn transformed_cycle(pts: &[(f64, f64)], shift: usize, reverse: bool) -> Vec<(f64, f64)> {
    let n = pts.len();
    let mut out: Vec<(f64, f64)> = (0..n).map(|i| pts[(i + shift) % n]).collect();
    if reverse {
        out.reverse();
    }
    out
}

/// 辺 `e` の両端座標を昇順ソートしたキー。循環移動・反転で不変な物理辺の識別子。
fn edge_key(pts: &[(f64, f64)], e: usize) -> ([f64; 2], [f64; 2]) {
    let a = pts[e];
    let b = pts[(e + 1) % pts.len()];
    let (lo, hi) = if (a.0, a.1) <= (b.0, b.1) {
        (a, b)
    } else {
        (b, a)
    };
    ([lo.0, lo.1], [hi.0, hi.1])
}

/// 物理辺を表すキー（両端座標のビット列を昇順ソートしたもの）。
type EdgeKey = ([u64; 2], [u64; 2]);

/// 物理辺のキー→負担荷重 [N] のマップ。
type EdgeLoadMap = std::collections::BTreeMap<EdgeKey, f64>;

/// 物理辺のキー→負担荷重 [N] のマップを作る。
fn edge_load_map(pts: &[(f64, f64)], loads: &[f64]) -> EdgeLoadMap {
    let mut map = EdgeLoadMap::new();
    for (e, &load) in loads.iter().enumerate() {
        let (a, b) = edge_key(pts, e);
        map.insert(
            (
                [a[0].to_bits(), a[1].to_bits()],
                [b[0].to_bits(), b[1].to_bits()],
            ),
            load,
        );
    }
    map
}

fn check_vertex_order(label: &str, pts: &[(f64, f64)]) {
    let patterns: Vec<(usize, bool)> = vec![
        (0, false),
        (1, false),
        (2, false),
        (3, false),
        (0, true),
        (1, true),
    ];
    let base_coords: Vec<_> = pts.iter().map(|&(x, y)| [x, y, 0.0]).collect();
    let all: Vec<_> = (0..pts.len()).collect();
    let base_result = integrate_polygon(&base_coords, &all, Default::default()).unwrap();
    let base_area = edge_load_map(pts, &base_result.edge_areas_mm2);
    let base_bounds = edge_load_map(pts, &base_result.edge_error_bounds_mm2);
    for &(shift, reverse) in &patterns {
        let transformed = transformed_cycle(pts, shift, reverse);
        let coords: Vec<_> = transformed.iter().map(|&(x, y)| [x, y, 0.]).collect();
        let result = integrate_polygon(&coords, &all, Default::default()).unwrap();
        let areas = edge_load_map(&transformed, &result.edge_areas_mm2);
        let bounds = edge_load_map(&transformed, &result.edge_error_bounds_mm2);
        for (key, a) in &base_area {
            assert!(
                (a - areas[key]).abs() <= base_bounds[key] + bounds[key],
                "{label}: {key:?}"
            );
        }
    }
}

/// L形・十字形の頂点列の循環移動・反転で、物理辺の負担面積が誤差限界内で一致する。
#[test]
fn case9_vertex_order_invariance() {
    check_vertex_order("L形 12000x12000", &LSHAPE_PTS);
    check_vertex_order("十字形 12000x12000", &CROSS_PTS);
}
