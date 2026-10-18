use quick_xml::events::Event;
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node, StbNodeIdentity,
};
use sepika_core::section_shape::SectionShape;
use sepika_io::stbridge::{export_stbridge, import_stbridge};
use smallvec::smallvec;

fn brace_model(name: &str) -> Model {
    let mut model = Model::default();
    for (id, coord, external_id) in [(0, [0.0, 0.0, 0.0], 41), (1, [6000.0, 0.0, 3000.0], 99)] {
        model.nodes.push(Node {
            id: NodeId(id),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
        model.stb_node_ids.push(StbNodeIdentity {
            node: NodeId(id),
            id: external_id,
            guid: None,
        });
    }
    model.materials.push(Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    });
    let mut section = SectionShape::SteelH {
        height: 300.0,
        width: 150.0,
        web_thick: 6.5,
        flange_thick: 9.0,
        root_r: Some(10.0),
    }
    .to_section(SectionId(0), name.into());
    section.frame_use = Some(FrameSectionUse::Brace);
    section.material = Some(MaterialId(0));
    section.floor = Some("1F".into());
    model.sections.push(section);
    model.elements.push(ElementData {
        id: ElemId(0),
        kind: ElementKind::Brace { tension_only: true },
        nodes: smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0.0, 1.0, 0.0],
        },
        end_cond: [EndCondition::Pinned, EndCondition::Pinned],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        plastic_zone: None,
        spring: None,
    });
    model
}

fn assert_element_only_content(xml: &str) {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut parents = Vec::new();
    let mut checked = Vec::new();
    loop {
        let event = reader.read_event().unwrap();
        let target = parents.last().is_some_and(|name: &Vec<u8>| {
            matches!(
                name.as_slice(),
                b"StbSecBrace_S" | b"StbSecSteelFigureBrace_S"
            )
        });
        match event {
            Event::Start(e) => {
                let name = e.name().as_ref().to_vec();
                if matches!(
                    name.as_slice(),
                    b"StbSecBrace_S" | b"StbSecSteelFigureBrace_S"
                ) {
                    checked.push(name.clone());
                }
                parents.push(name);
            }
            Event::End(_) => {
                parents.pop().unwrap();
            }
            Event::Text(e) if target => {
                assert!(e.iter().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')))
            }
            Event::CData(e) if target => {
                assert!(e.iter().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')))
            }
            Event::GeneralRef(_) if target => panic!("断面の文字子に参照が存在する"),
            Event::Eof => break,
            _ => {}
        }
    }
    assert!(parents.is_empty());
    assert_eq!(
        checked,
        [
            b"StbSecBrace_S".to_vec(),
            b"StbSecSteelFigureBrace_S".to_vec()
        ]
    );
}

fn assert_brace_meaning(xml: &str, expected_name: &str) {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut tags = std::collections::HashMap::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let attrs: std::collections::HashMap<_, _> = e
                    .attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (
                            a.key.as_ref().to_vec(),
                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .unwrap()
                                .into_owned(),
                        )
                    })
                    .collect();
                tags.insert(e.name().as_ref().to_vec(), attrs);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let attr = |tag: &[u8], name: &[u8]| tags.get(tag).unwrap().get(name).unwrap().as_str();
    assert_eq!(attr(b"StbSecBrace_S", b"name"), expected_name);
    assert_eq!(attr(b"StbSecBrace_S", b"floor"), "1F");
    assert_eq!(
        attr(b"StbBrace", b"id_section"),
        attr(b"StbSecBrace_S", b"id")
    );
    assert!(attr(b"StbSecBrace_S", b"id").parse::<u32>().unwrap() > 0);
    assert_eq!(attr(b"StbBrace", b"id_node_start"), "41");
    assert_eq!(attr(b"StbBrace", b"id_node_end"), "99");
    assert_eq!(attr(b"StbSecSteelBrace_S_Same", b"strength_main"), "SN400B");
    assert_eq!(
        attr(b"StbSecSteelBrace_S_Same", b"shape"),
        attr(b"StbSecRoll-H", b"name")
    );
    for (name, expected) in [
        (b"A".as_slice(), "300"),
        (b"B", "150"),
        (b"t1", "6.5"),
        (b"t2", "9"),
        (b"r", "10"),
    ] {
        assert_eq!(attr(b"StbSecRoll-H", name), expected);
    }
    let model = import_stbridge(xml).unwrap();
    assert_eq!(model.nodes.len(), 2);
    assert_eq!(model.elements.len(), 1);
    assert_eq!(model.sections.len(), 1);
    assert_eq!(model.materials.len(), 1);
    let brace = &model.elements[0];
    assert_eq!(brace.kind, ElementKind::Brace { tension_only: true });
    let section = model
        .sections
        .iter()
        .find(|s| Some(s.id) == brace.section)
        .unwrap();
    assert_eq!(section.name, expected_name);
    assert_eq!(section.floor.as_deref(), Some("1F"));
    assert_eq!(section.frame_use, Some(FrameSectionUse::Brace));
    assert_eq!(
        section.shape,
        Some(SectionShape::SteelH {
            height: 300.0,
            width: 150.0,
            web_thick: 6.5,
            flange_thick: 9.0,
            root_r: Some(10.0),
        })
    );
    let material = model
        .materials
        .iter()
        .find(|m| Some(m.id) == section.material)
        .unwrap();
    assert_eq!(material.name, "SN400B");
    assert_eq!(material.category, MaterialCategory::Steel);
    assert_eq!(material.fy, Some(235.0));
    for (node, expected_coord, expected_external_id) in [
        (brace.nodes[0], [0.0, 0.0, 0.0], 41),
        (brace.nodes[1], [6000.0, 0.0, 3000.0], 99),
    ] {
        assert_eq!(
            model.nodes.iter().find(|n| n.id == node).unwrap().coord,
            expected_coord
        );
        assert_eq!(
            model
                .stb_node_ids
                .iter()
                .find(|n| n.node == node)
                .unwrap()
                .id,
            expected_external_id
        );
    }
}

#[test]
fn steel_brace_export_has_only_xml_whitespace_between_children() {
    let xml = export_stbridge(&brace_model("BR550")).unwrap();
    assert_element_only_content(&xml);
    assert_brace_meaning(&xml, "BR550");
}

#[test]
fn steel_brace_export_preserves_escaped_attribute_characters() {
    let name = "BR&\"\\550";
    let xml = export_stbridge(&brace_model(name)).unwrap();
    assert_element_only_content(&xml);
    assert_brace_meaning(&xml, name);
}

#[test]
#[ignore = "公式XSD外部検証用。SEPIKA_BRACE_XSD_OUTPUTに出力ディレクトリを指定する"]
fn write_public_brace_exports_for_external_xsd() {
    let output = std::path::PathBuf::from(std::env::var_os("SEPIKA_BRACE_XSD_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (file, name) in [("positive.stb", "BR550"), ("escaped.stb", "BR&\"\\550")] {
        let xml = export_stbridge(&brace_model(name)).unwrap();
        assert_element_only_content(&xml);
        assert_brace_meaning(&xml, name);
        std::fs::write(output.join(file), xml).unwrap();
    }
}
