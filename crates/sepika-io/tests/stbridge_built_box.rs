use sepika_core::{model::Model, section_shape::SectionShape};
use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report};

const STEEL: &str = include_str!("fixtures/built_box_standard.stb");
const CFT: &str = include_str!("fixtures/cft_standard_steel.stb");

fn both_imports(xml: &str) -> [Model; 2] {
    let plain = import_stbridge(xml).unwrap();
    let reported = import_stbridge_with_report(xml).unwrap().0;
    assert!(plain.eq_ignoring_dofmap(&reported));
    [plain, reported]
}

fn rejected(xml: &str, expected: &[&str]) {
    let plain = import_stbridge(xml).unwrap_err().to_string();
    let reported = import_stbridge_with_report(xml).unwrap_err().to_string();
    assert_eq!(plain, reported);
    for part in expected {
        assert!(plain.contains(part), "{part:?} が診断にない: {plain}");
    }
}

fn dimension(xml: &str, attr: &str, value: Option<&str>) -> String {
    let old = match attr {
        "A" => "300",
        "B" => "200",
        "t1" | "t2" => "9",
        _ => unreachable!(),
    };
    xml.replace(
        &format!("{attr}=\"{old}\""),
        &value.map(|v| format!("{attr}=\"{v}\"")).unwrap_or_default(),
    )
}

#[test]
fn equal_built_box_preserves_shape_properties_and_steel_beam_mass_at_both_entrances() {
    for t2 in ["9", "9.0"] {
        let xml = dimension(STEEL, "t2", Some(t2));
        for model in both_imports(&xml) {
            let section = &model.sections[0];
            assert_eq!(
                section.shape,
                Some(SectionShape::SteelBox {
                    height: 300.0,
                    width: 200.0,
                    thick: 9.0,
                    horizontal_thick: 9.0,
                    corner_r: Some(0.0),
                })
            );
            assert_eq!(section.area, 8676.0);
            assert_eq!(section.iy, 109_875_852.0);
            assert_eq!(section.iz, 58_328_652.0);
            assert!((section.j - 115_366_091.48962656).abs() < 1e-6);
            let mass = model.element_mass_properties(&model.elements[0]).unwrap();
            assert!((mass.mass_per_length * 1_000_000.0 - 68.1066).abs() < 1e-12);
        }
        let (_, report) = import_stbridge_with_report(&xml).unwrap();
        let t2 = report
            .attributes
            .iter()
            .find(|a| a.element == "StbSecBuild-BOX" && a.attribute == "t2")
            .unwrap();
        assert_eq!((t2.count, t2.imported), (1, 1));
    }
    for model in both_imports(CFT) {
        assert!(matches!(
            model.sections[0].shape,
            Some(SectionShape::CftBox {
                thick: 9.0,
                corner_r: Some(0.0),
                ..
            })
        ));
    }
}

#[test]
fn unequal_built_box_is_rejected_for_steel_and_cft_without_rounding() {
    for (base, section) in [(STEEL, "G1"), (CFT, "CFT1")] {
        for t2 in ["6", "12", "9.000000000000002"] {
            rejected(
                &dimension(base, "t2", Some(t2)),
                &[
                    "BOX1",
                    section,
                    "20",
                    "t1=\"9\"",
                    &format!("t2=\"{t2}\""),
                    "未対応",
                ],
            );
        }
    }
}

#[test]
fn every_built_box_dimension_is_required_finite_and_positive() {
    for base in [STEEL, CFT] {
        for attr in ["A", "B", "t1", "t2"] {
            rejected(&dimension(base, attr, None), &["BOX1", "20", attr, "欠落"]);
            for value in ["0", "-1", "bad", "NaN", "INF", "-INF", "1e309", ""] {
                rejected(
                    &dimension(base, attr, Some(value)),
                    &["BOX1", "20", &format!("{attr}=\"{value}\""), "有限の正値"],
                );
            }
        }
    }
}

#[test]
fn built_box_inner_dimensions_must_be_finite_and_strictly_positive() {
    for base in [STEEL, CFT] {
        for (attr, value, inner, inputs) in [
            ("A", "18", "A−2*t2", ["A=\"18\"", "t2=\"9\""]),
            ("A", "17", "A−2*t2", ["A=\"17\"", "t2=\"9\""]),
            ("B", "18", "B−2*t1", ["B=\"18\"", "t1=\"9\""]),
            ("B", "17", "B−2*t1", ["B=\"17\"", "t1=\"9\""]),
            ("t1", "100", "B−2*t1", ["B=\"200\"", "t1=\"100\""]),
            ("t1", "101", "B−2*t1", ["B=\"200\"", "t1=\"101\""]),
            ("t2", "150", "A−2*t2", ["A=\"300\"", "t2=\"150\""]),
            ("t2", "151", "A−2*t2", ["A=\"300\"", "t2=\"151\""]),
            ("t2", "1e308", "A−2*t2", ["A=\"300\"", "t2=\"1e308\""]),
            ("t1", "1e308", "B−2*t1", ["B=\"200\"", "t1=\"1e308\""]),
        ] {
            rejected(
                &dimension(base, attr, Some(value)),
                &[
                    "BOX1",
                    "20",
                    inner,
                    inputs[0],
                    inputs[1],
                    "内法",
                    "有限の正値",
                ],
            );
        }
    }
}

#[test]
fn unassigned_imported_sections_are_checked_but_unused_library_entries_do_not_reject() {
    let unassigned = STEEL.replace(
        r#"<StbGirder id="5" name="G1" id_node_start="11" id_node_end="37" id_section="20" kind_structure="S"/>"#, "");
    assert!(both_imports(&unassigned)[0].elements.is_empty());
    rejected(
        &dimension(&unassigned, "t2", Some("6")),
        &["BOX1", "G1", "20", "未対応"],
    );
    let extra = STEEL.replace(
        "</StbSecSteel>",
        r#"<StbSecBuild-BOX name="UNUSED" A="300" B="200" t1="9" t2="6"/></StbSecSteel>"#,
    );
    for model in both_imports(&extra) {
        assert_eq!(model.sections.len(), 1);
        assert_eq!(model.sections[0].area, 8676.0);
    }
}

#[test]
fn same_symbol_and_floor_sections_differing_only_in_t2_cannot_merge_successfully() {
    let xml = STEEL.replace("<StbSecSteel>", r#"<StbSecBeam_S id="21" name="G1" floor="2F"><StbSecSteelFigureBeam_S><StbSecSteelBeam_S_Straight shape="BOX2" strength_main="SN400B"/></StbSecSteelFigureBeam_S></StbSecBeam_S><StbSecSteel>"#)
        .replace("</StbSecSteel>", r#"<StbSecBuild-BOX name="BOX2" A="300" B="200" t1="9" t2="12"/></StbSecSteel>"#)
        .replace("</StbGirders>", r#"<StbGirder id="6" name="G2" id_node_start="11" id_node_end="37" id_section="21" kind_structure="S"/></StbGirders>"#);
    rejected(
        &xml,
        &["BOX2", "G1", "21", "t1=\"9\"", "t2=\"12\"", "未対応"],
    );
    rejected(
        &dimension(&xml, "t2", Some("6")),
        &["BOX1", "G1", "20", "t1=\"9\"", "t2=\"6\"", "未対応"],
    );
}

#[test]
fn rolled_box_import_conditions_are_unchanged() {
    let xml = STEEL
        .replace("StbSecBuild-BOX", "StbSecRoll-BOX")
        .replace("t1=\"9\" t2=\"9\"", "t=\"9\"");
    for model in both_imports(&xml) {
        assert!(matches!(
            model.sections[0].shape,
            Some(SectionShape::SteelBox {
                corner_r: None,
                thick: 9.0,
                ..
            })
        ));
    }
}
