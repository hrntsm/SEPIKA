use sepika_core::geom::vec3::dist;
use sepika_core::model::{EndCondition, SupportMemberId};
use sepika_core::section_shape::SectionShape;
use sepika_io::stbridge::{import_stbridge, import_stbridge_with_report, StbError};

#[test]
fn rejects_two_or_three_steel_column_regions_with_the_same_reason_in_either_order() {
    let bottom_top = include_str!("fixtures/column_steel_bottom_top.stb");
    let top_bottom = include_str!("fixtures/column_steel_top_bottom.stb");
    let three_types = include_str!("fixtures/column_steel_three_types.stb");

    let diagnostic = |xml| {
        let Err(StbError::Unmappable(message)) = import_stbridge(xml) else {
            panic!("区間別の鉄骨断面は読み込みを拒否する必要があります");
        };
        let Err(StbError::Unmappable(with_report)) = import_stbridge_with_report(xml) else {
            panic!("報告付きの読み込みでも区間別の鉄骨断面は拒否する必要があります");
        };
        assert_eq!(with_report, message);
        for fragment in [
            "StbColumn id=Some(41)",
            "name=Some(\"C41\")",
            "入力断面 StbSecColumn_S:71, name=\"C71\"",
            "継手で切り替わる区間ごとの断面形状を保持できません",
        ] {
            assert!(message.contains(fragment), "{fragment}: {message}");
        }
        message
    };

    assert_eq!(diagnostic(bottom_top), diagnostic(top_bottom));
    diagnostic(three_types);
}

#[test]
fn uniform_steel_column_keeps_shape_bottom_top_release_offsets_and_rotation() {
    let xml = include_str!("fixtures/column_steel_uniform_end_conditions.stb");
    let (model, report) = import_stbridge_with_report(xml).unwrap();
    let column = &model.elements[0];
    let section = &model.sections[column.section.unwrap().index()];
    assert_eq!(
        section.shape,
        Some(SectionShape::SteelH {
            height: 600.0,
            width: 300.0,
            web_thick: 12.0,
            flange_thick: 20.0,
            root_r: Some(12.0),
        })
    );
    assert_eq!(column.end_cond, [EndCondition::Pinned, EndCondition::Fixed]);
    let placement = model
        .beam_placement(SupportMemberId::Primary(column.id))
        .unwrap();
    assert_eq!(
        placement.actual_ends,
        [[0.0, 0.0, 25.0], [0.0, 0.0, 2925.0]]
    );
    assert!(dist(placement.axes[1], [0.0, 1.0, 0.0]) < 1e-12);
    assert!(dist(placement.axes[2], [-1.0, 0.0, 0.0]) < 1e-12);
    assert!(report
        .dropped_attributes()
        .any(|entry| entry.element == "StbColumn" && entry.attribute == "name"));
}
