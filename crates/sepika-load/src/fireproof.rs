use sepika_core::model::{
    ElementData, ElementKind, FireproofKind, FrameSectionUse, Model, SecondaryMember,
    SecondaryMemberKind, Section,
};
use sepika_core::structure_kind::{structure_kind_of, StructureKind};

fn condition(
    story: &sepika_core::model::Story,
    kind: StructureKind,
    column: bool,
) -> (FireproofKind, f64) {
    let c = story.fireproof;
    if kind == StructureKind::Cft {
        (c.cft_kind, c.cft_column_area_weight)
    } else {
        (
            c.steel_kind,
            if column {
                c.steel_column_area_weight
            } else {
                c.steel_beam_area_weight
            },
        )
    }
}

fn enabled(model: &Model, kind: StructureKind, column: bool) -> bool {
    model.stories.iter().any(|story| {
        let (kind, q) = condition(story, kind, column);
        kind != FireproofKind::None && q != 0.0
    })
}

fn line_weight(
    model: &Model,
    section: &Section,
    kind: StructureKind,
    column: bool,
    z: f64,
) -> Result<f64, String> {
    if !enabled(model, kind, column) {
        return Ok(0.0);
    }
    let spans = model.story_spans();
    let story = model
        .story_at(&spans, z)
        .and_then(|id| model.stories.get(id.index()))
        .ok_or_else(|| {
            format!(
                "断面 {} の耐火被覆条件の所属階を解決できません",
                section.name
            )
        })?;
    let (coating, q) = condition(story, kind, column);
    if coating == FireproofKind::None {
        return Ok(0.0);
    }
    if !q.is_finite() || q < 0.0 {
        return Err(format!("階 {} の耐火被覆面重量が不正です", story.name));
    }
    if q == 0.0 {
        return Ok(0.0);
    }
    let shape = section
        .shape
        .as_ref()
        .ok_or_else(|| format!("断面 {} の耐火被覆周長を解決できません", section.name))?;
    let perimeter = match coating {
        FireproofKind::Spray => shape.coating_surface_perimeter(),
        FireproofKind::Board => shape.coating_envelope_perimeter(),
        FireproofKind::None => unreachable!(),
    }
    .map_err(|error| {
        format!(
            "階 {}・断面 {} の耐火被覆: {error}",
            story.name, section.name
        )
    })?;
    let w = q * perimeter;
    if !w.is_finite() {
        return Err("耐火被覆線重量が非有限値です".into());
    }
    Ok(w)
}

pub(crate) fn primary_line_weight(model: &Model, element: &ElementData) -> Result<f64, String> {
    if element.kind != ElementKind::Beam {
        return Ok(0.0);
    }
    let active = enabled(model, StructureKind::S, true)
        || enabled(model, StructureKind::S, false)
        || enabled(model, StructureKind::Cft, true);
    let Some(section) = model.element_section(element) else {
        return if active {
            Err(format!(
                "部材 {} の耐火被覆対象断面を解決できません",
                element.id.0
            ))
        } else {
            Ok(0.0)
        };
    };
    match section.frame_use {
        Some(FrameSectionUse::Brace) => return Ok(0.0),
        Some(FrameSectionUse::Girder) if !enabled(model, StructureKind::S, false) => {
            return Ok(0.0)
        }
        Some(FrameSectionUse::Column)
            if !enabled(model, StructureKind::S, true)
                && !enabled(model, StructureKind::Cft, true) =>
        {
            return Ok(0.0)
        }
        _ => {}
    }
    let material = model.element_material(element);
    if active
        && section.shape.is_none()
        && (material.is_none()
            || (section.steel_material.is_some()
                && section.frame_use != Some(FrameSectionUse::Girder)
                && enabled(model, StructureKind::Cft, true)))
    {
        return Err(format!(
            "部材 {} の耐火被覆対象構造種別を解決できません",
            element.id.0
        ));
    }
    let kind = structure_kind_of(Some(section), material.map(|m| m.category));
    if !matches!(kind, StructureKind::S | StructureKind::Cft) {
        return Ok(0.0);
    }
    let column = match section.frame_use {
        Some(FrameSectionUse::Column) => true,
        Some(FrameSectionUse::Girder) if kind == StructureKind::S => false,
        Some(_) => return Ok(0.0),
        None => {
            return if active {
                Err(format!(
                    "部材 {} の耐火被覆対象用途を解決できません",
                    element.id.0
                ))
            } else {
                Ok(0.0)
            }
        }
    };
    if !enabled(model, kind, column) {
        return Ok(0.0);
    }
    let z = element
        .nodes
        .iter()
        .map(|id| model.nodes.get(id.index()).map(|n| n.coord[2]))
        .collect::<Option<Vec<_>>>()
        .filter(|z| !z.is_empty() && z.iter().all(|value| value.is_finite()))
        .ok_or_else(|| format!("部材 {} の耐火被覆算定節点を解決できません", element.id.0))?
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);
    let w = line_weight(model, section, kind, column, z)?;
    if w > 0.0 && material.is_none() {
        return Err(format!("部材 {} の自重材料を解決できません", element.id.0));
    }
    Ok(w)
}

pub(crate) fn secondary_line_weight(
    model: &Model,
    member: &SecondaryMember,
) -> Result<f64, String> {
    if member.kind != SecondaryMemberKind::Beam || !enabled(model, StructureKind::S, false) {
        return Ok(0.0);
    }
    let section = member
        .section
        .and_then(|id| model.sections.get(id.index()))
        .ok_or_else(|| format!("小梁 {} の耐火被覆対象断面を解決できません", member.id.0))?;
    let material = model.secondary_material(member);
    if section.shape.is_none() && material.is_none() {
        return Err(format!(
            "小梁 {} の耐火被覆対象構造種別を解決できません",
            member.id.0
        ));
    }
    if structure_kind_of(Some(section), material.map(|m| m.category)) != StructureKind::S {
        return Ok(0.0);
    }
    let (a, b, _) = model
        .secondary_member_axis(member)
        .ok_or_else(|| format!("小梁 {} の耐火被覆算定材軸を解決できません", member.id.0))?;
    let w = line_weight(model, section, StructureKind::S, false, a[2].max(b[2]))?;
    if w > 0.0 && material.is_none() {
        return Err(format!("小梁 {} の自重材料を解決できません", member.id.0));
    }
    Ok(w)
}
