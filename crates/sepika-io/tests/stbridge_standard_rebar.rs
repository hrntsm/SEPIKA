use sepika_core::{model::Model, section_shape::SectionShape};
use sepika_io::{
    ovika::{load_ovika, save_ovika},
    stbridge::{import_stbridge, import_stbridge_with_report},
};

fn standard_input(family: &str, circle: bool, apply: Option<bool>, center: bool) -> String {
    let beam = family.starts_with("Beam");
    let src = family.ends_with("SRC");
    let position_key = if beam { "top" } else { "start_X" };
    let position = if center { "center" } else { "depth_cover" };
    let apply = apply.map(|value| format!(
        r#"<StbApplyConditionsList><Stb{family}_RebarPositionApply set_default="{value}"/></StbApplyConditionsList>"#
    )).unwrap_or_default();
    let geometry = if beam {
        format!(r#"<StbSec{family}_Straight width="400" depth="600"/>"#)
    } else if circle {
        format!(r#"<StbSec{family}_Circle D="600"/>"#)
    } else {
        format!(r#"<StbSec{family}_Rect width_X="500" width_Y="600"/>"#)
    };
    let bars = if beam {
        format!(
            r#"<StbSecBar{family}_Same D_main="D25" D_stirrup="D13" strength_main="SD390" strength_stirrup="SD295A" N_main_top_1st="4" N_main_top_2nd="2" N_main_bottom_1st="3" N_stirrup="2" pitch_stirrup="100"/>"#
        )
    } else if circle {
        format!(
            r#"<StbSecBar{family}_CircleSame D_main="D25" D_band="D13" strength_main="SD390" strength_band="SD295A" N_main="12" N_band="1" pitch_band="100"/>"#
        )
    } else {
        format!(
            r#"<StbSecBar{family}_RectSame D_main="D25" D_band="D13" strength_main="SD390" strength_band="SD295A" N_main_X_1st="4" N_main_Y_1st="3" N_main_total="10" N_band_direction_X="2" N_band_direction_Y="3" pitch_band="100"/>"#
        )
    };
    let auxiliary = if family == "Column_RC" && !circle {
        r#"<StbSecBarColumnXReinforced N_main_X="2" N_main_Y="2" N_main_total="4"/>"#
    } else {
        ""
    };
    let steel = if !src {
        String::new()
    } else if beam {
        r#"<StbSecSteelFigureBeam_SRC><StbSecSteelBeam_SRC_Straight shape="H1" strength_main="SN490B"/></StbSecSteelFigureBeam_SRC>"#.into()
    } else {
        r#"<StbSecSteelFigureColumn_SRC><StbSecSteelColumn_SRC_Same><StbSecColumn_SRC_SameShapeH direction_type="H" shape="H1" strength_main="SN490B"/></StbSecSteelColumn_SRC_Same></StbSecSteelFigureColumn_SRC>"#.into()
    };
    let member = if beam {
        r#"<StbGirders><StbGirder id="41" name="G" id_node_start="11" id_node_end="29" id_section="71" kind_structure="RC" isFoundation="false" strength_concrete="Fc36"/></StbGirders>"#
    } else {
        r#"<StbColumns><StbColumn id="41" name="C" id_node_bottom="11" id_node_top="29" id_section="71" kind_structure="RC" strength_concrete="Fc36"/></StbColumns>"#
    }.replace("kind_structure=\"RC\"", if src { "kind_structure=\"SRC\"" } else { "kind_structure=\"RC\"" });
    let coords = if beam {
        "X=\"6000\" Y=\"0\" Z=\"0\""
    } else {
        "X=\"0\" Y=\"0\" Z=\"3000\""
    };
    format!(
        r#"<ST_BRIDGE xmlns="https://www.building-smart.or.jp/dl" version="2.0.2">
<StbCommon project_name="standard reinforcement" app_name="independent fixture" app_version="1" strength_concrete="Fc24">{apply}</StbCommon>
<StbModel><StbNodes><StbNode id="11" X="0" Y="0" Z="0" kind="OTHER"/><StbNode id="29" {coords} kind="OTHER"/></StbNodes>
<StbMembers>{member}</StbMembers><StbSections>
<StbSec{family} id="71" name="section" strength_concrete="Fc30">
<StbSecFigure{family}>{geometry}</StbSecFigure{family}>
<StbSecBarArrangement{family} {position}_{position_key}="60">{bars}{auxiliary}</StbSecBarArrangement{family}>{steel}
</StbSec{family}><StbSecSteel><StbSecRoll-H name="H1" type="H" A="300" B="150" t1="6" t2="9" r="13"/></StbSecSteel>
</StbSections></StbModel></ST_BRIDGE>"#
    )
}

fn cover(model: &Model) -> f64 {
    match model.sections[0].shape.as_ref().unwrap() {
        SectionShape::RcBeamRect { rebar, .. } | SectionShape::SrcBeamRect { rebar, .. } => {
            assert_eq!(rebar.main_dia, 25.);
            assert_eq!(rebar.top, [4, 2]);
            assert_eq!(rebar.bottom, [3]);
            assert_eq!(
                (rebar.stirrup.dia, rebar.stirrup.pitch, rebar.stirrup.legs),
                (13., 100., 2)
            );
            rebar.cover
        }
        SectionShape::RcColumnRect { rebar, .. } | SectionShape::SrcColumnRect { rebar, .. } => {
            assert_eq!(rebar.main_dia, 25.);
            assert_eq!(rebar.x, [3]);
            assert_eq!(rebar.y, [4]);
            assert_eq!(
                (
                    rebar.hoop.dia,
                    rebar.hoop.pitch,
                    rebar.hoop.legs_x,
                    rebar.hoop.legs_y
                ),
                (13., 100., 2, 3)
            );
            rebar.cover
        }
        SectionShape::RcColumnCircle { rebar, .. } => {
            assert_eq!(
                (
                    rebar.main_dia,
                    rebar.count,
                    rebar.hoop.dia,
                    rebar.hoop.pitch
                ),
                (25., 12, 13., 100.)
            );
            rebar.cover
        }
        shape => panic!("unexpected shape: {shape:?}"),
    }
}

#[test]
fn standard_rc_src_rebar_and_grade_priority_reach_both_public_imports() {
    for (family, circle) in [
        ("Beam_RC", false),
        ("Beam_SRC", false),
        ("Column_RC", false),
        ("Column_SRC", false),
        ("Column_RC", true),
    ] {
        for apply in [None, Some(false), Some(true)] {
            for center in [false, true] {
                let xml = standard_input(family, circle, apply, center);
                for model in [
                    import_stbridge(&xml).unwrap(),
                    import_stbridge_with_report(&xml).unwrap().0,
                ] {
                    assert_eq!(model.elements.len(), 1);
                    assert_eq!(model.sections[0].id.0, 0);
                    assert_eq!(
                        model.element_material(&model.elements[0]).unwrap().fc,
                        Some(36.)
                    );
                    assert_eq!(
                        model.element_rebar_material(&model.elements[0]).unwrap().fy,
                        Some(390.)
                    );
                    assert_eq!(
                        model
                            .element_shear_rebar_material(&model.elements[0])
                            .unwrap()
                            .fy,
                        Some(295.)
                    );
                    assert_eq!(
                        cover(&model),
                        if apply.is_none() {
                            0.
                        } else if center {
                            34.5
                        } else {
                            60.
                        }
                    );
                    let raw = model.stb_strengths.sections[0].rebar.as_ref().unwrap();
                    assert_eq!(raw.container.attributes.len(), 1);
                    assert!(raw.positions[0]
                        .attributes
                        .keys()
                        .all(|key| !key.starts_with("depth_cover") && !key.starts_with("center")));
                    if family == "Column_RC" && !circle {
                        assert_eq!(raw.auxiliary.len(), 1);
                        assert_eq!(raw.auxiliary[0].attributes["N_main_X"], "2");
                        assert_eq!(raw.auxiliary[0].attributes["N_main_Y"], "2");
                        assert_eq!(raw.auxiliary[0].attributes["N_main_total"], "4");
                    }
                    if family.ends_with("SRC") {
                        let steel = &model.stb_strengths.sections[0].steel;
                        assert_eq!(steel.len(), 1);
                        assert_eq!(model.resolve_stb_steel(&steel[0]).unwrap().grade, "SN490B");
                    }
                }
            }
        }
    }
}

#[test]
fn standard_container_positions_and_apply_conditions_survive_ovika() {
    let path = std::env::temp_dir().join(format!(
        "sepika-660-standard-positions-{}.ovika",
        std::process::id()
    ));
    for (family, circle) in [
        ("Beam_RC", false),
        ("Beam_SRC", false),
        ("Column_RC", false),
        ("Column_SRC", false),
        ("Column_RC", true),
    ] {
        for apply in [None, Some(false), Some(true)] {
            for center in [false, true] {
                let model =
                    import_stbridge(&standard_input(family, circle, apply, center)).unwrap();
                save_ovika(&path, &model, Default::default()).unwrap();
                let loaded = load_ovika(&path).unwrap().model;
                assert_eq!(loaded.stb_strengths, model.stb_strengths);
                assert_eq!(loaded.sections, model.sections);
            }
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn misplaced_child_cover_and_center_do_not_supply_geometry_or_metadata() {
    for (family, circle) in [
        ("Beam_RC", false),
        ("Beam_SRC", false),
        ("Column_RC", false),
        ("Column_SRC", false),
        ("Column_RC", true),
    ] {
        let xml = standard_input(family, circle, Some(false), false);
        let key = if family.starts_with("Beam") {
            "top"
        } else {
            "start_X"
        };
        let xml = xml.replace(&format!(" depth_cover_{key}=\"60\""), "")
            .replace(" D_main=\"D25\"", &format!(" depth_cover_{key}=\"90\" center_{key}=\"100\" cover=\"80\" kaburi=\"70\" D_main=\"D25\""));
        let model = import_stbridge(&xml).unwrap();
        assert_eq!(cover(&model), 0.);
        let raw = model.stb_strengths.sections[0].rebar.as_ref().unwrap();
        assert!(raw.positions[0]
            .attributes
            .keys()
            .all(|key| !key.starts_with("depth_cover") && !key.starts_with("center")));
    }
}

#[test]
fn internal_material_and_strength_edits_survive_ovika() {
    let mut model =
        import_stbridge(&standard_input("Column_SRC", false, Some(true), true)).unwrap();
    let steel_id = model.sections[0].steel_material.unwrap();
    let material = &mut model.materials[steel_id.index()];
    material.name = "custom steel".into();
    material.young = 123456.;
    material.poisson = 0.27;
    material.density = 6.1e-9;
    material.shear = Some(45678.);
    material.fy = Some(177.);
    model.stb_strengths.sections[0].steel[0].native_material = Some(steel_id);
    model.stb_strengths.members[0].concrete = Some("Fc42".into());
    let expected_material = model.materials[steel_id.index()].clone();
    model.prepare_stb_strength_materials();
    assert_eq!(model.materials[steel_id.index()], expected_material);
    let path = std::env::temp_dir().join(format!(
        "sepika-660-standard-rebar-{}.ovika",
        std::process::id()
    ));
    save_ovika(&path, &model, Default::default()).unwrap();
    let loaded = load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.materials, model.materials);
    assert_eq!(loaded.stb_strengths, model.stb_strengths);
    assert_eq!(loaded.sections, model.sections);
    assert_eq!(
        loaded.element_material(&loaded.elements[0]).unwrap().fc,
        Some(42.)
    );
    let resolved = loaded
        .resolve_stb_steel(&loaded.stb_strengths.sections[0].steel[0])
        .unwrap();
    assert_eq!(resolved.value, 177.);
    assert!(resolved.native_override);
    assert_eq!(cover(&loaded), 34.5);
}
