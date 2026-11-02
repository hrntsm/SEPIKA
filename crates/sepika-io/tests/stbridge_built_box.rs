use sepika_core::{model::Model, section_shape::SectionShape};
use sepika_io::stbridge::{export_stbridge, import_stbridge, import_stbridge_with_report};

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
fn built_box_preserves_both_thicknesses_geometry_mass_and_pending_properties() {
    for (t2, area, iy, iz, steel_kg_m, cft_kg_m) in [
        (
            "6",
            7584.0,
            87700608.0,
            55314368.0,
            59.5344,
            182.4681235447375,
        ),
        (
            "9",
            8676.0,
            109875852.0,
            58328652.0,
            68.1066,
            188.47920430422213,
        ),
        (
            "12",
            9768.0,
            131127264.0,
            61342936.0,
            76.6788,
            194.49028506370677,
        ),
    ] {
        for (base, cft) in [(STEEL, false), (CFT, true)] {
            for model in both_imports(&dimension(base, "t2", Some(t2))) {
                let section = &model.sections[0];
                let shape = section.shape.as_ref().unwrap();
                assert_eq!(shape.box_thicknesses(), Some((9.0, t2.parse().unwrap())));
                assert_eq!((section.area, section.iy, section.iz), (area, iy, iz));
                let mass = model.element_mass_properties(&model.elements[0]).unwrap();
                let expected = if cft { cft_kg_m } else { steel_kg_m };
                assert!(
                    (mass.mass_per_length * 1_000_000.0 - expected).abs() < 1e-10,
                    "{}",
                    mass.mass_per_length * 1_000_000.0
                );
                if t2 != "9" {
                    assert!(section
                        .resolved_j()
                        .unwrap_err()
                        .contains("採用式・適用条件が未確認"));
                    assert!(section
                        .ensure_properties_resolved()
                        .unwrap_err()
                        .contains("せん断性能"));
                    if cft {
                        assert!(shape
                            .try_cft_equivalent_props(205000.0, 0.3, 78846.0, 36.0, 23.0, 1.0)
                            .unwrap_err()
                            .contains("せん断性能"));
                    }
                }
            }
        }
    }
    for base in [STEEL, CFT] {
        for model in both_imports(&dimension(base, "t2", Some("9.000000000000002"))) {
            assert_eq!(
                model.sections[0].shape.as_ref().unwrap().box_thicknesses(),
                Some((9.0, 9.000000000000002))
            );
            assert!(model.sections[0].resolved_j().is_err());
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
    assert_eq!(
        both_imports(&dimension(&unassigned, "t2", Some("6")))[0].sections[0].area,
        7584.0
    );
    rejected(
        &dimension(&unassigned, "t2", Some("0")),
        &["BOX1", "G1", "20", "有限の正値"],
    );
    let extra = STEEL.replace(
        "</StbSecSteel>",
        r#"<StbSecBuild-BOX name="UNUSED" A="300" B="200" t1="9" t2="bad"/></StbSecSteel>"#,
    );
    for model in both_imports(&extra) {
        assert_eq!(model.sections.len(), 1);
        assert_eq!(model.sections[0].area, 8676.0);
    }
}

#[test]
fn same_symbol_and_floor_sections_keep_distinct_thicknesses_and_references_after_save() {
    let cft = CFT.replace(
        "name=\"CFT1\" strength_concrete",
        "name=\"CFT1\" floor=\"2F\" strength_concrete",
    );
    for (input, section_tag, member_tag, members_tag) in [
        (STEEL, "StbSecBeam_S", "StbGirder", "StbGirders"),
        (cft.as_str(), "StbSecColumn_CFT", "StbColumn", "StbColumns"),
    ] {
        let start = input.find(&format!("<{section_tag} ")).unwrap();
        let end = input.find(&format!("</{section_tag}>")).unwrap() + section_tag.len() + 3;
        let second_section = input[start..end]
            .replace("id=\"20\"", "id=\"21\"")
            .replace("BOX1", "BOX2");
        let start = input.find(&format!("<{member_tag} ")).unwrap();
        let end = start + input[start..].find("/>").unwrap() + 2;
        let second_member = input[start..end]
            .replace("id=\"5\"", "id=\"6\"")
            .replace("id_section=\"20\"", "id_section=\"21\"");
        let xml = input
            .replace("<StbSecSteel>", &format!("{second_section}<StbSecSteel>"))
            .replace(
                "</StbSecSteel>",
                r#"<StbSecBuild-BOX name="BOX2" A="300" B="200" t1="9" t2="12"/></StbSecSteel>"#,
            )
            .replace(
                &format!("</{members_tag}>"),
                &format!("{second_member}</{members_tag}>"),
            );
        for (base, first_t2) in [(&xml, 9.0), (&dimension(&xml, "t2", Some("6")), 6.0)] {
            for model in both_imports(base) {
                assert_eq!(model.sections.len(), 2);
                assert_eq!(
                    model.sections[1].name,
                    format!("{}#2", model.sections[0].name)
                );
                assert_eq!(model.sections[0].floor.as_deref(), Some("2F"));
                assert_eq!(model.sections[0].floor, model.sections[1].floor);
                assert_eq!(model.sections[0].frame_use, model.sections[1].frame_use);
                assert_ne!(model.elements[0].section, model.elements[1].section);
                let thicknesses: Vec<_> = model
                    .elements
                    .iter()
                    .map(|e| {
                        model
                            .section(e.section.unwrap())
                            .unwrap()
                            .shape
                            .as_ref()
                            .unwrap()
                            .box_thicknesses()
                            .unwrap()
                    })
                    .collect();
                assert_eq!(thicknesses, vec![(9.0, first_t2), (9.0, 12.0)]);
                let file = std::env::temp_dir()
                    .join(format!("sepika683-box-{}.ovika", std::process::id()));
                sepika_io::ovika::save_ovika(&file, &model, Default::default()).unwrap();
                let loaded = sepika_io::ovika::load_ovika(&file).unwrap().model;
                assert!(model.eq_ignoring_dofmap(&loaded));
                let reimported = import_stbridge(&export_stbridge(&loaded).unwrap()).unwrap();
                let roundtrip_thicknesses: Vec<_> = reimported
                    .elements
                    .iter()
                    .map(|element| {
                        reimported
                            .section(element.section.unwrap())
                            .unwrap()
                            .shape
                            .as_ref()
                            .unwrap()
                            .box_thicknesses()
                            .unwrap()
                    })
                    .collect();
                assert_eq!(roundtrip_thicknesses, thicknesses);
                std::fs::remove_file(file).unwrap();
            }
            let (_, report) = import_stbridge_with_report(base).unwrap();
            for attr in ["A", "B", "t1", "t2"] {
                let entry = report
                    .attributes
                    .iter()
                    .find(|a| a.element == "StbSecBuild-BOX" && a.attribute == attr)
                    .unwrap();
                assert_eq!((entry.count, entry.imported), (2, 2));
            }
        }
    }
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

#[test]
fn built_box_standard_export_preserves_steel_and_cft_shapes_with_or_without_members() {
    for (base, member, family) in [
        (STEEL, "StbGirder", "StbSecBeam_S"),
        (CFT, "StbColumn", "StbSecColumn_CFT"),
    ] {
        for t2 in ["6", "9", "12", "9.000000000000002"] {
            let xml = dimension(base, "t2", Some(t2));
            for mut model in both_imports(&xml) {
                for unused in [false, true] {
                    if unused {
                        model.elements.clear();
                    }
                    let expected = model.sections[0].shape.clone();
                    let exported = export_stbridge(&model).unwrap();
                    assert!(exported.contains("<StbSecBuild-BOX "), "{exported}");
                    assert!(exported.contains(&format!("<{family} ")), "{exported}");
                    assert!(!exported.contains("<StbSecRoll-BOX "));
                    assert!(!exported.contains("<StbSecRaw "));
                    let loaded = import_stbridge(&exported).unwrap();
                    assert_eq!(loaded.sections[0].shape, expected);
                    assert_eq!(loaded.elements.len(), model.elements.len(), "{member}");
                    assert_eq!(loaded.stb_strengths.sections[0].built_box, expected);
                }
            }
        }
    }
}

#[test]
fn built_box_provenance_survives_ovika_and_rejects_edited_geometry() {
    for base in [STEEL, CFT] {
        let model = import_stbridge(&dimension(base, "t2", Some("6"))).unwrap();
        let file =
            std::env::temp_dir().join(format!("sepika1056-box-{}.ovika", std::process::id()));
        sepika_io::ovika::save_ovika(&file, &model, Default::default()).unwrap();
        let loaded = sepika_io::ovika::load_ovika(&file).unwrap().model;
        std::fs::remove_file(file).unwrap();
        assert!(model.eq_ignoring_dofmap(&loaded));
        assert_eq!(
            import_stbridge(&export_stbridge(&loaded).unwrap())
                .unwrap()
                .sections[0]
                .shape,
            model.sections[0].shape
        );
        for (height, width, side, horizontal, radius) in [
            (301.0, 200.0, 9.0, 6.0, Some(0.0)),
            (300.0, 200.0, 9.0, 9.0, Some(12.0)),
            (300.0, 200.0, 9.0, 6.0, None),
            (0.0, 200.0, 9.0, 6.0, Some(0.0)),
        ] {
            let mut edited = loaded.clone();
            let shape = edited.sections[0].shape.as_mut().unwrap();
            match shape {
                SectionShape::SteelBox {
                    height: h,
                    width: w,
                    thick: t,
                    horizontal_thick: t2,
                    corner_r: r,
                }
                | SectionShape::CftBox {
                    height: h,
                    width: w,
                    thick: t,
                    horizontal_thick: t2,
                    corner_r: r,
                } => {
                    *h = height;
                    *w = width;
                    *t = side;
                    *t2 = horizontal;
                    *r = radius;
                }
                _ => unreachable!(),
            }
            assert!(export_stbridge(&edited)
                .unwrap_err()
                .to_string()
                .contains("原組立BOXと編集後形状が一致しません"));
        }
    }
}

#[test]
fn rolled_box_export_still_requires_known_valid_positive_radius() {
    for base in [STEEL, CFT] {
        let rolled = base
            .replace("StbSecBuild-BOX", "StbSecRoll-BOX")
            .replace("t1=\"9\" t2=\"9\"", "t=\"9\"");
        for (attr, accepted) in [("", false), (" r=\"0\"", false), (" r=\"12\"", true)] {
            let xml = rolled.replace("t=\"9\"/>", &format!("t=\"9\"{attr}/>"));
            let model = import_stbridge(&xml).unwrap();
            assert!(model
                .stb_strengths
                .sections
                .iter()
                .all(|s| s.built_box.is_none()));
            let result = export_stbridge(&model);
            assert_eq!(result.is_ok(), accepted);
            if accepted {
                assert!(result.unwrap().contains("<StbSecRoll-BOX "));
            } else {
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("未知または0の半径"));
            }
        }
        for radius in [Some(-1.0), Some(f64::NAN), Some(f64::INFINITY), Some(101.0)] {
            let mut model = import_stbridge(&rolled).unwrap();
            match model.sections[0].shape.as_mut().unwrap() {
                SectionShape::SteelBox { corner_r, .. } | SectionShape::CftBox { corner_r, .. } => {
                    *corner_r = radius
                }
                _ => unreachable!(),
            }
            assert!(export_stbridge(&model)
                .unwrap_err()
                .to_string()
                .contains("半径が不正"));
        }
    }
}

#[test]
fn invalid_built_box_dimensions_and_nonzero_radius_are_never_exported() {
    for base in [STEEL, CFT] {
        for (height, width, side, horizontal, radius, message) in [
            (f64::INFINITY, 200.0, 9.0, 9.0, Some(0.0), "有限な正値"),
            (300.0, 0.0, 9.0, 9.0, Some(0.0), "有限な正値"),
            (300.0, 200.0, -1.0, 9.0, Some(0.0), "有限な正値"),
            (300.0, 200.0, 9.0, 0.0, Some(0.0), "有限な正値"),
            (300.0, 200.0, 100.0, 9.0, Some(0.0), "内法寸法"),
            (300.0, 200.0, 9.0, 150.0, Some(0.0), "内法寸法"),
            (300.0, 200.0, 9.0, 9.0, Some(12.0), "角R=0"),
        ] {
            let mut model = import_stbridge(base).unwrap();
            let shape = model.sections[0].shape.as_mut().unwrap();
            match shape {
                SectionShape::SteelBox {
                    height: h,
                    width: w,
                    thick: t1,
                    horizontal_thick: t2,
                    corner_r: r,
                }
                | SectionShape::CftBox {
                    height: h,
                    width: w,
                    thick: t1,
                    horizontal_thick: t2,
                    corner_r: r,
                } => {
                    *h = height;
                    *w = width;
                    *t1 = side;
                    *t2 = horizontal;
                    *r = radius;
                }
                _ => unreachable!(),
            }
            model.stb_strengths.sections[0].built_box = Some(shape.clone());
            let error = export_stbridge(&model).unwrap_err().to_string();
            assert!(error.contains(message), "{error}");
        }
    }
}
