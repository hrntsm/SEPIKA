//! 床面荷重の支持部材への分配と、重力荷重ケース内容への変換。

use crate::floor::{self, BeamLoad, LoadShape, LoadTarget};
use crate::secondary::{beam_span_position, resolve_nodal_to_primary, SPAN_TOL_MM};
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{ElementKind, MemberLoad, MemberLoadKind, Model, NodalLoad, Slab};
use std::collections::HashMap;

/// 節点対 (min,max) → 実 `Beam`（2 節点）要素の `ElemId` を引く索引。
pub fn beam_elem_map(model: &Model) -> HashMap<(NodeId, NodeId), ElemId> {
    let mut map = HashMap::new();
    for e in &model.elements {
        if e.kind == ElementKind::Beam && e.nodes.len() == 2 {
            map.entry(floor::span_node_key(e.nodes[0], e.nodes[1]))
                .or_insert(e.id);
        }
    }
    map
}

/// `distribute_region`/`distribute_slab_resolved` が返す `BeamLoad`を
/// 実部材へ解決して `out` へ積む。
fn push_resolved_loads(
    loads: Vec<BeamLoad>,
    beam_map: &HashMap<(NodeId, NodeId), ElemId>,
    out: &mut Vec<BeamLoad>,
) {
    let find_beam = |n0: NodeId, n1: NodeId| -> Option<ElemId> {
        beam_map.get(&floor::span_node_key(n0, n1)).copied()
    };
    for mut bl in loads {
        match bl.target {
            LoadTarget::Node(_) => out.push(bl),
            LoadTarget::Edge(_) => {
                continue;
            }
            LoadTarget::Span {
                nodes: [n0, n1], ..
            } => {
                if let Some(elem) = find_beam(n0, n1) {
                    bl.elem = elem;
                }
                out.push(bl);
            }
            LoadTarget::Secondary { .. } => out.push(bl),
        }
    }
}

/// 各床板について面荷重強度 `w_of(slab)` を境界へ分配し、`BeamLoad` 列を返す。
/// `w_of` は床板ごとの面荷重強度 [N/mm²]。
///
/// 分配は全床板（床領域に属する床板と取り付く床板の両方）を二次部材の逐次伝達
/// （[`crate::cascade`]）へ通す。床板の境界辺荷重のうち二次部材が受け持った
/// ぶんは反力として主架構へ渡り、残り（主架構の辺荷重）だけをここで解決する。
pub fn slab_beam_loads_with(
    model: &Model,
    w_of: impl Fn(&Slab) -> f64,
    include_secondary_self_weight: bool,
    beam_map: &HashMap<(NodeId, NodeId), ElemId>,
) -> Result<Vec<BeamLoad>, crate::floor::FloorDistributionError> {
    let transfer = crate::cascade::solve(model, &w_of, include_secondary_self_weight)?;
    Ok(transfer_loads(model, transfer, beam_map))
}

fn transfer_loads(
    model: &Model,
    mut transfer: crate::cascade::SecondaryTransfer,
    beam_map: &HashMap<(NodeId, NodeId), ElemId>,
) -> Vec<BeamLoad> {
    let mut beam_loads = Vec::new();
    push_resolved_loads(
        std::mem::take(&mut transfer.leftover_region_loads),
        beam_map,
        &mut beam_loads,
    );

    let (nodal_reactions, member_reactions) = transfer.primary_loads(model);
    for (node, r) in nodal_reactions {
        beam_loads.push(BeamLoad {
            elem: ElemId(u32::MAX),
            target: LoadTarget::Node(node),
            shape: LoadShape::Point { p: r, x: 0.0 },
            cmq: floor::Cmq {
                c_i: 0.0,
                c_j: 0.0,
                q_i: r,
                q_j: 0.0,
            },
        });
    }
    beam_loads.extend(member_reactions);

    beam_loads
}

/// `BeamLoad` 列を荷重ケースへ書き込める `NodalLoad`/`MemberLoad` へ変換する。
pub fn slab_load_case_content(
    model: &Model,
    beam_loads: &[BeamLoad],
) -> (Vec<NodalLoad>, Vec<MemberLoad>) {
    const DIR: [f64; 3] = [0.0, 0.0, -1.0];
    let mut nodal = Vec::new();
    let mut member = Vec::new();

    fn push_dist(member: &mut Vec<MemberLoad>, elem: ElemId, a: f64, b: f64, w1: f64, w2: f64) {
        if b - a <= 1e-9 {
            return;
        }
        member.push(MemberLoad::auto(
            elem,
            DIR,
            MemberLoadKind::Distributed { a, b, w1, w2 },
        ));
    }

    fn emit_shape(
        member: &mut Vec<MemberLoad>,
        elem: ElemId,
        a0: f64,
        len_e: f64,
        flip: bool,
        shape: &LoadShape,
    ) {
        match *shape {
            LoadShape::Uniform { w } => push_dist(member, elem, a0, a0 + len_e, w, w),
            LoadShape::Linear { w_i, w_j } => {
                let (w1, w2) = if flip { (w_j, w_i) } else { (w_i, w_j) };
                push_dist(member, elem, a0, a0 + len_e, w1, w2);
            }
            LoadShape::Triangle { w0 } => {
                let mid = len_e / 2.0;
                push_dist(member, elem, a0, a0 + mid, 0.0, w0);
                push_dist(member, elem, a0 + mid, a0 + len_e, w0, 0.0);
            }
            LoadShape::Trapezoid { w0, a, b } => {
                push_dist(member, elem, a0, a0 + a, 0.0, w0);
                push_dist(member, elem, a0 + a, a0 + a + b, w0, w0);
                push_dist(member, elem, a0 + a + b, a0 + len_e, w0, 0.0);
            }
            LoadShape::Point { p, x } => {
                let xx = if flip { len_e - x } else { x };
                member.push(MemberLoad::auto(
                    elem,
                    DIR,
                    MemberLoadKind::Point { a: a0 + xx, p },
                ));
            }
        }
    }

    /// 線分に沿った荷重強度 [N/mm]（線分始点からの距離 `x`）。`Point` は強度を持たない。
    fn line_intensity(shape: &LoadShape, len: f64, x: f64) -> f64 {
        match *shape {
            LoadShape::Uniform { w } => w,
            LoadShape::Linear { w_i, w_j } => {
                if len <= 1e-9 {
                    w_i
                } else {
                    w_i + (w_j - w_i) * (x / len)
                }
            }
            LoadShape::Triangle { w0 } => {
                let mid = len / 2.0;
                if x <= mid {
                    if mid <= 1e-9 {
                        0.0
                    } else {
                        w0 * (x / mid)
                    }
                } else if len - mid <= 1e-9 {
                    0.0
                } else {
                    w0 * ((len - x) / (len - mid))
                }
            }
            LoadShape::Trapezoid { w0, a, b } => {
                if x < a {
                    if a <= 1e-9 {
                        w0
                    } else {
                        w0 * (x / a)
                    }
                } else if x <= a + b {
                    w0
                } else {
                    let tail = len - (a + b);
                    if tail <= 1e-9 {
                        w0
                    } else {
                        w0 * ((len - x) / tail)
                    }
                }
            }
            LoadShape::Point { .. } => 0.0,
        }
    }

    /// 荷重強度が折れる位置（線分始点からの距離）。区間をここで割ると各片が線形になる。
    fn shape_breakpoints(shape: &LoadShape, len: f64) -> Vec<f64> {
        match *shape {
            LoadShape::Uniform { .. } | LoadShape::Linear { .. } => Vec::new(),
            LoadShape::Triangle { .. } => vec![len / 2.0],
            LoadShape::Trapezoid { a, b, .. } => vec![a, a + b],
            LoadShape::Point { x, .. } => vec![x],
        }
    }

    /// 線分 `p0`→`p1` に載る荷重を、それを覆う大梁へ割り付ける。
    /// 線分の全長を覆えなかった場合は `false` を返す。
    fn emit_along_segment(
        model: &Model,
        member: &mut Vec<MemberLoad>,
        p0: [f64; 3],
        p1: [f64; 3],
        shape: &LoadShape,
    ) -> bool {
        let len = {
            let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        };
        if len <= 1e-9 {
            return false;
        }
        let cover = crate::secondary::beams_along_segment(model, p0, p1, SPAN_TOL_MM);
        if cover.is_empty() {
            return false;
        }
        if !crate::secondary::coverage_covers_full(&cover, len, SPAN_TOL_MM) {
            return false;
        }

        let mut breaks = shape_breakpoints(shape, len);
        breaks.retain(|x| *x > 0.0 && *x < len);
        let mut point_placed = false;
        for c in &cover {
            let mut cuts = vec![c.seg[0], c.seg[1]];
            cuts.extend(
                breaks
                    .iter()
                    .copied()
                    .filter(|x| *x > c.seg[0] && *x < c.seg[1]),
            );
            cuts.sort_by(f64::total_cmp);
            let span = c.seg[1] - c.seg[0];
            let to_elem = |x: f64| -> f64 {
                if span <= 1e-9 {
                    c.elem_pos[0]
                } else {
                    c.elem_pos[0] + (x - c.seg[0]) / span * (c.elem_pos[1] - c.elem_pos[0])
                }
            };
            if let LoadShape::Point { p, x } = *shape {
                if !point_placed && x >= c.seg[0] && x <= c.seg[1] {
                    member.push(MemberLoad::auto(
                        c.elem,
                        DIR,
                        MemberLoadKind::Point { a: to_elem(x), p },
                    ));
                    point_placed = true;
                }
                continue;
            }
            for w in cuts.windows(2) {
                let (x0, x1) = (w[0], w[1]);
                if x1 - x0 <= 1e-9 {
                    continue;
                }
                let (mut a, mut b) = (to_elem(x0), to_elem(x1));
                let (mut w1, mut w2) = (
                    line_intensity(shape, len, x0),
                    line_intensity(shape, len, x1),
                );
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                    std::mem::swap(&mut w1, &mut w2);
                }
                push_dist(member, c.elem, a, b, w1, w2);
            }
        }
        true
    }

    fn simple_reactions(shape: &LoadShape, len: f64) -> (f64, f64) {
        match *shape {
            LoadShape::Uniform { w } => {
                let total = w * len;
                (total / 2.0, total / 2.0)
            }
            LoadShape::Linear { w_i, w_j } => {
                (len * (2.0 * w_i + w_j) / 6.0, len * (w_i + 2.0 * w_j) / 6.0)
            }
            LoadShape::Triangle { w0 } => {
                let total = w0 * len / 2.0;
                (total / 2.0, total / 2.0)
            }
            LoadShape::Trapezoid { w0, a, b } => {
                let total = w0 * (a + b);
                (total / 2.0, total / 2.0)
            }
            LoadShape::Point { p, x } => {
                if len <= 1e-9 {
                    (p / 2.0, p / 2.0)
                } else {
                    let t = (x / len).clamp(0.0, 1.0);
                    (p * (1.0 - t), p * t)
                }
            }
        }
    }

    for bl in beam_loads {
        match bl.target {
            LoadTarget::Node(n) => {
                let LoadShape::Point { p, .. } = bl.shape else {
                    continue;
                };
                nodal.push(NodalLoad::auto(n, [0.0, 0.0, -p, 0.0, 0.0, 0.0]));
            }
            LoadTarget::Edge(_) => {
                let Some(elem) = model.element(bl.elem) else {
                    continue;
                };
                let l = model.member_length(elem);
                if l <= 1e-9 {
                    continue;
                }
                emit_shape(&mut member, elem.id, 0.0, l, false, &bl.shape);
            }
            LoadTarget::Span { nodes: [n0, n1], t } => {
                if let Some(elem) = model.element(bl.elem) {
                    let length = model.member_length(elem);
                    let a = t[0] * length;
                    let b = t[1] * length;
                    emit_shape(
                        &mut member,
                        elem.id,
                        a.min(b),
                        (a - b).abs(),
                        a > b,
                        &bl.shape,
                    );
                    continue;
                }
                let full = (t[0] - 0.0).abs() <= 1e-9 && (t[1] - 1.0).abs() <= 1e-9;
                let (Some(node0), Some(node1)) =
                    (model.nodes.get(n0.index()), model.nodes.get(n1.index()))
                else {
                    continue;
                };
                let lerp = |c0: [f64; 3], c1: [f64; 3], f: f64| {
                    [
                        c0[0] + (c1[0] - c0[0]) * f,
                        c0[1] + (c1[1] - c0[1]) * f,
                        c0[2] + (c1[2] - c0[2]) * f,
                    ]
                };
                let (p0, p1) = if full {
                    (node0.coord, node1.coord)
                } else {
                    (
                        lerp(node0.coord, node1.coord, t[0]),
                        lerp(node0.coord, node1.coord, t[1]),
                    )
                };
                let hit0 = beam_span_position(model, p0, SPAN_TOL_MM);
                let hit1 = beam_span_position(model, p1, SPAN_TOL_MM);
                if let (Some((e0, a0)), Some((e1, a1))) = (hit0, hit1) {
                    if e0 == e1 {
                        let start = a0.min(a1);
                        let len_e = (a1 - a0).abs();
                        if len_e > 1e-9 {
                            emit_shape(&mut member, e0, start, len_e, a0 > a1, &bl.shape);
                        }
                        continue;
                    }
                }
                if emit_along_segment(model, &mut member, p0, p1, &bl.shape) {
                    continue;
                }
                let len = {
                    let (a, b) = (p0, p1);
                    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
                };
                let (r0, r1) = simple_reactions(&bl.shape, len);
                for (n, r) in [(n0, r0), (n1, r1)] {
                    if r.abs() > 1e-9 {
                        nodal.push(NodalLoad::auto(n, [0.0, 0.0, -r, 0.0, 0.0, 0.0]));
                    }
                }
            }
            LoadTarget::Secondary { member: sm_id, t } => {
                let Some(sm) = model.secondary_member(sm_id) else {
                    continue;
                };
                let Some((a, b, _)) = model.secondary_member_axis(sm) else {
                    continue;
                };
                let lerp = |c0: [f64; 3], c1: [f64; 3], f: f64| {
                    [
                        c0[0] + (c1[0] - c0[0]) * f,
                        c0[1] + (c1[1] - c0[1]) * f,
                        c0[2] + (c1[2] - c0[2]) * f,
                    ]
                };
                let p0 = lerp(a, b, t[0]);
                let p1 = lerp(a, b, t[1]);
                let hit0 = beam_span_position(model, p0, SPAN_TOL_MM);
                let hit1 = beam_span_position(model, p1, SPAN_TOL_MM);
                if let (Some((e0, a0)), Some((e1, a1))) = (hit0, hit1) {
                    if e0 == e1 {
                        let start = a0.min(a1);
                        let len_e = (a1 - a0).abs();
                        if len_e > 1e-9 {
                            emit_shape(&mut member, e0, start, len_e, a0 > a1, &bl.shape);
                        }
                        continue;
                    }
                }
                if emit_along_segment(model, &mut member, p0, p1, &bl.shape) {
                    continue;
                }
                let find = |p: [f64; 3]| {
                    model
                        .nodes
                        .iter()
                        .find(|n| sepika_core::geom::vec3::dist(n.coord, p) <= SPAN_TOL_MM)
                        .map(|n| n.id)
                };
                let (Some(n0), Some(n1)) = (find(p0), find(p1)) else {
                    continue;
                };
                let len = sepika_core::geom::vec3::dist(p0, p1);
                let (r0, r1) = simple_reactions(&bl.shape, len);
                for (n, r) in [(n0, r0), (n1, r1)] {
                    if r.abs() > 1e-9 {
                        nodal.push(NodalLoad::auto(n, [0.0, 0.0, -r, 0.0, 0.0, 0.0]));
                    }
                }
            }
        }
    }

    let (nodal, extra_member) = resolve_nodal_to_primary(model, nodal, SPAN_TOL_MM);
    member.extend(extra_member);
    (nodal, member)
}

/// 材料密度から床板本体の物理重量 `[N]` を支持部材へ分配する。
/// 設計単位重量の採用可否とは独立。参照・密度・形状・二次部材の支持異常はエラー。
pub fn physical_self_weight_loads(
    model: &Model,
) -> Result<Vec<BeamLoad>, crate::floor::FloorDistributionError> {
    let mut intensity = HashMap::new();
    for slab in &model.slabs {
        let w = model
            .slab_physical_self_weight_intensity(slab)
            .map_err(crate::floor::FloorDistributionError::SelfWeight)?;
        intensity.insert(slab.id, w);
    }
    let transfer = crate::cascade::solve(model, |s| intensity[&s.id], false)?;
    if !transfer.invalid_end_shares.is_empty()
        || !transfer.unresolved.is_empty()
        || !transfer.cyclic.is_empty()
    {
        return Err(crate::floor::FloorDistributionError::SelfWeight(
            "二次部材の端部負担率または支持先が不正で、床板荷重を伝えられません".into(),
        ));
    }
    Ok(transfer_loads(model, transfer, &beam_elem_map(model)))
}

pub(crate) fn accumulate_synced_self_weight(
    model: &Model,
    design: &mut [f64],
    physical: &mut [f64],
) -> Result<(), String> {
    for slab in &model.slabs {
        crate::floor::distribute_slab(model, slab).map_err(|e| e.to_string())?;
    }
    let design_loads = slab_beam_loads_with(
        model,
        |s| {
            model
                .slab_self_weight_intensity(s)
                .expect("検証済みの床板設計自重")
        },
        false,
        &beam_elem_map(model),
    )
    .map_err(|e| e.to_string())?;
    let physical_loads = physical_self_weight_loads(model).map_err(|e| e.to_string())?;
    for (loads, target) in [(&design_loads, design), (&physical_loads, physical)] {
        let (nodal, member) = slab_load_case_content(model, loads);
        for load in nodal {
            target[load.node.index()] -= load.values[2];
        }
        for load in member {
            let element = model
                .element(load.elem)
                .ok_or("床板の支持部材を解決できません")?;
            let (ri, rj) = crate::floor::simple_reactions(&load.kind, model.member_length(element));
            target[element.nodes[0].index()] += ri;
            target[element.nodes[1].index()] += rj;
        }
    }
    Ok(())
}
