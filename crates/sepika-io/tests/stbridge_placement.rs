use sepika_core::geom::vec3::dist;
use sepika_core::model::{Model, SupportMemberId};
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
    let members = r#"<StbColumns><StbColumn id="1" id_node_bottom="1" id_node_top="2" id_section="1" kind_structure="S" rotate="27"/></StbColumns>
<StbGirders><StbGirder id="1" id_node_start="2" id_node_end="3" id_section="1" kind_structure="S" rotate="15"/><StbGirder id="2" id_node_start="7" id_node_end="8" id_section="1" kind_structure="S" rotate="15"/></StbGirders>
<StbBeams><StbBeam id="1" id_node_start="5" id_node_end="4" id_section="1" kind_structure="S" rotate="35"/></StbBeams>
<StbPosts><StbPost id="1" id_node_bottom="5" id_node_top="6" id_section="1" kind_structure="S" rotate="45"/></StbPosts>
<StbBraces><StbBrace id="1" id_node_start="1" id_node_end="3" id_section="1" kind_structure="S" rotate="55" feature_brace="TENSION"/></StbBraces>"#;
    for (name, shape) in [("all-five-h", H)] {
        let model = import_stbridge(&input("", members, shape)).unwrap();
        let xml = export_stbridge(&model).unwrap();
        let reread = import_stbridge(&xml).unwrap();
        assert_eq!(model.elements.len(), reread.elements.len());
        for index in 0..model.elements.len() {
            assert_ends(ends(&reread, index), ends(&model, index));
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
        }
        std::fs::write(directory.join(format!("{name}.stb")), xml).unwrap();
    }
}
