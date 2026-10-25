use std::collections::HashMap;

use sepika_core::ids::{ElemId, SlabId};
use sepika_core::model::{
    BeamPlacement, ElementData, FrameSectionUse, MaterialCategory, Model, PlateAssignment,
    RegionAnchor, SlabShape, SupportMemberId,
};
use sepika_core::section_shape::SectionShape;

/// 矩形RC/SRC大梁の自重内訳 [N]。仕上げ・付加重量・嵩上げは含まない。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GirderSelfWeightBreakdown {
    pub element: ElemId,
    pub body_weight_n: f64,
    pub slab_overlap_weight_n: f64,
}

/// 大梁の実配置と囲まれた床板から自重内訳を算定する。適用外は None、不明・不正入力はエラー。
pub fn girder_self_weight_breakdown(
    model: &Model,
    elem: &ElementData,
) -> Result<Option<GirderSelfWeightBreakdown>, String> {
    if !elem.kind.is_weight_frame() {
        return Ok(None);
    }
    let sec = model
        .element_section(elem)
        .ok_or_else(|| format!("線材 {}: 自重の断面を解決できません", elem.id.0))?;
    if sec.frame_use != Some(FrameSectionUse::Girder) {
        return Ok(None);
    }
    let (b, d) = match sec.shape {
        Some(SectionShape::RcBeamRect { b, d, .. } | SectionShape::SrcBeamRect { b, d, .. }) => {
            (b, d)
        }
        _ => return Ok(None),
    };
    let fail = |reason: &str| format!("大梁 {} の床板重複控除: {reason}", elem.id.0);
    if ![b, d].iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err(fail("実断面の幅・せいは有限な正値が必要です"));
    }
    let mat = model
        .element_material(elem)
        .ok_or_else(|| fail("材料を解決できません"))?;
    if mat.category != MaterialCategory::Concrete
        || mat.fc.is_none_or(|fc| !fc.is_finite() || fc <= 0.0)
    {
        return Err(fail("RC/SRCのコンクリート材料を解決できません"));
    }
    mat.validate_design_self_weight(&format!("大梁 {}", elem.id.0), "床板重複控除")?;
    let beam = model.beam_placement(SupportMemberId::Primary(elem.id))?;
    let len = sepika_core::geom::vec3::dist(beam.actual_ends[0], beam.actual_ends[1]);
    let interval = sepika_core::face_distance::girder_self_weight_interval(model, elem)?;
    for slab in &model.slabs {
        let SlabShape::Attached {
            anchor: RegionAnchor::Line { nodes, .. },
            ..
        } = slab.shape
        else {
            continue;
        };
        let direct = elem.nodes.as_slice() == nodes || elem.nodes.iter().rev().copied().eq(nodes);
        let supported = model
            .attached_slab_supports(slab)
            .is_ok_and(|supports| supports.iter().any(|s| s.elem == elem.id));
        if direct || supported {
            let placement = model.resolved_slab_placement(slab.id)?;
            if beam.axes[2][2].abs() > 1e-12 {
                if disjoint_vertical_bounds(&beam, [placement.bottom_z_mm, placement.top_z_mm])? {
                    continue;
                }
            } else if overlap_integral(
                &beam,
                b,
                d,
                len,
                interval,
                [placement.bottom_z_mm, placement.top_z_mm],
            )? == 0.0
            {
                continue;
            }
            return Err(fail(&format!(
                "取り付く床板 {} の実重複控除は未対応です",
                slab.id.0
            )));
        }
    }
    let plan_ratio = beam.axes[0][0].hypot(beam.axes[0][1]);
    if !plan_ratio.is_finite() || plan_ratio <= 0.0 {
        return Err(fail("水平投影材軸を解決できません"));
    }
    let mut sides: [HashMap<SlabId, Vec<[f64; 2]>>; 2] = Default::default();
    for region in &model.floor_assignment_regions.regions {
        let edges: Vec<_> = region
            .boundary
            .iter()
            .filter(|e| e.support == beam.member)
            .collect();
        if edges.is_empty() {
            continue;
        }
        let id = match region.assignment {
            PlateAssignment::NoPlate => continue,
            PlateAssignment::Unset => {
                return Err(fail(&format!("床板割当領域 {:?} が未設定です", region.id)))
            }
            PlateAssignment::Plate(id) => id,
        };
        let slab = model
            .slab(id)
            .ok_or_else(|| fail(&format!("床板 {} の参照先がありません", id.0)))?;
        if !matches!(slab.shape, SlabShape::Enclosed) {
            return Err(fail(&format!("床板 {} が囲まれた床板ではありません", id.0)));
        }
        let placement = model.resolved_slab_placement(id)?;
        if placement.support_boundary != region.boundary {
            return Err(fail(&format!("床板 {} の割当境界が矛盾しています", id.0)));
        }
        let pts = &placement.top_boundary;
        let winding: f64 = pts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let q = pts[(i + 1) % pts.len()];
                p[0] * q[1] - q[0] * p[1]
            })
            .sum();
        if !winding.is_finite() || winding == 0.0 {
            return Err(fail(&format!("床板 {} の平面方向が不正です", id.0)));
        }
        if model.slab_plate_material(slab).is_none() {
            return Err(fail(&format!("床板 {} の材料を解決できません", id.0)));
        }
        for edge in edges {
            if !edge
                .span
                .iter()
                .all(|t| t.is_finite() && (0.0..=1.0).contains(t))
                || edge.span[0] == edge.span[1]
            {
                return Err(fail(&format!("床板 {} の支持区間が不正です", id.0)));
            }
            let lo = edge.span[0].min(edge.span[1]) * len;
            let hi = edge.span[0].max(edge.span[1]) * len;
            if hi <= interval[0] || lo >= interval[1] {
                continue;
            }
            let side = usize::from((edge.span[1] - edge.span[0]) * winding < 0.0);
            sides[side]
                .entry(id)
                .or_default()
                .push([lo.max(interval[0]), hi.min(interval[1])]);
        }
    }
    let mut cuts = interval.to_vec();
    for side in &mut sides {
        for spans in side.values_mut() {
            spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
            let mut merged: Vec<[f64; 2]> = Vec::new();
            for span in spans.iter().copied() {
                match merged.last_mut() {
                    Some(previous) if span[0] <= previous[1] => {
                        previous[1] = previous[1].max(span[1])
                    }
                    _ => merged.push(span),
                }
            }
            for span in &merged {
                cuts.extend_from_slice(span);
            }
            *spans = merged;
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let occupied = sides.iter().filter(|s| !s.is_empty()).count();
    let tilted = beam.axes[2][2].abs() > 1e-12;
    let mut has_overlap = false;
    for side in &sides {
        for (id, spans) in side {
            let slab = model.resolved_slab_placement(*id)?;
            if tilted {
                if !disjoint_vertical_bounds(&beam, [slab.bottom_z_mm, slab.top_z_mm])? {
                    return Err(fail("幅方向が水平でない矩形断面の実重複控除は未対応です"));
                }
                continue;
            }
            for span in spans {
                has_overlap |=
                    overlap_integral(&beam, b, d, len, *span, [slab.bottom_z_mm, slab.top_z_mm])?
                        > 0.0;
            }
        }
    }
    let mut overlap_volume_mm3 = 0.0;
    for span in cuts.windows(2) {
        let mut active = [None, None];
        for (side_index, side) in sides.iter().enumerate() {
            let mut ids: Vec<_> = side
                .iter()
                .filter(|(_, spans)| spans.iter().any(|s| s[0] < span[1] && span[0] < s[1]))
                .map(|(id, _)| *id)
                .collect();
            ids.sort_unstable_by_key(|id| id.0);
            match ids.as_slice() {
                [] if side.is_empty() || !has_overlap => {}
                [] => {
                    return Err(fail(&format!(
                    "側 {side_index} の床板群の支持区間が自重算定区間全長を覆いません（{}〜{} mm）",
                    span[0], span[1]
                )))
                }
                [id] => active[side_index] = Some(*id),
                _ => {
                    return Err(fail(&format!(
                        "同じ側の床板候補 {ids:?} が区間 {}〜{} mmで矛盾しています",
                        span[0], span[1]
                    )))
                }
            }
        }
        if !has_overlap {
            continue;
        }
        for id in active.into_iter().flatten() {
            let slab = model.resolved_slab_placement(id)?;
            overlap_volume_mm3 += b / occupied as f64
                * plan_ratio
                * overlap_integral(
                    &beam,
                    b,
                    d,
                    len,
                    [span[0], span[1]],
                    [slab.bottom_z_mm, slab.top_z_mm],
                )?;
        }
    }
    let body_volume_mm3 = b * d * (interval[1] - interval[0]);
    if ![body_volume_mm3, overlap_volume_mm3]
        .iter()
        .all(|v| v.is_finite())
        || body_volume_mm3 <= 0.0
        || overlap_volume_mm3 < 0.0
        || overlap_volume_mm3 > body_volume_mm3
    {
        return Err(fail(
            "体積が非有限、非正、または控除体積が梁本体を超えています",
        ));
    }
    let unit_weight = mat.design_unit_weight_n_per_mm3();
    if !unit_weight.is_finite() || unit_weight < 0.0 {
        return Err(fail("設計単位重量が非有限または負です"));
    }
    let result = GirderSelfWeightBreakdown {
        element: elem.id,
        body_weight_n: body_volume_mm3 * unit_weight,
        slab_overlap_weight_n: overlap_volume_mm3 * unit_weight,
    };
    if ![result.body_weight_n, result.slab_overlap_weight_n]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(fail("重量が非有限です"));
    }
    Ok(Some(result))
}

fn disjoint_vertical_bounds(beam: &BeamPlacement, slab: [f64; 2]) -> Result<bool, String> {
    let geometry =
        sepika_core::section_shape::face_geometry::SectionGeometry::of(&beam.section_shape)?;
    let bounds = geometry.projected_range([beam.axes[1][2], beam.axes[2][2]])?;
    let bottom = beam.actual_ends[0][2].min(beam.actual_ends[1][2]) + bounds[0];
    let top = beam.actual_ends[0][2].max(beam.actual_ends[1][2]) + bounds[1];
    if ![bottom, top].iter().all(|z| z.is_finite()) {
        return Err(format!("大梁 {:?}: 高さ包絡が非有限です", beam.member));
    }
    Ok(slab[0] >= top || slab[1] <= bottom)
}

fn overlap_integral(
    beam: &BeamPlacement,
    b: f64,
    d: f64,
    len: f64,
    interval: [f64; 2],
    slab: [f64; 2],
) -> Result<f64, String> {
    let mut lines = vec![[slab[0], 0.0], [slab[1], 0.0]];
    for (axis, bounds) in
        beam.axes
            .iter()
            .zip([[0.0, len], [-d / 2.0, d / 2.0], [-b / 2.0, b / 2.0]])
    {
        if axis[2] == 0.0 {
            continue;
        }
        let slope = -(beam.axes[0][0] * axis[0] + beam.axes[0][1] * axis[1]) / axis[2];
        for bound in bounds {
            lines.push([beam.actual_ends[0][2] + bound / axis[2], slope]);
        }
    }
    if !lines.iter().flatten().all(|v| v.is_finite()) {
        return Err(format!(
            "大梁 {:?}: 実形状の交差式が非有限です",
            beam.member
        ));
    }
    let mut cuts = interval.to_vec();
    for (i, a) in lines.iter().enumerate() {
        for c in &lines[..i] {
            if a[1] != c[1] {
                let s = (c[0] - a[0]) / (a[1] - c[1]);
                if s > interval[0] && s < interval[1] {
                    cuts.push(s);
                }
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let height = |s: f64| -> Result<f64, String> {
        let xy = std::array::from_fn(|i| beam.actual_ends[0][i] + beam.axes[0][i] * s);
        let Some(z) = beam.vertical_range(xy)? else {
            return Ok(0.0);
        };
        Ok((z[1].min(slab[1]) - z[0].max(slab[0])).max(0.0))
    };
    let mut integral_mm2 = 0.0;
    for pair in cuts.windows(2) {
        integral_mm2 += (height(pair[0])? + height(pair[1])?) * (pair[1] - pair[0]) / 2.0;
    }
    Ok(integral_mm2)
}
