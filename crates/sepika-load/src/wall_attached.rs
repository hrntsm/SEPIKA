//! 取り付く壁版（[`WallPlateShape::Attached`]）の自重配分。
//!
//! `Attached` な壁版（パラペット・腰壁・垂れ壁・自立壁）は解析要素を持たない（D5）ため、
//! 壁展開（[`crate::wall_expand`]）を経由する自重算定（`story_gen::enumerate_self_weight`）
//! では検出できず、これまで地震用重量・長期応力解析のDLのどちらからも自重が
//! 抜け落ちていた（`dev_docs/handoff/床領域・壁領域の再設計_申し送り.md` §9・§5.22）。
//! 本モジュールは、取付き先（`RegionAnchor`）の種別ごとに定めた伝達規則（D16）に沿って、
//! 壁版の自重（[`Model::wall_plate_self_weight`]）を配分する。重量の算定式自体は
//! 既存関数をそのまま使い、本モジュールが新たに担うのは配分のみである。
//!
//! - [`attached_wall_beam_loads`] — 「線」アンカー（[`RegionAnchor::Line`]）の壁版を
//!   [`BeamLoad`] へ変換する。取付き先の梁へ分布させる（[`LoadTransfer::Anchor`]、
//!   梁の全長ではなく取付き線の区間 `span` にのみ載せる）か、取付き線両端の柱へ
//!   集中させる（[`LoadTransfer::Columns`]）かは壁版側の設定に従う。床の取り付く床板
//!   （`SlabShape::Attached`）と同じ幾何解決パイプライン
//!   （`sepika-job::auto_loads::slab_load_case_content`）に合流させる想定で、
//!   `span`（部分区間）を梁全長へ薄めず正確に尊重する（危険側の近似を避けるため。
//!   `AGENTS.md`「実装方針」参照）。
//! - [`floor_region_wall_extra_intensity`] — 「床領域」アンカー（[`RegionAnchor::FloorRegion`]、
//!   自立壁、D17）の壁版の自重を、壁が載っている床領域の床板へ等価な面荷重として
//!   上乗せするための追加強度 [N/mm²] を床板 ID ごとに返す。分配先の床領域 ID は
//!   保存せず、[`Model::self_standing_wall_coverage`] が位置から都度求める。強度の
//!   分母は床の分配と同じ XY 投影面積とする。荷重を流せる床の上に載っていない部分
//!   は分配せず、解析前チェック（`sepika-solver::precheck`）がエラーで止める。
//!   両端節点への集中荷重へ逃がすフォールバックは持たない（非構造節点への節点荷重は
//!   `DofMap` が無視し、長期 DL から黙って消える危険側だったため）。
//!
//! 張り出し量と位置付き開口の実領域を線方向へ積分し、区分的な線形荷重を作る。
//! 地震用階重量は支持反力から生成せず、共通領域の水平帯積分を使う。

use std::collections::HashMap;

use sepika_core::geom::polygon::area_xy;
use sepika_core::geom::vec3::dist as dist3;
use sepika_core::ids::{ElemId, FloorRegionId, NodeId, SlabId};
use sepika_core::model::{LoadTransfer, Model, RegionAnchor, WallPlateShape};

use crate::floor::{fem_linear, fem_uniform, BeamLoad, Cmq, LoadShape, LoadTarget};

/// 節点 `node` への集中荷重（[`LoadTarget::Node`]）を1件積む。総量が実質0なら積まない。
fn push_node_load(loads: &mut Vec<BeamLoad>, node: NodeId, total: f64) {
    if total.abs() <= 1e-9 {
        return;
    }
    loads.push(BeamLoad {
        elem: ElemId(u32::MAX),
        target: LoadTarget::Node(node),
        shape: LoadShape::Point { p: total, x: 0.0 },
        cmq: Cmq {
            c_i: 0.0,
            c_j: 0.0,
            q_i: total,
            q_j: 0.0,
        },
    });
}

/// 「線」アンカーの取り付く壁版（パラペット・腰壁・垂れ壁で梁に取り付くもの）の
/// 自重を [`BeamLoad`] へ変換する（D16）。
pub fn attached_wall_beam_loads(model: &Model) -> Result<Vec<BeamLoad>, String> {
    let mut loads = Vec::new();
    for plate in &model.wall_plates {
        let WallPlateShape::Attached { anchor, .. } = &plate.shape else {
            continue;
        };
        let RegionAnchor::Line {
            nodes,
            span,
            transfer,
        } = anchor
        else {
            continue;
        };
        model.validate_wall_design_self_weight(plate)?;
        let Ok(weight) = model.wall_weight(plate) else {
            continue;
        };
        let Ok(parts) = weight.projected_design_line_loads() else {
            continue;
        };
        let Some(coords) = plate.boundary_coords(model) else {
            continue;
        };
        let plan_len = (coords[1][0] - coords[0][0]).hypot(coords[1][1] - coords[0][1]);
        let len = dist3(coords[0], coords[1]);
        if plan_len <= 1e-9 || len <= 1e-9 {
            continue;
        }
        for [a, b, w1, w2] in parts {
            let piece_len = (b - a) * len / plan_len;
            let t0 = span[0] + (span[1] - span[0]) * a / plan_len;
            let t1 = span[0] + (span[1] - span[0]) * b / plan_len;
            let wi = w1 * plan_len / len;
            let wj = w2 * plan_len / len;
            let average = wi / 2.0 + wj / 2.0;
            let total = average * piece_len;
            match transfer {
                LoadTransfer::Anchor => {
                    let (shape, cmq) = if (wi - wj).abs() <= 1e-12 * wi.max(wj).max(1.0) {
                        (
                            LoadShape::Uniform { w: average },
                            fem_uniform(average, piece_len),
                        )
                    } else {
                        (
                            LoadShape::Linear { w_i: wi, w_j: wj },
                            fem_linear(wi, wj, piece_len),
                        )
                    };
                    loads.push(BeamLoad {
                        elem: ElemId(u32::MAX),
                        target: LoadTarget::Span {
                            nodes: *nodes,
                            t: [t0, t1],
                        },
                        shape,
                        cmq,
                    });
                }
                LoadTransfer::Columns => {
                    let scale = wi.max(wj);
                    if scale == 0.0 {
                        continue;
                    }
                    let mean =
                        (wi / scale + 2.0 * (wj / scale)) / (3.0 * (wi / scale + wj / scale));
                    let t = t0 + (t1 - t0) * mean;
                    push_node_load(&mut loads, nodes[0], total * (1.0 - t));
                    push_node_load(&mut loads, nodes[1], total * t);
                }
            }
        }
    }
    Ok(loads)
}

/// 床領域の床板合計面積 [mm²]（床板を1枚も持たない、または境界座標が引けない
/// 場合は 0.0）。
///
/// 床の分配（[`area_xy`]）と同じ XY 投影面積を使う。
/// 3 次元面積（[`sepika_core::geom::polygon::area_3d`]）で割ると、分配側が XY 面積に
/// 強度を掛けるため
/// 総重量が `(A_xy / A_3d)` 倍に縮小し、傾斜床では地震用重量・梁荷重が過小
/// （危険側）になる。鉛直に近い床板は XY 面積が 0 になり、等価面荷重へならせない。
/// その床領域は [`Model::self_standing_wall_coverage`] の候補から外れる。
fn region_slab_area(model: &Model, slab_ids: &[SlabId]) -> f64 {
    slab_ids
        .iter()
        .filter_map(|&id| model.slab(id))
        .filter_map(|s| s.boundary_coords(model))
        .map(|pts| area_xy(&pts))
        .sum()
}

/// 節点重量配列へ集中荷重を足す（添字が範囲外なら無視）。
fn add_node_weight(node_weight: &mut [f64], node: NodeId, w: f64) {
    let i = node.index();
    if i < node_weight.len() && w.abs() > 1e-9 {
        node_weight[i] += w;
    }
}

/// 取り付く壁版のDL支持反力相当量を節点へ集計する。
/// 地震用階重量はこの反力を使わず、`Model::wall_weight` の水平帯積分で算定する。
pub fn accumulate_attached_wall_dl_weight(
    model: &Model,
    node_weight: &mut [f64],
) -> Result<(), String> {
    for plate in model.wall_plates.iter().filter(|p| p.is_attached()) {
        model.validate_wall_design_self_weight(plate)?;
    }
    accumulate_attached_wall_weight_with(model, node_weight, false);
    Ok(())
}

/// 物理質量相当重量をDL支持反力と同じ端点比で集計する補助API。
/// 地震用階帯・代表節点質量・質量行列の組立には使わない。
pub fn accumulate_attached_wall_dl_mass_equiv(model: &Model, node_weight: &mut [f64]) {
    accumulate_attached_wall_weight_with(model, node_weight, true);
}

fn accumulate_attached_wall_weight_with(model: &Model, node_weight: &mut [f64], physical: bool) {
    for plate in &model.wall_plates {
        let WallPlateShape::Attached { anchor, .. } = &plate.shape else {
            continue;
        };
        let Ok(weight) = model.wall_weight(plate) else {
            continue;
        };
        let Ok(band) = weight.story_band(weight.z_range_mm[0], weight.z_range_mm[1]) else {
            continue;
        };
        let total = if physical {
            band.band.physical_n
        } else {
            band.band.design_n
        };
        let nodes = match anchor {
            RegionAnchor::Line { nodes, .. } | RegionAnchor::FloorRegion { nodes, .. } => nodes,
            RegionAnchor::Point(_) => continue,
        };
        let (Some(a), Some(b)) = (model.node(nodes[0]), model.node(nodes[1])) else {
            continue;
        };
        let delta = [b.coord[0] - a.coord[0], b.coord[1] - a.coord[1]];
        let length_sq = delta[0].powi(2) + delta[1].powi(2);
        if length_sq <= 0.0 || total <= 0.0 {
            continue;
        }
        let t = ((band.center_xy_mm[0] - a.coord[0]) * delta[0]
            + (band.center_xy_mm[1] - a.coord[1]) * delta[1])
            / length_sq;
        add_node_weight(node_weight, nodes[0], total * (1.0 - t));
        add_node_weight(node_weight, nodes[1], total * t);
    }
}

/// 「床領域」アンカーの取り付く壁版（自立壁）の自重を配分する（D17）。
///
/// 戻り値は床板ごとの追加面荷重強度 [N/mm²]。追加強度は、載っている床領域内の
/// 全床板へ床板面積によらず同一の値を返す（D17「等価な面荷重へならす」を、
/// 床領域内の床板ごとの強度差を無視する形で実装したもの。床領域内で面荷重強度が
/// 異なる床板が混在する場合の扱いは
/// `dev_docs/handoff/床領域・壁領域の再設計_申し送り.md` §8.1 の残課題と同じ性質の近似）。
///
/// # 荷重を渡す床領域は保存せず、壁の位置から都度求める
///
/// 分配先は [`sepika_core::model::Model::self_standing_wall_coverage`] が解決する。
/// 床領域をまたぐ壁は境界で内部的に分割し、それぞれの床領域へ台形面積比で配る。
///
/// # 荷重の行き先が無い壁は分配しない
///
/// どの床領域にも載らない部分を持つ自立壁は、解析前チェックがエラーで止める。
/// ここでは覆われている部分だけを配る。
pub fn floor_region_wall_extra_intensity(model: &Model) -> Result<HashMap<SlabId, f64>, String> {
    for plate in model.wall_plates.iter().filter(|p| p.is_attached()) {
        model.validate_wall_design_self_weight(plate)?;
    }
    let mut total_by_region: HashMap<FloorRegionId, f64> = HashMap::new();
    for plate in &model.wall_plates {
        let Some(total) = model.wall_plate_self_weight(plate, model) else {
            continue;
        };
        if total <= 0.0 {
            continue;
        }
        let Some(cov) = model.self_standing_wall_coverage(plate) else {
            continue;
        };
        for (region, frac) in cov.per_region {
            *total_by_region.entry(region).or_insert(0.0) += total * frac;
        }
    }
    if total_by_region.is_empty() {
        return Ok(HashMap::new());
    }

    let mut extra_intensity: HashMap<SlabId, f64> = HashMap::new();
    for region in &model.floor_regions {
        let Some(&extra) = total_by_region.get(&region.id) else {
            continue;
        };
        let area = region_slab_area(model, &region.slab_ids);
        if area <= 0.0 {
            continue;
        }
        let dw = extra / area;
        for &sid in &region.slab_ids {
            *extra_intensity.entry(sid).or_insert(0.0) += dw;
        }
    }

    Ok(extra_intensity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_core::model::{
        DistributionMethod, FloorRegion, Material, MaterialCategory, Node, Section, SlabPlate,
        WallPlate,
    };

    const THICKNESS_MM: f64 = 150.0;
    const DENSITY_TON_MM3: f64 = 2.4e-9;

    fn mk_node(id: u32, x: f64, y: f64, z: f64) -> Node {
        Node {
            id: NodeId(id),
            coord: [x, y, z],
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        }
    }

    /// 節点0・1（取付き線、z=3000）＋壁厚150mmの断面・材料を持つモデル。
    fn model_with_line_anchor_nodes() -> Model {
        let mut m = Model {
            nodes: vec![
                mk_node(0, 0.0, 0.0, 3000.0),
                mk_node(1, 4000.0, 0.0, 3000.0),
            ],
            ..Default::default()
        };
        m.materials.push(Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "Fc24".into(),
            category: MaterialCategory::Concrete,
            young: 23000.0,
            poisson: 0.2,
            density: DENSITY_TON_MM3,
            shear: None,
            fc: Some(24.0),
            fy: None,
        });
        m.sections.push(Section {
            frame_use: None,
            id: SectionId(0),
            name: "壁 t150".into(),
            area: 0.0,
            iy: 1.0,
            iz: 1.0,
            j: 1.0,
            depth: 0.0,
            width: 0.0,
            as_y: 1.0,
            as_z: 1.0,
            floor: None,
            panel_thickness: None,
            thickness: Some(THICKNESS_MM),
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        });
        m
    }

    fn line_attached_plate(span: [f64; 2], extent: [f64; 2], transfer: LoadTransfer) -> WallPlate {
        WallPlate {
            dl_support: None,
            self_weight_shares: Vec::new(),
            id: sepika_core::ids::WallPlateId(0),
            shape: WallPlateShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span,
                    transfer,
                },
                extent: Some(extent),
            },
            section: Some(SectionId(0)),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: vec![],
            slit: Default::default(),
        }
    }

    /// `Anchor`（分布）・全区間: 取付き線全長への等分布荷重1件になり、
    /// `w×len` が壁版の自重総量と一致する（総和保存）。
    #[test]
    fn anchor_transfer_full_span_is_uniform_over_full_line() {
        let mut m = model_with_line_anchor_nodes();
        let plate = line_attached_plate([0.0, 1.0], [1000.0, 1000.0], LoadTransfer::Anchor);
        m.wall_plates.push(plate.clone());
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");

        let loads = attached_wall_beam_loads(&m).unwrap();
        assert_eq!(loads.len(), 1);
        let bl = &loads[0];
        assert_eq!(
            bl.target,
            LoadTarget::Span {
                nodes: [NodeId(0), NodeId(1)],
                t: [0.0, 1.0],
            }
        );
        let LoadShape::Uniform { w } = bl.shape else {
            panic!("Uniform を期待");
        };
        let len = 4000.0;
        assert!(
            (w * len - total).abs() / total < 1e-9,
            "w×len={} total={}",
            w * len,
            total
        );
    }

    /// `Anchor`（分布）・部分区間: `span` をそのまま `t` へ引き継ぎ、梁全長へ薄めない
    /// （危険側の近似を避ける。dig 2026-08-27 Q2=A）。
    #[test]
    fn anchor_transfer_partial_span_keeps_span_and_conserves_total() {
        let mut m = model_with_line_anchor_nodes();
        let span = [0.25, 0.75];
        let plate = line_attached_plate(span, [1000.0, 1000.0], LoadTransfer::Anchor);
        m.wall_plates.push(plate.clone());
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");

        let loads = attached_wall_beam_loads(&m).unwrap();
        assert_eq!(loads.len(), 1);
        let bl = &loads[0];
        assert_eq!(
            bl.target,
            LoadTarget::Span {
                nodes: [NodeId(0), NodeId(1)],
                t: span,
            }
        );
        let LoadShape::Uniform { w } = bl.shape else {
            panic!("Uniform を期待");
        };
        // 実際に覆う長さは全長4000mmの半分（span 0.25〜0.75）。
        let len = 4000.0 * (span[1] - span[0]);
        assert!(
            (w * len - total).abs() / total < 1e-9,
            "w×len={} total={}",
            w * len,
            total
        );
    }

    /// `Columns`（集中）・全区間: 取付き線両端の柱2本へ半分ずつ。
    #[test]
    fn columns_transfer_full_span_splits_evenly() {
        let mut m = model_with_line_anchor_nodes();
        let plate = line_attached_plate([0.0, 1.0], [1000.0, 1000.0], LoadTransfer::Columns);
        m.wall_plates.push(plate.clone());
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");

        let loads = attached_wall_beam_loads(&m).unwrap();
        assert_eq!(loads.len(), 2);
        for bl in &loads {
            let LoadTarget::Node(n) = bl.target else {
                panic!("Node を期待");
            };
            assert!(n == NodeId(0) || n == NodeId(1));
            let LoadShape::Point { p, .. } = bl.shape else {
                panic!("Point を期待");
            };
            assert!((p - total / 2.0).abs() / total < 1e-9);
        }
    }

    /// `Columns`（集中）・偏った区間中点: 単純梁反力按分（`t_mid` から離れた側が少ない）。
    #[test]
    fn columns_transfer_uses_span_midpoint_ratio() {
        let mut m = model_with_line_anchor_nodes();
        let span = [0.0, 0.5]; // t_mid = 0.25
        let plate = line_attached_plate(span, [1000.0, 1000.0], LoadTransfer::Columns);
        m.wall_plates.push(plate.clone());
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");

        let loads = attached_wall_beam_loads(&m).unwrap();
        assert_eq!(loads.len(), 2);
        let get = |n: NodeId| -> f64 {
            loads
                .iter()
                .find(|bl| bl.target == LoadTarget::Node(n))
                .map(|bl| match bl.shape {
                    LoadShape::Point { p, .. } => p,
                    _ => panic!("Point を期待"),
                })
                .unwrap()
        };
        let (r0, r1) = (get(NodeId(0)), get(NodeId(1)));
        assert!((r0 - total * 0.75).abs() / total < 1e-9, "r0={r0}");
        assert!((r1 - total * 0.25).abs() / total < 1e-9, "r1={r1}");
    }

    /// 台形の壁（`extent[0] != extent[1]`）は高さに比例する線形分布・面積重心按分。
    /// 総重量も保存する。
    #[test]
    fn asymmetric_extent_uses_height_proportional_linear_and_centroid() {
        let mut m = model_with_line_anchor_nodes();
        let extent = [500.0, 1500.0]; // 台形（始端 500mm・終端 1500mm）。
        let h0 = 500.0;
        let h1 = 1500.0;
        let len = 4000.0;
        let s = (h0 + 2.0 * h1) / (3.0 * (h0 + h1)); // 7/12

        let anchor_plate = line_attached_plate([0.0, 1.0], extent, LoadTransfer::Anchor);
        let anchor_total = m
            .wall_plate_self_weight(&anchor_plate, &m)
            .expect("自重が求まる");
        m.wall_plates.push(anchor_plate);
        let anchor_loads = attached_wall_beam_loads(&m).unwrap();
        assert_eq!(anchor_loads.len(), 1);
        let LoadShape::Linear { w_i, w_j } = anchor_loads[0].shape else {
            panic!("Linear を期待");
        };
        assert!(
            (w_i / w_j - h0 / h1).abs() < 1e-12,
            "強度比が張り出し高さ比と一致しない: w_i={w_i} w_j={w_j}"
        );
        let integral = len * (w_i + w_j) / 2.0;
        assert!(
            (integral - anchor_total).abs() / anchor_total < 1e-9,
            "台形でも総重量は保存されるはず: integral={integral} total={anchor_total}"
        );

        m.wall_plates.clear();
        let columns_plate = line_attached_plate([0.0, 1.0], extent, LoadTransfer::Columns);
        let columns_total = m
            .wall_plate_self_weight(&columns_plate, &m)
            .expect("自重が求まる");
        m.wall_plates.push(columns_plate);
        let columns_loads = attached_wall_beam_loads(&m).unwrap();
        let get = |n: NodeId| -> f64 {
            columns_loads
                .iter()
                .find(|bl| bl.target == LoadTarget::Node(n))
                .map(|bl| match bl.shape {
                    LoadShape::Point { p, .. } => p,
                    _ => panic!("Point を期待"),
                })
                .unwrap()
        };
        let (r0, r1) = (get(NodeId(0)), get(NodeId(1)));
        assert!(
            (r0 - columns_total * (1.0 - s)).abs() / columns_total < 1e-9,
            "r0={r0}"
        );
        assert!(
            (r1 - columns_total * s).abs() / columns_total < 1e-9,
            "r1={r1}"
        );
        assert!((r0 + r1 - columns_total).abs() / columns_total < 1e-9);
    }

    /// 台形＋部分区間: 重心は区間内の相対位置に置く（区間中点ではない）。
    #[test]
    fn trapezoid_columns_partial_span_uses_centroid_in_span() {
        let mut m = model_with_line_anchor_nodes();
        let span = [0.0, 0.5];
        let extent = [500.0, 1500.0];
        let s = (500.0 + 2.0 * 1500.0) / (3.0 * 2000.0); // 7/12
        let t = span[0] + (span[1] - span[0]) * s; // 7/24
        let plate = line_attached_plate(span, extent, LoadTransfer::Columns);
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");
        m.wall_plates.push(plate);
        let loads = attached_wall_beam_loads(&m).unwrap();
        let get = |n: NodeId| -> f64 {
            loads
                .iter()
                .find(|bl| bl.target == LoadTarget::Node(n))
                .map(|bl| match bl.shape {
                    LoadShape::Point { p, .. } => p,
                    _ => panic!("Point を期待"),
                })
                .unwrap()
        };
        let (r0, r1) = (get(NodeId(0)), get(NodeId(1)));
        assert!(
            (r0 - total * (1.0 - t)).abs() / total < 1e-9,
            "r0={r0} t={t}"
        );
        assert!((r1 - total * t).abs() / total < 1e-9, "r1={r1}");
    }

    /// 自立壁が載る床領域を作る。境界は Z=3000（壁の節点と同じレベル）の矩形で、
    /// `slab` が真なら床板を 1 枚持たせる。`z_far` は奥側 2 点の Z（傾斜床の検証用）。
    fn push_floor_region(m: &mut Model, x: [f64; 2], id: u32, slab: bool, z_far: f64) {
        let n0 = m.nodes.len() as u32;
        for (dx, y, z) in [
            (x[0], -2000.0, 3000.0),
            (x[1], -2000.0, 3000.0),
            (x[1], 2000.0, z_far),
            (x[0], 2000.0, z_far),
        ] {
            m.nodes.push(mk_node(m.nodes.len() as u32, dx, y, z));
        }
        let boundary = vec![NodeId(n0), NodeId(n0 + 1), NodeId(n0 + 2), NodeId(n0 + 3)];
        let mut region = FloorRegion::new(FloorRegionId(id), boundary.clone());
        if slab {
            let sid = m.add_enclosed_slab_from_nodes(
                &boundary,
                SlabPlate {
                    section: None,
                    loads: Vec::new(),
                    usage: None,
                    method: DistributionMethod::TriTrapezoid,
                    one_way: None,
                },
            );
            region.slab_ids.push(sid);
        }
        m.floor_regions.push(region);
    }

    /// 自立壁の壁版（節点 0-1 の間、Z=3000）。
    fn self_standing_plate() -> WallPlate {
        WallPlate {
            dl_support: None,
            self_weight_shares: Vec::new(),
            id: sepika_core::ids::WallPlateId(0),
            shape: WallPlateShape::Attached {
                anchor: RegionAnchor::FloorRegion {
                    nodes: [NodeId(0), NodeId(1)],
                },
                extent: Some([1000.0, 1000.0]),
            },
            section: Some(SectionId(0)),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: vec![],
            slit: Default::default(),
        }
    }

    /// 「床領域」アンカー（自立壁）: 床板を持つ床領域では、床領域内の全床板へ
    /// 同一の追加強度（総重量÷床板合計面積）が上乗せされる。
    #[test]
    fn floor_region_positioned_opening_changes_coverage_not_story_band_rule() {
        let mut m = model_with_line_anchor_nodes();
        m.sections[0].thickness = Some(200.0);
        m.materials[0].density = 24e-6 / sepika_core::units::GRAVITY_MM_S2;
        push_floor_region(&mut m, [0.0, 2000.0], 0, true, 3000.0);
        push_floor_region(&mut m, [2000.0, 4000.0], 1, true, 3000.0);
        let mut plate = self_standing_plate();
        plate.openings = vec![sepika_core::model::WallOpening {
            width: 1000.0,
            height: 500.0,
            offset: Some([500.0, 250.0]),
        }];
        m.wall_plates.push(plate);
        let weight = m.wall_weight(&m.wall_plates[0]).unwrap();
        assert!((weight.totals.design_n - 16800.0).abs() < 1e-7);
        assert!((weight.totals.physical_n - 16800.0).abs() < 1e-7);
        assert_eq!(weight.totals.matrix_n, 0.0);
        assert!((weight.band(3000.0, 4500.0).unwrap().design_n - 16800.0).abs() < 1e-7);
        assert_eq!(weight.band(4500.0, 6000.0).unwrap().design_n, 0.0);
        let extra = floor_region_wall_extra_intensity(&m).unwrap();
        assert!((extra[&SlabId(0)] * 8_000_000.0 - 7200.0).abs() < 1e-7);
        assert!((extra[&SlabId(1)] * 8_000_000.0 - 9600.0).abs() < 1e-7);
    }

    #[test]
    fn floor_region_sign_reversal_positioned_opening_uses_both_components() {
        for (x, z, expected) in [
            (3000.0, 1250.0, [4800.0, 4320.0]),
            (500.0, 550.0, [4320.0, 4800.0]),
        ] {
            let mut m = model_with_line_anchor_nodes();
            m.sections[0].thickness = Some(200.0);
            m.materials[0].density = 24e-6 / sepika_core::units::GRAVITY_MM_S2;
            push_floor_region(&mut m, [0.0, 2000.0], 0, true, 3000.0);
            push_floor_region(&mut m, [2000.0, 4000.0], 1, true, 3000.0);
            let mut plate = self_standing_plate();
            if let WallPlateShape::Attached { extent, .. } = &mut plate.shape {
                *extent = Some([-1000.0, 1000.0]);
            }
            plate.openings = vec![sepika_core::model::WallOpening {
                width: 500.0,
                height: 200.0,
                offset: Some([x, z]),
            }];
            m.wall_plates.push(plate);
            let w = m.wall_weight(&m.wall_plates[0]).unwrap();
            assert!((w.totals.design_n - 9120.0).abs() < 1e-7);
            assert!((w.band(1500.0, 4500.0).unwrap().design_n - 9120.0).abs() < 1e-7);
            let extra = floor_region_wall_extra_intensity(&m).unwrap();
            for (i, expected) in expected.into_iter().enumerate() {
                assert!((extra[&SlabId(i as u32)] * 8_000_000.0 - expected).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn floor_region_anchor_adds_equivalent_intensity_to_slabs() {
        let mut m = model_with_line_anchor_nodes();
        // 壁（X=0..4000）を完全に含む床領域（X=-1000..5000）。
        push_floor_region(&mut m, [-1000.0, 5000.0], 0, true, 3000.0);
        let plate = self_standing_plate();
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");
        m.wall_plates.push(plate);

        let extra = floor_region_wall_extra_intensity(&m).unwrap();
        let dw = extra.get(&SlabId(0)).copied().expect("床板への追加強度");
        let slab_area = 6000.0 * 4000.0;
        assert!(
            (dw - total / slab_area).abs() / (total / slab_area) < 1e-9,
            "dw={dw}"
        );
    }

    /// 床領域をまたぐ自立壁は境界で内部分割し、それぞれの床領域の床板へ配る。
    /// 総重量は保存する。
    #[test]
    fn floor_region_anchor_splits_across_regions() {
        let mut m = model_with_line_anchor_nodes();
        // 壁は X=0..4000。床領域を X=-1000..2000 と X=2000..5000 に置き、
        // 壁は X=2000 で 2:2 に分かれる。
        push_floor_region(&mut m, [-1000.0, 2000.0], 0, true, 3000.0);
        push_floor_region(&mut m, [2000.0, 5000.0], 1, true, 3000.0);
        let plate = self_standing_plate();
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");
        m.wall_plates.push(plate);

        let extra = floor_region_wall_extra_intensity(&m).unwrap();
        assert_eq!(extra.len(), 2, "両方の床板へ配ること: {extra:?}");
        // 各床板の面積は 3000×4000。強度×面積の和が総重量に一致する。
        let sum: f64 = extra.values().map(|dw| dw * 3000.0 * 4000.0).sum();
        assert!(
            (sum - total).abs() / total < 1e-9,
            "総重量を保存すること sum={sum} total={total}"
        );
        // 半分ずつなので強度も等しい。
        let v: Vec<f64> = extra.values().copied().collect();
        assert!((v[0] - v[1]).abs() / v[0] < 1e-9);
    }

    /// 両端で立ち上がり高さの符号が反転する自立壁でも、配分の総和は自重と一致する。
    #[test]
    fn floor_region_sign_reversal_conserves_total_weight() {
        let mut m = model_with_line_anchor_nodes();
        push_floor_region(&mut m, [-1000.0, 2000.0], 0, true, 3000.0);
        push_floor_region(&mut m, [2000.0, 5000.0], 1, true, 3000.0);
        let mut plate = self_standing_plate();
        if let WallPlateShape::Attached { extent, .. } = &mut plate.shape {
            *extent = Some([2000.0, -2000.0]);
        }
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");
        assert!(total > 0.0, "符号反転でも面積は正: {total}");
        m.wall_plates.push(plate);

        let extra = floor_region_wall_extra_intensity(&m).unwrap();
        let sum: f64 = extra.values().map(|dw| dw * 3000.0 * 4000.0).sum();
        assert!(
            (sum - total).abs() / total < 1e-9,
            "符号反転でも総重量を保存すること sum={sum} total={total} extra={extra:?}"
        );
    }

    /// 版なし床領域（床板 0 枚）は「力を流す先が無い」ため覆いとみなさず、
    /// 分配しない（フォールバックもしない。解析前チェックが止める）。
    #[test]
    fn floor_region_without_slabs_distributes_nothing() {
        let mut m = model_with_line_anchor_nodes();
        push_floor_region(&mut m, [-1000.0, 5000.0], 0, false, 3000.0);
        let plate = self_standing_plate();
        m.wall_plates.push(plate);

        let extra = floor_region_wall_extra_intensity(&m).unwrap();
        assert!(
            extra.is_empty(),
            "床板が無ければ分配しない（危険側のフォールバックはしない）: {extra:?}"
        );
    }

    /// 床領域の外へはみ出す自立壁は、覆われている分だけを配る。
    #[test]
    fn floor_region_anchor_distributes_only_covered_part() {
        let mut m = model_with_line_anchor_nodes();
        // 壁は X=0..4000。床領域は X=-1000..2000 だけなので半分がはみ出す。
        push_floor_region(&mut m, [-1000.0, 2000.0], 0, true, 3000.0);
        let plate = self_standing_plate();
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");
        m.wall_plates.push(plate);

        let extra = floor_region_wall_extra_intensity(&m).unwrap();
        let dw = extra.get(&SlabId(0)).copied().expect("追加強度");
        let carried = dw * 3000.0 * 4000.0;
        assert!(
            (carried - total * 0.5).abs() / total < 1e-9,
            "覆われた半分だけを配ること carried={carried} total={total}"
        );
    }

    /// 床分配は XY 投影面積に強度を掛ける。追加強度の分母も同じ面積でないと
    /// 傾斜床で総重量が縮小する（危険側）。
    #[test]
    fn floor_region_extra_intensity_uses_xy_area_so_distribution_conserves() {
        let mut m = model_with_line_anchor_nodes();
        // 奥側 2 点を Z=3000 から持ち上げ、XY 面積は変えずに 3 次元面積だけ大きくする。
        // レベル判定は境界節点の Z の平均なので、平均が 3000 のままになるよう
        // 手前 2 点を下げる（±500）。
        let n0 = m.nodes.len() as u32;
        for (x, y, z) in [
            (-1000.0, -2000.0, 2500.0),
            (5000.0, -2000.0, 2500.0),
            (5000.0, 2000.0, 3500.0),
            (-1000.0, 2000.0, 3500.0),
        ] {
            m.nodes.push(mk_node(m.nodes.len() as u32, x, y, z));
        }
        let boundary = vec![NodeId(n0), NodeId(n0 + 1), NodeId(n0 + 2), NodeId(n0 + 3)];
        let sid = m.add_enclosed_slab_from_nodes(
            &boundary,
            SlabPlate {
                section: None,
                loads: Vec::new(),
                usage: None,
                method: DistributionMethod::TriTrapezoid,
                one_way: None,
            },
        );
        let mut region = FloorRegion::new(FloorRegionId(0), boundary.clone());
        region.slab_ids.push(sid);
        m.floor_regions.push(region);

        let plate = self_standing_plate();
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");
        m.wall_plates.push(plate);

        let extra = floor_region_wall_extra_intensity(&m).unwrap();
        let dw = extra.get(&SlabId(0)).copied().expect("追加強度");
        let xy_area = 6000.0 * 4000.0;
        assert!(
            (dw * xy_area - total).abs() / total < 1e-9,
            "dw×A_xy={} total={}（3次元面積で割ると縮小する）",
            dw * xy_area,
            total
        );
    }

    #[test]
    fn accumulate_dl_reference_weight_conserves_line_and_floor_region() {
        let mut m = model_with_line_anchor_nodes();
        let plate = line_attached_plate([0.0, 0.5], [1000.0, 1000.0], LoadTransfer::Anchor);
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");
        m.wall_plates.push(plate);
        let mut nw = vec![0.0; m.nodes.len()];
        accumulate_attached_wall_dl_weight(&m, &mut nw).unwrap();
        assert!((nw[0] - total * 0.75).abs() / total < 1e-9);
        assert!((nw[1] - total * 0.25).abs() / total < 1e-9);
    }

    #[test]
    fn accumulate_dl_reference_weight_uses_trapezoid_centroid() {
        let mut m = model_with_line_anchor_nodes();
        let plate = line_attached_plate([0.0, 1.0], [500.0, 1500.0], LoadTransfer::Anchor);
        let total = m.wall_plate_self_weight(&plate, &m).expect("自重が求まる");
        m.wall_plates.push(plate);
        let s = (500.0 + 2.0 * 1500.0) / (3.0 * 2000.0);
        let mut nw = vec![0.0; m.nodes.len()];
        accumulate_attached_wall_dl_weight(&m, &mut nw).unwrap();
        assert!((nw[0] - total * (1.0 - s)).abs() / total < 1e-9);
        assert!((nw[1] - total * s).abs() / total < 1e-9);
    }
}
