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

#[test]
fn exact_member_and_section_binding_keeps_coincident_members_distinct() {
    let xml = column(Some("Fc36"), Some("Fc30"), Some("Fc27"), Some("Fc24"))
        .replace("</StbColumns>", r#"<StbColumn id="2" name="C2" id_node_bottom="1" id_node_top="2" id_section="2" kind_structure="RC" strength_concrete="Fc33"/></StbColumns>"#)
        .replace("</StbSections>", r#"<StbSecColumn_RC id="2" name="C" strength_concrete="Fc21"><StbSecFigureColumn_RC><StbSecColumn_RC_Rect width_X="400" width_Y="400"/></StbSecFigureColumn_RC></StbSecColumn_RC></StbSections>"#);
    let model = import_stbridge(&xml).unwrap();
    assert_eq!(model.elements.len(), 2);
    assert_ne!(model.elements[0].section, model.elements[1].section);
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().fc,
        Some(36.)
    );
    assert_eq!(
        model.element_material(&model.elements[1]).unwrap().fc,
        Some(33.)
    );
    let mut model = model;
    model
        .stb_strengths
        .members
        .iter_mut()
        .for_each(|m| m.concrete = None);
    assert_eq!(
        model.element_material(&model.elements[0]).unwrap().fc,
        Some(30.)
    );
    assert_eq!(
        model.element_material(&model.elements[1]).unwrap().fc,
        Some(21.)
    );
}

#[test]
fn unknown_present_grade_never_uses_known_lower_grade() {
    for unknown in [
        "",
        "SN400UNKNOWN",
        "FcNaN",
        "Fcinf",
        "Fc0",
        "Fc-24",
        "Fc24BAD",
    ] {
        let model = import_stbridge(&column(
            Some(unknown),
            Some("Fc30"),
            Some("Fc27"),
            Some("Fc24"),
        ))
        .unwrap();
        assert!(
            model
                .resolve_stb_concrete(&model.stb_strengths.members[0])
                .is_err(),
            "{unknown}"
        );
        assert!(
            model.element_material(&model.elements[0]).is_none(),
            "{unknown}"
        );
    }
    let model = import_stbridge(&column(None, Some("Fc24"), None, None)).unwrap();
    for strength in [Some(""), Some("SD345BAD"), None] {
        let bar = StbRebarStrength {
            element: "StbSecBarColumn_RC_RectSame".into(),
            part: "main".into(),
            position: None,
            native_material: None,
            diameter: Some("D13".into()),
            strength: strength.map(str::to_owned),
        };
        assert!(model.resolve_stb_rebar(&bar).is_err());
    }
}

#[test]
fn designated_original_node_survives_order_level_and_prepare_changes() {
    use sepika_core::ids::NodeId;
    for (tag, first, second, expected) in [
        ("StbColumn", "id_node_bottom", "id_node_top", 27.),
        ("StbPost", "id_node_bottom", "id_node_top", 27.),
        ("StbGirder", "id_node_start", "id_node_end", 24.),
        ("StbBeam", "id_node_start", "id_node_end", 24.),
    ] {
        let source = column(None, None, Some("Fc27"), Some("Fc21"));
        let section_tag = if matches!(tag, "StbGirder" | "StbBeam") {
            "StbSecBeam_RC"
        } else {
            "StbSecColumn_RC"
        };
        let source = source.replace("<StbColumns>", "").replace("</StbColumns>", "").replace("StbColumn id=", &format!("{tag} id=")).replace("id_node_bottom=",&format!("{first}=")).replace("id_node_top=",&format!("{second}=")).replace("StbSecColumn_RC",section_tag)
            .replace("StbSecFigureColumn_RC", if section_tag=="StbSecBeam_RC" {"StbSecFigureBeam_RC"} else {"StbSecFigureColumn_RC"})
            .replace("StbSecColumn_RC_Rect width_X=\"400\" width_Y=\"400\"", if section_tag=="StbSecBeam_RC" {"StbSecBeam_RC_Straight width=\"400\" depth=\"400\""} else {"StbSecColumn_RC_Rect width_X=\"400\" width_Y=\"400\""})
            .replace("</StbStories>",r#"<StbStory id="2" name="bottom" height="0" kind="GENERAL" strength_concrete="Fc24"><StbNodeIdList><StbNodeId id="1"/></StbNodeIdList></StbStory></StbStories>"#);
        let mut model = import_stbridge(&source).unwrap();
        let input = &model.stb_strengths.members[0];
        assert_eq!(
            input.node,
            NodeId(if expected == 27. { 1 } else { 0 }),
            "{tag}"
        );
        assert_eq!(
            model.resolve_stb_concrete(input).unwrap().value,
            expected,
            "{tag}"
        );
        model.nodes[0].coord[2] = 9000.;
        model.nodes[1].coord[2] = -3000.;
        for sm in model
            .unassigned_beams
            .iter_mut()
            .chain(&mut model.unassigned_posts)
        {
            sm.ends = SecondaryMemberEnds::Detached([model.nodes[0].coord, model.nodes[1].coord]);
        }
        model.source_stories.reverse();
        model.prepare_stb_strength_materials();
        assert_eq!(
            model
                .resolve_stb_concrete(&model.stb_strengths.members[0])
                .unwrap()
                .value,
            expected,
            "{tag}"
        );
        let again = import_stbridge(&export_stbridge(&model).unwrap()).unwrap();
        assert_eq!(
            again
                .resolve_stb_concrete(&again.stb_strengths.members[0])
                .unwrap()
                .value,
            expected,
            "{tag}"
        );
    }
}

#[test]
fn ovika_preserves_raw_omission_and_arbitrary_properties() {
    let mut model = import_stbridge(&column(None, None, Some("Fc27"), Some("Fc24"))).unwrap();
    model.materials[0].young = 12345.;
    model.materials[0].poisson = 0.23;
    model.materials[0].density = 2345.;
    let path = std::env::temp_dir().join(format!("sepika-strength-{}.ovika", std::process::id()));
    crate::ovika::save_ovika(&path, &model, crate::ovika::OvikaExtras::default()).unwrap();
    let again = crate::ovika::load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    assert_eq!(model.stb_strengths, again.stb_strengths);
    assert_eq!(model.materials, again.materials);
    assert_eq!(model.source_stories, again.source_stories);
}

#[test]
fn split_slab_keeps_original_first_node_supply_and_refuses_unrepresentable_export() {
    let xml = include_str!("../../tests/fixtures/strength_split_slab.stb");
    let model = import_stbridge(xml).unwrap();
    assert_eq!(model.slabs.len(), 2);
    assert_eq!(
        model
            .stb_strengths
            .members
            .iter()
            .filter(|m| matches!(m.target, StrengthTarget::Slab(_)))
            .count(),
        2
    );
    for input in model
        .stb_strengths
        .members
        .iter()
        .filter(|m| matches!(m.target, StrengthTarget::Slab(_)))
    {
        assert_eq!(input.node, sepika_core::ids::NodeId(0));
        assert_eq!(model.resolve_stb_concrete(input).unwrap().value, 27.);
        let StrengthTarget::Slab(id) = input.target else {
            panic!("slab")
        };
        assert_eq!(
            model
                .slab_plate_material(&model.slabs[id.index()])
                .unwrap()
                .fc,
            Some(27.)
        );
    }
    assert!(model.validate().is_ok());
    assert!(
        matches!(export_stbridge(&model), Err(StbError::Unmappable(reason)) if reason.contains("元第1節点"))
    );
    let explicit = xml.replace(
        "name=\"S1\" id_section=\"8\" kind_structure=\"RC\"",
        "name=\"S1\" id_section=\"8\" kind_structure=\"RC\" strength_concrete=\"Fc36\"",
    );
    let model = import_stbridge(&explicit).unwrap();
    for input in model
        .stb_strengths
        .members
        .iter()
        .filter(|m| matches!(m.target, StrengthTarget::Slab(_)))
    {
        assert_eq!(model.resolve_stb_concrete(input).unwrap().value, 36.);
    }
    let output = export_stbridge(&model).unwrap();
    let again = import_stbridge(&output).unwrap();
    let slabs: Vec<_> = again
        .stb_strengths
        .members
        .iter()
        .filter(|input| matches!(input.target, StrengthTarget::Slab(_)))
        .collect();
    assert_eq!(slabs.len(), 2);
    for input in slabs {
        assert_eq!(input.concrete.as_deref(), Some("Fc36"));
        let resolved = again.resolve_stb_concrete(input).unwrap();
        assert_eq!(resolved.value, 36.);
        assert_eq!(resolved.source, StrengthSource::Member);
    }
    let conflict=xml.replace("</StbMembers>",r#"<StbSlab id="1" name="S2" id_section="8" kind_structure="RC" strength_concrete="Fc36"><StbNodeIdOrder>1 2 3 4</StbNodeIdOrder></StbSlab></StbMembers>"#);
    assert!(matches!(
        import_stbridge(&conflict),
        Err(StbError::SlabRegionConflict(_))
    ));
}

#[test]
fn qualified_numeric_section_id_is_rejected_without_silent_strength_binding() {
    let xml=column(None,Some("Fc30"),None,Some("Fc24")).replace("</StbSections>",r#"<StbSecBeam_RC id="1" name="B" strength_concrete="Fc24"><StbSecFigureBeam_RC><StbSecBeam_RC_Straight width="400" depth="600"/></StbSecFigureBeam_RC></StbSecBeam_RC></StbSections>"#);
    assert!(
        matches!(import_stbridge(&xml),Err(StbError::Unmappable(reason)) if reason.contains("系列"))
    );
}

#[test]
fn wall_last_node_and_explicit_member_omission_roundtrip() {
    let source = include_str!("../../tests/fixtures/strength_wall.stb");
    for (member, expected) in [(None, 27.), (Some("Fc36"), 36.)] {
        let source = if let Some(member) = member {
            source.replace("name=\"W\" id_section=\"3\" kind_structure=\"RC\"",&format!("name=\"W\" id_section=\"3\" kind_structure=\"RC\" strength_concrete=\"{member}\""))
        } else {
            source.to_owned()
        };
        let mut model = import_stbridge(&source).unwrap();
        assert_eq!(model.stb_strengths.members.len(), 1);
        let input = &model.stb_strengths.members[0];
        assert_eq!(input.node, sepika_core::ids::NodeId(2));
        assert_eq!(model.resolve_stb_concrete(input).unwrap().value, expected);
        assert_eq!(
            model.wall_plate_material(&model.wall_plates[0]).unwrap().fc,
            Some(expected)
        );
        model.source_stories.reverse();
        model.prepare_stb_strength_materials();
        let output = export_stbridge(&model).unwrap();
        let again = import_stbridge(&output).unwrap();
        assert_eq!(
            again.stb_strengths.members[0].concrete,
            member.map(str::to_owned)
        );
        assert_eq!(
            again
                .resolve_stb_concrete(&again.stb_strengths.members[0])
                .unwrap()
                .value,
            expected
        );
        assert_eq!(
            again.stb_strengths.members[0].node_order,
            model.stb_strengths.members[0].node_order
        );
    }
}

#[test]
fn src_standard_steel_child_survives_and_numeric_override_is_not_exported_as_grade() {
    let xml = include_str!("../../tests/fixtures/strength_src.stb");
    let mut model = import_stbridge(xml).unwrap();
    let input = &model.stb_strengths.sections[0].steel[0];
    assert_eq!(input.strength, "SN490B");
    assert_eq!(model.resolve_stb_steel(input).unwrap().value, 325.);
    let steel = model
        .section(model.elements[0].section.unwrap())
        .unwrap()
        .steel_material
        .unwrap();
    assert_eq!(model.materials[steel.index()].fy, Some(325.));
    let output = export_stbridge(&model).unwrap();
    assert!(output.contains("strength_main=\"SN490B\""));
    assert!(!output.contains("strength_steel="));
    let again = import_stbridge(&output).unwrap();
    assert_eq!(
        again.stb_strengths.sections[0].steel,
        model.stb_strengths.sections[0].steel
    );
    assert_eq!(
        again
            .resolve_stb_concrete(&again.stb_strengths.members[0])
            .unwrap()
            .value,
        36.
    );
    model.materials[steel.index()].fy = Some(320.);
    let resolved = model
        .resolve_stb_steel(&model.stb_strengths.sections[0].steel[0])
        .unwrap();
    assert!(resolved.native_override);
    assert_eq!(resolved.value, 320.);
    assert!(
        matches!(export_stbridge(&model), Err(StbError::Unmappable(reason)) if reason.contains("明示鋼材fy"))
    );
    let unknown = import_stbridge(&xml.replace("SN490B", "SN490UNKNOWN")).unwrap();
    assert!(unknown
        .stb_strength_diagnostics()
        .iter()
        .any(|r| r.contains("SN490UNKNOWN")));
}
