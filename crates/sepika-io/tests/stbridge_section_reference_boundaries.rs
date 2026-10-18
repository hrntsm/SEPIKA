use sepika_core::{
    ids::{ElemId, WallPlateId},
    model::{ElementKind, Model, StrengthSource, StrengthTarget},
};
use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report};

const COLUMN: &str = r#"<StbSecColumn_S id="1" name="C"><StbSecSteelFigureColumn_S><StbSecSteelColumn_S_Same shape="H1" strength_main="SN400B"/></StbSecSteelFigureColumn_S></StbSecColumn_S>"#;
const GIRDER: &str = r#"<StbSecBeam_S id="1" name="G"><StbSecSteelFigureBeam_S><StbSecSteelBeam_S_Straight shape="H1" strength_main="SN400B"/></StbSecSteelFigureBeam_S></StbSecBeam_S>"#;
const BRACE: &str = r#"<StbSecBrace_S id="1" name="BR490"><StbSecSteelFigureBrace_S><StbSecSteelBrace_S_Same shape="H1" strength_main="SN490B"/></StbSecSteelFigureBrace_S></StbSecBrace_S>"#;
const WALL: &str = r#"<StbSecWall_RC id="1" name="W33" strength_concrete="Fc33"><StbSecFigureWall_RC><StbSecWall_RC_Straight t="180"/></StbSecFigureWall_RC></StbSecWall_RC>"#;
const STEEL: &str = r#"<StbSecSteel><StbSecRoll-H name="H1" type="H" A="300" B="150" t1="6.5" t2="9" r="13"/></StbSecSteel>"#;
const FRAME: &str = r#"<StbColumns><StbColumn id="11" name="C1" id_node_bottom="10" id_node_top="20" id_section="1" kind_structure="S"/><StbColumn id="12" name="C2" id_node_bottom="30" id_node_top="40" id_section="1" kind_structure="S"/></StbColumns><StbGirders><StbGirder id="21" name="G1" id_node_start="10" id_node_end="30" id_section="1" kind_structure="S" isFoundation="false"/><StbGirder id="22" name="G2" id_node_start="20" id_node_end="40" id_section="1" kind_structure="S" isFoundation="false"/></StbGirders>"#;
const BRACES: &str = r#"<StbBraces><StbBrace id="41" name="BR490" id_node_start="10" id_node_end="40" id_section="1" kind_structure="S"/></StbBraces>"#;
const WALLS: &str = r#"<StbWalls><StbWall id="51" name="W33" id_section="1" kind_structure="RC" kind_layout="ON_GIRDER"><StbNodeIdOrder>10 20 40 30</StbNodeIdOrder></StbWall></StbWalls>"#;

fn input(members: &str, sections: &str) -> String {
    format!(
        r#"<ST_BRIDGE xmlns="https://www.building-smart.or.jp/dl" version="2.0.2">
<StbCommon project_name="independent brace wall" app_name="fixture" app_version="1"/>
<StbModel><StbNodes>
<StbNode id="10" X="0" Y="0" Z="0" kind="ON_COLUMN"/>
<StbNode id="20" X="0" Y="0" Z="3000" kind="ON_COLUMN"/>
<StbNode id="30" X="4000" Y="0" Z="0" kind="ON_COLUMN"/>
<StbNode id="40" X="4000" Y="0" Z="3000" kind="ON_COLUMN"/>
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

#[test]
fn brace_and_wall_bind_their_own_sections_materials_and_strength_targets() {
    let xml = input(
        &format!("{FRAME}{BRACES}{WALLS}"),
        &format!("{COLUMN}{GIRDER}{BRACE}{WALL}{STEEL}"),
    );
    for model in both_imports(&xml) {
        assert_eq!(model.elements.len(), 5);
        let brace = model.elements.iter().find(|e| e.id == ElemId(4)).unwrap();
        assert!(matches!(brace.kind, ElementKind::Brace { .. }));
        assert_eq!(model.node(brace.nodes[0]).unwrap().coord, [0., 0., 0.]);
        assert_eq!(
            model.node(brace.nodes[1]).unwrap().coord,
            [4000., 0., 3000.]
        );
        let brace_section = model.section(brace.section.unwrap()).unwrap();
        assert_eq!(brace_section.name, "BR490");
        assert_eq!(model.element_material(brace).unwrap().name, "SN490B");
        assert_eq!(model.element_material(brace).unwrap().fy, Some(325.));
        let brace_strength = model
            .stb_strengths
            .sections
            .iter()
            .find(|s| s.section == brace_section.id)
            .unwrap();
        assert_eq!(brace_strength.steel.len(), 1);
        assert_eq!(brace_strength.steel[0].strength, "SN490B");
        let resolved = model.resolve_stb_steel(&brace_strength.steel[0]).unwrap();
        assert_eq!(resolved.grade, "SN490B");
        assert_eq!(resolved.value, 325.);
        assert_eq!(resolved.source, StrengthSource::Section);

        assert_eq!(model.wall_plates.len(), 1);
        let wall = model.wall_plate(WallPlateId(0)).unwrap();
        let wall_section = model.section(wall.section.unwrap()).unwrap();
        assert_eq!(wall_section.name, "Wall t180");
        assert_eq!(wall_section.thickness, Some(180.));
        assert_ne!(wall.section, brace.section);
        assert_eq!(model.wall_plate_material(wall).unwrap().name, "Fc33");
        assert_eq!(model.wall_plate_material(wall).unwrap().fc, Some(33.));
        let wall_strength = model
            .stb_strengths
            .members
            .iter()
            .find(|s| s.target == StrengthTarget::Wall(wall.id))
            .unwrap();
        let resolved = model.resolve_stb_concrete(wall_strength).unwrap();
        assert_eq!(resolved.grade, "Fc33");
        assert_eq!(resolved.value, 33.);
        assert_eq!(resolved.source, StrengthSource::Section);
    }
}

#[test]
fn brace_and_wall_never_fall_back_to_another_series_with_the_same_id() {
    for (members, tag, id, kind, family) in [
        (BRACES, "StbBrace", "id=Some(41)", "S", "StbSecBrace_S"),
        (WALLS, "StbWall", "id=Some(51)", "RC", "StbSecWall_RC"),
    ] {
        rejected_by_both(
            &input(
                &format!("{FRAME}{members}"),
                &format!("{COLUMN}{GIRDER}{STEEL}"),
            ),
            &[
                tag,
                id,
                &format!("kind_structure=Some(\"{kind}\")"),
                family,
                "id_section=Some(\"1\")",
                "要求系列内に断面定義がありません",
                "他系列にのみ存在",
            ],
        );
    }
}

#[test]
fn all_nineteen_concrete_series_reject_normalized_duplicates_before_shape_support() {
    let families = [
        "StbSecColumn_RC",
        "StbSecColumn_S",
        "StbSecColumn_SRC",
        "StbSecColumn_CFT",
        "StbSecBeam_RC",
        "StbSecBeam_S",
        "StbSecBeam_SRC",
        "StbSecBrace_S",
        "StbSecSlab_RC",
        "StbSecSlabDeck",
        "StbSecSlabPrecast",
        "StbSecWall_RC",
        "StbSecFoundation_RC",
        "StbSecPile_RC",
        "StbSecPile_S",
        "StbSecPileProduct",
        "StbSecOpen_RC",
        "StbSecParapet_RC",
        "StbSecUndefined",
    ];
    for family in families {
        for second_id in ["1", "01"] {
            let sections = format!(
                r#"<{family} id="1" name="first"/><{family} id="{second_id}" name="second"/>"#
            );
            rejected_by_both(&input("", &sections), &[&format!("{family}:1"), "重複"]);
        }
    }
}

fn member(tag: &str, group: &str, node_attributes: &str, kind: &str) -> String {
    format!(
        r#"<{group}><{tag} id="71" name="invalid reference" id_section="1" {node_attributes} {kind}/></{group}>"#
    )
}

const MEMBER_FAMILIES: [(&str, &str, &str, &str, &str); 7] = [
    (
        "StbColumn",
        "StbColumns",
        r#"id_node_bottom="10" id_node_top="20""#,
        "RC",
        "StbSecColumn_RC",
    ),
    (
        "StbPost",
        "StbPosts",
        r#"id_node_bottom="10" id_node_top="20""#,
        "RC",
        "StbSecColumn_RC",
    ),
    (
        "StbGirder",
        "StbGirders",
        r#"id_node_start="20" id_node_end="40" isFoundation="false""#,
        "RC",
        "StbSecBeam_RC",
    ),
    (
        "StbBeam",
        "StbBeams",
        r#"id_node_start="20" id_node_end="40" isFoundation="false""#,
        "RC",
        "StbSecBeam_RC",
    ),
    (
        "StbBrace",
        "StbBraces",
        r#"id_node_start="10" id_node_end="40""#,
        "S",
        "StbSecBrace_S",
    ),
    ("StbSlab", "StbSlabs", "", "RC", "StbSecSlab_RC"),
    ("StbWall", "StbWalls", "", "RC", "StbSecWall_RC"),
];

#[test]
fn every_member_tag_reports_source_kind_requested_family_and_section_id() {
    for (tag, group, nodes, kind, family) in MEMBER_FAMILIES {
        for (attribute, expected_kind, expected_family, reason) in [
            (
                format!(r#"kind_structure="{kind}""#),
                format!("Some(\"{kind}\")"),
                family,
                "要求系列内に断面定義がありません",
            ),
            (
                String::new(),
                "None".to_owned(),
                "解決不能",
                "kind_structure がありません",
            ),
            (
                r#"kind_structure="INVALID""#.to_owned(),
                "Some(\"INVALID\")".to_owned(),
                "解決不能",
                "kind_structure が不正です",
            ),
        ] {
            rejected_by_both(
                &input(&member(tag, group, nodes, &attribute), ""),
                &[
                    tag,
                    "id=Some(71)",
                    &format!("kind_structure={expected_kind}"),
                    &format!("要求系列={expected_family}"),
                    "id_section=Some(\"1\")",
                    reason,
                ],
            );
        }
    }
}

#[test]
fn a_defined_undefined_section_stays_unsupported_for_columns_posts_girders_and_beams() {
    for (tag, group, nodes, _, _) in MEMBER_FAMILIES.into_iter().take(4) {
        rejected_by_both(
            &input(
                &member(tag, group, nodes, r#"kind_structure="UNDEFINED""#),
                r#"<StbSecUndefined id="1" name="defined but unsupported"/>"#,
            ),
            &[
                tag,
                "id=Some(71)",
                "kind_structure=Some(\"UNDEFINED\")",
                "要求系列=StbSecUndefined",
                "id_section=Some(\"1\")",
                "断面系列の取り込みは未対応",
            ],
        );
    }
}

#[test]
fn brace_rc_and_src_report_unsupported_kind_without_inventing_a_section_family() {
    for kind in ["RC", "SRC"] {
        rejected_by_both(
            &input(
                &member(
                    "StbBrace",
                    "StbBraces",
                    r#"id_node_start="10" id_node_end="40""#,
                    &format!(r#"kind_structure="{kind}""#),
                ),
                BRACE,
            ),
            &[
                "StbBrace",
                "id=Some(71)",
                &format!("kind_structure=Some(\"{kind}\")"),
                "要求系列=解決不能",
                "id_section=Some(\"1\")",
                "規格上有効",
                "断面系列が未対応",
            ],
        );
    }
}
