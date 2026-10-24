use std::collections::HashMap;

use sepika_core::geom::{LEVEL_TOL_MM, MEMBER_AXIS_TOL_MM};
use sepika_core::model::{
    ElementKind, FrameSectionUse, Model, PlateAssignment, Section, SlabShape, SupportMemberId,
};
use sepika_core::section_shape::SectionShape;

#[derive(Clone, Copy)]
struct Rect {
    s: [f64; 2],
    z: [f64; 2],
}

#[derive(Default)]
pub(super) struct Contacts {
    areas: HashMap<SupportMemberId, f64>,
}

impl Contacts {
    pub(super) fn area(&self, support: SupportMemberId) -> f64 {
        self.areas.get(&support).copied().unwrap_or(0.0)
    }

    pub(super) fn from_model(
        model: &Model,
        secondary: &HashMap<sepika_core::ids::SecondaryMemberId, super::SecondaryQuantity>,
    ) -> Result<Self, String> {
        let mut rectangles: HashMap<SupportMemberId, [Vec<Rect>; 2]> = HashMap::new();
        for region in &model.floor_assignment_regions.regions {
            match region.assignment {
                PlateAssignment::NoPlate => continue,
                PlateAssignment::Unset => {
                    return Err(format!(
                        "床板割当領域 {:?}: Unset のため数量未確定です",
                        region.id
                    ));
                }
                PlateAssignment::Plate(id) => {
                    let slab = model.slab(id).ok_or_else(|| {
                        format!("床板割当領域 {:?}: 床板 {id:?} の参照が不明です", region.id)
                    })?;
                    if !matches!(slab.shape, SlabShape::Enclosed) {
                        return Err(format!("床板 {id:?}: 割当領域と床板形状が一致しません"));
                    }
                    let placement = model.resolved_slab_placement(id)?;
                    let pts = placement.top_boundary;
                    let winding =
                        validate_polygon(&pts).map_err(|e| format!("床板 {id:?}: {e}"))?;
                    let slab_z = [placement.bottom_z_mm, placement.top_z_mm];
                    let horizontal = true;
                    for (i, edge) in region.boundary.iter().enumerate() {
                        if edge
                            .span
                            .iter()
                            .any(|t| !t.is_finite() || !(0.0..=1.0).contains(t))
                        {
                            return Err(format!("床板 {id:?}: 有向材軸区間が不正です"));
                        }
                        let (a, b) = model.support_member_axis(edge.support).ok_or_else(|| {
                            format!("床板 {id:?}: 支持材 {:?} の参照が不明です", edge.support)
                        })?;
                        let at =
                            |t: f64| std::array::from_fn::<_, 3, _>(|k| a[k] + (b[k] - a[k]) * t);
                        let distance_xy =
                            |p: [f64; 3], q: [f64; 3]| (p[0] - q[0]).hypot(p[1] - q[1]);
                        if distance_xy(at(edge.span[0]), pts[i]) > MEMBER_AXIS_TOL_MM
                            || distance_xy(at(edge.span[1]), pts[(i + 1) % pts.len()])
                                > MEMBER_AXIS_TOL_MM
                        {
                            return Err(format!("床板 {id:?}: 支持境界が閉じていません"));
                        }
                        let side = usize::from((edge.span[1] - edge.span[0]) * winding < 0.0);
                        add_contact(
                            model,
                            secondary,
                            edge.support,
                            edge.span,
                            (slab_z, horizontal),
                            side,
                            &mut rectangles,
                        )?;
                    }
                }
            }
        }
        for slab in &model.slabs {
            if matches!(slab.shape, SlabShape::Enclosed)
                && model.slab_assignment_region(slab.id).is_none()
            {
                return Err(format!("床板 {:?}: 割当領域の参照が不明です", slab.id));
            }
            if let SlabShape::Attached {
                anchor: sepika_core::model::RegionAnchor::Line { span, .. },
                ..
            } = slab.shape
            {
                if !sepika_core::model::span_is_valid(span) {
                    return Err(format!("床板 {:?}: 取付き材軸区間が不正です", slab.id));
                }
            }
            let placement = model.resolved_slab_placement(slab.id)?;
            let pts = placement.top_boundary;
            validate_polygon(&pts).map_err(|e| format!("床板 {:?}: {e}", slab.id))?;
            let slab_z = [placement.bottom_z_mm, placement.top_z_mm];
            let horizontal = true;
            let supports = model
                .elements
                .iter()
                .filter(|e| e.kind == ElementKind::Beam)
                .map(|e| SupportMemberId::Primary(e.id))
                .chain(model.beams().map(|m| SupportMemberId::Secondary(m.id)));
            for support in supports {
                let Some((a, b)) = model.support_member_axis(support) else {
                    continue;
                };
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let len = dx.hypot(dy);
                if len <= 0.0 {
                    continue;
                }
                let along = |p: [f64; 3]| ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (len * len);
                let across = |p: [f64; 3]| ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len;
                for i in 0..pts.len() {
                    let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                    if across(p) <= MEMBER_AXIS_TOL_MM && across(q) <= MEMBER_AXIS_TOL_MM {
                        let span = [along(p), along(q)];
                        for (span, side) in local_contact_spans(&pts, a, b, i, span) {
                            add_contact(
                                model,
                                secondary,
                                support,
                                span,
                                (slab_z, horizontal),
                                side,
                                &mut rectangles,
                            )?;
                        }
                    }
                }
            }
        }
        Ok(Self {
            areas: rectangles
                .into_iter()
                .map(|(support, sides)| {
                    let area = union_area(&sides[0]) + union_area(&sides[1]);
                    if !area.is_finite() {
                        return Err(format!(
                            "支持材 {support:?}: 接触面積が有限範囲を超えています"
                        ));
                    }
                    Ok((support, area))
                })
                .collect::<Result<_, String>>()?,
        })
    }
}

fn local_contact_spans(
    pts: &[[f64; 3]],
    a: [f64; 3],
    b: [f64; 3],
    edge: usize,
    span: [f64; 2],
) -> Vec<([f64; 2], usize)> {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = dx.hypot(dy);
    let along = |p: [f64; 3]| ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (len * len);
    let across = |p: [f64; 3]| (dx * (p[1] - a[1]) - dy * (p[0] - a[0])) / len;
    let mut cuts = vec![span[0].min(span[1]), span[0].max(span[1])];
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        cuts.push(along(p));
        if across(p) * across(q) < 0.0 {
            cuts.push(along(p) + (along(q) - along(p)) * across(p) / (across(p) - across(q)));
        }
    }
    cuts.retain(|s| *s >= span[0].min(span[1]) && *s <= span[0].max(span[1]));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    cuts.windows(2)
        .filter_map(|cut| {
            let middle = cut[0] + (cut[1] - cut[0]) / 2.0;
            let mut crossings: Vec<_> = (0..pts.len())
                .filter_map(|i| {
                    let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                    let (s0, s1) = (along(p), along(q));
                    if middle <= s0.min(s1) || middle >= s0.max(s1) {
                        return None;
                    }
                    Some((
                        i,
                        across(p) + (across(q) - across(p)) * (middle - s0) / (s1 - s0),
                    ))
                })
                .collect();
            crossings.sort_by(|p, q| p.1.total_cmp(&q.1));
            let rank = crossings.iter().position(|(i, _)| *i == edge)?;
            let side = rank % 2;
            let occupied = if side == 0 {
                crossings.get(rank + 1).is_some_and(|(_, r)| *r > 0.0)
            } else {
                crossings[rank - 1].1 < 0.0
            };
            occupied.then_some(([cut[0], cut[1]], side))
        })
        .collect()
}

fn support_section(model: &Model, support: SupportMemberId) -> Result<Option<&Section>, String> {
    let section = match support {
        SupportMemberId::Primary(id) => model
            .elements
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.section),
        SupportMemberId::Secondary(id) => {
            model.beams().find(|m| m.id == id).and_then(|m| m.section)
        }
    };
    section
        .map(|id| {
            model
                .sections
                .get(id.index())
                .ok_or_else(|| format!("支持材 {support:?}: 梁断面 {id:?} の参照が不明です"))
        })
        .transpose()
}

fn add_contact(
    model: &Model,
    secondary: &HashMap<sepika_core::ids::SecondaryMemberId, super::SecondaryQuantity>,
    support: SupportMemberId,
    span: [f64; 2],
    (slab_z, horizontal): ([f64; 2], bool),
    side: usize,
    out: &mut HashMap<SupportMemberId, [Vec<Rect>; 2]>,
) -> Result<(), String> {
    let (support, span, side) = match support {
        SupportMemberId::Secondary(id) => match secondary[&id].materialized {
            Some((elem, reverse)) => (
                SupportMemberId::Primary(elem),
                if reverse { span.map(|s| 1.0 - s) } else { span },
                if reverse { 1 - side } else { side },
            ),
            None => (support, span, side),
        },
        _ => (support, span, side),
    };
    let (a, b) = model
        .support_member_axis(support)
        .ok_or_else(|| format!("支持材 {support:?}: 材軸の参照が不明です"))?;
    if span[0].max(span[1]) <= 0.0 || span[0].min(span[1]) >= 1.0 {
        return Ok(());
    }
    let Some(sec) = support_section(model, support)? else {
        // 断面未割当の床境界線と実梁は現モデルで区別できない。
        return Ok(());
    };
    if matches!(support, SupportMemberId::Primary(_))
        && sec.frame_use != Some(FrameSectionUse::Girder)
    {
        return Ok(());
    }
    let structure = match support {
        SupportMemberId::Primary(id) => model
            .elements
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| model.element_material(e)),
        SupportMemberId::Secondary(id) => model
            .beams()
            .find(|m| m.id == id)
            .and_then(|m| model.secondary_material(m)),
    };
    if !matches!(
        sepika_core::structure_kind::structure_kind_of(Some(sec), structure.map(|m| m.category)),
        super::StructureKind::Rc | super::StructureKind::Src
    ) {
        return Ok(());
    }
    let len = sepika_core::geom::vec3::dist(a, b);
    if !len.is_finite() || len <= 0.0 || a.iter().chain(b.iter()).any(|v| !v.is_finite()) {
        return Err(format!(
            "支持材 {support:?}: 梁の数量寸法・標高 [mm] が不正です"
        ));
    }
    let (mut start, mut end) = (0.0, len);
    if let SupportMemberId::Primary(id) = support {
        let elem = model.elements.iter().find(|e| e.id == id).unwrap();
        if elem.rigid_zone.faces_computed() {
            start = elem.rigid_zone.face_i_or_zero();
            end = len - elem.rigid_zone.face_j_or_zero();
        }
    }
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end > len || end < start {
        return Err(format!(
            "支持材 {support:?}: 梁の数量内法区間 [mm] が不正です"
        ));
    }
    let s = [
        (span[0].min(span[1]) * len).max(start),
        (span[0].max(span[1]) * len).min(end),
    ];
    let (width_mm, d) = match sec.shape {
        Some(SectionShape::RcBeamRect { b, d, .. } | SectionShape::SrcBeamRect { b, d, .. }) => {
            (b, d)
        }
        _ => (sec.width, sec.depth),
    };
    if !width_mm.is_finite() || width_mm <= 0.0 || !d.is_finite() || d <= 0.0 {
        return Err(format!(
            "支持材 {support:?}: 梁の断面幅・せい [mm] は有限かつ正である必要があります"
        ));
    }
    if !((width_mm + 2.0 * d) * (end - start)).is_finite() {
        return Err(format!(
            "支持材 {support:?}: 梁の型枠面積・下端高さが有限範囲を超えています"
        ));
    }
    let placement = model.beam_placement(support)?;
    let [actual_a, actual_b] = placement.actual_ends;
    let sloped = actual_a[2] != actual_b[2];
    if !sloped && (placement.axes[1][0] != 0.0 || placement.axes[1][1] != 0.0) {
        return Err(format!(
            "支持材 {support:?}: 断面せい方向が鉛直でない梁の接触型枠は未対応です"
        ));
    }
    let half_depth_z_mm = if sloped {
        (d / (2.0 * placement.axes[1][2].abs())).min(width_mm / (2.0 * placement.axes[2][2].abs()))
    } else {
        d / 2.0
    };
    let beam_z = if sloped {
        let centers = s.map(|s| actual_a[2] + (actual_b[2] - actual_a[2]) * (s / len));
        [
            centers[0].min(centers[1]) - half_depth_z_mm,
            centers[0].max(centers[1]) + half_depth_z_mm,
        ]
    } else {
        [actual_a[2] - half_depth_z_mm, actual_a[2] + half_depth_z_mm]
    };
    if beam_z.iter().any(|z| !z.is_finite()) {
        return Err(format!(
            "支持材 {support:?}: 梁の上下端高さが有限範囲を超えています"
        ));
    }
    let z = [slab_z[0].max(beam_z[0]), slab_z[1].min(beam_z[1])];
    if s[1] <= s[0] || z[1] <= z[0] {
        return Ok(());
    }
    if let SupportMemberId::Primary(id) = support {
        if model
            .elements
            .iter()
            .find(|e| e.id == id)
            .is_some_and(|e| e.nodes.len() != 2)
        {
            return Err(format!(
                "支持材 {support:?}: 2節点以外の主架構梁の接触型枠は未対応です"
            ));
        }
    }
    if !horizontal {
        return Err(format!(
            "支持材 {support:?}: 傾斜床との接触型枠は未対応です"
        ));
    }
    if sloped {
        return Err(if (a[2] - b[2]).abs() > LEVEL_TOL_MM {
            format!("支持材 {support:?}: 傾斜梁との接触型枠は未対応です")
        } else {
            format!("支持材 {support:?}: 水平幾何判定の許容差内でも高さが一定でない梁の接触型枠は未対応です")
        });
    }
    if !matches!(
        sec.shape,
        Some(SectionShape::RcBeamRect { .. } | SectionShape::SrcBeamRect { .. })
    ) {
        return Err(format!(
            "支持材 {support:?}: 一定矩形断面以外の接触型枠は未対応です"
        ));
    }
    if let SupportMemberId::Primary(id) = support {
        if model.member_detail(id).is_some_and(|dt| {
            dt.haunch_i.as_ref().is_some_and(|h| h.length > 0.0)
                || dt.haunch_j.as_ref().is_some_and(|h| h.length > 0.0)
        }) {
            return Err(format!(
                "支持材 {support:?}: ハンチ梁との接触型枠は未対応です"
            ));
        }
        let min_z = model
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Beam)
            .flat_map(|e| e.nodes.iter())
            .filter_map(|id| model.nodes.get(id.index()))
            .map(|n| n.coord[2])
            .fold(f64::INFINITY, f64::min);
        let touches_column = model.elements.iter().any(|e| {
            model
                .element_section(e)
                .is_some_and(|s| s.frame_use == Some(FrameSectionUse::Column))
                && model
                    .elements
                    .iter()
                    .find(|e| e.id == id)
                    .is_some_and(|target| e.nodes.iter().any(|n| target.nodes.contains(n)))
        });
        if touches_column
            && (a[2] - min_z).abs() < super::LEVEL_TOL_MM
            && (b[2] - min_z).abs() < super::LEVEL_TOL_MM
        {
            return Err(format!(
                "支持材 {support:?}: 基礎梁の型枠対象側面が未確定です"
            ));
        }
    }
    out.entry(support).or_default()[side].push(Rect { s, z });
    Ok(())
}

fn validate_polygon(pts: &[[f64; 3]]) -> Result<f64, String> {
    if pts.len() < 3 || pts.iter().flatten().any(|v| !v.is_finite()) {
        return Err("床境界は有限座標の3頂点以上が必要です".into());
    }
    let cross = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let on = |a: [f64; 3], b: [f64; 3], p: [f64; 3]| {
        cross(a, b, p) == 0.0
            && p[0] >= a[0].min(b[0])
            && p[0] <= a[0].max(b[0])
            && p[1] >= a[1].min(b[1])
            && p[1] <= a[1].max(b[1])
    };
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        for j in (i + 1)..pts.len() {
            if j == i + 1 || (j + 1) % pts.len() == i {
                continue;
            }
            let (c, d) = (pts[j], pts[(j + 1) % pts.len()]);
            if cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0
                || on(a, b, c)
                || on(a, b, d)
                || on(c, d, a)
                || on(c, d, b)
            {
                return Err("自己交差する床境界は数量算定できません".into());
            }
        }
    }
    let area2: f64 = (0..pts.len())
        .map(|i| pts[i][0] * pts[(i + 1) % pts.len()][1] - pts[(i + 1) % pts.len()][0] * pts[i][1])
        .sum();
    if area2 == 0.0 || !area2.is_finite() {
        return Err("床境界の面積が不正です".into());
    }
    Ok(area2.signum())
}

fn union_area(rects: &[Rect]) -> f64 {
    let mut cuts: Vec<_> = rects.iter().flat_map(|r| r.s).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    cuts.windows(2)
        .map(|cut| {
            let mut heights: Vec<_> = rects
                .iter()
                .filter(|r| r.s[0] <= cut[0] && r.s[1] >= cut[1])
                .map(|r| r.z)
                .collect();
            heights.sort_by(|a, b| a[0].total_cmp(&b[0]));
            let mut height = 0.0;
            let mut end = f64::NEG_INFINITY;
            for z in heights {
                height += (z[1] - z[0].max(end)).max(0.0);
                end = end.max(z[1]);
            }
            (cut[1] - cut[0]) * height
        })
        .sum()
}
