use super::StbError;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use sepika_core::model::{Model, StbSectionStrength, StrengthTarget};

pub(super) fn steel_element_key(tag: &str) -> &str {
    match tag {
        "StbSecSteelColumn_S_Same" | "StbSecSteelBeam_S_Straight" | "StbSecSteelBrace_S_Same" => {
            "uniform-steel"
        }
        other => other,
    }
}
pub(super) fn section(xml: &str, input: &StbSectionStrength) -> Result<String, StbError> {
    let xml = replace_rebar(xml, input)?;
    let mut reader = Reader::from_str(&xml);
    let mut writer = Writer::new(Vec::new());
    let mut depth = 0usize;
    let mut represented = Vec::new();
    let mut skip_depth = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| StbError::Parse(e.to_string()))?;
        if event == Event::Eof {
            break;
        }
        if skip_depth > 0 {
            match &event {
                Event::Start(_) => skip_depth += 1,
                Event::End(_) => skip_depth -= 1,
                _ => {}
            }
            continue;
        }
        if input.reinforcement.is_empty() && input.rebar.is_none() && !input.native_rebar {
            match &event {
                Event::Start(e) if e.name().as_ref().starts_with(b"StbSecBarArrangement") => {
                    skip_depth = 1;
                    continue;
                }
                Event::Empty(e) if e.name().as_ref().starts_with(b"StbSecBarArrangement") => {
                    continue
                }
                _ => {}
            }
        }
        let event = match event {
            Event::Start(e) => {
                represented.push(element_position(&e)?);
                let result = attributes(&e, input, depth == 0)?;
                depth += 1;
                Event::Start(result)
            }
            Event::Empty(e) => {
                represented.push(element_position(&e)?);
                Event::Empty(attributes(&e, input, depth == 0)?)
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                Event::End(e.into_owned())
            }
            event => event.into_owned(),
        };
        writer
            .write_event(event)
            .map_err(|e| StbError::Io(e.to_string()))?;
    }
    if input
        .reinforcement
        .iter()
        .any(|r| !represented.contains(&(r.element.clone(), r.position.clone())))
    {
        return Err(StbError::Unmappable(format!(
            "断面 {} の位置別鉄筋強度を現行配筋出力で表現できません",
            input.section.0
        )));
    }
    if input.steel.iter().any(|r| {
        r.position.is_some()
            || !represented
                .iter()
                .any(|(tag, _)| steel_element_key(tag) == steel_element_key(&r.element))
    }) {
        return Err(StbError::Unmappable(format!(
            "断面 {} の部位別鋼材強度を現行出力で表現できません",
            input.section.0
        )));
    }
    String::from_utf8(writer.into_inner()).map_err(|e| StbError::Decode(e.to_string()))
}
fn attributes(
    e: &BytesStart<'_>,
    input: &StbSectionStrength,
    root: bool,
) -> Result<BytesStart<'static>, StbError> {
    let (tag, position) = element_position(e)?;
    let mut pairs = Vec::new();
    let steel = input
        .steel
        .iter()
        .any(|r| steel_element_key(&r.element) == steel_element_key(&tag));
    for a in e.attributes() {
        let a = a.map_err(|e| StbError::Parse(e.to_string()))?;
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let discard = if root {
            key == "strength_concrete" || key == "strength_steel"
        } else {
            (tag.starts_with("StbSecBar")
                && !input.native_rebar
                && (key.starts_with("strength_") || key.starts_with("D_")))
                || (steel && (key.starts_with("strength_") || key == "strength"))
        };
        if !discard {
            pairs.push((
                key,
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map_err(|e| StbError::Parse(e.to_string()))?
                    .into_owned(),
            ));
        }
    }
    if root {
        if let Some(grade) = &input.concrete {
            pairs.push(("strength_concrete".into(), grade.clone()));
        }
    } else if tag.starts_with("StbSecBar") {
        for r in input.reinforcement.iter().filter(|r| {
            steel_element_key(&r.element) == steel_element_key(&tag) && r.position == position
        }) {
            if let Some(d) = &r.diameter {
                pairs.push((format!("D_{}", r.part), d.clone()));
            }
            if let Some(g) = &r.strength {
                pairs.push((format!("strength_{}", r.part), g.clone()));
            }
        }
    }
    for r in input.steel.iter().filter(|r| {
        steel_element_key(&r.element) == steel_element_key(&tag) && r.position == position
    }) {
        pairs.push((
            if r.part.is_empty() {
                "strength".into()
            } else {
                format!("strength_{}", r.part)
            },
            r.strength.clone(),
        ));
    }
    let mut result = BytesStart::new(tag);
    for (key, value) in &pairs {
        result.push_attribute((key.as_str(), value.as_str()));
    }
    Ok(result.into_owned())
}

pub(super) fn member_attr(model: &Model, target: StrengthTarget) -> String {
    model
        .stb_strengths
        .members
        .iter()
        .find(|m| m.target == target)
        .and_then(|m| m.concrete.as_ref())
        .map(|g| format!(" strength_concrete=\"{}\"", super::export::esc(g)))
        .unwrap_or_default()
}

fn element_position(e: &BytesStart<'_>) -> Result<(String, Option<String>), StbError> {
    let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let mut position = None;
    for attr in e.attributes() {
        let attr = attr.map_err(|e| StbError::Parse(e.to_string()))?;
        if attr.key.as_ref() == b"pos" {
            position = Some(
                attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map_err(|e| StbError::Parse(e.to_string()))?
                    .into_owned(),
            );
        }
    }
    Ok((tag, position))
}

pub(super) fn raw_element(input: &sepika_core::model::StbRebarElement) -> String {
    let mut xml = format!("<{}", input.element);
    for (key, value) in &input.attributes {
        xml.push_str(&format!(" {key}=\"{}\"", super::export::esc(value)));
    }
    xml.push_str("/>");
    xml
}

fn replace_rebar(xml: &str, input: &StbSectionStrength) -> Result<String, StbError> {
    let Some(raw) = &input.rebar else {
        return Ok(xml.into());
    };
    raw.validate_standard()
        .map_err(|e| StbError::Unmappable(format!("断面 {}: {e}", input.section.0)))?;
    let mut result = xml.to_owned();
    let start = result.find("<StbSecBarArrangement").ok_or_else(|| {
        StbError::Unmappable(format!(
            "断面 {}: 原配筋の出力先がありません",
            input.section.0
        ))
    })?;
    let close = format!("</{}>", raw.container.element);
    let end = result[start..].find(&close).ok_or_else(|| {
        StbError::Unmappable(format!(
            "断面 {}: 原配筋系列と出力先が一致しません",
            input.section.0
        ))
    })? + start
        + close.len();
    let head = raw_element(&raw.container);
    let mut replacement = head[..head.len() - 2].to_owned();
    replacement.push('>');
    for part in &raw.positions {
        replacement.push_str(&raw_element(part));
    }
    for part in &raw.auxiliary {
        replacement.push_str(&raw_element(part));
    }
    replacement.push_str(&close);
    result.replace_range(start..end, &replacement);
    Ok(result)
}
