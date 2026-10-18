use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report};

fn document(element: &str) -> String {
    format!(
        r#"<ST_BRIDGE xmlns="https://www.building-smart.or.jp/dl" version="2.0.2"><StbModel><StbNodes><StbNode id="0" X="0" Y="0" Z="0"/><StbNode id="1" X="0" Y="0" Z="3000"/></StbNodes>{element}</StbModel></ST_BRIDGE>"#
    )
}

fn assert_rejected(xml: &str, field: &str) {
    for result in [
        import_stbridge(xml).map(|_| ()),
        import_stbridge_with_report(xml).map(|_| ()),
    ] {
        let reason = result.unwrap_err().to_string();
        assert!(reason.contains(field), "{field}: {reason}\n{xml}");
        assert!(
            reason.contains("expected integer 0..=4294967295"),
            "{reason}"
        );
    }
}

#[test]
fn stb_whole_document_rejects_invalid_present_member_reference_and_source_ids() {
    for tag in [
        "StbColumn",
        "StbGirder",
        "StbBrace",
        "StbBeam",
        "StbPost",
        "StbSlab",
        "StbWall",
    ] {
        let endpoints = if matches!(tag, "StbColumn" | "StbPost") {
            r#"id_node_bottom="0" id_node_top="1""#
        } else {
            r#"id_node_start="0" id_node_end="1""#
        };
        for field in ["id_section", "id"] {
            for invalid in [
                "4294967296",
                "18446744073709551615",
                "-1",
                "0.5",
                "1.0",
                "abc",
                "",
                "null",
            ] {
                let source = if field == "id" {
                    String::new()
                } else {
                    r#"id="0""#.to_string()
                };
                let kind = if matches!(tag, "StbSlab" | "StbWall") {
                    "RC"
                } else {
                    "S"
                };
                let element = format!(
                    r#"<{tag} {source} {endpoints} kind_structure="{kind}" {field}="{invalid}"/>"#
                );
                assert_rejected(&document(&element), field);
            }
        }
    }
}

#[test]
fn stb_required_definition_and_endpoint_ids_reject_missing_or_invalid_values() {
    for (tag, field, other) in [
        ("StbNode", "id", r#"X="0" Y="0" Z="0""#),
        ("StbSecColumn_S", "id", ""),
        ("StbSecBeam_S", "id", ""),
        ("StbSecBrace_S", "id", ""),
        ("StbSecColumn_RC", "id", ""),
        ("StbSecBeam_RC", "id", ""),
        ("StbSecColumn_SRC", "id", ""),
        ("StbSecBeam_SRC", "id", ""),
        ("StbSecColumn_CFT", "id", ""),
        ("StbSecSlab_RC", "id", ""),
        ("StbSecWall_RC", "id", ""),
        (
            "StbColumn",
            "id_node_bottom",
            r#"id="0" id_node_top="1" kind_structure="S""#,
        ),
        (
            "StbColumn",
            "id_node_top",
            r#"id="0" id_node_bottom="1" kind_structure="S""#,
        ),
        (
            "StbGirder",
            "id_node_start",
            r#"id="0" id_node_end="1" kind_structure="S""#,
        ),
        (
            "StbGirder",
            "id_node_end",
            r#"id="0" id_node_start="1" kind_structure="S""#,
        ),
        (
            "StbBrace",
            "id_node_start",
            r#"id="0" id_node_end="1" kind_structure="S""#,
        ),
        (
            "StbBrace",
            "id_node_end",
            r#"id="0" id_node_start="1" kind_structure="S""#,
        ),
        (
            "StbBeam",
            "id_node_start",
            r#"id="0" id_node_end="1" kind_structure="S""#,
        ),
        (
            "StbBeam",
            "id_node_end",
            r#"id="0" id_node_start="1" kind_structure="S""#,
        ),
        (
            "StbPost",
            "id_node_bottom",
            r#"id="0" id_node_top="1" kind_structure="S""#,
        ),
        (
            "StbPost",
            "id_node_top",
            r#"id="0" id_node_bottom="1" kind_structure="S""#,
        ),
    ] {
        let xml = document(&format!("<{tag} {other}/>"));
        for result in [
            import_stbridge(&xml).map(|_| ()),
            import_stbridge_with_report(&xml).map(|_| ()),
        ] {
            let reason = result.unwrap_err().to_string();
            assert!(
                reason.contains(field) && reason.contains("missing"),
                "{reason}"
            );
        }
        for invalid in ["4294967296", "-1", "0.5", "1.0", "abc", "", "null"] {
            assert_rejected(
                &document(&format!(r#"<{tag} {other} {field}="{invalid}"/>"#)),
                field,
            );
        }
    }
}

#[test]
fn stb_boundary_node_reference_tokens_are_not_silently_discarded() {
    for tag in ["StbSlab", "StbWall"] {
        for invalid in ["4294967296", "-1", "0.5", "1.0", "abc"] {
            assert_rejected(
                &document(&format!(
                    r#"<{tag} id="0" kind_structure="RC"><StbNodeIdOrder>0 {invalid} 1</StbNodeIdOrder></{tag}>"#
                )),
                "StbNodeIdOrder",
            );
            assert_rejected(
                &document(&format!(
                    r#"<{tag} id="0" kind_structure="RC"><StbNodeId id="{invalid}"/></{tag}>"#
                )),
                "id",
            );
        }
        let xml = document(&format!(
            r#"<{tag} id="0" kind_structure="RC"><StbNodeId/></{tag}>"#
        ));
        assert!(import_stbridge(&xml)
            .unwrap_err()
            .to_string()
            .contains("missing attr id"));
    }
}

fn steel_column_section(id: u32) -> String {
    format!(
        r#"<StbSections><StbSecColumn_S id="{id}" name="C603" kind_column="COLUMN"><StbSecSteelFigureColumn_S><StbSecSteelColumn_S_Same shape="H603" strength_main="SN400B"/></StbSecSteelFigureColumn_S></StbSecColumn_S><StbSecSteel><StbSecRoll-H name="H603" type="H" A="300" B="150" t1="6.5" t2="9" r="0"/></StbSecSteel></StbSections>"#
    )
}

#[test]
fn stb_member_source_id_parser_accepts_omission_and_u32_boundaries_with_resolved_section() {
    for source_id in ["", r#"id="0""#, r#"id="4294967295""#] {
        let xml = document(&format!(
            r#"{}<StbMembers><StbColumns><StbColumn {source_id} id_node_bottom="0" id_node_top="1" id_section="2" kind_structure="S"/></StbColumns></StbMembers>"#,
            steel_column_section(2),
        ));
        for model in [
            import_stbridge(&xml).unwrap(),
            import_stbridge_with_report(&xml).unwrap().0,
        ] {
            assert_eq!(model.elements.len(), 1);
            assert_eq!(
                model.element_section(&model.elements[0]).unwrap().name,
                "C603"
            );
        }
    }
}

#[test]
fn stb_u32_parser_boundary_ids_resolve_to_the_same_section_and_grade_material() {
    for value in [0_u32, u32::MAX] {
        let xml = document(&format!(
            r#"{}<StbMembers><StbColumns><StbColumn id="{value}" id_node_bottom="{value}" id_node_top="1" id_section="{value}" kind_structure="S"/></StbColumns></StbMembers>"#,
            steel_column_section(value),
        ))
        .replace("StbNode id=\"0\"", &format!("StbNode id=\"{value}\""));
        for model in [
            import_stbridge(&xml).unwrap(),
            import_stbridge_with_report(&xml).unwrap().0,
        ] {
            assert_eq!(model.sections.len(), 1);
            assert_eq!(
                model
                    .stb_node_ids
                    .iter()
                    .find(|identity| identity.node == model.elements[0].nodes[0])
                    .unwrap()
                    .id,
                value,
            );
            let section = model.element_section(&model.elements[0]).unwrap();
            assert_eq!(section.name, "C603");
            let material = &model.materials[section.material.unwrap().index()];
            assert_eq!(material.name, "SN400B");
            assert_eq!(material.young, 205000.);
        }
    }
}

#[test]
fn stb_plate_section_parser_boundary_ids_resolve_at_zero_and_u32_max() {
    for value in [0_u32, u32::MAX] {
        let wall = include_str!("fixtures/strength_wall.stb")
            .replace("id_section=\"3\"", &format!("id_section=\"{value}\""))
            .replace(
                "StbSecWall_RC id=\"3\"",
                &format!("StbSecWall_RC id=\"{value}\""),
            );
        for model in [
            import_stbridge(&wall).unwrap(),
            import_stbridge_with_report(&wall).unwrap().0,
        ] {
            assert_eq!(model.wall_plates.len(), 1);
            let section = &model.sections[model.wall_plates[0].section.unwrap().index()];
            assert_eq!(section.name, "Wall t180");
            assert_eq!(section.thickness, Some(180.));
        }
        let slab = include_str!("fixtures/strength_split_slab.stb")
            .replace("id_section=\"8\"", &format!("id_section=\"{value}\""))
            .replace(
                "StbSecSlab_RC id=\"8\"",
                &format!("StbSecSlab_RC id=\"{value}\""),
            );
        for model in [
            import_stbridge(&slab).unwrap(),
            import_stbridge_with_report(&slab).unwrap().0,
        ] {
            assert_eq!(model.slabs.len(), 2);
            for slab in &model.slabs {
                let section = &model.sections[slab.plate.section.unwrap().index()];
                assert_eq!(section.name, "S1");
                assert_eq!(section.thickness, Some(150.));
            }
        }
    }
}

#[test]
fn stb_post_invalid_or_missing_required_endpoint_never_uses_another_alias() {
    for (field, other) in [
        ("id_node_bottom", "id_node_top"),
        ("id_node_top", "id_node_bottom"),
    ] {
        for invalid in ["4294967296", "-1", "0.5", "1.0", "abc", "null"] {
            let xml = document(&format!(
                r#"<StbPost id="0" {field}="{invalid}" {other}="1" id_node_start="0" id_node_end="1" kind_structure="S"/>"#
            ));
            assert_rejected(&xml, field);
        }
        let xml = document(&format!(
            r#"<StbPost id="0" {other}="1" id_node_start="0" id_node_end="1" kind_structure="S"/>"#
        ));
        for result in [
            import_stbridge(&xml).map(|_| ()),
            import_stbridge_with_report(&xml).map(|_| ()),
        ] {
            let reason = result.unwrap_err().to_string();
            assert!(
                reason.contains(field) && reason.contains("missing"),
                "{reason}"
            );
        }
    }
}

#[test]
fn stb_negative_section_reference_is_rejected_for_all_linear_member_types() {
    for (tag, endpoints) in [
        ("StbColumn", r#"id_node_bottom="0" id_node_top="1""#),
        ("StbGirder", r#"id_node_start="0" id_node_end="1""#),
        ("StbBrace", r#"id_node_start="0" id_node_end="1""#),
        ("StbBeam", r#"id_node_start="0" id_node_end="1""#),
        ("StbPost", r#"id_node_bottom="0" id_node_top="1""#),
    ] {
        let xml = document(&format!(
            r#"<{tag} id="0" {endpoints} id_section="-1" kind_structure="S"/>"#
        ));
        assert_rejected(&xml, "id_section");
    }
}
