use super::*;
use sepika_core::ids::SectionId;
use sepika_core::section_shape::SectionShape;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/rebar507/{name}.stb",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn standard_rebar_research_cases_use_product_import_and_ovika_roundtrip() {
    for name in [
        "normal_same",
        "normal_omitted_optional_stage",
        "normal_column38_axial4",
        "normal_StartEnd_reverse_xml_order",
        "normal_ThreeTypes_reverse_xml_order",
        "normal_column_BASE_TOP_reverse_xml_DIR_XY",
    ] {
        let (model, report) = import_stbridge_with_report(&fixture(name)).unwrap();
        assert!(
            !report.warnings.is_empty(),
            "保存可能と計算可能は別: {name}"
        );
        let path = std::env::temp_dir().join(format!(
            "sepika-rebar507-{}-{name}.ovika",
            std::process::id()
        ));
        crate::ovika::save_ovika(&path, &model, crate::ovika::OvikaExtras::default()).unwrap();
        let saved = crate::ovika::load_ovika(&path).unwrap().model;
        assert_eq!(model.stb_strengths, saved.stb_strengths);
        let xml = export_stbridge(&saved).unwrap();
        let again = import_stbridge(&xml).unwrap();
        assert_eq!(saved.stb_strengths, again.stb_strengths, "{name}");
        assert_eq!(saved.sections, again.sections, "{name}");
        assert!(!again.stb_rebar_diagnostics().is_empty(), "{name}");
        if let Ok(output) = std::env::var("SEPIKA_REBAR507_OUTPUT") {
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(
                std::path::Path::new(&output).join(format!("{name}.stb")),
                xml,
            )
            .unwrap();
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn standard_rebar_invalid_values_and_semantics_are_rejected() {
    for name in [
        "invalid_N_main_bottom_1st",
        "invalid_N_main_top_1st",
        "invalid_N_main_top_2nd",
        "invalid_N_stirrup",
        "invalid_pitch_stirrup",
        "schema_valid_semantic_bad_total25",
        "schema_valid_semantic_duplicate_StartEnd",
        "schema_valid_semantic_duplicate_ThreeTypes",
        "schema_valid_semantic_duplicate_columnTOP",
    ] {
        let error = import_stbridge(&fixture(name)).unwrap_err().to_string();
        assert!(error.contains("断面"), "{name}: {error}");
    }
}

#[test]
fn standard_roles_positions_and_omissions_remain_distinct() {
    let model = import_stbridge(&fixture("normal_same")).unwrap();
    let beam = &model.stb_strengths.sections[1];
    let raw = beam.rebar.as_ref().unwrap();
    let attrs = &raw.positions[0].attributes;
    assert_eq!(attrs["N_main_top_1st"], "4");
    assert_eq!(attrs["N_main_top_2nd"], "2");
    assert_eq!(attrs["N_main_bottom_1st"], "3");
    assert_eq!(attrs["N_2nd_main_top_1st"], "2");
    assert_eq!(attrs["N_2nd_main_bottom_1st"], "1");
    for (part, diameter, value) in [
        ("main", "D22", 345.),
        ("2nd_main", "D13", 295.),
        ("stirrup", "D10", 295.),
        ("web", "D10", 295.),
        ("bar_spacing", "D10", 295.),
    ] {
        let input = beam.reinforcement.iter().find(|r| r.part == part).unwrap();
        assert_eq!(input.diameter.as_deref(), Some(diameter));
        assert_eq!(model.resolve_stb_rebar(input).unwrap().value, value);
    }
    assert_eq!(attrs["N_stirrup"], "2");
    assert_eq!(attrs["pitch_stirrup"], "100");
    assert_eq!(attrs["N_web"], "2");
    assert_eq!(attrs["N_bar_spacing"], "1");
    assert_eq!(attrs["pitch_bar_spacing"], "200");
    let effective =
        raw.effective_attributes(&raw.positions[0], &model.stb_strengths.apply_conditions);
    assert!(!effective.contains_key("N_web"));
    assert!(!effective.contains_key("N_bar_spacing"));
    assert!(effective.contains_key("N_2nd_main_top_1st"));
}

#[test]
fn apply_conditions_absence_false_and_partial_true_do_not_invent_values() {
    use sepika_core::model::StbRebarElement;
    let model = import_stbridge(&fixture("normal_same")).unwrap();
    let raw = model.stb_strengths.sections[1].rebar.as_ref().unwrap();
    let condition = |set_default: &str| StbRebarElement {
        element: "StbBeam_RC_RebarPositionApply".into(),
        attributes: [
            ("set_default".into(), set_default.into()),
            ("center_top_bottom".into(), "51".into()),
            ("depth_cover_side".into(), "40".into()),
        ]
        .into_iter()
        .collect(),
    };
    let absent = raw.effective_attributes(&raw.positions[0], &[]);
    assert!(!absent.contains_key("center_top"));
    let not_default = raw.effective_attributes(&raw.positions[0], &[condition("false")]);
    assert!(!not_default.contains_key("center_top"));
    let default = raw.effective_attributes(&raw.positions[0], &[condition("true")]);
    assert_eq!(default["center_top"], "51");
    assert_eq!(default["center_bottom"], "51");
    assert_eq!(default["depth_cover_side"], "40");
    assert!(!default.contains_key("center_side"));
    assert!(!default.contains_key("interval"));
    let mut explicit = raw.positions[0].clone();
    explicit.attributes.insert("center_top".into(), "61".into());
    assert_eq!(
        raw.effective_attributes(&explicit, &[condition("true")])["center_top"],
        "61"
    );
}

#[test]
fn standard_unrepresentable_geometry_is_preserved_and_unused_sections_do_not_block_analysis() {
    use sepika_core::ids::SectionId;
    let source = fixture("normal_same");
    for (key, value, reason) in [
        ("kind_corner", "DIR_X", "kind_corner"),
        ("isSpiral", "true", "スパイラル"),
    ] {
        let xml = source.replace(
            "<StbSecBarArrangementColumn_RC",
            &format!("<StbSecBarArrangementColumn_RC {key}=\"{value}\""),
        );
        let mut model = import_stbridge(&xml).unwrap();
        assert!(model
            .ensure_stb_rebar_supported(SectionId(0))
            .unwrap_err()
            .contains(reason));
        let back = import_stbridge(&export_stbridge(&model).unwrap()).unwrap();
        assert_eq!(back.stb_strengths, model.stb_strengths);
        model.elements.clear();
        assert!(model.stb_rebar_analysis_diagnostics().is_empty());
    }
    let model = import_stbridge(&source).unwrap();
    let roles = model.stb_rebar_roles(SectionId(1));
    assert_eq!(
        roles.iter().find(|r| r.part == "2nd_main").unwrap().counts["N_2nd_main_top_1st"],
        2
    );
    assert!(!roles.iter().find(|r| r.part == "web").unwrap().applied);
    assert!(roles.iter().all(|r| r.points_mm.is_none()));
    let column = import_stbridge(&fixture("normal_column38_axial4")).unwrap();
    let roles = column.stb_rebar_roles(SectionId(0));
    assert_eq!(
        roles.iter().find(|r| r.part == "axial").unwrap().counts["N_axial"],
        4
    );
    assert!(!roles
        .iter()
        .find(|r| r.part == "main")
        .unwrap()
        .counts
        .contains_key("N_main_total"));
}

#[test]
fn explicit_center_takes_priority_without_overwriting_original_cover_or_omission() {
    use sepika_core::model::StbRebarElement;
    let mut model = import_stbridge(&fixture("normal_same")).unwrap();
    model.stb_strengths.apply_conditions.push(StbRebarElement {
        element: "StbBeam_RC_RebarPositionApply".into(),
        attributes: [
            ("set_default".into(), "true".into()),
            ("depth_cover_top_bottom".into(), "40".into()),
            ("center_top_bottom".into(), "51".into()),
            ("center_side".into(), "51".into()),
        ]
        .into_iter()
        .collect(),
    });
    let back = import_stbridge(&export_stbridge(&model).unwrap()).unwrap();
    let raw = back.stb_strengths.sections[1].rebar.as_ref().unwrap();
    let attrs = raw.effective_attributes(&raw.positions[0], &back.stb_strengths.apply_conditions);
    assert_eq!(attrs["depth_cover_top"], "40");
    assert_eq!(attrs["center_top"], "51");
    match back.sections[1].shape.as_ref().unwrap() {
        sepika_core::section_shape::SectionShape::RcBeamRect { rebar, .. } => {
            assert_eq!(rebar.cover, 30.0)
        }
        _ => panic!("beam"),
    }
    assert!(!raw.container.attributes.contains_key("center_top"));
    assert_eq!(
        back.stb_rebar_roles(sepika_core::ids::SectionId(1))
            .iter()
            .find(|r| r.part == "main")
            .unwrap()
            .center_mm["center_top"],
        51.0
    );
}

#[test]
fn third_party_unmodified_known_invalid_common_is_rejected() {
    let xml = include_str!("../../tests/fixtures/third-party-known-invalid-507.stb");
    let error = import_stbridge(xml).unwrap_err().to_string();
    assert!(error.contains("StbCommon app_version"), "{error}");
}

#[test]
fn unresolved_required_diameter_is_preserved_and_never_treated_as_unreinforced() {
    for diameter in ["D0", "UNKNOWN", "NaN", "inf"] {
        let xml =
            fixture("normal_same").replace("D_main=\"D22\"", &format!("D_main=\"{diameter}\""));
        let model = import_stbridge(&xml).unwrap();
        assert_eq!(
            model.stb_strengths.sections[1]
                .rebar
                .as_ref()
                .unwrap()
                .positions[0]
                .attributes["D_main"],
            diameter
        );
        let main = model.stb_strengths.sections[1]
            .reinforcement
            .iter()
            .find(|r| r.part == "main")
            .unwrap();
        assert!(model.resolve_stb_rebar(main).is_ok());
        assert!(model
            .stb_rebar_analysis_diagnostics()
            .iter()
            .any(|m| m.contains("径を解決できません")));
        assert!(export_stbridge(&model)
            .unwrap()
            .contains(&format!("D_main=\"{diameter}\"")));
    }
}

#[test]
fn standard_rebar_fourth_and_later_source_stages_are_rejected() {
    for stage in ["4th", "5th"] {
        let xml = fixture("normal_same").replace(
            "N_main_top_1st=\"4\"",
            &format!("N_main_top_1st=\"4\" N_main_top_{stage}=\"1\""),
        );
        let error = import_stbridge(&xml).unwrap_err().to_string();
        assert!(
            error.contains("断面 1") && error.contains("標準段数"),
            "{error}"
        );
    }
}

fn save_supplemental_output(name: &str, xml: &str) {
    if let Ok(output) = std::env::var("SEPIKA_REBAR507_OUTPUT") {
        let path = std::path::Path::new(&output).join("supplemental");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join(format!("{name}.stb")), xml).unwrap();
    }
}

#[test]
fn circle_band_count_is_required_preserved_and_gated_before_reduction() {
    let source = fixture("supplemental/circle_same");
    let missing = source.replace(" N_band=\"2\"", "");
    let error = import_stbridge(&missing).unwrap_err().to_string();
    assert!(
        error.contains("断面 0") && error.contains("N_band"),
        "{error}"
    );
    for count in [2, 3] {
        let xml = source.replace("N_band=\"2\"", &format!("N_band=\"{count}\""));
        let model = import_stbridge(&xml).unwrap();
        let supported = model.ensure_stb_rebar_supported(SectionId(0));
        if count == 2 {
            supported.unwrap();
        } else {
            assert!(supported.unwrap_err().contains("N_band"));
        }
        assert_eq!(
            model
                .stb_rebar_roles(SectionId(0))
                .iter()
                .find(|r| r.part == "band")
                .unwrap()
                .counts["N_band"],
            count
        );
        let path = std::env::temp_dir().join(format!(
            "sepika507-circle-{}-{count}.ovika",
            std::process::id()
        ));
        crate::ovika::save_ovika(&path, &model, Default::default()).unwrap();
        let saved = crate::ovika::load_ovika(&path).unwrap().model;
        std::fs::remove_file(path).unwrap();
        let output = export_stbridge(&saved).unwrap();
        let again = import_stbridge(&output).unwrap();
        assert_eq!(model.stb_strengths, again.stb_strengths);
        assert_eq!(
            count == 2,
            again.ensure_stb_rebar_supported(SectionId(0)).is_ok()
        );
        save_supplemental_output(&format!("circle_band{count}"), &output);
    }
}

#[test]
fn native_circle_exports_explicit_band_count_and_remains_supported() {
    let mut model = import_stbridge(&fixture("supplemental/circle_same")).unwrap();
    let source = &mut model.stb_strengths.sections[0];
    source.rebar = None;
    source.reinforcement.clear();
    source.native_rebar = true;
    let output = export_stbridge(&model).unwrap();
    assert!(output.contains("N_band=\"2\""));
    let again = import_stbridge(&output).unwrap();
    again.ensure_stb_rebar_supported(SectionId(0)).unwrap();
    assert_eq!(model.sections[0].shape, again.sections[0].shape);
    save_supplemental_output("circle_native", &output);
}

#[test]
fn common_apply_roles_resolve_effective_counts_diameters_and_strength_without_filling_raw() {
    let common = "<StbReinforcementStrengthList><StbReinforcementStrength D=\"D10\" strength=\"SD295A\"/></StbReinforcementStrengthList>";
    let conditions = "<StbBeam_RC_BarWebApply set_default=\"true\" D_web=\"D10\" N_web=\"2\"/><StbBeam_RC_BarSpacingApply set_default=\"true\" D_bar_spacing=\"D10\" N_bar_spacing=\"1\" pitch_bar_spacing=\"200\"/>";
    let source = fixture("supplemental/circle_same")
        .replace(
            "<StbApplyConditionsList>",
            &format!("{common}<StbApplyConditionsList>"),
        )
        .replace(
            "</StbApplyConditionsList>",
            &format!("{conditions}</StbApplyConditionsList>"),
        );
    for explicit in [false, true] {
        let xml = if explicit {
            source.replace("N_stirrup=\"2\"", "N_stirrup=\"2\" D_web=\"D13\" N_web=\"3\" strength_web=\"SD345\" D_bar_spacing=\"D13\" N_bar_spacing=\"2\" pitch_bar_spacing=\"150\" strength_bar_spacing=\"SD345\"")
        } else {
            source.clone()
        };
        let model = import_stbridge(&xml).unwrap();
        let raw = &model.stb_strengths.sections[1]
            .rebar
            .as_ref()
            .unwrap()
            .positions[0]
            .attributes;
        assert_eq!(raw.contains_key("N_web"), explicit);
        assert_eq!(raw.contains_key("N_bar_spacing"), explicit);
        let roles = model.stb_rebar_roles(SectionId(1));
        for (part, key, expected) in [
            ("web", "N_web", if explicit { 3 } else { 2 }),
            ("bar_spacing", "N_bar_spacing", if explicit { 2 } else { 1 }),
        ] {
            let role = roles.iter().find(|r| r.part == part).unwrap();
            assert!(role.applied);
            assert_eq!(role.counts[key], expected);
            assert_eq!(
                role.diameter.as_deref(),
                Some(if explicit { "D13" } else { "D10" })
            );
            let resolved = role.strength.as_ref().unwrap();
            assert_eq!(resolved.value, if explicit { 345.0 } else { 295.0 });
            assert_eq!(
                resolved.source,
                if explicit {
                    sepika_core::model::StrengthSource::Section
                } else {
                    sepika_core::model::StrengthSource::Diameter
                }
            );
        }
        assert_eq!(
            roles
                .iter()
                .find(|r| r.part == "bar_spacing")
                .unwrap()
                .pitch_mm,
            Some(if explicit { 150.0 } else { 200.0 })
        );
        assert!(model.ensure_stb_rebar_supported(SectionId(1)).is_err());
        let output = export_stbridge(&model).unwrap();
        let again = import_stbridge(&output).unwrap();
        assert_eq!(model.stb_strengths, again.stb_strengths);
        assert_eq!(roles, again.stb_rebar_roles(SectionId(1)));
        save_supplemental_output(
            if explicit {
                "apply_individual"
            } else {
                "apply_default"
            },
            &output,
        );
    }
    let partial = source.replace(" D_web=\"D10\"", "");
    let model = import_stbridge(&partial).unwrap();
    let roles = model.stb_rebar_roles(SectionId(1));
    let web = roles.iter().find(|r| r.part == "web").unwrap();
    assert_eq!(web.counts["N_web"], 2);
    assert!(web.diameter.is_none());
    assert!(web.strength.as_ref().unwrap_err().contains("鉄筋径"));
    let disabled = source
        .replace(
            "BarWebApply set_default=\"true\"",
            "BarWebApply set_default=\"false\"",
        )
        .replace(
            "BarSpacingApply set_default=\"true\"",
            "BarSpacingApply set_default=\"false\"",
        );
    let model = import_stbridge(&disabled).unwrap();
    assert!(!model
        .stb_rebar_roles(SectionId(1))
        .iter()
        .any(|r| r.part == "web" || r.part == "bar_spacing"));
    model.ensure_stb_rebar_supported(SectionId(1)).unwrap();
}

#[test]
fn unresolved_standard_topology_is_saved_instead_of_rejected_by_an_unproved_total_formula() {
    let original = fixture("normal_same");
    let unknown = original
        .replace("N_main_X_1st=\"7\"", "N_main_X_1st=\"3\"")
        .replace("N_main_Y_1st=\"7\"", "N_main_Y_1st=\"3\"")
        .replace("N_main_X_2nd=\"2\"", "N_main_X_2nd=\"1\"")
        .replace("N_main_Y_2nd=\"2\"", "N_main_Y_2nd=\"1\"")
        .replace("N_main_total=\"24\"", "N_main_total=\"16\"");
    let mixed = original.replace(
        "N_main_total=\"24\"",
        "N_main_total=\"29\" D_2nd_main=\"D13\" N_2nd_main_X_1st=\"2\" N_2nd_main_Y_1st=\"2\"",
    );
    for (name, input) in [
        ("unknown_small_topology", unknown),
        ("unknown_mixed_topology", mixed),
    ] {
        let model = import_stbridge(&input).unwrap();
        assert!(model
            .ensure_stb_rebar_supported(SectionId(0))
            .unwrap_err()
            .contains("位相"));
        let path =
            std::env::temp_dir().join(format!("sepika507-{name}-{}.ovika", std::process::id()));
        crate::ovika::save_ovika(&path, &model, crate::ovika::OvikaExtras::default()).unwrap();
        let saved = crate::ovika::load_ovika(&path).unwrap().model;
        let output = export_stbridge(&saved).unwrap();
        let again = import_stbridge(&output).unwrap();
        assert_eq!(model.stb_strengths, again.stb_strengths);
        assert!(again.ensure_stb_rebar_supported(SectionId(0)).is_err());
        save_supplemental_output(name, &output);
        std::fs::remove_file(path).unwrap();
    }
    let wrong38 =
        fixture("normal_column38_axial4").replace("N_main_total=\"38\"", "N_main_total=\"39\"");
    assert!(import_stbridge(&wrong38)
        .unwrap_err()
        .to_string()
        .contains("N_main_total"));
}

#[test]
fn standard_column_x_counts_vertical_rows_and_native_export_inverts_the_mapping() {
    let input = fixture("normal_same")
        .replace("N_main_X_1st=\"7\" N_main_X_2nd=\"2\" N_main_Y_1st=\"7\" N_main_Y_2nd=\"2\" N_main_total=\"24\"", "N_main_X_1st=\"4\" N_main_Y_1st=\"3\" N_main_total=\"10\"")
;
    let mut model = import_stbridge(&input).unwrap();
    let Some(SectionShape::RcColumnRect { rebar, .. }) = &model.sections[0].shape else {
        panic!("column")
    };
    assert_eq!(rebar.x, vec![3], "horizontal top/bottom rows");
    assert_eq!(rebar.y, vec![4], "vertical left/right rows");
    let points = rebar.bar_positions(600.0, 600.0).unwrap();
    assert_eq!(points.len(), 10, "four shared corners counted once");
    let outer_y = points.iter().map(|p| p.y.abs()).fold(0.0_f64, f64::max);
    let outer_x = points.iter().map(|p| p.x.abs()).fold(0.0_f64, f64::max);
    assert_eq!(
        points
            .iter()
            .filter(|p| (p.y - outer_y).abs() < 1e-8)
            .count(),
        3
    );
    assert_eq!(
        points
            .iter()
            .filter(|p| (p.x - outer_x).abs() < 1e-8)
            .count(),
        4
    );
    let record = &mut model.stb_strengths.sections[0];
    record.rebar = None;
    record.reinforcement.clear();
    record.native_rebar = true;
    let output = export_stbridge(&model).unwrap();
    assert!(output.contains("N_main_X_1st=\"4\""));
    assert!(output.contains("N_main_Y_1st=\"3\""));
    let again = import_stbridge(&output).unwrap();
    assert_eq!(model.sections[0].shape, again.sections[0].shape);
    save_supplemental_output("native_column_direction", &output);
}

#[test]
fn src_standard_column_and_beam_rebar_ovika_roundtrip_preserves_schema_child_order() {
    let model = import_stbridge(&fixture("supplemental/src_column_beam")).unwrap();
    model.ensure_stb_rebar_supported(SectionId(0)).unwrap();
    model.ensure_stb_rebar_supported(SectionId(1)).unwrap();
    let path = std::env::temp_dir().join(format!("sepika507-src-{}.ovika", std::process::id()));
    crate::ovika::save_ovika(&path, &model, crate::ovika::OvikaExtras::default()).unwrap();
    let saved = crate::ovika::load_ovika(&path).unwrap().model;
    let output = export_stbridge(&saved).unwrap();
    for family in ["Column", "Beam"] {
        let section = output
            .split(&format!("<StbSec{family}_SRC "))
            .nth(1)
            .unwrap()
            .split(&format!("</StbSec{family}_SRC>"))
            .next()
            .unwrap();
        assert!(
            section
                .find(&format!("<StbSecBarArrangement{family}_SRC"))
                .unwrap()
                < section
                    .find(&format!("<StbSecSteelFigure{family}_SRC"))
                    .unwrap(),
            "採用XSDは配筋→鉄骨図形の順"
        );
    }
    let again = import_stbridge(&output).unwrap();
    assert_eq!(model.stb_strengths, again.stb_strengths);
    assert_eq!(model.sections, again.sections);
    save_supplemental_output("src_column_beam", &output);
    std::fs::remove_file(path).unwrap();
}
