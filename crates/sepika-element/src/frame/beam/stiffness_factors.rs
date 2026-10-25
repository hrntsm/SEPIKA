//! スラブ協力幅・合成梁・壁エレメント上下大梁による剛性の増大率算定。

use sepika_core::ids::NodeId;
use sepika_core::model::Model;

/// スラブ協力幅 bf = b + ba(左) + ba(右) [mm]。
///
/// 対象は水平材のみ。適用不能時は None。
fn slab_cooperating_width(
    model: &Model,
    data: &sepika_core::model::ElementData,
    b: f64,
) -> Option<(f64, f64)> {
    if b <= 0.0
        || data.nodes.len() < 2
        || (model.floor_regions.is_empty() && model.slabs.is_empty())
    {
        return None;
    }
    let n0 = data.nodes[0];
    let n1 = data.nodes[data.nodes.len() - 1];
    let (Some(node0), Some(node1)) = (model.nodes.get(n0.index()), model.nodes.get(n1.index()))
    else {
        return None;
    };
    let (p0, p1) = (node0.coord, node1.coord);
    let (dx, dy, dz) = (p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]);
    let lp = (dx * dx + dy * dy).sqrt();
    if lp < 1e-9 || dz.abs() > 0.05 * lp {
        return None;
    }
    let l = (lp * lp + dz * dz).sqrt();
    let (ex, ey) = (dx / lp, dy / lp);
    let signed_dist =
        |coord: [f64; 3]| -> f64 { -(coord[0] - p0[0]) * ey + (coord[1] - p0[1]) * ex };

    let far_beam_width = |boundary: &[sepika_core::ids::NodeId], target_s: f64, sign: f64| -> f64 {
        const TOL_MM: f64 = 1.0;
        for e in &model.elements {
            if !matches!(e.kind, sepika_core::model::ElementKind::Beam) || e.nodes.len() < 2 {
                continue;
            }
            let (m0, m1) = (e.nodes[0], e.nodes[e.nodes.len() - 1]);
            if m0 == n0 && m1 == n1 || m0 == n1 && m1 == n0 {
                continue;
            }
            if !(boundary.contains(&m0) && boundary.contains(&m1)) {
                continue;
            }
            let (Some(q0), Some(q1)) = (model.nodes.get(m0.index()), model.nodes.get(m1.index()))
            else {
                continue;
            };
            let s0 = signed_dist(q0.coord) * sign;
            let s1 = signed_dist(q1.coord) * sign;
            if (s0 - target_s).abs() > TOL_MM || (s1 - target_s).abs() > TOL_MM {
                continue;
            }
            if let Some(sec) = e.section.and_then(|sid| model.sections.get(sid.index())) {
                if sec.width > 0.0 {
                    return sec.width;
                }
            }
        }
        b
    };

    let mut a_pos: f64 = 0.0;
    let mut a_neg: f64 = 0.0;
    let mut t_used: f64 = 0.0;
    let mut matched = false;

    let mut referenced = std::collections::HashSet::new();
    let mut candidates: Vec<(Vec<NodeId>, f64)> = Vec::new();
    for region in &model.floor_regions {
        referenced.extend(region.slab_ids.iter().copied());
        let t = region
            .slab_ids
            .iter()
            .filter_map(|&id| model.slab(id))
            .filter_map(|s| model.slab_plate_thickness(s))
            .fold(0.0_f64, f64::max);
        if t > 0.0 {
            candidates.push((region.boundary.clone(), t));
        }
    }
    for slab in &model.slabs {
        if referenced.contains(&slab.id) {
            continue;
        }
        let Some(boundary) = slab.boundary_nodes(model) else {
            continue;
        };
        let Some(t) = model.slab_plate_thickness(slab) else {
            continue;
        };
        candidates.push((boundary, t));
    }

    for (boundary, t) in candidates {
        if !(boundary.contains(&n0) && boundary.contains(&n1)) {
            continue;
        }
        matched = true;
        t_used = t_used.max(t);
        let mut s_pos: f64 = 0.0;
        let mut s_neg: f64 = 0.0;
        for nid in &boundary {
            let Some(q) = model.nodes.get(nid.index()) else {
                continue;
            };
            let s = signed_dist(q.coord);
            s_pos = s_pos.max(s);
            s_neg = s_neg.max(-s);
        }
        if s_pos > 0.0 {
            let far_w = far_beam_width(&boundary, s_pos, 1.0);
            a_pos = a_pos.max((s_pos - b / 2.0 - far_w / 2.0).max(0.0));
        }
        if s_neg > 0.0 {
            let far_w = far_beam_width(&boundary, s_neg, -1.0);
            a_neg = a_neg.max((s_neg - b / 2.0 - far_w / 2.0).max(0.0));
        }
    }

    let ba = |a: f64| -> f64 {
        if a <= 0.0 {
            0.0
        } else if a < 0.5 * l {
            (0.5 - 0.6 * a / l) * a
        } else {
            0.1 * l
        }
    };
    let bf = b + ba(a_pos) + ba(a_neg);
    if !matched || bf <= b {
        return None;
    }
    let t = if t_used > 0.0 {
        t_used
    } else {
        model.slab_thickness
    };
    if t <= 0.0 {
        return None;
    }
    Some((bf, t))
}

#[derive(Clone, Copy, Debug)]
struct SlabFlange {
    slab: sepika_core::ids::SlabId,
    width_mm: f64,
    thickness_mm: f64,
}

fn flanged_inertia(b_mm: f64, d_mm: f64, sides: &[Option<SlabFlange>; 2]) -> Result<f64, String> {
    if !b_mm.is_finite() || b_mm <= 0.0 || !d_mm.is_finite() || d_mm <= 0.0 {
        return Err("梁幅・せいは正の有限値が必要です".into());
    }
    let mut rectangles = vec![(b_mm, d_mm, d_mm / 2.0)];
    for side in sides.iter().flatten() {
        let (w, t) = (side.width_mm, side.thickness_mm);
        if !w.is_finite() || w < 0.0 || !t.is_finite() || t <= 0.0 || t > d_mm {
            return Err(format!(
                "床板 {}: 協力幅は非負の有限値、実厚は0超・梁せい以下が必要です",
                side.slab.0
            ));
        }
        rectangles.push((w, t, d_mm - t / 2.0));
    }
    let area: f64 = rectangles.iter().map(|(w, t, _)| w * t).sum();
    let centroid: f64 = rectangles.iter().map(|(w, t, z)| w * t * z).sum::<f64>() / area;
    let inertia: f64 = rectangles
        .iter()
        .map(|(w, t, z)| w * t.powi(3) / 12.0 + w * t * (z - centroid).powi(2))
        .sum();
    if !inertia.is_finite() || inertia <= 0.0 {
        return Err("合成断面二次モーメントが正の有限値ではありません".into());
    }
    Ok(inertia)
}

#[derive(Clone, Copy, Debug)]
struct SlabStrip {
    slab: sepika_core::ids::SlabId,
    span: [f64; 2],
    clear_width_mm: f64,
    flange_width_mm: f64,
    thickness_mm: f64,
}

#[derive(Debug)]
struct SlabSide {
    slabs: Vec<sepika_core::ids::SlabId>,
    flange: SlabFlange,
}

fn average_slab_side(mut strips: Vec<SlabStrip>) -> Result<Option<SlabSide>, String> {
    if strips.is_empty() {
        return Ok(None);
    }
    strips.sort_by(|a, b| a.span[0].total_cmp(&b.span[0]));
    let first = strips[0];
    let mut cursor = 0.0;
    let mut thickness_mm = 0.0;
    for strip in &strips {
        if (strip.clear_width_mm - first.clear_width_mm).abs() > 1e-6
            || (strip.flange_width_mm - first.flange_width_mm).abs() > 1e-6
        {
            return Err(format!(
                "床板 {} と {}: 同じ側の内法距離・協力幅が変化しています",
                first.slab.0, strip.slab.0
            ));
        }
        if strip.span[0] < cursor - 1e-12 {
            return Err(format!(
                "床板 {}: 同じ側の取付区間が重複しています",
                strip.slab.0
            ));
        }
        if strip.span[0] > cursor + 1e-12 {
            return Err(format!(
                "床板 {}: 取付区間に隙間があり梁全長を覆いません",
                strip.slab.0
            ));
        }
        thickness_mm += (strip.span[1] - strip.span[0]) * strip.thickness_mm;
        cursor = strip.span[1];
    }
    if (cursor - 1.0).abs() > 1e-12 {
        return Err(format!(
            "床板 {}: 取付区間が梁全長を覆いません",
            first.slab.0
        ));
    }
    Ok(Some(SlabSide {
        slabs: strips.iter().map(|s| s.slab).collect(),
        flange: SlabFlange {
            slab: first.slab,
            width_mm: first.flange_width_mm,
            thickness_mm,
        },
    }))
}

fn rc_slab_sides(
    model: &Model,
    data: &sepika_core::model::ElementData,
    b_mm: f64,
    d_mm: f64,
) -> Result<[Option<SlabSide>; 2], String> {
    use sepika_core::model::{PlateAssignment, SlabShape, SupportMemberId};
    let support = SupportMemberId::Primary(data.id);
    let mut candidates = std::collections::HashSet::new();
    let mut no_plate_regions = Vec::new();
    for region in &model.floor_assignment_regions.regions {
        if !region.boundary.iter().any(|edge| edge.support == support) {
            continue;
        }
        match region.assignment {
            PlateAssignment::NoPlate => {
                no_plate_regions.push(region);
                continue;
            }
            PlateAssignment::Unset => {
                return Err(format!("床板割当領域 {}: 床板が未設定です", region.id.0))
            }
            PlateAssignment::Plate(id) => {
                if model
                    .floor_assignment_regions
                    .regions
                    .iter()
                    .filter(|r| r.assignment == PlateAssignment::Plate(id))
                    .count()
                    != 1
                {
                    return Err(format!("床板 {}: 割当領域が重複しています", id.0));
                }
                candidates.insert(id);
            }
        }
    }
    for &id in &candidates {
        model
            .slab(id)
            .ok_or_else(|| format!("床板 {}: 参照先がありません", id.0))?;
    }
    for region in &model.floor_regions {
        if data.nodes.iter().all(|n| region.boundary.contains(n)) {
            for &id in &region.slab_ids {
                let slab = model
                    .slab(id)
                    .ok_or_else(|| format!("床板 {}: 参照先がありません", id.0))?;
                if model.slab_assignment_region(id).is_none()
                    && matches!(slab.shape, SlabShape::Enclosed)
                {
                    return Err(format!("床板 {}: 支持する割当領域がありません", id.0));
                }
            }
        }
    }
    for slab in &model.slabs {
        if let SlabShape::Attached {
            anchor: sepika_core::model::RegionAnchor::Line { nodes, .. },
            ..
        } = slab.shape
        {
            if data.nodes.iter().all(|n| nodes.contains(n)) {
                return Err(format!(
                    "床板 {}: 取り付く床板は剛性算定の適用範囲外です",
                    slab.id.0
                ));
            }
        }
    }
    let mut strips: [Vec<SlabStrip>; 2] = [Vec::new(), Vec::new()];
    if candidates.is_empty() {
        return Ok([None, None]);
    }
    let placement = model.beam_placement(support)?;
    let [p0, p1] = placement.actual_ends;
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let length_mm = dx.hypot(dy);
    let tol = 1e-6;
    if length_mm <= tol || (p1[2] - p0[2]).abs() > tol || placement.axes[1][2].abs() < 1.0 - 1e-9 {
        return Err("水平・上端高さ一定の梁断面を確認できません".into());
    }
    let signed_dist = |p: [f64; 3]| (-(p[0] - p0[0]) * dy + (p[1] - p0[1]) * dx) / length_mm;
    for id in candidates {
        let slab = model
            .slab(id)
            .ok_or_else(|| format!("床板 {}: 参照先がありません", id.0))?;
        if !matches!(slab.shape, SlabShape::Enclosed) {
            return Err(format!("床板 {}: 囲まれた床板ではありません", id.0));
        }
        let floor = model.resolved_slab_placement(id)?;
        let edges: Vec<_> = floor
            .support_boundary
            .iter()
            .filter(|e| e.support == support)
            .collect();
        if edges.len() != 1
            || edges[0]
                .span
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(format!("床板 {}: 支持取付区間が不正です", id.0));
        }
        let span = [
            edges[0].span[0].min(edges[0].span[1]),
            edges[0].span[0].max(edges[0].span[1]),
        ];
        if span[1] <= span[0] {
            return Err(format!("床板 {}: 取付区間長は正値が必要です", id.0));
        }
        if !floor.thickness_mm.is_finite() || floor.thickness_mm <= 0.0 || floor.thickness_mm > d_mm
        {
            return Err(format!(
                "床板 {}: 実厚は正の有限値で梁せい以下が必要です",
                id.0
            ));
        }
        if (floor.top_z_mm - (p0[2] + d_mm / 2.0)).abs() > tol {
            return Err(format!("床板 {}: 床上面が梁上端と一致しません", id.0));
        }
        let distances: Vec<_> = floor.top_boundary.iter().map(|&p| signed_dist(p)).collect();
        if distances.iter().any(|v| !v.is_finite()) {
            return Err(format!("床板 {}: 梁からの距離が非有限です", id.0));
        }
        let positive = distances.iter().copied().fold(0.0_f64, f64::max);
        let negative = distances.iter().copied().fold(0.0_f64, f64::min);
        if positive > tol && negative < -tol || positive <= tol && negative >= -tol {
            return Err(format!("床板 {}: 梁の片側の床範囲を確認できません", id.0));
        }
        let (side, sign, distance) = if positive > tol {
            (0, 1.0, positive)
        } else {
            (1, -1.0, -negative)
        };
        if floor.top_boundary.len() != 4
            || floor.top_boundary.iter().any(|p| {
                let x = ((p[0] - p0[0]) * dx + (p[1] - p0[1]) * dy) / length_mm;
                let y = signed_dist(*p) * sign;
                ((x - span[0] * length_mm).abs() > tol && (x - span[1] * length_mm).abs() > tol)
                    || (y.abs() > tol && (y - distance).abs() > tol)
            })
        {
            return Err(format!(
                "床板 {}: 取付区間で一定幅の矩形床範囲を確認できません",
                id.0
            ));
        }

        let mut far_width_mm = b_mm;
        for edge in &floor.support_boundary {
            let SupportMemberId::Primary(eid) = edge.support else {
                continue;
            };
            let Some(e) = model.element(eid) else {
                continue;
            };
            if e.kind != sepika_core::model::ElementKind::Beam
                || e.nodes.len() != 2
                || eid == data.id
            {
                continue;
            }
            let a = model
                .node(e.nodes[0])
                .ok_or("対向梁の節点がありません")?
                .coord;
            let z = model
                .node(e.nodes[1])
                .ok_or("対向梁の節点がありません")?
                .coord;
            if (signed_dist(a) * sign - distance).abs() <= 1.0
                && (signed_dist(z) * sign - distance).abs() <= 1.0
            {
                far_width_mm = model
                    .element_section(e)
                    .ok_or("対向梁の断面がありません")?
                    .width;
                break;
            }
        }
        if !far_width_mm.is_finite() || far_width_mm <= 0.0 {
            return Err(format!("床板 {}: 対向梁幅が不正です", id.0));
        }
        let raw_a_mm = distance - b_mm / 2.0 - far_width_mm / 2.0;
        if !raw_a_mm.is_finite() {
            return Err(format!("床板 {}: 内法距離が非有限です", id.0));
        }
        let a_mm = raw_a_mm.max(0.0);
        let ba_mm = if a_mm < 0.5 * length_mm {
            (0.5 - 0.6 * a_mm / length_mm) * a_mm
        } else {
            0.1 * length_mm
        };
        strips[side].push(SlabStrip {
            slab: id,
            span,
            clear_width_mm: a_mm,
            flange_width_mm: ba_mm,
            thickness_mm: floor.thickness_mm,
        });
    }
    for region in no_plate_regions {
        let points = model
            .assignment_region_boundary_coords(&region.boundary)
            .ok_or_else(|| format!("床板割当領域 {}: 版なしの支持境界が未解決です", region.id.0))?;
        if !points.iter().flatten().all(|v| v.is_finite()) {
            return Err(format!(
                "床板割当領域 {}: 版なしの支持境界が非有限です",
                region.id.0
            ));
        }
        let positive = points.iter().any(|&p| signed_dist(p) > tol);
        let negative = points.iter().any(|&p| signed_dist(p) < -tol);
        for (side, exists) in [positive, negative].into_iter().enumerate() {
            if exists && !strips[side].is_empty() {
                return Err(format!(
                    "床板 {} と床板割当領域 {}: 同じ側で床板ありとNoPlateが混在しています",
                    strips[side][0].slab.0, region.id.0
                ));
            }
        }
    }
    let [left, right] = strips;
    Ok([average_slab_side(left)?, average_slab_side(right)?])
}

fn slab_stiffness_factor(
    model: &Model,
    data: &sepika_core::model::ElementData,
    b_mm: f64,
    d_mm: f64,
) -> Result<f64, String> {
    let sides = rc_slab_sides(model, data, b_mm, d_mm)?;
    let flanges = sides.each_ref().map(|s| {
        s.as_ref().map(|s| {
            debug_assert!(!s.slabs.is_empty());
            s.flange
        })
    });
    let inertia = flanged_inertia(b_mm, d_mm, &flanges)?;
    let factor = inertia / (b_mm * d_mm.powi(3) / 12.0);
    if !factor.is_finite() || factor <= 0.0 {
        return Err("曲げ剛性増大率が正の有限値ではありません".into());
    }
    Ok(factor)
}

/// 床スラブコンクリートの設計基準強度の仮定値 [N/mm²]。
const COMPOSITE_SLAB_FC: f64 = 21.0;

/// S 造合成梁の強軸曲げ剛性の増大率。適用不能時は 1.0。
pub(super) fn composite_beam_stiffness_factor(
    model: &Model,
    data: &sepika_core::model::ElementData,
    sec: &sepika_core::model::Section,
    es: f64,
) -> f64 {
    let (sa, si, sh) = (sec.area, sec.iy, sec.depth);
    if sa <= 0.0 || si <= 0.0 || sh <= 0.0 || es <= 0.0 {
        return 1.0;
    }
    let Some((bf, t)) = slab_cooperating_width(model, data, sec.width.max(1.0)) else {
        return 1.0;
    };
    let ec = sepika_core::section_shape::concrete_young_modulus(COMPOSITE_SLAB_FC);
    let hd = 0.0;
    let ca = bf * t;
    let denom = ec * ca + es * sa;
    if denom <= 0.0 {
        return 1.0;
    }
    let g = (ec * ca * (t / 2.0) + es * sa * (t + hd + sh / 2.0)) / denom;
    let i_comp = (ec / es) * (bf * t.powi(3) / 12.0 + ca * (g - t / 2.0).powi(2))
        + si
        + sa * (g - t - hd - sh / 2.0).powi(2);
    ((i_comp + si) / (2.0 * si)).max(1.0)
}

/// 壁エレメントモデルの上下大梁の剛性倍率。
pub const WALL_GIRDER_STIFF_FACTOR: f64 = 100.0;

/// 部材の剛性算定で断面性能へ乗じられる割増し率の内訳。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StiffnessBreakdown {
    /// 強軸曲げ剛性の増大率。対象外は 1.0。
    pub slab: f64,
    /// 上下大梁の剛性倍率（対象外は 1.0）。
    pub wall_girder: f64,
}

impl Default for StiffnessBreakdown {
    fn default() -> Self {
        Self {
            slab: 1.0,
            wall_girder: 1.0,
        }
    }
}

/// 断面・材料が確定した状態で割増し率を求める。
pub(super) fn breakdown_with(
    model: &Model,
    data: &sepika_core::model::ElementData,
    sec: &sepika_core::model::Section,
    es: f64,
    is_horizontal: bool,
) -> Result<StiffnessBreakdown, String> {
    use sepika_core::section_shape::SectionShape;
    let slab = match &sec.shape {
        Some(SectionShape::RcBeamRect { .. }) => {
            slab_stiffness_factor(model, data, sec.width, sec.depth)
                .map_err(|reason| format!("部材 ID {} の床板協力剛性: {reason}", data.id.0))?
        }
        Some(SectionShape::SteelH { .. }) => composite_beam_stiffness_factor(model, data, sec, es),
        _ => 1.0,
    };
    let wall_girder = if is_horizontal
        && data.nodes.len() >= 2
        && is_wall_top_bottom_girder(model, data.nodes[0], data.nodes[1])
    {
        WALL_GIRDER_STIFF_FACTOR
    } else {
        1.0
    };
    Ok(StiffnessBreakdown { slab, wall_girder })
}

/// 部材の剛性算定で適用される割増し率を、モデルと要素データから求める。
/// RC床板の未解決・不正・適用範囲外、SRC材料不正は理由付きエラー。対象外で断面・材料が引けない場合はすべて 1.0。
pub fn stiffness_breakdown(
    model: &Model,
    data: &sepika_core::model::ElementData,
) -> Result<StiffnessBreakdown, String> {
    if model
        .element_section(data)
        .and_then(|s| s.shape.as_ref())
        .is_some_and(|shape| {
            matches!(
                shape,
                sepika_core::section_shape::SectionShape::SrcBeamRect { .. }
                    | sepika_core::section_shape::SectionShape::SrcColumnRect { .. }
            )
        })
    {
        composite_props_of(model, data)?;
    }
    if data.nodes.len() < 2 {
        return Ok(StiffnessBreakdown::default());
    }
    let (Some(sec), Some(mat)) = (
        data.section.and_then(|sid| model.sections.get(sid.index())),
        model.element_material(data),
    ) else {
        return Ok(StiffnessBreakdown::default());
    };
    let (Some(p0), Some(p1)) = (
        model.nodes.get(data.nodes[0].index()),
        model.nodes.get(data.nodes[1].index()),
    ) else {
        return Ok(StiffnessBreakdown::default());
    };
    let (dx, dy, dz) = (
        p1.coord[0] - p0.coord[0],
        p1.coord[1] - p0.coord[1],
        p1.coord[2] - p0.coord[2],
    );
    let lp = (dx * dx + dy * dy).sqrt();
    let is_horizontal = lp > 1e-9 && dz.abs() <= 0.05 * lp;
    breakdown_with(model, data, sec, mat.young, is_horizontal)
}

/// 壁上下の弾性剛梁として、壁剛性倍率が適用される線材かを返す。
pub fn is_elastic_wall_girder(model: &Model, data: &sepika_core::model::ElementData) -> bool {
    matches!(
        data.kind,
        sepika_core::model::ElementKind::Beam
            | sepika_core::model::ElementKind::Fiber
            | sepika_core::model::ElementKind::MultiSpring
    ) && stiffness_breakdown(model, data).is_ok_and(|b| b.wall_girder > 1.0)
}

/// SRC/CFT 等価性能。対象外は `Ok(None)`、材料や断面性能の不正は理由付きエラー。
pub fn composite_props_of(
    model: &Model,
    data: &sepika_core::model::ElementData,
) -> Result<Option<sepika_core::section_shape::CompositeProps>, String> {
    let Some(sec) = data.section.and_then(|sid| model.sections.get(sid.index())) else {
        return Ok(None);
    };
    let Some(shape) = sec.shape.as_ref() else {
        return Ok(None);
    };
    let Some(mat) = model.element_material(data) else {
        if matches!(
            shape,
            sepika_core::section_shape::SectionShape::SrcBeamRect { .. }
                | sepika_core::section_shape::SectionShape::SrcColumnRect { .. }
                | sepika_core::section_shape::SectionShape::CftBox { .. }
                | sepika_core::section_shape::SectionShape::CftPipe { .. }
        ) {
            return Err(format!(
                "部材 ID {} の断面「{}」の SRC/CFT 主材料が未割当です",
                data.id.0, sec.name
            ));
        }
        return Ok(None);
    };
    composite_props_with(sec, mat, model.element_steel_material(data))
        .map_err(|error| format!("部材 ID {} の断面「{}」: {error}", data.id.0, sec.name))
}

pub(super) fn validate_composite_material(
    shape: &sepika_core::section_shape::SectionShape,
    mat: &sepika_core::model::Material,
    steel: Option<&sepika_core::model::Material>,
) -> Result<(), String> {
    use sepika_core::section_shape::SectionShape;
    if matches!(
        shape,
        SectionShape::RcBeamRect { .. }
            | SectionShape::RcColumnRect { .. }
            | SectionShape::RcColumnCircle { .. }
            | SectionShape::SrcBeamRect { .. }
            | SectionShape::SrcColumnRect { .. }
            | SectionShape::CftBox { .. }
            | SectionShape::CftPipe { .. }
    ) && !mat.fc.is_some_and(|fc| fc.is_finite() && fc > 0.0)
    {
        return Err("RC/SRC/CFT 断面のコンクリート Fc が未設定または不正です".into());
    }
    if matches!(
        shape,
        SectionShape::CftBox { .. } | SectionShape::CftPipe { .. }
    ) {
        shape.rounded_steel_properties()?;
        shape.try_cft_core_props()?;
        let Some(steel) = steel else {
            return Err("CFT 断面の鋼管材料が未設定です".into());
        };
        if steel.category != sepika_core::model::MaterialCategory::Steel {
            return Err("CFT 断面の鋼管材料が鋼材ではありません".into());
        }
        if !steel.young.is_finite() || steel.young <= 0.0 {
            return Err("CFT 断面の鋼管ヤング係数 E が未設定または不正です".into());
        }
        if !steel.poisson.is_finite() {
            return Err("CFT 断面の鋼管ポアソン比 ν が不正です".into());
        }
    }
    Ok(())
}

pub(super) fn composite_props_with(
    sec: &sepika_core::model::Section,
    mat: &sepika_core::model::Material,
    steel: Option<&sepika_core::model::Material>,
) -> Result<Option<sepika_core::section_shape::CompositeProps>, String> {
    use sepika_core::section_shape::SectionShape;
    let Some(shape) = sec.shape.as_ref() else {
        return Ok(None);
    };
    match shape {
        SectionShape::SrcBeamRect { .. } | SectionShape::SrcColumnRect { .. } => {
            if !mat.fc.is_some_and(|fc| fc.is_finite() && fc > 0.0) {
                return Err("SRC の Fc は設定済みの正の有限値が必要です".into());
            }
            let props = shape.src_equivalent_props(mat.young, mat.poisson)?;
            if let Some(p) = props {
                let g = mat.shear_modulus();
                if [g, g * p.j, g * p.as_y, g * p.as_z]
                    .iter()
                    .any(|v| !v.is_finite() || *v <= 0.0)
                {
                    return Err("SRC のせん断剛性は正の有限値が必要です".into());
                }
            }
            Ok(props)
        }
        SectionShape::CftBox { .. } | SectionShape::CftPipe { .. } => {
            validate_composite_material(shape, mat, steel)?;
            let fc = mat.fc.ok_or_else(|| "CFT の Fc が未設定です".to_string())?;
            let gamma_c = sepika_core::units::concrete_unit_weight_kn_m3(
                fc,
                mat.concrete_class,
                sepika_core::units::ConcreteComposition::Plain,
            );
            let steel = steel.ok_or_else(|| "CFT 断面の鋼管材料が未設定です".to_string())?;
            shape.try_cft_equivalent_props(
                steel.young,
                steel.poisson,
                steel.shear_modulus(),
                fc,
                gamma_c,
                sec.resolved_j()?,
            )
        }
        _ => Ok(None),
    }
}

/// 自部材（両端節点 n0, n1）が壁エレメントモデルの上辺・下辺大梁かどうかを判定する。
pub(super) fn is_wall_top_bottom_girder(model: &Model, n0: NodeId, n1: NodeId) -> bool {
    model.elements.iter().any(|e| {
        matches!(e.kind, sepika_core::model::ElementKind::Wall)
            && e.nodes.len() >= 4
            && e.nodes.contains(&n0)
            && e.nodes.contains(&n1)
            && crate::wall::misc_wall::wall_is_seismic(e, model)
    })
}

#[cfg(test)]
mod slab_geometry_tests {
    use super::*;
    use sepika_core::ids::SlabId;

    #[test]
    fn unequal_flange_rectangles_and_exchange_match_independent_inertia() {
        let sides = [
            Some(SlabFlange {
                slab: SlabId(0),
                width_mm: 500.0,
                thickness_mm: 150.0,
            }),
            Some(SlabFlange {
                slab: SlabId(1),
                width_mm: 700.0,
                thickness_mm: 200.0,
            }),
        ];
        for s in [sides, [sides[1], sides[0]]] {
            let inertia = flanged_inertia(300.0, 600.0, &s).unwrap();
            assert!((inertia - 10306025843.881857).abs() < 1e-5);
            assert!((inertia / 5400000000.0 - 1.908523304422566).abs() < 1e-12);
        }
    }

    #[test]
    fn invalid_beam_and_flange_dimensions_are_rejected() {
        for b in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(flanged_inertia(b, 600.0, &[None; 2]).is_err());
        }
        for d in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(flanged_inertia(300.0, d, &[None; 2]).is_err());
        }
        for width_mm in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(flanged_inertia(
                300.0,
                600.0,
                &[
                    Some(SlabFlange {
                        slab: SlabId(7),
                        width_mm,
                        thickness_mm: 150.0
                    }),
                    None
                ]
            )
            .unwrap_err()
            .contains("床板 7"));
        }
    }
}

#[cfg(test)]
mod averaged_thickness_safety_tests {
    use super::*;
    use sepika_core::ids::SlabId;

    #[test]
    fn side_mean_uses_support_length_and_retains_every_slab_id() {
        let strip = |id, span, t| SlabStrip {
            slab: SlabId(id),
            span,
            clear_width_mm: 1000.0,
            flange_width_mm: 440.0,
            thickness_mm: t,
        };
        let side = average_slab_side(vec![
            strip(1, [0.25, 1.0], 175.0),
            strip(0, [0.0, 0.25], 75.0),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(side.flange.thickness_mm, 150.0);
        assert_eq!(side.slabs, vec![SlabId(0), SlabId(1)]);
        assert_eq!(side.flange.width_mm, 440.0);
    }

    #[test]
    fn mean_thickness_can_overestimate_stiffness_and_underestimate_constant_moment_rotation() {
        let section = |t| {
            flanged_inertia(
                300.0,
                600.0,
                &[
                    Some(SlabFlange {
                        slab: SlabId(0),
                        width_mm: 1200.0,
                        thickness_mm: t,
                    }),
                    None,
                ],
            )
            .unwrap()
        };
        let average = section(150.0);
        let thin = section(50.0);
        let thick = section(250.0);
        assert!((average - 10293750000.0).abs() < 1e-5);
        assert!((thin - 8815625000.0).abs() < 1e-5);
        assert!((thick - 10407812500.0).abs() < 1e-5);
        let equivalent = 1.0 / (0.5 / thin + 0.5 / thick);
        assert!((equivalent - 9545782024.709421).abs() < 1e-5);
        assert!(average > equivalent);
        assert!(1.0 / average < 1.0 / equivalent);
    }
}
