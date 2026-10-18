use sepika_core::ids::{NodeId, SectionId};
use sepika_core::model::{FrameSectionUse, Model};
use sepika_core::section_shape::SectionShape;
use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report};

fn standard_input(member: &str, section: &str, steel: &str) -> String {
    format!(
        r#"<ST_BRIDGE xmlns="https://www.building-smart.or.jp/dl" version="2.0.2">
<StbCommon project_name="standard input" app_name="fixture" app_version="1"/>
<StbModel><StbNodes>
<StbNode id="11" X="0" Y="0" Z="0" kind="ON_GRID"/>
<StbNode id="37" X="6000" Y="0" Z="3000" kind="ON_GRID"/>
</StbNodes><StbMembers>{member}</StbMembers>
<StbSections>{section}<StbSecSteel>{steel}</StbSecSteel></StbSections>
</StbModel></ST_BRIDGE>"#
    )
}

const GIRDER: &str = r#"<StbGirders><StbGirder id="5" name="G1" id_node_start="11" id_node_end="37" id_section="20" kind_structure="S" isFoundation="false"/></StbGirders>"#;
const STEEL_SECTION: &str = r#"<StbSecBeam_S id="20" name="G1" floor="2F" kind_beam="GIRDER" isCanti="false"><StbSecSteelFigureBeam_S><StbSecSteelBeam_S_Straight shape="H300" strength_main="SN400B"/></StbSecSteelFigureBeam_S></StbSecBeam_S>"#;
const ROLL_H: &str =
    r#"<StbSecRoll-H name="H300" type="H" A="300" B="150" t1="6.5" t2="9" r="13"/>"#;

fn both_imports(xml: &str) -> [Model; 2] {
    [
        import_stbridge(xml).unwrap(),
        import_stbridge_with_report(xml).unwrap().0,
    ]
}

fn rejected_by_both(xml: &str, diagnostic: &str) {
    let plain = import_stbridge(xml).unwrap_err().to_string();
    let reported = import_stbridge_with_report(xml).unwrap_err().to_string();
    assert_eq!(plain, reported);
    assert!(plain.contains(diagnostic), "{plain}");
}

#[test]
fn standard_steel_input_resolves_shape_material_and_sparse_references() {
    for model in both_imports(&standard_input(GIRDER, STEEL_SECTION, ROLL_H)) {
        assert_eq!(model.nodes.len(), 2);
        assert_eq!(model.elements.len(), 1);
        assert_eq!(model.elements[0].nodes.as_slice(), &[NodeId(0), NodeId(1)]);
        assert_eq!(model.elements[0].section, Some(SectionId(0)));
        let section = &model.sections[0];
        assert_eq!(section.frame_use, Some(FrameSectionUse::Girder));
        assert_eq!(section.floor.as_deref(), Some("2F"));
        assert!(matches!(
            section.shape,
            Some(SectionShape::SteelH {
                height: 300.0,
                width: 150.0,
                ..
            })
        ));
        let material = model
            .materials
            .get(section.material.unwrap().index())
            .unwrap();
        assert_eq!(material.name, "SN400B");
        assert_eq!(material.young, 205_000.0);
    }
}

#[test]
fn ordinary_missing_node_section_and_shape_references_fail_at_both_entrances() {
    let xml = standard_input(GIRDER, STEEL_SECTION, ROLL_H);
    rejected_by_both(
        &xml.replace("id_node_end=\"37\"", "id_node_end=\"999\""),
        "節点参照",
    );
    rejected_by_both(
        &xml.replace("id_section=\"20\"", "id_section=\"999\""),
        "断面参照",
    );
    rejected_by_both(
        &xml.replace("shape=\"H300\"", "shape=\"missing\""),
        "形鋼参照",
    );
}

#[test]
fn ordinary_secondary_missing_references_fail_at_both_entrances() {
    let beam = GIRDER
        .replace("StbGirders", "StbBeams")
        .replace("StbGirder", "StbBeam");
    let xml = standard_input(&beam, STEEL_SECTION, ROLL_H);
    rejected_by_both(
        &xml.replace("id_node_end=\"37\"", "id_node_end=\"999\""),
        "節点参照",
    );
    rejected_by_both(
        &xml.replace("id_section=\"20\"", "id_section=\"999\""),
        "断面参照",
    );
}

#[test]
fn standard_rc_dimensions_and_grade_survive_without_custom_material_ids() {
    let member = GIRDER.replace("kind_structure=\"S\"", "kind_structure=\"RC\"");
    let section = r#"<StbSecBeam_RC id="20" name="G1" kind_beam="GIRDER" isFoundation="false" isCanti="false" strength_concrete="Fc24"><StbSecFigureBeam_RC><StbSecBeam_RC_Straight width="400" depth="600"/></StbSecFigureBeam_RC></StbSecBeam_RC>"#;
    for model in both_imports(&standard_input(&member, section, "")) {
        assert!(matches!(
            model.sections[0].shape,
            Some(SectionShape::RcBeamRect {
                b: 400.0,
                d: 600.0,
                ..
            })
        ));
        let material = model
            .materials
            .get(model.sections[0].material.unwrap().index())
            .unwrap();
        assert_eq!(material.fc, Some(24.0));
    }
}

#[test]
fn standard_build_h_and_pipe_dimensions_are_preserved() {
    for (steel, expected_area) in [
        (
            r#"<StbSecBuild-H name="H300" A="300" B="150" t1="6" t2="10"/>"#,
            4680.0,
        ),
        (
            r#"<StbSecPipe name="H300" D="100" t="5"/>"#,
            std::f64::consts::PI * 475.0,
        ),
    ] {
        for model in both_imports(&standard_input(GIRDER, STEEL_SECTION, steel)) {
            assert!((model.sections[0].area - expected_area).abs() < 1e-8);
        }
    }
}

#[test]
fn ordinary_slab_and_wall_reference_errors_fail_at_both_entrances() {
    for (members, section) in [
        (
            r#"<StbSlabs><StbSlab id="5" name="S1" id_section="20" kind_structure="RC" kind_slab="NORMAL" isFoundation="false"><StbNodeIdOrder>11 37 999</StbNodeIdOrder></StbSlab></StbSlabs>"#,
            r#"<StbSecSlab_RC id="20" name="S1" isFoundation="false" isEarthen="false" isCanti="false" strength_concrete="Fc24"><StbSecFigureSlab_RC><StbSecSlab_RC_Straight depth="150"/></StbSecFigureSlab_RC></StbSecSlab_RC>"#,
        ),
        (
            r#"<StbWalls><StbWall id="5" name="W1" id_section="20" kind_structure="RC" kind_layout="ON_GIRDER"><StbNodeIdOrder>11 37 999</StbNodeIdOrder></StbWall></StbWalls>"#,
            r#"<StbSecWall_RC id="20" name="W1" strength_concrete="Fc24"><StbSecFigureWall_RC><StbSecWall_RC_Straight t="200"/></StbSecFigureWall_RC></StbSecWall_RC>"#,
        ),
    ] {
        let xml = standard_input(members, section, "");
        rejected_by_both(&xml, "境界節点参照");
        let xml = xml
            .replace("11 37 999", "11 37 11")
            .replace("id_section=\"20\"", "id_section=\"999\"");
        rejected_by_both(&xml, "断面参照");
    }
}
