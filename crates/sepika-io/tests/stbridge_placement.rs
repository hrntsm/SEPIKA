use sepika_core::geom::vec3::dist;
use sepika_core::model::{Model, SupportMemberId};
use sepika_io::stbridge::export_stbridge_with_report;
use sepika_io::stbridge::{export_stbridge, import_stbridge, import_stbridge_with_report};

fn input(common: &str, members: &str, steel: &str) -> String {
    format!(
        r#"<ST_BRIDGE xmlns="https://www.building-smart.or.jp/dl" version="2.0.2">
<StbCommon project_name="placement" app_name="test" app_version="1" {common}/>
<StbModel><StbNodes>
<StbNode id="1" X="0" Y="0" Z="0"/><StbNode id="2" X="0" Y="0" Z="3000"/>
<StbNode id="3" X="6000" Y="0" Z="3000"/><StbNode id="4" X="3000" Y="2000" Z="3000"/>
<StbNode id="5" X="3000" Y="0" Z="3000"/><StbNode id="6" X="3000" Y="0" Z="6000"/>
<StbNode id="7" X="0" Y="0" Z="6000"/><StbNode id="8" X="6000" Y="0" Z="6000"/>
</StbNodes><StbAxes><StbParallelAxes group_name="X" X="10" Y="20" angle="0"><StbParallelAxis id="1" name="X1" distance="0"><StbNodeIdList><StbNodeId id="2"/><StbNodeId id="3"/></StbNodeIdList></StbParallelAxis></StbParallelAxes></StbAxes>
<StbStories><StbStory id="1" name="1F" height="3000" kind="GENERAL"><StbNodeIdList><StbNodeId id="2"/><StbNodeId id="3"/></StbNodeIdList></StbStory></StbStories>
<StbMembers>{members}</StbMembers><StbSections>
<StbSecColumn_S id="1" name="C"><StbSecSteelFigureColumn_S><StbSecSteelColumn_S_Same shape="shape" strength_main="SN400B"/></StbSecSteelFigureColumn_S></StbSecColumn_S>
<StbSecBeam_S id="1" name="G"><StbSecSteelFigureBeam_S><StbSecSteelBeam_S_Straight shape="shape" strength_main="SN400B"/></StbSecSteelFigureBeam_S></StbSecBeam_S>
<StbSecBrace_S id="1" name="BR"><StbSecSteelFigureBrace_S><StbSecSteelBrace_S_Same shape="shape" strength_main="SN400B"/></StbSecSteelFigureBrace_S></StbSecBrace_S>
<StbSecSteel>{steel}</StbSecSteel></StbSections></StbModel></ST_BRIDGE>"#
    )
}

const H: &str = r#"<StbSecRoll-H name="shape" type="H" A="600" B="300" t1="12" t2="20" r="12"/>"#;
const BUILT_H: &str = r#"<StbSecBuild-H name="shape" A="600" B="300" t1="12" t2="20"/>"#;
const GIRDER: &str = r#"<StbGirders><StbGirder id="1" id_node_start="2" id_node_end="3" id_section="1" kind_structure="S"/></StbGirders>"#;

fn ends(model: &Model, index: usize) -> [[f64; 3]; 2] {
    model
        .beam_placement(SupportMemberId::Primary(model.elements[index].id))
        .unwrap()
        .actual_ends
}

fn assert_ends(actual: [[f64; 3]; 2], expected: [[f64; 3]; 2]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            dist(actual, expected) < 1e-8,
            "actual={actual:?}, expected={expected:?}"
        );
    }
}

#[test]
fn public_import_uses_top_width_center_and_public_export_preserves_axis() {
    let model = import_stbridge(&input("", GIRDER, H)).unwrap();
    assert_ends(ends(&model, 0), [[0.0, 0.0, 2700.0], [6000.0, 0.0, 2700.0]]);
    let output = export_stbridge(&model).unwrap();
    let reread = import_stbridge(&output).unwrap();
    assert_ends(ends(&reread, 0), ends(&model, 0));
}

#[test]
fn axial_offset_differences_preserve_both_physical_ends_and_section_axes() {
    for (start, end, offsets, common, expected, section_axes) in [
        (
            2,
            3,
            r#"offset_start_X="250""#,
            "",
            [[250.0, 0.0, 2700.0], [6000.0, 0.0, 2700.0]],
            [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]],
        ),
        (
            2,
            3,
            r#"offset_start_X="250" offset_end_X="-200" offset_start_Z="-150" offset_end_Z="-150""#,
            "",
            [[250.0, 0.0, 2550.0], [5800.0, 0.0, 2550.0]],
            [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]],
        ),
        (
            3,
            2,
            r#"offset_start_X="-200" offset_end_X="250" offset_start_Z="-150" offset_end_Z="-150" rotate="90""#,
            r#"global_rotation="90" global_offset_X="100" global_offset_Y="200" global_offset_Z="100""#,
            [[400.0, 6000.0, 2950.0], [400.0, 450.0, 2950.0]],
            [[0.0, -1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
        ),
    ] {
        let members = format!(
            r#"<StbGirders><StbGirder id="42" id_node_start="{start}" id_node_end="{end}" id_section="1" kind_structure="S" {offsets}/></StbGirders>"#
        );
        let xml = input(common, &members, H);
        let (model, report) = import_stbridge_with_report(&xml).unwrap();
        for attr in report
            .attributes
            .iter()
            .filter(|attr| attr.attribute.starts_with("offset_"))
        {
            assert_eq!(attr.imported, attr.count, "{attr:?}");
        }
        let output = export_stbridge(&model).unwrap();
        assert_eq!(output, export_stbridge_with_report(&model).unwrap().0);
        for model in [model, import_stbridge(&output).unwrap()] {
            let placement = model
                .beam_placement(SupportMemberId::Primary(model.elements[0].id))
                .unwrap();
            assert_ends(placement.actual_ends, expected);
            for (axis, expected) in placement.axes.into_iter().zip(section_axes) {
                assert!(dist(axis, expected) < 1e-12, "{axis:?} != {expected:?}");
            }
        }
    }
}

#[test]
fn axial_offset_differences_that_collapse_reverse_or_lose_precision_are_rejected() {
    for (offset, common, expected, reason) in [
        (
            "6000",
            "",
            "StbGirder id=Some(1)",
            "材軸・断面姿勢の対応保持",
        ),
        (
            "6001",
            "",
            "StbGirder id=Some(1)",
            "材軸・断面姿勢の対応保持",
        ),
        (
            "250",
            r#"global_rotation="23" global_offset_X="10000000000000000""#,
            "global変換後",
            "端点位置",
        ),
    ] {
        let member = GIRDER.replace(
            "kind_structure=\"S\"",
            &format!(r#"kind_structure="S" offset_start_X="{offset}""#),
        );
        let error = import_stbridge(&input(common, &member, H))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(expected) && error.contains(reason),
            "{error}"
        );
    }

    let mut model = import_stbridge(&input("", GIRDER, H)).unwrap();
    let element = &mut model.elements[0];
    element.end_offsets_mm[0][0] = 6001.0;
    let error = export_stbridge(&model).unwrap_err().to_string();
    assert!(
        error.contains("部材 ID 0") && error.contains("材軸・断面姿勢の対応保持"),
        "{error}"
    );
}

#[test]
fn axial_offsets_on_large_node_coordinates_roundtrip_without_integer_saturation() {
    let members = GIRDER.replace(
        "kind_structure=\"S\"",
        r#"kind_structure="S" offset_start_X="8192" offset_end_X="16384""#,
    );
    let xml = input("", &members, H)
        .replace(
            r#"<StbNode id="2" X="0""#,
            r#"<StbNode id="2" X="9223372036854775808""#,
        )
        .replace(
            r#"<StbNode id="3" X="6000""#,
            r#"<StbNode id="3" X="9223372036855824384""#,
        );
    let model = import_stbridge(&xml).unwrap();
    let expected = [
        [9_223_372_036_854_784_000.0, 0.0, 2700.0],
        [9_223_372_036_855_840_768.0, 0.0, 2700.0],
    ];
    assert_eq!(ends(&model, 0), expected);
    let reread = import_stbridge(&export_stbridge(&model).unwrap()).unwrap();
    assert_eq!(ends(&reread, 0), expected);
}

#[test]
fn axial_offsets_reject_rounding_both_absolute_ends_by_the_same_amount() {
    let members = GIRDER.replace(
        "kind_structure=\"S\"",
        r#"kind_structure="S" offset_start_X="0.25" offset_end_X="2.25""#,
    );
    let local = input("", &members, H)
        .replace(
            r#"<StbNode id="2" X="0""#,
            r#"<StbNode id="2" X="9007199254740992""#,
        )
        .replace(
            r#"<StbNode id="3" X="6000""#,
            r#"<StbNode id="3" X="9007199254746992""#,
        );
    let mut native =
        import_stbridge(&local.replace(r#" offset_start_X="0.25" offset_end_X="2.25""#, ""))
            .unwrap();
    native.elements[0].end_offsets_mm[0][0] = 0.25;
    native.elements[0].end_offsets_mm[1][0] = 2.25;
    let members = GIRDER.replace(
        "kind_structure=\"S\"",
        r#"kind_structure="S" offset_start_X="1" offset_end_X="5""#,
    );
    let global = input(r#"global_offset_X="9007199254740992""#, &members, H);
    let members = GIRDER.replace(
        "kind_structure=\"S\"",
        r#"kind_structure="S" offset_start_X="2" offset_end_X="6" offset_start_Z="1" offset_end_Z="1""#,
    );
    let global_transverse = input(r#"global_offset_Z="9007199254740992""#, &members, H);
    let global_baseline = input(r#"global_offset_X="9007199254740992""#, &members, H)
        .replace(r#"<StbNode id="2" X="0""#, r#"<StbNode id="2" X="1""#)
        .replace(r#"<StbNode id="3" X="6000""#, r#"<StbNode id="3" X="6001""#);
    for (xml, target) in [
        (local, "StbGirder id=Some(1)"),
        (global, "global変換後"),
        (global_transverse, "global変換後"),
        (global_baseline, "global変換後"),
    ] {
        for error in [
            import_stbridge(&xml).unwrap_err(),
            import_stbridge_with_report(&xml).unwrap_err(),
        ] {
            let error = error.to_string();
            for expected in [target, "異値offset", "端点位置", "丸め"] {
                assert!(error.contains(expected), "{error}");
            }
        }
    }
    for error in [
        export_stbridge(&native).unwrap_err(),
        export_stbridge_with_report(&native).unwrap_err(),
    ] {
        let error = error.to_string();
        for expected in ["部材 ID 0", "異値offset", "端点位置", "丸め"] {
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn sloped_member_with_a_quarter_turn_preserves_its_axial_offset_and_section_axes() {
    let members = GIRDER
        .replace(r#"id_node_start="2""#, r#"id_node_start="1""#)
        .replace(
            r#"kind_structure="S""#,
            r#"kind_structure="S" rotate="90" offset_start_X="200" offset_start_Z="100""#,
        );
    let model = import_stbridge(&input("", &members, H)).unwrap();
    let reread = import_stbridge(&export_stbridge(&model).unwrap()).unwrap();
    let expected = [[200.0, 300.0, 100.0], [6000.0, 300.0, 3000.0]];
    let norm = 5.0_f64.sqrt();
    let expected_axes = [
        [2.0 / norm, 0.0, 1.0 / norm],
        [0.0, -1.0, 0.0],
        [1.0 / norm, 0.0, -2.0 / norm],
    ];
    for model in [model, reread] {
        let placement = model
            .beam_placement(SupportMemberId::Primary(model.elements[0].id))
            .unwrap();
        assert_ends(placement.actual_ends, expected);
        for (axis, expected) in placement.axes.into_iter().zip(expected_axes) {
            assert!(dist(axis, expected) < 1e-12, "{axis:?} != {expected:?}");
        }
    }
}

#[test]
fn axial_offsets_reject_a_common_rounding_shift_from_centroid_conversion() {
    let deep_h = H.replace("A=\"600\"", "A=\"601\"");
    let directly_offset = GIRDER.replace(
        "kind_structure=\"S\"",
        r#"kind_structure="S" offset_start_X="2""#,
    );
    let large_reference = |xml: String| {
        xml.replace(
            r#"<StbNode id="2" X="0" Y="0" Z="3000""#,
            r#"<StbNode id="2" X="0" Y="0" Z="9007199254740992""#,
        )
        .replace(
            r#"<StbNode id="3" X="6000" Y="0" Z="3000""#,
            r#"<StbNode id="3" X="6000" Y="0" Z="9007199254740992""#,
        )
    };
    let large_offset = |members: &str| {
        input("", members, &deep_h)
            .replace(
                r#"<StbNode id="2" X="0" Y="0" Z="3000""#,
                r#"<StbNode id="2" X="0" Y="0" Z="0""#,
            )
            .replace(
                r#"<StbNode id="3" X="6000" Y="0" Z="3000""#,
                r#"<StbNode id="3" X="6000" Y="0" Z="0""#,
            )
    };
    let offset_before_centroid = directly_offset.replace(
        "kind_structure=\"S\"",
        r#"kind_structure="S" offset_start_Z="9007199254740992" offset_end_Z="9007199254740992""#,
    );
    for xml in [
        large_reference(input("", &directly_offset, &deep_h)),
        large_offset(&offset_before_centroid),
    ] {
        for error in [
            import_stbridge(&xml).unwrap_err(),
            import_stbridge_with_report(&xml).unwrap_err(),
        ] {
            let error = error.to_string();
            for expected in ["StbGirder id=Some(1)", "異値offset", "端点位置", "丸め"] {
                assert!(error.contains(expected), "{error}");
            }
        }
    }
    let equal = import_stbridge(&large_reference(input("", GIRDER, &deep_h))).unwrap();
    export_stbridge(&equal).unwrap();
    let mut centered_end = equal;
    centered_end.elements[0].end_offsets_mm[0][0] = 2.0;
    let mut inverse_shift = import_stbridge(&large_offset(GIRDER)).unwrap();
    inverse_shift.elements[0].end_offsets_mm =
        [[2.0, 0.0, 2_f64.powi(53)], [0.0, 0.0, 2_f64.powi(53)]];
    for model in [centered_end, inverse_shift] {
        for error in [
            export_stbridge(&model).unwrap_err(),
            export_stbridge_with_report(&model).unwrap_err(),
        ] {
            let error = error.to_string();
            for expected in ["部材 ID 0", "異値offset", "端点位置", "丸め"] {
                assert!(error.contains(expected), "{error}");
            }
        }
    }
}

#[test]
fn axial_offsets_on_a_shapeless_column_are_rejected_at_both_exports() {
    let members = r#"<StbColumns><StbColumn id="1" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="S" offset_top_Z="-20"/></StbColumns>"#;
    let mut model = import_stbridge(&input("", members, H)).unwrap();
    let section = model.elements[0].section.unwrap();
    model.sections[section.index()].shape = None;
    for error in [
        export_stbridge(&model).unwrap_err(),
        export_stbridge_with_report(&model).unwrap_err(),
    ] {
        let error = error.to_string();
        assert!(
            error.contains("部材 ID 0") && error.contains("断面形状"),
            "{error}"
        );
    }
}

#[test]
fn axial_offsets_reject_lost_coordinates_in_the_common_rotation() {
    for (degrees, x, y, before_rotation) in [
        (
            "45",
            "120000000000000032768",
            "120000000000000016384",
            [120_000_000_000_000_032_768.0, 120_000_000_000_000_016_384.0],
        ),
        (
            "23",
            "9007199254740992",
            "0.0009765625",
            [9_007_199_254_740_992.0, 0.000_976_562_5],
        ),
    ] {
        let members = format!(
            r#"<StbColumns><StbColumn id="3000000001" id_node_bottom="1" id_node_top="2"
                id_section="1" kind_structure="S" offset_bottom_X="{x}" offset_top_X="{x}"
                offset_bottom_Y="{y}" offset_top_Y="{y}" offset_bottom_Z="25"/></StbColumns>"#
        );
        let original = import_stbridge(&input("", &members, H)).unwrap();
        let [x_mm, y_mm] = before_rotation;
        assert_eq!(
            ends(&original, 0),
            [[x_mm, y_mm, 25.0], [x_mm, y_mm, 3000.0]]
        );
        let xml = input(&format!(r#"global_rotation="{degrees}""#), &members, H);
        for error in [
            import_stbridge(&xml).unwrap_err(),
            import_stbridge_with_report(&xml).unwrap_err(),
        ] {
            let error = error.to_string();
            for expected in [
                "StbColumn id=Some(3000000001)",
                "global変換後",
                "異値offset",
                "共通回転の積・加減算",
            ] {
                assert!(error.contains(expected), "{degrees}: {error}");
            }
        }
    }

    let members = r#"<StbBeams><StbBeam id="7" id_node_start="5" id_node_end="4" id_section="1" kind_structure="S"/></StbBeams>
<StbPosts><StbPost id="4000000001" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="S"
    offset_bottom_X="120000000000000032768" offset_top_X="120000000000000032768"
    offset_bottom_Y="120000000000000016384" offset_top_Y="120000000000000016384" offset_bottom_Z="25"/></StbPosts>"#;
    let xml = input(r#"global_rotation="45""#, members, H);
    for error in [
        import_stbridge(&xml).unwrap_err(),
        import_stbridge_with_report(&xml).unwrap_err(),
    ] {
        let error = error.to_string();
        for expected in [
            "StbPost id=Some(4000000001)",
            "global変換後",
            "共通回転の積・加減算",
        ] {
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn axial_offsets_roundtrip_with_an_oblique_global_rotation_when_ends_are_preserved() {
    let members = GIRDER.replace(
        "kind_structure=\"S\"",
        r#"kind_structure="S" offset_start_X="2048""#,
    );
    let xml = input(r#"global_rotation="23""#, &members, H)
        .replace(r#"<StbNode id="3" X="6000""#, r#"<StbNode id="3" X="4096""#);
    let expected = [
        [1885.193939870598, 800.2173511460327, 2700.0],
        [3770.387879741196, 1600.4347022920654, 2700.0],
    ];
    for model in [
        import_stbridge(&xml).unwrap(),
        import_stbridge_with_report(&xml).unwrap().0,
    ] {
        assert_ends(ends(&model, 0), expected);
        assert_ends(
            ends(
                &import_stbridge(&export_stbridge(&model).unwrap()).unwrap(),
                0,
            ),
            expected,
        );
    }
}

#[test]
fn secondary_axial_offsets_preserve_the_support_and_reject_snapping_a_reference_end() {
    let members = format!(
        r#"{GIRDER}<StbBeams><StbBeam id="4000000001" id_node_start="5" id_node_end="4" id_section="1" kind_structure="S" offset_start_Y="20"/></StbBeams>"#
    );
    let xml = input("", &members, H);
    let model = import_stbridge(&xml).unwrap();
    let output = export_stbridge(&model).unwrap();
    let path = std::env::temp_dir().join(format!(
        "sepika-1367-axial-support-{}.ovika",
        std::process::id()
    ));
    sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
    let saved = sepika_io::ovika::load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    let original = model.beams().next().unwrap();
    let stored = saved.beams().next().unwrap();
    assert_eq!(stored.ends, original.ends);
    assert_eq!(stored.display_local_axis, original.display_local_axis);
    assert_eq!(
        stored.end_offsets_mm,
        [[0.0, 20.0, -300.0], [0.0, 0.0, -300.0]]
    );
    for model in [model, saved, import_stbridge(&output).unwrap()] {
        let member = model.beams().next().unwrap();
        let placement = model
            .beam_placement(SupportMemberId::Secondary(member.id))
            .unwrap();
        assert_eq!(
            placement.reference_ends,
            [[3000.0, 0.0, 3000.0], [3000.0, 2000.0, 3000.0]]
        );
        assert_eq!(
            placement.actual_ends,
            [[3000.0, 20.0, 2700.0], [3000.0, 2000.0, 2700.0]]
        );
        assert_eq!(
            placement.axes,
            [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]]
        );
        let sepika_core::model::SecondaryMemberEnds::Cantilever { support, .. } = member.ends
        else {
            panic!("支持端と自由端の組を保持する");
        };
        assert_eq!(
            support.support,
            SupportMemberId::Primary(model.elements[0].id)
        );
        assert_eq!(support.position, 0.5);
    }
    let xml = xml
        .replace("<StbBeams>", r#"<StbPosts><StbPost id="7001" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="S"/></StbPosts><StbBeams>"#)
        .replace(
            r#"<StbNode id="5" X="3000" Y="0""#,
            r#"<StbNode id="5" X="3000" Y="0.25""#,
        )
        .replace(
            r#"<StbNode id="4" X="3000" Y="2000""#,
            r#"<StbNode id="4" X="3000" Y="2000.25""#,
        );
    for error in [
        import_stbridge(&xml).unwrap_err(),
        import_stbridge_with_report(&xml).unwrap_err(),
    ] {
        let error = error.to_string();
        for expected in ["StbBeam id=Some(4000000001)", "異値offset", "支持アンカー"] {
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn secondary_axial_offsets_reject_ambiguous_reference_nodes_and_changed_parent_support() {
    use sepika_core::model::SecondaryMemberEnds;

    let members = r#"<StbGirders><StbGirder id="1" id_node_start="2" id_node_end="3" id_section="1" kind_structure="S"/><StbGirder id="2" id_node_start="10" id_node_end="11" id_section="1" kind_structure="S"/></StbGirders>
<StbBeams><StbBeam id="1" id_node_start="5" id_node_end="4" id_section="1" kind_structure="S" offset_start_Y="20"/></StbBeams>"#;
    let xml = input("", members, H).replace(
        "</StbNodes>", r#"<StbNode id="10" X="2500" Y="-500" Z="3000"/><StbNode id="11" X="3500" Y="500" Z="3000"/></StbNodes>"#,
    );
    let mut model = import_stbridge(&xml).unwrap();
    let member_id = model.beams().next().unwrap().id;
    let primary = model.elements[0].id;
    let alternate = model.elements[1].id;
    let original = model
        .beam_placement(SupportMemberId::Secondary(member_id))
        .unwrap();
    let SecondaryMemberEnds::Cantilever {
        support,
        free_end_vector,
    } = model.secondary_member(member_id).unwrap().ends
    else {
        panic!("支持端を一つ持つ");
    };
    assert_eq!(support.support, SupportMemberId::Primary(primary));
    let member = model
        .unassigned_beams
        .iter_mut()
        .find(|member| member.id == member_id)
        .unwrap();
    member.ends = SecondaryMemberEnds::Cantilever {
        support: sepika_core::model::SecondaryMemberAnchor {
            support: SupportMemberId::Primary(alternate),
            position: 0.5,
        },
        free_end_vector,
    };
    assert_eq!(
        model
            .beam_placement(SupportMemberId::Secondary(member_id))
            .unwrap()
            .actual_ends,
        original.actual_ends
    );
    for error in [
        export_stbridge(&model).unwrap_err(),
        export_stbridge_with_report(&model).unwrap_err(),
    ] {
        let error = error.to_string();
        for expected in ["二次部材 ID 0", "異値offset", "支持先"] {
            assert!(error.contains(expected), "{error}");
        }
    }

    let duplicate = xml
        .replace(
            "</StbNodes>",
            r#"<StbNode id="9" X="3000" Y="0" Z="3000"/></StbNodes>"#,
        )
        .replace(r#"id_node_start="5""#, r#"id_node_start="9""#);
    let model = import_stbridge(&duplicate).unwrap();
    for error in [
        export_stbridge(&model).unwrap_err(),
        export_stbridge_with_report(&model).unwrap_err(),
    ] {
        let error = error.to_string();
        for expected in ["二次部材 ID 0", "異値offset", "節点", "一意"] {
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn secondary_axial_offsets_report_the_import_error_when_supports_cannot_be_checked() {
    let members = format!(
        r#"<StbColumns><StbColumn id="1" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="S"/></StbColumns>
{GIRDER}<StbBeams><StbBeam id="1" id_node_start="5" id_node_end="4" id_section="1" kind_structure="S" offset_start_Y="20"/></StbBeams>"#,
    );
    let mut model = import_stbridge(&input("", &members, H)).unwrap();
    let column = model.elements[0].section.unwrap();
    model.sections[column.index()].shape = None;
    for error in [
        export_stbridge(&model).unwrap_err(),
        export_stbridge_with_report(&model).unwrap_err(),
    ] {
        let error = error.to_string();
        for expected in [
            "二次部材 ID 0",
            "異値offsetの支持関係",
            "再読込",
            "StbColumn id=Some(1)",
        ] {
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn transverse_end_offset_differences_report_the_target_at_public_entrances() {
    for (kind, container, node_attrs, offset_end, export_target, axial_component) in [
        (
            "Column",
            "Columns",
            r#"id_node_bottom="1" id_node_top="2""#,
            "bottom",
            "部材 ID 0",
            Some(2),
        ),
        (
            "Girder",
            "Girders",
            r#"id_node_start="2" id_node_end="3""#,
            "start",
            "部材 ID 0",
            Some(0),
        ),
        (
            "Brace",
            "Braces",
            r#"id_node_start="1" id_node_end="3""#,
            "start",
            "ブレース ID 0",
            None,
        ),
        (
            "Beam",
            "Beams",
            r#"id_node_start="2" id_node_end="3""#,
            "start",
            "二次部材 ID 0",
            Some(0),
        ),
        (
            "Post",
            "Posts",
            r#"id_node_bottom="1" id_node_top="2""#,
            "bottom",
            "二次部材 ID 0",
            Some(2),
        ),
    ] {
        let members = format!(
            r#"<Stb{container}><Stb{kind} id="42" {node_attrs} id_section="1" kind_structure="S"/></Stb{container}>"#
        );
        for (component, axis, value) in [(0, "X", 10.0), (1, "Y", -20.0), (2, "Z", 30.0)]
            .into_iter()
            .filter(|(component, _, _)| Some(*component) != axial_component)
        {
            let unequal_members = members.replace(
                "kind_structure=\"S\"",
                &format!("kind_structure=\"S\" offset_{offset_end}_{axis}=\"{value}\""),
            );
            let xml = input("", &unequal_members, H);
            let import_target = format!("Stb{kind} id=Some(42)");
            for error in [
                import_stbridge(&xml).unwrap_err(),
                import_stbridge_with_report(&xml).unwrap_err(),
            ] {
                let reason = error.to_string();
                for expected in [
                    import_target.as_str(),
                    "異値offset",
                    "軸方向差のみ",
                    "材軸・断面姿勢の対応保持",
                ] {
                    assert!(reason.contains(expected), "{kind}/{axis}: {reason}");
                }
                assert!(
                    !reason.contains("仕様確認中") && !reason.contains("ユーザー"),
                    "{reason}"
                );
            }
            let mut model = import_stbridge(&input("", &members, H)).unwrap();
            if let Some(element) = model.elements.first_mut() {
                element.end_offsets_mm[0][component] += value;
            } else {
                model
                    .unassigned_beams
                    .iter_mut()
                    .chain(&mut model.unassigned_posts)
                    .next()
                    .unwrap()
                    .end_offsets_mm[0][component] += value;
            }
            for error in [
                export_stbridge(&model).unwrap_err(),
                export_stbridge_with_report(&model).unwrap_err(),
            ] {
                let reason = error.to_string();
                for expected in [
                    export_target,
                    "異値offset",
                    "軸方向差のみ",
                    "材軸・断面姿勢の対応保持",
                ] {
                    assert!(reason.contains(expected), "{kind}/{axis}: {reason}");
                }
                assert!(
                    !reason.contains("仕様確認中") && !reason.contains("ユーザー"),
                    "{reason}"
                );
            }
        }
    }
}

#[test]
fn asymmetric_built_h_rejects_both_exports_and_preserves_native_ovika_geometry_and_pose() {
    for (index, (lower_width, lower_thick)) in [(150.0, 10.0), (150.0, 20.0), (300.0, 10.0)]
        .into_iter()
        .enumerate()
    {
        let mut model = import_stbridge(&input("", GIRDER, H)).unwrap();
        let section_id = model.elements[0].section.unwrap();
        let old = model.section(section_id).unwrap();
        let mut section = sepika_core::section_shape::SectionShape::SteelBuiltH {
            height: 600.0,
            upper_width: 300.0,
            upper_thick: 20.0,
            lower_width,
            lower_thick,
            web_thick: 12.0,
        }
        .to_section(section_id, "非対称BH".into());
        section.material = old.material;
        section.frame_use = old.frame_use;
        model.sections[section_id.index()] = section;
        model.elements[0].local_axis.ref_vector = [
            0.0,
            -15.0_f64.to_radians().sin(),
            15.0_f64.to_radians().cos(),
        ];
        let original = model
            .beam_placement(SupportMemberId::Primary(model.elements[0].id))
            .unwrap();
        for error in [
            export_stbridge(&model).unwrap_err(),
            export_stbridge_with_report(&model).unwrap_err(),
        ] {
            let reason = error.to_string();
            for expected in [
                "非対称BH",
                "断面 ID",
                "非対称組立H",
                "A/B/t1/t2",
                "上下差",
                "OVIKA",
            ] {
                assert!(reason.contains(expected), "{reason}");
            }
        }
        let path = std::env::temp_dir().join(format!(
            "sepika-1367-asymmetric-bh-{}-{index}.ovika",
            std::process::id()
        ));
        sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
        let saved = sepika_io::ovika::load_ovika(&path).unwrap().model;
        std::fs::remove_file(path).unwrap();
        assert_eq!(saved.sections, model.sections);
        let placement = saved
            .beam_placement(SupportMemberId::Primary(saved.elements[0].id))
            .unwrap();
        assert_eq!(placement.actual_ends, original.actual_ends);
        assert_eq!(placement.axes, original.axes);
    }
}

#[test]
fn shapes_without_connected_official_tags_reject_both_exports_and_preserve_ovika() {
    use sepika_core::section_shape::SectionShape;
    for (index, (shape, tag)) in [
        (
            SectionShape::SteelFlatBar {
                width: 100.0,
                thick: 12.0,
            },
            "StbSecFlatBar",
        ),
        (SectionShape::SteelRoundBar { dia: 32.0 }, "StbSecRoundBar"),
        (
            SectionShape::SteelLipChannel {
                height: 150.0,
                width: 75.0,
                lip: 20.0,
                thick: 2.3,
            },
            "StbSecLipC",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut model = import_stbridge(&input("", GIRDER, H)).unwrap();
        let section_id = model.elements[0].section.unwrap();
        let old = model.section(section_id).unwrap();
        let mut section = shape.to_section(section_id, "公式出力未接続".into());
        section.material = old.material;
        section.frame_use = old.frame_use;
        model.sections[section_id.index()] = section;
        for error in [
            export_stbridge(&model).unwrap_err(),
            export_stbridge_with_report(&model).unwrap_err(),
        ] {
            let reason = error.to_string();
            for expected in ["断面 ID", "公式出力未接続", tag, "未対応", "OVIKA"] {
                assert!(reason.contains(expected), "{reason}");
            }
        }
        let path = std::env::temp_dir().join(format!(
            "sepika-1367-official-tags-{}-{index}.ovika",
            std::process::id()
        ));
        sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
        let saved = sepika_io::ovika::load_ovika(&path).unwrap().model;
        std::fs::remove_file(path).unwrap();
        assert_eq!(saved.sections, model.sections);
        assert_eq!(saved.elements, model.elements);
    }
}

#[test]
fn symmetric_built_h_preserves_rotated_top_reference_and_uses_only_official_attributes() {
    let members = GIRDER.replace("kind_structure=\"S\"", "kind_structure=\"S\" rotate=\"15\"");
    let model = import_stbridge(&input("", &members, BUILT_H)).unwrap();
    let y = 75.0 * (6.0_f64.sqrt() - 2.0_f64.sqrt());
    let z = 3000.0 - 75.0 * (6.0_f64.sqrt() + 2.0_f64.sqrt());
    assert_ends(ends(&model, 0), [[0.0, y, z], [6000.0, y, z]]);
    let xml = export_stbridge(&model).unwrap();
    assert_eq!(xml, export_stbridge_with_report(&model).unwrap().0);
    for line in xml.lines().filter(|line| line.contains("<StbSecBuild-H ")) {
        for forbidden in [" type=", " B2=", " t2_lower="] {
            assert!(!line.contains(forbidden), "{line}");
        }
    }
    let reread = import_stbridge(&xml).unwrap();
    assert_ends(ends(&reread, 0), [[0.0, y, z], [6000.0, y, z]]);
    assert_eq!(
        model.element_section(&model.elements[0]).unwrap().shape,
        reread.element_section(&reread.elements[0]).unwrap().shape
    );
    for (a, b) in model
        .beam_placement(SupportMemberId::Primary(model.elements[0].id))
        .unwrap()
        .axes
        .into_iter()
        .zip(
            reread
                .beam_placement(SupportMemberId::Primary(reread.elements[0].id))
                .unwrap()
                .axes,
        )
    {
        assert!(dist(a, b) < 1e-12);
    }
}

#[test]
fn global_rotation_translates_points_once_and_only_rotates_offsets_and_axes() {
    let member = GIRDER.replace("kind_structure=\"S\"", r#"kind_structure="S" offset_start_X="30" offset_start_Y="40" offset_start_Z="50" offset_end_X="30" offset_end_Y="40" offset_end_Z="50""#);
    let (model, report) = import_stbridge_with_report(&input(
        r#"global_rotation="90" global_offset_X="100" global_offset_Y="200" global_offset_Z="100""#,
        &member,
        H,
    ))
    .unwrap();
    assert_ends(
        ends(&model, 0),
        [[60.0, 230.0, 2850.0], [60.0, 6230.0, 2850.0]],
    );
    assert!(dist(model.nodes[1].coord, [100.0, 200.0, 3100.0]) < 1e-8);
    assert_eq!(model.stories[0].elevation, Some(3100.0));
    assert_eq!(model.source_stories[0].height, 3100.0);
    match model.axes[0].kind {
        sepika_core::model::AxisGroupKind::Parallel { origin, angle_deg } => {
            assert!(origin
                .into_iter()
                .zip([80.0, 210.0])
                .all(|(a, b)| (a - b).abs() < 1e-8));
            assert_eq!(angle_deg, 90.0);
        }
        _ => panic!("平行芯を保持する"),
    }
    for attr in report.attributes.iter().filter(|attr| {
        attr.attribute.starts_with("offset_") || attr.attribute.starts_with("global_")
    }) {
        assert_eq!(attr.imported, attr.count, "{attr:?}");
    }
    let reread = import_stbridge(&export_stbridge(&model).unwrap()).unwrap();
    assert_ends(ends(&reread, 0), ends(&model, 0));
}

#[test]
fn global_rotation_changes_force_and_moment_components_without_translating_them() {
    let xml = input(r#"global_rotation="90" global_offset_X="100" global_offset_Y="200" global_offset_Z="100""#, GIRDER, H)
        .replace("</StbModel>", r#"<StbLoadCases><StbLoadCase name="L"><StbNodalLoad id_node="2" fx="10" fy="20" fz="30" mx="40" my="50" mz="60"/></StbLoadCase></StbLoadCases></StbModel>"#);
    let model = import_stbridge(&xml).unwrap();
    let load = &model.load_cases[0].nodal[0];
    assert_eq!(load.node.0, 1);
    for (actual, expected) in load
        .values
        .iter()
        .zip([-20.0, 10.0, 30.0, -50.0, 40.0, 60.0])
    {
        assert!((actual - expected).abs() < 1e-8);
    }
}

#[test]
fn finite_global_translation_that_collapses_a_member_axis_is_rejected_at_both_public_imports() {
    for (members, target) in [
        (GIRDER, "部材 ID 0"),
        (
            r#"<StbBeams><StbBeam id="1" id_node_start="2" id_node_end="3" id_section="1" kind_structure="S"/></StbBeams>"#,
            "二次部材 ID 0",
        ),
        (
            r#"<StbBraces><StbBrace id="1" id_node_start="2" id_node_end="3" id_section="1" kind_structure="S"/></StbBraces>"#,
            "部材 ID 0",
        ),
    ] {
        let xml = input(r#"global_offset_X="1e100""#, members, H);
        for error in [
            import_stbridge(&xml).unwrap_err(),
            import_stbridge_with_report(&xml).unwrap_err(),
        ] {
            let reason = error.to_string();
            for expected in [target, "global変換後", "基準材軸", "零長"] {
                assert!(reason.contains(expected), "{reason}");
            }
        }
    }
    for (members, target) in [
        (
            r#"<StbColumns><StbColumn id="1" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="S"/></StbColumns>"#,
            "部材 ID 0",
        ),
        (
            r#"<StbPosts><StbPost id="1" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="S"/></StbPosts>"#,
            "二次部材 ID 0",
        ),
    ] {
        let xml = input(r#"global_offset_Z="1e100""#, members, H);
        for error in [
            import_stbridge(&xml).unwrap_err(),
            import_stbridge_with_report(&xml).unwrap_err(),
        ] {
            let reason = error.to_string();
            for expected in [target, "global変換後", "基準材軸", "零長"] {
                assert!(reason.contains(expected), "{reason}");
            }
        }
    }
}

#[test]
fn global_rotation_that_collapses_only_the_actual_axis_is_rejected() {
    let member = GIRDER.replace(
        "kind_structure=\"S\"",
        r#"kind_structure="S" offset_start_Y="1e100" offset_end_Y="1e100""#,
    );
    let xml = input(r#"global_rotation="45""#, &member, H);
    for error in [
        import_stbridge(&xml).unwrap_err(),
        import_stbridge_with_report(&xml).unwrap_err(),
    ] {
        let reason = error.to_string();
        for expected in ["部材 ID 0", "global変換後", "実材軸", "零長"] {
            assert!(reason.contains(expected), "{reason}");
        }
    }
}

#[test]
fn asymmetric_rolled_sections_reject_unrepresentable_radius_instead_of_silently_losing_it() {
    for (tag, kind, radius) in [
        ("StbSecRoll-L", "SINGLE", "r1=\"5\" r2=\"2\""),
        ("StbSecRoll-C", "SINGLE", "r1=\"5\" r2=\"2\""),
        ("StbSecRoll-T", "T", "r=\"5\""),
    ] {
        let shape = format!(
            r#"<{tag} name="shape" type="{kind}" A="100" B="80" t1="10" t2="10" {radius}/>"#
        );
        let error = import_stbridge(&input("", GIRDER, &shape))
            .unwrap_err()
            .to_string();
        for expected in ["入力断面", "shape", tag, "半径", "未対応"] {
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn intermediate_information_reports_the_actual_parent_and_does_not_borrow_a_previous_member() {
    let nested = GIRDER.replace(
        "kind_structure=\"S\"/>",
        "kind_structure=\"S\"><StbGirderViaNode/></StbGirder>",
    );
    let error = import_stbridge(&input("", &nested, H))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("StbGirder id=Some(1)") && error.contains("StbGirderViaNode"),
        "{error}"
    );
    let outside = format!(
        "{GIRDER}<StbMemberOffsetList id_node=\"5\" offset_X=\"0\" offset_Y=\"0\" offset_Z=\"0\"/>"
    );
    let error = import_stbridge(&input("", &outside, H))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("StbMemberOffsetList") && !error.contains("StbGirder id="),
        "{error}"
    );
}

#[test]
fn unverified_schema_versions_are_rejected_before_placement_conversion() {
    for version in ["2.0.0", "2.0.1", "2.1.0", "3.0.0"] {
        let xml =
            input("", GIRDER, H).replace("version=\"2.0.2\"", &format!("version=\"{version}\""));
        assert!(import_stbridge(&xml)
            .unwrap_err()
            .to_string()
            .contains(version));
    }
}

#[test]
#[ignore = "公式XSD照合用XMLを指定したディレクトリへ生成する"]
fn write_placement_outputs_for_official_xsd_validation() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("SEPIKA_STB_XSD_OUTPUT").expect("SEPIKA_STB_XSD_OUTPUT"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let members = r#"<StbColumns><StbColumn id="1" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="S" rotate="27" offset_bottom_Z="25"/></StbColumns>
<StbGirders><StbGirder id="1" id_node_start="2" id_node_end="3" id_section="1" kind_structure="S" rotate="90" offset_start_X="250" offset_end_X="-200" offset_start_Z="-150" offset_end_Z="-150"/><StbGirder id="2" id_node_start="7" id_node_end="8" id_section="1" kind_structure="S" rotate="15"/></StbGirders>
<StbBeams><StbBeam id="1" id_node_start="5" id_node_end="4" id_section="1" kind_structure="S" rotate="90" offset_start_Y="20"/></StbBeams>
<StbPosts><StbPost id="1" id_node_bottom="5" id_node_top="6" id_section="1" kind_structure="S" rotate="45" offset_top_Z="-10"/></StbPosts>
<StbBraces><StbBrace id="1" id_node_start="1" id_node_end="3" id_section="1" kind_structure="S" rotate="55" feature_brace="TENSION" offset_start_X="200" offset_start_Z="100"/></StbBraces>"#;
    for (name, shape) in [
        ("all-five-h", H),
        ("all-five-built-h", BUILT_H),
        (
            "all-five-box",
            r#"<StbSecRoll-BOX name="shape" type="ELSE" A="600" B="300" t="12" r="36"/>"#,
        ),
        (
            "all-five-pipe",
            r#"<StbSecPipe name="shape" D="300" t="12"/>"#,
        ),
        (
            "all-five-built-box",
            r#"<StbSecBuild-BOX name="shape" A="600" B="300" t1="12" t2="20"/>"#,
        ),
    ] {
        let model = import_stbridge(&input("", members, shape)).unwrap();
        let xml = export_stbridge(&model).unwrap();
        let reread = import_stbridge(&xml).unwrap();
        assert_eq!(model.elements.len(), reread.elements.len());
        for index in 0..model.elements.len() {
            assert_ends(ends(&reread, index), ends(&model, index));
            let a = model
                .beam_placement(SupportMemberId::Primary(model.elements[index].id))
                .unwrap();
            let b = reread
                .beam_placement(SupportMemberId::Primary(reread.elements[index].id))
                .unwrap();
            assert_eq!(a.section_shape, b.section_shape);
            for (a, b) in a.axes.into_iter().zip(b.axes) {
                assert!(dist(a, b) < 1e-12);
            }
        }
        let original_secondary: Vec<_> = model
            .beams()
            .chain(model.posts())
            .map(|member| {
                model
                    .beam_placement(SupportMemberId::Secondary(member.id))
                    .unwrap()
            })
            .collect();
        let reread_secondary: Vec<_> = reread
            .beams()
            .chain(reread.posts())
            .map(|member| {
                reread
                    .beam_placement(SupportMemberId::Secondary(member.id))
                    .unwrap()
            })
            .collect();
        for (a, b) in original_secondary.into_iter().zip(reread_secondary) {
            assert_ends(a.actual_ends, b.actual_ends);
            assert_eq!(a.section_shape, b.section_shape);
            for (a, b) in a.axes.into_iter().zip(b.axes) {
                assert!(dist(a, b) < 1e-12);
            }
        }
        std::fs::write(directory.join(format!("{name}.stb")), xml).unwrap();
    }
    let cft = include_str!("fixtures/cft_standard_steel.stb");
    let built_box = r#"<StbSecBuild-BOX name="BOX1" A="300" B="200" t1="9" t2="9"/>"#;
    for (name, source) in [
        (
            "rc-rect",
            include_str!("fixtures/rebar507/normal_same.stb").to_owned(),
        ),
        (
            "rc-circle",
            include_str!("fixtures/rebar507/supplemental/circle_same.stb").to_owned(),
        ),
        ("cft-built-box", cft.to_owned()),
        (
            "cft-box",
            cft.replace(
                built_box,
                r#"<StbSecRoll-BOX name="BOX1" type="ELSE" A="300" B="200" t="9" r="27"/>"#,
            ),
        ),
        (
            "cft-pipe",
            cft.replace(built_box, r#"<StbSecPipe name="BOX1" D="300" t="9"/>"#),
        ),
    ] {
        let model = import_stbridge(&source).unwrap();
        let xml = export_stbridge(&model).unwrap();
        let reread = import_stbridge(&xml).unwrap();
        assert_eq!(model.elements.len(), reread.elements.len());
        for index in 0..model.elements.len() {
            assert_ends(ends(&reread, index), ends(&model, index));
            let a = model
                .beam_placement(SupportMemberId::Primary(model.elements[index].id))
                .unwrap();
            let b = reread
                .beam_placement(SupportMemberId::Primary(reread.elements[index].id))
                .unwrap();
            assert_eq!(a.section_shape, b.section_shape);
            for (a, b) in a.axes.into_iter().zip(b.axes) {
                assert!(dist(a, b) < 1e-12);
            }
        }
        std::fs::write(directory.join(format!("{name}.stb")), xml).unwrap();
    }
}
