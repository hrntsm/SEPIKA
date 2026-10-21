use sepika_core::model::{MaterialCategory, Model};
use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report};

const CFT: &str = include_str!("fixtures/cft_standard_steel.stb");

fn both_imports(xml: &str) -> [Model; 2] {
    let plain = import_stbridge(xml).unwrap();
    let reported = import_stbridge_with_report(xml).unwrap().0;
    assert!(plain.eq_ignoring_dofmap(&reported));
    plain.validate().unwrap();
    reported.validate().unwrap();
    [plain, reported]
}

fn cft_mass_per_length(model: &Model) -> f64 {
    let element = model
        .elements
        .iter()
        .find(|e| model.element_section(e).unwrap().name == "CFT1")
        .unwrap();
    let steel = model.element_steel_material(element).unwrap();
    assert_eq!(steel.category, MaterialCategory::Steel);
    assert_eq!(steel.young, 205_000.0);
    assert_eq!(steel.poisson, 0.3);
    assert_eq!(steel.density, 7.85e-9);
    let concrete = model.element_material(element).unwrap();
    assert_eq!(concrete.category, MaterialCategory::Concrete);
    assert_eq!(concrete.fc, Some(36.0));
    assert_eq!(concrete.cft_core_design_unit_weight_n_per_mm3(), 23.0e-6);
    let expected_t_per_mm = 8676.0 * 7.85e-9 + 51324.0 * concrete.cft_core_mass_density();
    let actual = model.element_mass_properties(element).unwrap();
    assert!((actual.mass_per_length - expected_t_per_mm).abs() < 1e-15);
    assert!((8676.0 * steel.density * 1_000_000.0 - 68.1066).abs() < 1e-12);
    actual.mass_per_length
}

#[test]
fn cft_standard_steel_has_density_and_independent_composite_mass_at_both_entrances() {
    for (grade, fy) in [
        ("SN400B", 235.0),
        ("SN490B", 325.0),
        ("STKR400", 235.0),
        ("SN400", 235.0),
    ] {
        for model in both_imports(&CFT.replace("SN400B", grade)) {
            let steel = model.element_steel_material(&model.elements[0]).unwrap();
            assert_eq!(steel.name, grade);
            assert_eq!(steel.fy, Some(fy));
            cft_mass_per_length(&model);
        }
    }
}

#[test]
fn ordinary_steel_in_the_same_document_does_not_change_cft_density_or_mass() {
    let mixed = CFT.replace(
        "</StbColumns>",
        r#"<StbColumn id="6" name="S1" id_node_bottom="41" id_node_top="59" id_section="21" kind_structure="S"/></StbColumns>"#,
    ).replace(
        "      <StbSecSteel>",
        r#"      <StbSecColumn_S id="21" name="S1"><StbSecSteelFigureColumn_S><StbSecSteelColumn_S_Same shape="BOX1" strength_main="SN400B"/></StbSecSteelFigureColumn_S></StbSecColumn_S><StbSecSteel>"#,
    );
    for (alone, mixed) in both_imports(CFT).into_iter().zip(both_imports(&mixed)) {
        assert_eq!(mixed.elements.len(), 2);
        assert_eq!(cft_mass_per_length(&alone), cft_mass_per_length(&mixed));
        let ordinary = mixed
            .elements
            .iter()
            .find(|e| mixed.element_section(e).unwrap().name == "S1")
            .unwrap();
        let steel = mixed.element_material(ordinary).unwrap();
        assert_eq!(steel.name, "SN400B");
        assert_eq!(steel.density, 7.85e-9);
        assert!(
            (mixed
                .element_mass_properties(ordinary)
                .unwrap()
                .mass_per_length
                * 1_000_000.0
                - 68.1066)
                .abs()
                < 1e-12
        );
    }
}

#[test]
fn unknown_cft_steel_is_not_given_standard_strength_or_density() {
    for model in both_imports(&CFT.replace("SN400B", "SN400UNKNOWN")) {
        let element = &model.elements[0];
        let section = model.element_section(element).unwrap();
        let steel = &model.materials[section.steel_material.unwrap().index()];
        assert_eq!(steel.name, "SN400UNKNOWN");
        assert_eq!(steel.fy, None);
        assert_eq!(steel.density, 0.0);
        assert!(model.element_steel_material(element).is_none());
        assert!(model
            .stb_strength_diagnostics()
            .iter()
            .any(|s| s.contains("SN400UNKNOWN")));
        assert!(model
            .element_mass_properties(element)
            .unwrap_err()
            .contains("鋼管材料が未設定"));
    }
}

#[test]
fn cft_with_explicitly_invalid_steel_density_still_rejects_mass_evaluation() {
    for density in [0.0, -7.85e-9, f64::NAN, f64::INFINITY] {
        let mut model = import_stbridge(CFT).unwrap();
        let steel_id = model.sections[0].steel_material.unwrap();
        model.materials[steel_id.index()].density = density;
        assert!(model
            .element_mass_properties(&model.elements[0])
            .unwrap_err()
            .contains("鋼管材料の密度が不正"));
    }
}
