use sepika_core::{
    ids::{ElemId, SectionId},
    model::{FrameSectionUse, Model, StrengthSource, StrengthTarget},
};
use sepika_io::{
    ovika::{load_ovika, save_ovika, OvikaExtras},
    stbridge::{import_stbridge, import_stbridge_with_report},
};

const COLUMN: &str = r#"<StbSecColumn_RC id="1" name="C1" strength_concrete="Fc30"><StbSecFigureColumn_RC><StbSecColumn_RC_Rect width_X="500" width_Y="500"/></StbSecFigureColumn_RC></StbSecColumn_RC>"#;
const GIRDER: &str = r#"<StbSecBeam_RC id="1" name="G1" strength_concrete="Fc24"><StbSecFigureBeam_RC><StbSecBeam_RC_Straight width="300" depth="600"/></StbSecFigureBeam_RC></StbSecBeam_RC>"#;
const MEMBERS: &str = r#"<StbColumns><StbColumn id="1" name="C1" id_node_bottom="10" id_node_top="20" id_section="1" kind_structure="RC"/></StbColumns><StbGirders><StbGirder id="1" name="G1" id_node_start="20" id_node_end="30" id_section="1" kind_structure="RC" isFoundation="false"/></StbGirders>"#;

fn input(members: &str, sections: &str) -> String {
    format!(
        r#"<ST_BRIDGE xmlns="https://www.building-smart.or.jp/dl" version="2.0.2">
<StbCommon project_name="independent" app_name="fixture" app_version="1"/>
<StbModel><StbNodes>
<StbNode id="10" X="0" Y="0" Z="0" kind="ON_GRID"/>
<StbNode id="20" X="0" Y="0" Z="3000" kind="ON_GRID"/>
<StbNode id="30" X="6000" Y="0" Z="3000" kind="ON_GRID"/>
<StbNode id="40" X="6000" Y="4000" Z="3000" kind="ON_GRID"/>
<StbNode id="50" X="6000" Y="-4000" Z="3000" kind="ON_GRID"/>
</StbNodes><StbMembers>{members}</StbMembers><StbSections>{sections}</StbSections>
</StbModel></ST_BRIDGE>"#
    )
}

fn both_imports(xml: &str) -> [Model; 2] {
    [
        import_stbridge(xml).unwrap(),
        import_stbridge_with_report(xml).unwrap().0,
    ]
}

fn rejected_by_both(xml: &str, details: &[&str]) {
    let plain = import_stbridge(xml).unwrap_err().to_string();
    let reported = import_stbridge_with_report(xml).unwrap_err().to_string();
    assert_eq!(plain, reported);
    for detail in details {
        assert!(plain.contains(detail), "{detail}: {plain}");
    }
}

fn assert_column_and_girder(model: &Model) {
    assert_eq!(model.elements.len(), 2);
    let mut sections = Vec::new();
    for (usage, expected_name, fc) in [
        (FrameSectionUse::Column, "C1", 30.),
        (FrameSectionUse::Girder, "G1", 24.),
    ] {
        let element = model
            .elements
            .iter()
            .find(|element| {
                model.section(element.section.unwrap()).unwrap().frame_use == Some(usage)
            })
            .unwrap();
        let section = model.section(element.section.unwrap()).unwrap();
        assert_eq!(section.name, expected_name);
        assert_eq!(model.element_material(element).unwrap().fc, Some(fc));
        let strength = model
            .stb_strengths
            .members
            .iter()
            .find(|strength| strength.target == StrengthTarget::Element(element.id))
            .unwrap();
        let resolved = model.resolve_stb_concrete(strength).unwrap();
        assert_eq!(resolved.value, fc);
        assert_eq!(resolved.source, StrengthSource::Section);
        sections.push(section.id);
    }
    assert_ne!(sections[0], sections[1]);
}

#[test]
fn independent_column_and_girder_with_same_numeric_id_keep_their_own_fc() {
    let unused_column = COLUMN
        .replace("id=\"1\"", "id=\"2\"")
        .replace("name=\"C1\"", "name=\"C2\"")
        .replace("Fc30", "Fc27");
    for sections in [
        format!("{COLUMN}{unused_column}{GIRDER}"),
        format!("{unused_column}{COLUMN}{GIRDER}"),
    ] {
        for model in both_imports(&input(MEMBERS, &sections)) {
            assert_column_and_girder(&model);
        }
    }
}

#[test]
fn sparse_original_ids_and_leading_zero_references_keep_their_own_fc() {
    for (column_id, beam_id) in [("101", "701"), ("01", "001")] {
        let members = MEMBERS.replacen(
            "id_section=\"1\"",
            &format!("id_section=\"{column_id}\""),
            1,
        );
        let members = members.replacen("id_section=\"1\"", &format!("id_section=\"{beam_id}\""), 1);
        let column = if column_id == "101" {
            COLUMN.replace("id=\"1\"", "id=\"101\"")
        } else {
            COLUMN.into()
        };
        let girder = if beam_id == "701" {
            GIRDER.replace("id=\"1\"", "id=\"701\"")
        } else {
            GIRDER.into()
        };
        for model in both_imports(&input(&members, &format!("{column}{girder}"))) {
            assert_column_and_girder(&model);
        }
    }
}

#[test]
fn ovika_preserves_binding_after_actual_internal_section_and_element_renumbering() {
    for (index, mut model) in both_imports(&input(MEMBERS, &format!("{COLUMN}{GIRDER}")))
        .into_iter()
        .enumerate()
    {
        let previous = model.elements[0].section.unwrap();
        model.visit_section_ids(|id| *id = SectionId(1 - id.0));
        model.sections.reverse();
        model.visit_elem_ids(|id| *id = ElemId(1 - id.0));
        model.elements.reverse();
        assert_ne!(model.elements[1].section.unwrap(), previous);
        assert_column_and_girder(&model);
        let path = std::env::temp_dir().join(format!(
            "sepika-571-series-{}-{index}.ovika",
            std::process::id()
        ));
        save_ovika(&path, &model, OvikaExtras::default()).unwrap();
        let loaded = load_ovika(&path).unwrap().model;
        std::fs::remove_file(path).unwrap();
        assert_eq!(loaded.stb_strengths, model.stb_strengths);
        assert_column_and_girder(&loaded);
    }
}

#[test]
fn same_name_same_shape_sections_with_different_fc_stay_distinct() {
    let other = COLUMN
        .replace("id=\"1\"", "id=\"2\"")
        .replace("Fc30", "Fc24");
    let columns = r#"<StbColumns><StbColumn id="11" name="C30" id_node_bottom="10" id_node_top="20" id_section="1" kind_structure="RC"/><StbColumn id="12" name="C24" id_node_bottom="10" id_node_top="20" id_section="2" kind_structure="RC"/></StbColumns>"#;
    for sections in [format!("{COLUMN}{other}"), format!("{other}{COLUMN}")] {
        for model in both_imports(&input(columns, &sections)) {
            assert_eq!(model.elements.len(), 2);
            assert_ne!(model.elements[0].section, model.elements[1].section);
            assert_eq!(
                model.element_material(&model.elements[0]).unwrap().fc,
                Some(30.)
            );
            assert_eq!(
                model.element_material(&model.elements[1]).unwrap().fc,
                Some(24.)
            );
        }
    }
}

#[test]
fn normalized_duplicate_ids_are_rejected_within_the_same_concrete_series() {
    for duplicate in [COLUMN.to_owned(), COLUMN.replace("id=\"1\"", "id=\"01\"")] {
        rejected_by_both(
            &input(MEMBERS, &format!("{COLUMN}{duplicate}{GIRDER}")),
            &["StbSecColumn_RC", "1", "重複"],
        );
    }
}

#[test]
fn missing_requested_family_never_uses_another_family_with_the_same_id() {
    for sections in [GIRDER.to_owned(), GIRDER.replace("id=\"1\"", "id=\"2\"")] {
        rejected_by_both(
            &input(MEMBERS, &sections),
            &[
                "StbColumn",
                "id=Some(1)",
                "RC",
                "StbSecColumn_RC",
                "id_section=Some(\"1\")",
                "断面参照",
            ],
        );
    }
}

#[test]
fn missing_invalid_and_unsupported_structure_kind_have_distinct_reasons() {
    for (replacement, reason, kind) in [
        ("", "ありません", "kind_structure"),
        ("kind_structure=\"INVALID\"", "不正", "INVALID"),
        ("kind_structure=\"UNDEFINED\"", "未対応", "UNDEFINED"),
    ] {
        let members = MEMBERS.replacen("kind_structure=\"RC\"", replacement, 1);
        let undefined = if kind == "UNDEFINED" {
            r#"<StbSecUndefined id="1" name="U1"/>"#
        } else {
            ""
        };
        rejected_by_both(
            &input(&members, &format!("{COLUMN}{GIRDER}{undefined}")),
            &[
                "StbColumn",
                "id=Some(1)",
                "id_section=Some(\"1\")",
                kind,
                reason,
            ],
        );
    }
}

#[test]
fn concrete_steel_src_and_cft_columns_and_beams_resolve_their_specific_series() {
    let steel_column = r#"<StbSecColumn_S id="1" name="CS"><StbSecSteelFigureColumn_S><StbSecSteelColumn_S_Same shape="H1" strength_main="SN400B"/></StbSecSteelFigureColumn_S></StbSecColumn_S>"#;
    let steel_beam = r#"<StbSecBeam_S id="1" name="GS" kind_beam="GIRDER" isCanti="false"><StbSecSteelFigureBeam_S><StbSecSteelBeam_S_Straight shape="H1" strength_main="SN490B"/></StbSecSteelFigureBeam_S></StbSecBeam_S>"#;
    let src_column = r#"<StbSecColumn_SRC id="1" name="CSRC" strength_concrete="Fc33"><StbSecFigureColumn_SRC><StbSecColumn_SRC_Rect width_X="600" width_Y="600"/></StbSecFigureColumn_SRC><StbSecSteelFigureColumn_SRC><StbSecSteelColumn_SRC_Same><StbSecColumn_SRC_SameShapeH direction_type="H" shape="H1" strength_main="SN490B"/></StbSecSteelColumn_SRC_Same></StbSecSteelFigureColumn_SRC></StbSecColumn_SRC>"#;
    let src_beam = r#"<StbSecBeam_SRC id="1" name="GSRC" strength_concrete="Fc27"><StbSecFigureBeam_SRC><StbSecBeam_SRC_Straight width="400" depth="700"/></StbSecFigureBeam_SRC><StbSecSteelFigureBeam_SRC><StbSecSteelBeam_SRC_Straight shape="H1" strength_main="SN490B"/></StbSecSteelFigureBeam_SRC></StbSecBeam_SRC>"#;
    let cft = r#"<StbSecColumn_CFT id="1" name="CCFT" strength_concrete="Fc36"><StbSecSteelFigureColumn_CFT><StbSecSteelColumn_CFT_Same shape="BOX1" strength="SN400B"/></StbSecSteelFigureColumn_CFT></StbSecColumn_CFT>"#;
    let steel = r#"<StbSecSteel><StbSecRoll-H name="H1" type="H" A="300" B="150" t1="6.5" t2="9" r="13"/><StbSecRoll-BOX name="BOX1" type="ELSE" A="400" B="400" t="12" r="24"/></StbSecSteel>"#;
    let mut columns = String::from("<StbColumns>");
    for (id, kind) in [(11, "RC"), (12, "S"), (13, "SRC"), (14, "CFT")] {
        columns.push_str(&format!(r#"<StbColumn id="{id}" name="C{kind}" id_node_bottom="10" id_node_top="20" id_section="1" kind_structure="{kind}"/>"#));
    }
    columns.push_str("</StbColumns><StbGirders>");
    for (id, kind) in [(21, "RC"), (22, "S"), (23, "SRC")] {
        columns.push_str(&format!(r#"<StbGirder id="{id}" name="G{kind}" id_node_start="20" id_node_end="30" id_section="1" kind_structure="{kind}" isFoundation="false"/>"#));
    }
    columns.push_str("</StbGirders>");
    let definitions = [
        COLUMN,
        steel_column,
        src_column,
        cft,
        GIRDER,
        steel_beam,
        src_beam,
    ];
    for model in both_imports(&input(
        &columns,
        &format!("{}{steel}", definitions.join("")),
    )) {
        assert_eq!(model.elements.len(), 7);
        let mut ids = std::collections::HashSet::new();
        for (element, (name, grade)) in model.elements.iter().zip([
            ("C1", "Fc30"),
            ("CS", "SN400B"),
            ("CSRC", "Fc33"),
            ("CCFT", "Fc36"),
            ("G1", "Fc24"),
            ("GS", "SN490B"),
            ("GSRC", "Fc27"),
        ]) {
            let section = model.section(element.section.unwrap()).unwrap();
            assert_eq!(section.name, name);
            assert!(ids.insert(section.id));
            assert_eq!(
                model.materials[section.material.unwrap().index()].name,
                grade
            );
            if grade.starts_with("Fc") {
                let strength = model
                    .stb_strengths
                    .members
                    .iter()
                    .find(|s| s.target == StrengthTarget::Element(element.id))
                    .unwrap();
                assert_eq!(model.resolve_stb_concrete(strength).unwrap().grade, grade);
            }
        }
    }
}

const SLAB_RC: &str = r#"<StbSecSlab_RC id="1" name="SLRC" isFoundation="false" isEarthen="false" isCanti="false" strength_concrete="Fc30"><StbSecFigureSlab_RC><StbSecSlab_RC_Straight depth="150"/></StbSecFigureSlab_RC></StbSecSlab_RC>"#;
const SLAB_DECK: &str = r#"<StbSecSlabDeck id="1" name="SLDECK" product_type="COMPOSITE" strength_concrete="Fc24"><StbSecFigureSlabDeck><StbSecSlabDeckStraight depth="180"/></StbSecFigureSlabDeck><StbSecProductSlabDeck product_code="D1" depth_deck="50"/></StbSecSlabDeck>"#;
const SLABS: &str = r#"<StbSlabs><StbSlab id="31" name="SLRC" id_section="1" kind_structure="RC" kind_slab="NORMAL" isFoundation="false"><StbNodeIdOrder>20 30 40</StbNodeIdOrder></StbSlab><StbSlab id="32" name="SLDECK" id_section="1" kind_structure="DECK" kind_slab="NORMAL" isFoundation="false"><StbNodeIdOrder>20 50 30</StbNodeIdOrder></StbSlab></StbSlabs>"#;

#[test]
fn slab_rc_and_deck_with_same_id_keep_thickness_and_concrete_binding() {
    let (rc, deck) = SLABS
        .strip_prefix("<StbSlabs>")
        .unwrap()
        .strip_suffix("</StbSlabs>")
        .unwrap()
        .split_once("</StbSlab>")
        .unwrap();
    let reversed = format!("<StbSlabs>{deck}{rc}</StbSlab></StbSlabs>");
    for slabs in [SLABS, &reversed] {
        let xml = input(
            &format!("{MEMBERS}{slabs}"),
            &format!("{COLUMN}{GIRDER}{SLAB_RC}{SLAB_DECK}"),
        );
        for model in both_imports(&xml) {
            assert_eq!(model.slabs.len(), 2);
            assert_ne!(model.slabs[0].plate.section, model.slabs[1].plate.section);
            for (name, thickness_mm, fc) in [("SLRC", 150., 30.), ("SLDECK", 180., 24.)] {
                let slab = model
                    .slabs
                    .iter()
                    .find(|slab| model.section(slab.plate.section.unwrap()).unwrap().name == name)
                    .unwrap();
                let section = model.section(slab.plate.section.unwrap()).unwrap();
                assert_eq!(section.name, name);
                assert_eq!(section.thickness, Some(thickness_mm));
                assert_eq!(model.slab_plate_material(slab).unwrap().fc, Some(fc));
                let strength = model
                    .stb_strengths
                    .members
                    .iter()
                    .find(|s| s.target == StrengthTarget::Slab(slab.id))
                    .unwrap();
                assert_eq!(model.resolve_stb_concrete(strength).unwrap().value, fc);
            }
        }
    }
}

#[test]
fn precast_is_rejected_as_unsupported_even_when_rc_and_deck_share_its_id() {
    let precast = r#"<StbSecSlabPrecast id="1" name="PC1" precast_type="FULL" strength_concrete="Fc36"><StbSecProductSlabPrecast product_code="P1" depth="200"/></StbSecSlabPrecast>"#;
    let members = SLABS.replace("kind_structure=\"RC\"", "kind_structure=\"PRECAST\"");
    rejected_by_both(
        &input(
            &format!("{MEMBERS}{members}"),
            &format!("{COLUMN}{GIRDER}{SLAB_RC}{SLAB_DECK}{precast}"),
        ),
        &[
            "StbSlab",
            "31",
            "PRECAST",
            "StbSecSlabPrecast",
            "id_section",
            "未対応",
        ],
    );
}

#[test]
fn section_definition_and_reference_outside_u32_are_rejected() {
    rejected_by_both(
        &input(
            MEMBERS,
            &format!(
                "{}{GIRDER}",
                COLUMN.replace("id=\"1\"", "id=\"4294967296\"")
            ),
        ),
        &["StbSecColumn_RC", "4294967296"],
    );
    rejected_by_both(
        &input(
            &MEMBERS.replacen("id_section=\"1\"", "id_section=\"4294967296\"", 1),
            &format!("{COLUMN}{GIRDER}"),
        ),
        &["StbColumn", "RC", "StbSecColumn_RC", "4294967296", "不正"],
    );
}

#[test]
fn column_post_and_girder_beam_share_their_concrete_section_series() {
    let secondaries = r#"<StbPosts><StbPost id="11" name="P1" id_node_bottom="10" id_node_top="20" id_section="1" kind_structure="RC"/></StbPosts><StbBeams><StbBeam id="12" name="B1" id_node_start="20" id_node_end="30" id_section="1" kind_structure="RC" isFoundation="false"/></StbBeams>"#;
    let (posts, beams) = secondaries.split_once("</StbPosts>").unwrap();
    let members =
        MEMBERS.replace("</StbColumns>", &format!("</StbColumns>{posts}</StbPosts>")) + beams;
    for model in both_imports(&input(&members, &format!("{COLUMN}{GIRDER}"))) {
        assert_column_and_girder(&model);
        for (secondary, usage, fc) in [
            (model.posts().next().unwrap(), FrameSectionUse::Column, 30.),
            (model.beams().next().unwrap(), FrameSectionUse::Girder, 24.),
        ] {
            let primary = model
                .elements
                .iter()
                .find(|e| model.section(e.section.unwrap()).unwrap().frame_use == Some(usage))
                .unwrap();
            assert_eq!(secondary.section, primary.section);
            assert_eq!(model.secondary_material(secondary).unwrap().fc, Some(fc));
            let strength = model
                .stb_strengths
                .members
                .iter()
                .find(|s| s.target == StrengthTarget::Secondary(secondary.id))
                .unwrap();
            assert_eq!(model.resolve_stb_concrete(strength).unwrap().value, fc);
        }
    }
}
