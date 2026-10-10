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

    pub(super) fn from_model(model: &Model) -> Result<Self, String> {
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
                    let t = thickness(model, slab)?;
                    let pts = model
                        .assignment_region_boundary_coords(&region.boundary)
                        .ok_or_else(|| format!("床板 {id:?}: 支持材の参照が不明です"))?;
                    let winding =
                        validate_polygon(&pts).map_err(|e| format!("床板 {id:?}: {e}"))?;
                    let (slab_z, horizontal) = contact_height(&pts, t);
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
                        if sepika_core::geom::vec3::dist(at(edge.span[0]), pts[i])
                            > MEMBER_AXIS_TOL_MM
                            || sepika_core::geom::vec3::dist(
                                at(edge.span[1]),
                                pts[(i + 1) % pts.len()],
                            ) > MEMBER_AXIS_TOL_MM
                        {
                            return Err(format!("床板 {id:?}: 支持境界が閉じていません"));
                        }
                        let side = usize::from((edge.span[1] - edge.span[0]) * winding < 0.0);
                        add_contact(
                            model,
                            edge.support,
                            edge.span,
                            slab_z,
                            horizontal,
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
            let t = thickness(model, slab)?;
            let pts = slab
                .boundary_coords(model)
                .ok_or_else(|| format!("床板 {:?}: 取付き先の参照が不明です", slab.id))?;
            let winding = validate_polygon(&pts).map_err(|e| format!("床板 {:?}: {e}", slab.id))?;
            let (slab_z, horizontal) = contact_height(&pts, t);
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
                        let side = usize::from((span[1] - span[0]) * winding < 0.0);
                        add_contact(
                            model,
                            support,
                            span,
                            slab_z,
                            horizontal,
                            side,
                            &mut rectangles,
                        )?;
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

fn thickness(model: &Model, slab: &sepika_core::model::Slab) -> Result<f64, String> {
    let sec = model
        .slab_section(slab)
        .ok_or_else(|| format!("床板 {:?}: 断面の参照が未設定または不明です", slab.id))?;
    let t = sec
        .thickness
        .ok_or_else(|| format!("床板 {:?}: 実厚が未設定です", slab.id))?;
    if !t.is_finite() || t <= 0.0 {
        return Err(format!(
            "床板 {:?}: 実厚 [mm] は有限かつ正である必要があります",
            slab.id
        ));
    }
    model
        .slab_plate_thickness(slab)
        .ok_or_else(|| format!("床板 {:?}: 実厚を解決できません", slab.id))
}

fn support_section(model: &Model, support: SupportMemberId) -> Option<&Section> {
    match support {
        SupportMemberId::Primary(id) => model
            .elements
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| model.element_section(e)),
        SupportMemberId::Secondary(id) => model
            .beams()
            .find(|m| m.id == id)
            .and_then(|m| m.section)
            .and_then(|id| model.sections.get(id.index())),
    }
}

fn add_contact(
    model: &Model,
    support: SupportMemberId,
    span: [f64; 2],
    slab_z: [f64; 2],
    horizontal: bool,
    side: usize,
    out: &mut HashMap<SupportMemberId, [Vec<Rect>; 2]>,
) -> Result<(), String> {
    let (a, b) = model
        .support_member_axis(support)
        .ok_or_else(|| format!("支持材 {support:?}: 材軸の参照が不明です"))?;
    let Some(sec) = support_section(model, support) else {
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
    if !((width_mm + 2.0 * d) * (end - start)).is_finite() || !(a[2] - d).is_finite() {
        return Err(format!(
            "支持材 {support:?}: 梁の型枠面積・下端高さが有限範囲を超えています"
        ));
    }
    let z = [slab_z[0].max(a[2] - d), slab_z[1].min(a[2])];
    if s[1] <= s[0] || z[1] <= z[0] {
        return Ok(());
    }
    if let SupportMemberId::Secondary(id) = support {
        if let Some(sm) = model.beams().find(|m| m.id == id) {
            if !model.secondary_member_materialized(sm)
                && [a, b].iter().any(|p| {
                    !model
                        .nodes
                        .iter()
                        .any(|n| sepika_core::geom::vec3::dist(n.coord, *p) <= MEMBER_AXIS_TOL_MM)
                })
            {
                return Err(format!(
                    "支持材 {support:?}: 実節点にない端点を持つ二次小梁の接触数量は未対応です"
                ));
            }
        }
    }
    if !horizontal {
        return Err(format!(
            "支持材 {support:?}: 傾斜床との接触型枠は未対応です"
        ));
    }
    if (a[2] - b[2]).abs() > LEVEL_TOL_MM {
        return Err(format!(
            "支持材 {support:?}: 傾斜梁との接触型枠は未対応です"
        ));
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

fn contact_height(pts: &[[f64; 3]], t: f64) -> ([f64; 2], bool) {
    let low = pts.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let high = pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    ([low - t, high], high - low <= LEVEL_TOL_MM)
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
