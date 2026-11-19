use sepika_core::geom::vec3::{dist, dot};
use sepika_core::model::{Model, SupportMemberId};
use sepika_io::stbridge::{
    export_stbridge, import_stbridge, import_stbridge_with_report, StbError,
};

const INPUT: &str = include_str!("fixtures/h_column_reference.stb");
const FALSE_ATTR: &str = r#" isReferenceDirection="false""#;

fn assert_placement(
    model: &Model,
    index: usize,
    reference_ends: [[f64; 3]; 2],
    offsets_mm: [[f64; 3]; 2],
    actual_ends: [[f64; 3]; 2],
    expected_axes: [[f64; 3]; 3],
) {
    let column = &model.elements[index];
    let placement = model
        .beam_placement(SupportMemberId::Primary(column.id))
        .unwrap();
    assert_eq!(placement.reference_ends, reference_ends);
    assert_eq!(column.end_offsets_mm, offsets_mm);
    assert_eq!(placement.actual_ends, actual_ends);
    assert!(dist(placement.axes[0], expected_axes[0]) < 1e-12);
    for (axis, expected) in placement.axes[1..].iter().zip(&expected_axes[1..]) {
        // 対称Hでは、断面軸の正負が同時に逆でも外形と剛性は一致する。
        assert!((dot(*axis, *expected).abs() - 1.0).abs() < 1e-12);
    }
}

fn standard_roundtrip(model: &Model) -> Model {
    let xml = export_stbridge(model).unwrap();
    assert!(xml.contains("<StbSecSteelColumn_S_Same "));
    assert!(!xml.contains("<StbSecRaw"));
    assert!(!xml.contains("isReferenceDirection"));
    import_stbridge(&xml).unwrap()
}

#[test]
fn 鉄骨向きの既定値とxml真偽値を断面の物理方向へ反映する() {
    let ends = [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]];
    for (attr, rotated) in [
        ("", false),
        (r#" isReferenceDirection="true""#, false),
        (r#" isReferenceDirection="1""#, false),
        (FALSE_ATTR, true),
        (r#" isReferenceDirection="0""#, true),
        (r#" isReferenceDirection="  0  ""#, true),
    ] {
        let input = INPUT.replace(FALSE_ATTR, attr);
        let model = import_stbridge(&input).unwrap();
        let (reported, report) = import_stbridge_with_report(&input).unwrap();
        let direction = report.attributes.iter().find(|entry| {
            entry.element == "StbSecColumn_S" && entry.attribute == "isReferenceDirection"
        });
        assert_eq!(
            direction.map(|entry| (entry.count, entry.imported)),
            (!attr.is_empty()).then_some((1, 1))
        );
        let axes = if rotated {
            [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]]
        } else {
            [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        };
        for imported in [standard_roundtrip(&model), model, reported] {
            assert_placement(&imported, 0, ends, [[0.0; 3]; 2], ends, axes);
        }
    }
}

#[test]
fn 組立hの傾斜柱の部材回転と端偏心を保持する() {
    let input = INPUT
        .replace(r#"X="0" Y="0" Z="3000""#, r#"X="1800" Y="0" Z="2400""#)
        .replace(
            r#"kind_structure="S""#,
            r#"kind_structure="S" rotate="30" offset_bottom_X="30" offset_top_X="30" offset_bottom_Y="-40" offset_top_Y="-40" offset_bottom_Z="25" offset_top_Z="25""#,
        )
        .replace(r#"StbSecRoll-H name="H600" type="H""#, r#"StbSecBuild-H name="H600""#)
        .replace(r#" r="12""#, "");
    let model = import_stbridge(&input).unwrap();
    let sin_120 = 3.0_f64.sqrt() / 2.0;
    for imported in [standard_roundtrip(&model), model] {
        assert_placement(
            &imported,
            0,
            [[0.0, 0.0, 0.0], [1800.0, 0.0, 2400.0]],
            [[30.0, -40.0, 25.0]; 2],
            [[30.0, -40.0, 25.0], [1830.0, -40.0, 2425.0]],
            [
                [0.6, 0.0, 0.8],
                [-0.4, sin_120, 0.3],
                [-0.8 * sin_120, -0.5, 0.6 * sin_120],
            ],
        );
    }
}

#[test]
fn 共通座標変換と軸方向の偏心差があっても断面の位置と向きを保持する() {
    let input = INPUT
        .replace(r#"app_version="1""#, r#"app_version="1" global_rotation="90" global_offset_X="100" global_offset_Y="200" global_offset_Z="300""#)
        .replace(
            r#"kind_structure="S""#,
            r#"kind_structure="S" rotate="90" offset_bottom_X="30" offset_top_X="30" offset_bottom_Y="-40" offset_top_Y="-40" offset_bottom_Z="25" offset_top_Z="-75""#,
        );
    let model = import_stbridge(&input).unwrap();
    for imported in [standard_roundtrip(&model), model] {
        assert_placement(
            &imported,
            0,
            [[100.0, 200.0, 300.0], [100.0, 200.0, 3300.0]],
            [[40.0, 30.0, 25.0], [40.0, 30.0, -75.0]],
            [[140.0, 230.0, 325.0], [140.0, 230.0, 3225.0]],
            [[0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]],
        );
    }
}

#[test]
fn 合成回転が一周する柱は基準方向を標準形式で保持する() {
    let ends = [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]];
    for input in [
        INPUT.replace(
            r#"kind_structure="S""#,
            r#"kind_structure="S" rotate="270""#,
        ),
        INPUT.replace(
            r#"app_version="1""#,
            r#"app_version="1" global_rotation="270""#,
        ),
    ] {
        let model = import_stbridge(&input).unwrap();
        let xml = export_stbridge(&model).unwrap();
        let member = xml
            .lines()
            .find(|line| line.contains("<StbColumn "))
            .unwrap();
        assert!(member.contains(r#"rotate="0""#), "{member}");
        let reread = import_stbridge(&xml).unwrap();
        for imported in [model, reread] {
            assert_placement(
                &imported,
                0,
                ends,
                [[0.0; 3]; 2],
                ends,
                [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            );
        }
    }
}

#[test]
fn 同じ断面へ統合する元定義ごとの向きを保持する() {
    let section = INPUT
        .split("<StbSecColumn_S ")
        .nth(1)
        .unwrap()
        .split_once("</StbSecColumn_S>")
        .unwrap()
        .0;
    let input = INPUT
        .replace(
            "</StbColumns>",
            r#"<StbColumn id="42" name="C2" id_node_bottom="11" id_node_top="29" id_section="72" kind_structure="S"/></StbColumns>"#,
        )
        .replace(
            "</StbSecColumn_S>",
            &format!(
                "</StbSecColumn_S><StbSecColumn_S {}</StbSecColumn_S>",
                section.replace(r#"id="71""#, r#"id="72""#).replace(FALSE_ATTR, "")
            ),
        );
    let model = import_stbridge(&input).unwrap();
    assert_eq!(model.sections.len(), 1);
    assert_eq!(model.elements[0].section, model.elements[1].section);
    for imported in [standard_roundtrip(&model), model] {
        let ends = [[0.0, 0.0, 0.0], [0.0, 0.0, 3000.0]];
        for (index, axes) in [
            [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
            [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        ]
        .into_iter()
        .enumerate()
        {
            assert_placement(&imported, index, ends, [[0.0; 3]; 2], ends, axes);
        }
    }
}

#[test]
fn 非一様断面は鉄骨向きを指定しても理由付きで拒否する() {
    let input = INPUT
        .replace(
            r#"<StbSecSteelColumn_S_Same shape="H600" strength_main="SN400B"/>"#,
            r#"<StbSecSteelColumn_S_NotSame pos="BOTTOM" shape="H600" strength_main="SN400B"/><StbSecSteelColumn_S_NotSame pos="TOP" shape="H400" strength_main="SN400B"/>"#,
        )
        .replace(
            "</StbSecSteel>",
            r#"<StbSecRoll-H name="H400" type="H" A="400" B="200" t1="10" t2="15" r="8"/></StbSecSteel>"#,
        );
    let error = import_stbridge_with_report(&input).unwrap_err();
    assert!(
        matches!(error, StbError::Unmappable(ref reason)
            if reason.contains("継手で切り替わる区間ごとの断面形状を保持できません")),
        "{error}"
    );
}

#[test]
fn 主架構の柱と断面を共有する間柱には向きが未適用と報告する() {
    let input = INPUT.replace(
        "</StbColumns>",
        r#"</StbColumns><StbPosts><StbPost id="43" name="P1" id_node_bottom="11" id_node_top="29" id_section="71" kind_structure="S"/></StbPosts>"#,
    );
    let (model, report) = import_stbridge_with_report(&input).unwrap();
    assert!(
        report.warnings.iter().any(|warning| {
            [
                "StbPost",
                "43",
                "StbSecColumn_S:71",
                "isReferenceDirection",
                "未対応",
            ]
            .iter()
            .all(|text| warning.contains(text))
        }),
        "{:?}",
        report.warnings
    );
    let post = model.posts().next().unwrap();
    let placement = model
        .beam_placement(SupportMemberId::Secondary(post.id))
        .unwrap();
    assert!(dist(placement.axes[1], [1.0, 0.0, 0.0]) < 1e-12);
}

#[test]
fn 不正な鉄骨向きと表現できない端偏心は対象と理由を報告する() {
    let invalid = INPUT.replace(FALSE_ATTR, r#" isReferenceDirection="False""#);
    let error = import_stbridge(&invalid).unwrap_err().to_string();
    for text in ["StbSecColumn_S:71", "isReferenceDirection", "False"] {
        assert!(error.contains(text), "{error}");
    }
    let impossible = INPUT.replace(
        r#"kind_structure="S""#,
        r#"kind_structure="S" offset_top_X="50""#,
    );
    let error = import_stbridge(&impossible).unwrap_err().to_string();
    for text in ["StbColumn", "41", "軸方向差"] {
        assert!(error.contains(text), "{error}");
    }
}
