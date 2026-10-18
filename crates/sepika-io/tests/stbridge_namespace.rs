use sepika_core::ids::{NodeId, SectionId};
use sepika_core::model::{Model, StrengthSource};
use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report};

const URI: &str = "https://www.building-smart.or.jp/dl";
const BASE: &str = r#"<ST_BRIDGE xmlns="https://www.building-smart.or.jp/dl" version="2.0.2">
<StbCommon project_name="namespace" app_name="fixture" app_version="1"/>
<StbModel><StbNodes>
<StbNode id="10" X="0" Y="0" Z="0" kind="ON_GRID"/>
<StbNode id="20" X="0" Y="0" Z="3000" kind="ON_GRID"/>
</StbNodes><StbMembers><StbColumns>
<StbColumn id="1" name="C1" id_node_bottom="10" id_node_top="20" id_section="1" kind_structure="RC"/>
</StbColumns></StbMembers><StbSections>
<StbSecColumn_RC id="1" name="C1" strength_concrete="Fc30"><StbSecFigureColumn_RC><StbSecColumn_RC_Rect width_X="500" width_Y="500"/></StbSecFigureColumn_RC></StbSecColumn_RC>
</StbSections></StbModel></ST_BRIDGE>"#;

const EXTENSION: &str = r#"<StbExtensions><StbExtension identifier="fixture">
<StbExtObject object_name="StbColumn" id_object="1"><StbExtProperty key="payload" type="string" value="&lt;StbNode id=&quot;999&quot;/&gt; &lt;StbSecColumn_RC strength_concrete=&quot;Fc99&quot;/&gt;"/></StbExtObject>
<StbExtElement object_name="StbColumn" element_name="StbColumn"><StbExtPropertyDef key="strength_concrete" type="string" default="Fc99"/></StbExtElement>
</StbExtension></StbExtensions>"#;

fn prefixed(xml: &str) -> String {
    xml.replace('<', "<s:")
        .replace("<s:/", "</s:")
        .replace("xmlns=", "xmlns:s=")
}

fn assert_column(xml: &str) {
    let models = [
        import_stbridge(xml).unwrap(),
        import_stbridge_with_report(xml).unwrap().0,
    ];
    for model in models {
        assert_column_model(&model);
    }
}

fn assert_column_model(model: &Model) {
    assert_eq!(model.nodes.len(), 2);
    assert_eq!(model.elements.len(), 1);
    assert_eq!(model.sections.len(), 1);
    assert_eq!(model.beams().count(), 0);
    assert_eq!(model.posts().count(), 0);
    assert!(model.slabs.is_empty());
    assert!(model.wall_plates.is_empty());
    let column = &model.elements[0];
    assert_eq!(column.nodes.as_slice(), &[NodeId(0), NodeId(1)]);
    assert_eq!(column.section, Some(SectionId(0)));
    assert_eq!(model.sections[0].name, "C1");
    assert_eq!(model.sections[0].area, 250_000.0);
    assert_eq!(model.element_material(column).unwrap().fc, Some(30.0));
    assert_eq!(model.stb_strengths.sections.len(), 1);
    assert_eq!(model.stb_strengths.members.len(), 1);
    let strength = model
        .resolve_stb_concrete(&model.stb_strengths.members[0])
        .unwrap();
    assert_eq!(strength.value, 30.0);
    assert_eq!(strength.source, StrengthSource::Section);
}

fn rejected_by_both(xml: &str, diagnostics: &[&str]) {
    let plain = import_stbridge(xml).unwrap_err().to_string();
    let reported = import_stbridge_with_report(xml).unwrap_err().to_string();
    assert_eq!(plain, reported);
    for diagnostic in diagnostics {
        assert!(plain.contains(diagnostic), "{diagnostic}: {plain}");
    }
}

#[test]
fn adopted_default_prefix_alias_and_same_uri_redeclaration_preserve_column_strength() {
    assert_column(BASE);
    let prefixed = prefixed(BASE);
    assert_column(&prefixed);
    assert_column(
        &prefixed
            .replacen("version=", &format!("xmlns:q=\"{URI}\" version="), 1)
            .replace("s:StbNode", "q:StbNode"),
    );
    assert_column(&BASE.replace("<StbNodes>", &format!("<StbNodes xmlns=\"{URI}\">")));
}

#[test]
fn root_without_adopted_namespace_is_rejected() {
    rejected_by_both(
        &BASE.replace(URI, "urn:foreign"),
        &["ST_BRIDGE", "urn:foreign", URI],
    );
    rejected_by_both(
        &BASE.replace(&format!(" xmlns=\"{URI}\""), ""),
        &["ST_BRIDGE", "namespace", URI],
    );
}

#[test]
fn foreign_known_element_and_foreign_wrapper_are_rejected_before_adopted_children() {
    rejected_by_both(
        &BASE.replace(
            "<StbNode id=\"10\"",
            "<f:StbNode xmlns:f=\"urn:foreign\" id=\"10\"",
        ),
        &["f:StbNode", "urn:foreign"],
    );
    rejected_by_both(
        &BASE.replace(
            "<StbNodes>",
            "<f:Unknown xmlns:f=\"urn:foreign\"><StbNode id=\"999\" X=\"0\" Y=\"0\" Z=\"0\"/></f:Unknown><StbNodes>",
        ),
        &["f:Unknown", "urn:foreign"],
    );
}

#[test]
fn child_default_namespace_removal_and_prefix_rebinding_are_rejected() {
    rejected_by_both(
        &BASE.replace("<StbNodes>", "<StbNodes xmlns=\"\">"),
        &["StbNodes", "namespace", URI],
    );
    rejected_by_both(
        &prefixed(BASE).replace("<s:StbNodes>", "<s:StbNodes xmlns:s=\"urn:rebound\">"),
        &["s:StbNodes", "urn:rebound"],
    );
}

#[test]
fn unbound_element_and_attribute_prefixes_are_rejected() {
    rejected_by_both(
        &BASE.replace("<StbNode id=\"10\"", "<u:StbNode id=\"10\""),
        &["u:StbNode", "未束縛"],
    );
    rejected_by_both(
        &BASE.replace("<StbNode id=\"10\"", "<StbNode u:extra=\"value\" id=\"10\""),
        &["u:extra", "未束縛"],
    );
}

#[test]
fn bound_attribute_qname_is_not_used_as_unqualified_id() {
    rejected_by_both(
        &BASE.replace(
            "<StbNode id=\"10\"",
            "<StbNode xmlns:f=\"urn:attributes\" f:id=\"10\"",
        ),
        &["id"],
    );
    assert_column(&BASE.replace(
        "<StbNode id=\"10\"",
        "<StbNode xmlns:f=\"urn:attributes\" f:id=\"999\" id=\"10\"",
    ));
}

#[test]
fn explicit_extension_structure_and_attribute_text_do_not_change_model_or_strength() {
    for extension in ["<StbExtensions/>", EXTENSION] {
        let xml = BASE.replace("</ST_BRIDGE>", &format!("{extension}</ST_BRIDGE>"));
        assert_column(&xml);
        assert_column(&prefixed(&xml));
    }
}

#[test]
fn schema_invalid_known_elements_inside_extension_are_not_dispatched() {
    let extension = r#"<StbExtensions><StbExtension identifier="defensive">
<StbNodes><StbNode id="999" X="0" Y="0" Z="0"/></StbNodes>
<StbCommon strength_concrete="Fc99"/>
<StbSections><StbSecColumn_RC id="1" name="fake" strength_concrete="Fc99"><StbSecFigureColumn_RC><StbSecColumn_RC_Rect width_X="999" width_Y="999"/></StbSecFigureColumn_RC></StbSecColumn_RC></StbSections>
</StbExtension></StbExtensions>"#;
    let xml = BASE.replace("</ST_BRIDGE>", &format!("{extension}</ST_BRIDGE>"));
    assert_column(&xml);
    assert_column(&prefixed(&xml));
}

#[test]
fn extension_payload_still_rejects_foreign_elements_and_unbound_attributes() {
    let xml = BASE.replace("</ST_BRIDGE>", &format!("{EXTENSION}</ST_BRIDGE>"));
    rejected_by_both(
        &xml.replace(
            "<StbExtProperty key=",
            "<StbExtProperty xmlns=\"urn:foreign\" key=",
        ),
        &["StbExtProperty", "urn:foreign"],
    );
    rejected_by_both(
        &xml.replace(
            "<StbExtProperty key=",
            "<StbExtProperty u:extra=\"value\" key=",
        ),
        &["u:extra", "未束縛"],
    );
}
