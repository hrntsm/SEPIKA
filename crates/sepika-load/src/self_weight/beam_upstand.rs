use sepika_core::ids::{MaterialId, SlabId};
use sepika_core::model::{
    FrameSectionUse, MemberLoadKind, Model, PlateAssignment, SecondaryMemberKind, SlabShape,
    SupportMemberId,
};
use sepika_core::units::GRAVITY_MM_S2;

/// 梁の嵩上げ一区間。位置・幅は mm、体積は mm³、重量は N、物理質量は t。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BeamUpstandSegment {
    pub beam: SupportMemberId,
    pub slab: SlabId,
    pub material: MaterialId,
    pub stb_strength_source: Option<sepika_core::model::StrengthSource>,
    pub design_unit_weight_n_per_mm3: f64,
    pub physical_density_t_per_mm3: f64,
    /// 始端から終端を向いた左側は0、右側は1。
    pub side: usize,
    pub interval_mm: [f64; 2],
    pub width_mm: f64,
    pub gap_mm: [f64; 2],
    pub volume_mm3: f64,
    pub design_weight_n: f64,
    pub physical_mass_t: f64,
    pub mass_equivalent_weight_n: f64,
    pub design_load: MemberLoadKind,
    pub mass_equivalent_load: MemberLoadKind,
}

impl BeamUpstandSegment {
    pub fn material_source_label(&self) -> &'static str {
        use sepika_core::model::StrengthSource;
        match self.stb_strength_source {
            None => "床断面",
            Some(StrengthSource::Member) => "STB部材",
            Some(StrengthSource::Section) => "STB断面",
            Some(StrengthSource::Story) => "STB階",
            Some(StrengthSource::Common) => "STB共通",
            Some(StrengthSource::Diameter) => "STB径別",
        }
    }
}

struct Candidate {
    slab: SlabId,
    side: usize,
    span: [f64; 2],
    bottom: f64,
    material: MaterialId,
    gamma: f64,
    density: f64,
    source: Option<sepika_core::model::StrengthSource>,
}

fn material_source(
    model: &Model,
    slab: SlabId,
) -> Result<Option<sepika_core::model::StrengthSource>, String> {
    model
        .stb_strengths
        .members
        .iter()
        .find(|input| input.target == sepika_core::model::StrengthTarget::Slab(slab))
        .map(|input| {
            model
                .resolve_stb_concrete(input)
                .map(|resolved| resolved.source)
        })
        .transpose()
}

/// 主架構と未実部材化小梁の嵩上げ区間を一度ずつ列挙する。
pub fn all_beam_upstand_segments(model: &Model) -> Result<Vec<BeamUpstandSegment>, String> {
    let mut segments = Vec::new();
    for element in model
        .elements
        .iter()
        .filter(|e| e.kind.is_weight_frame() && !model.uses_damper_total_weight(e))
    {
        segments.extend(beam_upstand_segments(
            model,
            SupportMemberId::Primary(element.id),
        )?);
    }
    for sm in model
        .beams()
        .filter(|sm| !model.secondary_member_materialized(sm))
    {
        segments.extend(beam_upstand_segments(
            model,
            SupportMemberId::Secondary(sm.id),
        )?);
    }
    Ok(segments)
}

/// 支持・割当から解決した床板下面までの嵩上げを返す。欠落・矛盾・不正入力は対象付きエラー。
pub fn beam_upstand_segments(
    model: &Model,
    member: SupportMemberId,
) -> Result<Vec<BeamUpstandSegment>, String> {
    let fail = |reason: &str| format!("梁 {member:?} の床板嵩上げ: {reason}");
    let interval = match member {
        SupportMemberId::Primary(id) => {
            let e = model.element(id).ok_or_else(|| fail("部材がありません"))?;
            let sec = model
                .element_section(e)
                .ok_or_else(|| fail("断面がありません"))?;
            if !e.kind.is_weight_frame() || sec.frame_use != Some(FrameSectionUse::Girder) {
                return Ok(Vec::new());
            }
            let mat = model
                .element_material(e)
                .ok_or_else(|| fail("梁材料がありません"))?;
            if mat.category == sepika_core::model::MaterialCategory::Concrete && mat.fc.is_some() {
                sepika_core::face_distance::girder_self_weight_interval(model, e)?
            } else {
                [0.0, model.member_length(e)]
            }
        }
        SupportMemberId::Secondary(id) => {
            let sm = model
                .secondary_member(id)
                .ok_or_else(|| fail("小梁がありません"))?;
            if sm.kind != SecondaryMemberKind::Beam || model.secondary_member_materialized(sm) {
                return Ok(Vec::new());
            }
            sepika_core::face_distance::secondary_self_weight_interval(model, sm)?
        }
    };
    let mut relevant = Vec::new();
    for region in &model.floor_assignment_regions.regions {
        for edge in &region.boundary {
            let mut edge = *edge;
            if edge.support != member {
                let (SupportMemberId::Primary(id), SupportMemberId::Secondary(secondary)) =
                    (member, edge.support)
                else {
                    continue;
                };
                let Some(sm) = model.secondary_member(secondary) else {
                    continue;
                };
                if !model.secondary_member_materialized(sm) {
                    continue;
                }
                let e = model.element(id).ok_or_else(|| fail("部材がありません"))?;
                if sm.section != e.section
                    || model.secondary_material(sm).map(|m| m.id)
                        != model.element_material(e).map(|m| m.id)
                    || sm.vertical_eccentricity != e.vertical_eccentricity
                {
                    continue;
                }
                let Some((a, b)) = model.secondary_member_end_points(sm) else {
                    continue;
                };
                let (Some(p), Some(q)) = (
                    e.nodes.first().and_then(|id| model.node(*id)),
                    e.nodes.last().and_then(|id| model.node(*id)),
                ) else {
                    continue;
                };
                let near = |a, b| {
                    sepika_core::geom::vec3::dist(a, b) <= sepika_core::geom::MEMBER_AXIS_TOL_MM
                };
                if near(a, p.coord) && near(b, q.coord) {
                } else if near(a, q.coord) && near(b, p.coord) {
                    edge.span = edge.span.map(|t| 1.0 - t);
                } else {
                    continue;
                }
                edge.support = member;
            }
            relevant.push((region, edge));
        }
    }
    let mut attached = Vec::new();
    if let SupportMemberId::Primary(id) = member {
        for slab in model.slabs.iter().filter(|s| {
            matches!(
                s.shape,
                SlabShape::Attached {
                    anchor: sepika_core::model::RegionAnchor::Line { .. },
                    ..
                }
            )
        }) {
            for support in model
                .attached_slab_supports(slab)
                .map_err(|e| fail(&e.to_string()))?
            {
                if support.elem == id {
                    attached.push((slab.id, support.span));
                }
            }
        }
    }
    if attached.is_empty()
        && (relevant.is_empty()
            || relevant
                .iter()
                .all(|(r, _)| r.assignment == PlateAssignment::NoPlate))
    {
        return Ok(Vec::new());
    }
    let beam = model.beam_placement(member)?;
    let len = sepika_core::geom::vec3::dist(beam.actual_ends[0], beam.actual_ends[1]);
    let plan_ratio = beam.axes[0][0].hypot(beam.axes[0][1]);
    let width = beam.plan_width_mm()?;
    if ![len, plan_ratio, width]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
        || !interval.iter().all(|v| v.is_finite())
        || interval[1] <= interval[0]
    {
        return Err(fail("長さ・水平幅・自重算定区間が不正です"));
    }
    let mut candidates = Vec::new();
    let mut cuts = interval.to_vec();
    cuts.extend(
        beam.top_surface_breaks_mm()?
            .into_iter()
            .filter(|s| *s > interval[0] && *s < interval[1]),
    );
    for (region, edge) in relevant {
        if !edge
            .span
            .iter()
            .all(|t| t.is_finite() && (0.0..=1.0).contains(t))
            || edge.span[0] == edge.span[1]
        {
            return Err(fail(&format!(
                "床板割当領域 {:?} の支持区間が不正です",
                region.id
            )));
        }
        let span = [
            edge.span[0].min(edge.span[1]) * len,
            edge.span[0].max(edge.span[1]) * len,
        ];
        let span = [span[0].max(interval[0]), span[1].min(interval[1])];
        if span[1] <= span[0] {
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
            return Err(fail(&format!("床板 {} の割当が矛盾しています", id.0)));
        }
        let placement = model.resolved_slab_placement(id).map_err(|e| fail(&e))?;
        let mat = model
            .slab_plate_material(slab)
            .ok_or_else(|| fail(&format!("床板 {} の材料を解決できません", id.0)))?;
        let gamma = mat.design_unit_weight_n_per_mm3();
        if ![mat.density, gamma]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
        {
            return Err(fail(&format!(
                "床板 {}、材料 {} の物理密度または設計単位重量が非有限・負です",
                id.0, mat.id.0
            )));
        }
        mat.validate_design_self_weight(&format!("床板 {}、梁 {member:?}", id.0), "床板嵩上げ")?;
        let edges = model
            .support_boundary_segments(&region.boundary)
            .ok_or_else(|| fail(&format!("床板 {} の支持境界が未解決です", id.0)))?;
        if edges.len() < 3
            || !edges.iter().enumerate().all(|(i, (a, b))| {
                let next = edges[(i + 1) % edges.len()].0;
                a.iter().chain(b.iter()).all(|v| v.is_finite())
                    && (b[0] - next[0]).hypot(b[1] - next[1])
                        <= sepika_core::geom::MEMBER_AXIS_TOL_MM
            })
        {
            return Err(fail(&format!("床板 {} の支持境界が不正です", id.0)));
        }
        let winding: f64 = edges.iter().map(|(p, q)| p[0] * q[1] - q[0] * p[1]).sum();
        if !winding.is_finite() || winding == 0.0 {
            return Err(fail(&format!("床板 {} の平面方向が不正です", id.0)));
        }
        let side = usize::from((edge.span[1] - edge.span[0]) * winding < 0.0);
        cuts.extend_from_slice(&span);
        candidates.push(Candidate {
            slab: id,
            side,
            span,
            bottom: placement.bottom_z_mm,
            material: mat.id,
            gamma,
            density: mat.density,
            source: material_source(model, id).map_err(|e| fail(&e))?,
        });
    }
    for (id, span) in attached {
        let span = [span[0].min(span[1]) * len, span[0].max(span[1]) * len];
        let span = [span[0].max(interval[0]), span[1].min(interval[1])];
        if span[1] <= span[0] {
            continue;
        }
        let slab = model
            .slab(id)
            .ok_or_else(|| fail(&format!("床板 {} の参照先がありません", id.0)))?;
        let placement = model.resolved_slab_placement(id).map_err(|e| fail(&e))?;
        let mat = model
            .slab_plate_material(slab)
            .ok_or_else(|| fail(&format!("床板 {} の材料を解決できません", id.0)))?;
        let gamma = mat.design_unit_weight_n_per_mm3();
        if ![mat.density, gamma]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
        {
            return Err(fail(&format!(
                "床板 {}、材料 {} の物理密度または設計単位重量が非有限・負です",
                id.0, mat.id.0
            )));
        }
        mat.validate_design_self_weight(&format!("床板 {}、梁 {member:?}", id.0), "床板嵩上げ")?;
        let distance: f64 = placement
            .top_boundary
            .iter()
            .map(|p| {
                beam.axes[0][0] * (p[1] - beam.actual_ends[0][1])
                    - beam.axes[0][1] * (p[0] - beam.actual_ends[0][0])
            })
            .sum();
        if !distance.is_finite() || distance == 0.0 {
            return Err(fail(&format!("床板 {} の取付き側が不正です", id.0)));
        }
        cuts.extend_from_slice(&span);
        candidates.push(Candidate {
            slab: id,
            side: usize::from(distance < 0.0),
            span,
            bottom: placement.bottom_z_mm,
            material: mat.id,
            gamma,
            density: mat.density,
            source: material_source(model, id).map_err(|e| fail(&e))?,
        });
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut result = Vec::new();
    for pair in cuts.windows(2) {
        let span = [pair[0], pair[1]];
        let mut active: [Option<&Candidate>; 2] = [None, None];
        for (side, selected) in active.iter_mut().enumerate() {
            for c in candidates
                .iter()
                .filter(|c| c.side == side && c.span[0] < span[1] && span[0] < c.span[1])
            {
                if let Some(previous) = selected.as_ref().filter(|previous| previous.slab != c.slab)
                {
                    return Err(fail(&format!(
                        "同じ側 {side} の床板 {} と {} が区間 {}〜{} mmで矛盾しています",
                        previous.slab.0, c.slab.0, span[0], span[1]
                    )));
                }
                *selected = Some(c);
            }
        }
        let occupied = active.iter().flatten().count();
        if occupied == 0 {
            continue;
        }
        let z = beam.top_surface_on_interval_mm(span).map_err(|reason| {
            let slabs: Vec<_> = active.iter().flatten().map(|c| c.slab.0).collect();
            fail(&format!(
                "床板 {slabs:?}、区間 {}〜{} mm: {reason}",
                span[0], span[1]
            ))
        })?;
        for c in active.into_iter().flatten() {
            let gaps = z.map(|v| c.bottom - v);
            if !gaps.iter().all(|v| v.is_finite()) {
                return Err(fail(&format!("床板 {} の空き高さが非有限です", c.slab.0)));
            }
            let difference = gaps[0] - gaps[1];
            if !difference.is_finite() {
                return Err(fail("空き高さの変化が非有限です"));
            }
            let mut local = span.to_vec();
            if gaps[0] != 0.0
                && gaps[1] != 0.0
                && gaps[0].is_sign_positive() != gaps[1].is_sign_positive()
            {
                local.push(span[0] + (gaps[0] / difference) * (span[1] - span[0]));
            }
            local.sort_by(f64::total_cmp);
            for part in local.windows(2) {
                let gap = std::array::from_fn(|i| {
                    gaps[0] - difference * ((part[i] - span[0]) / (span[1] - span[0]))
                });
                if !gap.iter().all(|v| v.is_finite()) {
                    return Err(fail("空き高さが非有限です"));
                }
                let gap = gap.map(|h| h.max(0.0));
                let width_mm = width / occupied as f64;
                let area = gap.map(|h| width_mm * h * plan_ratio);
                let volume_mm3 = (area[0] + area[1]) * (part[1] - part[0]) / 2.0;
                let design_weight_n = volume_mm3 * c.gamma;
                let physical_mass_t = volume_mm3 * c.density;
                let mass_equivalent_weight_n = physical_mass_t * GRAVITY_MM_S2;
                let load = |unit: f64| MemberLoadKind::Distributed {
                    a: part[0],
                    b: part[1],
                    w1: area[0] * unit,
                    w2: area[1] * unit,
                };
                if ![
                    volume_mm3,
                    design_weight_n,
                    physical_mass_t,
                    mass_equivalent_weight_n,
                ]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0)
                {
                    return Err(fail(&format!(
                        "床板 {}、材料 {} の嵩上げ体積・重量・質量が不正です",
                        c.slab.0, c.material.0
                    )));
                }
                result.push(BeamUpstandSegment {
                    beam: member,
                    slab: c.slab,
                    material: c.material,
                    stb_strength_source: c.source,
                    design_unit_weight_n_per_mm3: c.gamma,
                    physical_density_t_per_mm3: c.density,
                    side: c.side,
                    interval_mm: [part[0], part[1]],
                    width_mm,
                    gap_mm: gap,
                    volume_mm3,
                    design_weight_n,
                    physical_mass_t,
                    mass_equivalent_weight_n,
                    design_load: load(c.gamma),
                    mass_equivalent_load: load(c.density * GRAVITY_MM_S2),
                });
            }
        }
    }
    Ok(result)
}
