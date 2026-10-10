use super::*;
use sepika_core::model::*;

fn column(
    member: Option<&str>,
    section: Option<&str>,
    story: Option<&str>,
    common: Option<&str>,
) -> String {
    let attr = |grade: Option<&str>| {
        grade
            .map(|g| format!(" strength_concrete=\"{g}\""))
            .unwrap_or_default()
    };
    format!(
        r#"<ST_BRIDGE version="2.0.2"><StbCommon project_name="test" app_name="test" app_version="1"{common}/><StbModel>
<StbNodes><StbNode id="1" X="0" Y="0" Z="0"/><StbNode id="2" X="0" Y="0" Z="3000"/></StbNodes>
<StbStories><StbStory id="1" name="top" height="3000" kind="GENERAL"{story}><StbNodeIdList><StbNodeId id="2"/></StbNodeIdList></StbStory></StbStories>
<StbMembers><StbColumns><StbColumn id="1" name="C" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="RC"{member}/></StbColumns></StbMembers>
<StbSections><StbSecColumn_RC id="1" name="C"{section}><StbSecFigureColumn_RC><StbSecColumn_RC_Rect width_X="400" width_Y="400"/></StbSecFigureColumn_RC></StbSecColumn_RC></StbSections>
</StbModel></ST_BRIDGE>"#,
        common = attr(common),
        story = attr(story),
        member = attr(member),
        section = attr(section)
    )
}
#[test]
fn concrete_priority_omissions_and_unknown_upper_are_independent() {
    for (member, section, story, common, expected, source) in [
        (
            Some("Fc36"),
            Some("Fc30"),
            Some("Fc27"),
            Some("Fc24"),
            36.,
            StrengthSource::Member,
        ),
        (
            None,
            Some("Fc30"),
            Some("Fc27"),
            Some("Fc24"),
            30.,
            StrengthSource::Section,
        ),
        (
            None,
            None,
            Some("Fc27"),
            Some("Fc24"),
            27.,
            StrengthSource::Story,
        ),
        (None, None, None, Some("Fc24"), 24., StrengthSource::Common),
    ] {
        let m = import_stbridge(&column(member, section, story, common)).unwrap();
        let resolved = m.resolve_stb_concrete(&m.stb_strengths.members[0]).unwrap();
        assert_eq!(resolved.value, expected);
        assert_eq!(resolved.source, source);
        assert_eq!(
            m.element_material(&m.elements[0]).unwrap().fc,
            Some(expected)
        );
        let xml = export_stbridge(&m).unwrap();
        let again = import_stbridge(&xml).unwrap();
        assert_eq!(
            again.stb_strengths.members[0].concrete,
            member.map(str::to_owned)
        );
        assert_eq!(
            again.stb_strengths.sections[0].concrete,
            section.map(str::to_owned)
        );
        assert_eq!(
            again
                .resolve_stb_concrete(&again.stb_strengths.members[0])
                .unwrap()
                .value,
            expected
        );
    }
    for member in [None, Some("UNKNOWN")] {
        let m = import_stbridge(&column(
            member,
            None,
            None,
            if member.is_some() { Some("Fc24") } else { None },
        ))
        .unwrap();
        assert!(m.resolve_stb_concrete(&m.stb_strengths.members[0]).is_err());
        assert!(m.element_material(&m.elements[0]).is_none());
        assert!(!m.stb_strength_diagnostics().is_empty());
    }
}
#[test]
fn story_competition_does_not_block_upper_strength() {
    for (member, section, expected) in [
        (Some("Fc36"), Some("Fc30"), Some(36.)),
        (None, Some("Fc30"), Some(30.)),
        (None, None, None),
    ] {
        let xml=column(member,section,Some("Fc27"),Some("Fc21")).replace("</StbStories>",r#"<StbStory id="2" name="other" height="3100" kind="GENERAL" strength_concrete="Fc24"><StbNodeIdList><StbNodeId id="2"/></StbNodeIdList></StbStory></StbStories>"#);
        let m = import_stbridge(&xml).unwrap();
        let resolved = m.resolve_stb_concrete(&m.stb_strengths.members[0]);
        assert_eq!(resolved.ok().map(|r| r.value), expected);
        assert!(!m.source_story_diagnostics().is_empty());
        assert_eq!(m.stb_strength_diagnostics().is_empty(), expected.is_some());
    }
}
#[test]
fn diameter_and_individual_strength_keep_all_parts_separate() {
    let xml=column(None,Some("Fc24"),None,None).replace("app_version=\"1\"/>",r#"app_version="1"><StbReinforcementStrengthList><StbReinforcementStrength D="D13" strength="SD295A"/><StbReinforcementStrength D="D25" strength="SD390"/></StbReinforcementStrengthList></StbCommon>"#).replace("</StbSecFigureColumn_RC>",r#"</StbSecFigureColumn_RC><StbSecBarArrangementColumn_RC depth_cover="40"><StbSecBarColumn_RC_RectSame D_main="D25" D_2nd_main="D13" strength_2nd_main="SD345" D_band="D13" D_bar_spacing="D25" N_main_X_1st="3" N_main_Y_1st="3" N_main_total="8" pitch_band="100" N_band_direction_X="2" N_band_direction_Y="2"/></StbSecBarArrangementColumn_RC>"#);
    let m = import_stbridge(&xml).unwrap();
    for (part, grade, value, source) in [
        ("main", "SD390", 390., StrengthSource::Diameter),
        ("2nd_main", "SD345", 345., StrengthSource::Section),
        ("band", "SD295A", 295., StrengthSource::Diameter),
        ("bar_spacing", "SD390", 390., StrengthSource::Diameter),
    ] {
        let bar = m.stb_strengths.sections[0]
            .reinforcement
            .iter()
            .find(|r| r.part == part)
            .unwrap();
        let resolved = m.resolve_stb_rebar(bar).unwrap();
        assert_eq!(resolved.grade, grade);
        assert_eq!(resolved.value, value);
        assert_eq!(resolved.source, source);
    }
    assert_eq!(
        m.element_rebar_material(&m.elements[0]).unwrap().fy,
        Some(390.)
    );
    assert_eq!(
        m.element_shear_rebar_material(&m.elements[0]).unwrap().fy,
        Some(295.)
    );
    let output = export_stbridge(&m).unwrap();
    assert!(!output.contains("strength_main=\"SD390\""));
    let again = import_stbridge(&output).unwrap();
    assert_eq!(again.stb_strengths.common, m.stb_strengths.common);
    assert_eq!(
        again.stb_strengths.sections[0].reinforcement,
        m.stb_strengths.sections[0].reinforcement
    );
    let conflict = xml.replace(
        "</StbReinforcementStrengthList>",
        r#"<StbReinforcementStrength D="D13" strength="SD345"/></StbReinforcementStrengthList>"#,
    );
    let m = import_stbridge(&conflict).unwrap();
    assert!(m
        .stb_strength_diagnostics()
        .iter()
        .any(|e| e.contains("競合")));
}
