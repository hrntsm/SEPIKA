use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_core::model::{
    BrbAttr, DamperSpec, LoadCase, LoadCaseKind, LoadCfg, MemberDetailAttr, MemberJoint,
    MemberLoad, MemberLoadKind, Model, PanelZoneMode, DL_CASE_NAME,
};
use sepika_job::settings::AnalysisSettings;
use sepika_job::{apply_rigid_zones_and_panels, prepare_model_for_analysis};

#[path = "fixtures/steel_panel_model.rs"]
mod fixture;
use fixture::steel_panel_model;

#[test]
fn preparation_rejects_references_to_removed_panels_without_changing_input() {
    let mut input = steel_panel_model();
    input.stories.clear();
    assert_eq!(apply_rigid_zones_and_panels(&mut input).unwrap().len(), 1);
    input.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(0),
        name: DL_CASE_NAME.into(),
        kind: LoadCaseKind::Dead,
        nodal: Vec::new(),
        member: vec![MemberLoad::auto(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Point { a: 0.0, p: 100.0 },
        )],
    });
    type AssignInput = fn(&mut Model, ElemId);
    let inputs: [(&str, AssignInput); 5] = [
        ("BRB製品属性", |model, elem| {
            model.brb_attrs.push(BrbAttr {
                elem,
                allowable_axial_short: Some(100_000.0),
                critical_length: Some(5_000.0),
                length_reduction: Some(0.0),
            });
        }),
        ("部材荷重", |model, elem| {
            model.load_cases[0].member.push(MemberLoad::manual(
                elem,
                [0.0, 0.0, -1.0],
                MemberLoadKind::Point { a: 0.0, p: 100.0 },
            ));
        }),
        ("部材付加線重量", |model, elem| {
            model.load_cfg = Some(LoadCfg {
                extra_line_weight: vec![(elem, 0.4)],
                ..Default::default()
            });
        }),
        ("仕上げ面重量", |model, elem| {
            model.load_cfg = Some(LoadCfg {
                finish_area_weight: vec![(elem, 0.002)],
                ..Default::default()
            });
        }),
        ("ダンパー総重量", |model, elem| {
            model.load_cfg = Some(LoadCfg {
                dampers: vec![DamperSpec {
                    elem,
                    total_weight: 4500.0,
                }],
                ..Default::default()
            });
        }),
    ];

    for (mode, panel_index) in [
        (PanelZoneMode::None, 2),
        (PanelZoneMode::Model, 2),
        (PanelZoneMode::Model, 3),
    ] {
        for (input_kind, assign) in inputs {
            let mut model = input.clone();
            model.panel_zone = mode;
            model.elements[2].rigid_zone.length_i = 1_234.0;
            model.elements.swap(panel_index, 3);
            model.elements[2].id = ElemId(2);
            model.elements[3].id = ElemId(3);
            assign(&mut model, ElemId(panel_index as u32));
            let before = bincode::serialize(&model).unwrap();
            let mut rigid_zones = model.clone();
            let error = prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None)
                .err()
                .unwrap_or_else(|| panic!("{mode:?} の仕口パネル {panel_index} に付いた{input_kind}で準備が成功しました"));
            let diagnostic = error.to_string();
            assert!(
                diagnostic.contains(&format!("仕口パネル要素 {panel_index}")),
                "{diagnostic}"
            );
            assert!(diagnostic.contains(input_kind), "{diagnostic}");
            assert!(diagnostic.contains("削除"), "{diagnostic}");
            assert_eq!(bincode::serialize(&model).unwrap(), before);

            let error = apply_rigid_zones_and_panels(&mut rigid_zones).unwrap_err();
            assert!(diagnostic.contains(&error), "{error}");
            assert_eq!(bincode::serialize(&rigid_zones).unwrap(), before);
        }
    }
}

#[test]
fn preparation_keeps_attributes_loads_and_weights_on_the_surviving_member() {
    let mut input = steel_panel_model();
    input.stories.clear();
    assert_eq!(apply_rigid_zones_and_panels(&mut input).unwrap().len(), 1);
    input.elements.swap(2, 3);
    input.elements[2].id = ElemId(2);
    input.elements[3].id = ElemId(3);
    let beam = &input.elements[3];
    let mut attr = MemberDetailAttr::new(beam.id);
    attr.joints.push(MemberJoint {
        distance: 1_500.0,
        kind: Default::default(),
    });
    input.member_detail_attrs.push(attr.clone());
    let load = MemberLoad::full_length_uniform(beam.id, [0.0, 0.0, -1.0], 6_000.0, 2.0);
    input.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(0),
        name: "手入力荷重".into(),
        kind: Default::default(),
        nodal: Vec::new(),
        member: vec![load.clone()],
    });
    input.load_cfg = Some(LoadCfg {
        extra_line_weight: vec![(beam.id, 0.4)],
        finish_area_weight: vec![(beam.id, 0.002)],
        dampers: vec![DamperSpec {
            elem: beam.id,
            total_weight: 4500.0,
        }],
        ..Default::default()
    });

    for (mode, num_panels) in [(PanelZoneMode::None, 0), (PanelZoneMode::Model, 1)] {
        let mut model = input.clone();
        model.panel_zone = mode;
        let report =
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
        assert_eq!(report.panels.len(), num_panels);
        assert_eq!(model.elements[2].nodes, beam.nodes);
        assert_eq!(model.elements[2].section, beam.section);
        let weights = model.load_cfg.unwrap();
        assert_eq!(weights.extra_line_weight, [(ElemId(2), 0.4)]);
        assert_eq!(weights.finish_area_weight, [(ElemId(2), 0.002)]);
        assert_eq!(
            weights.dampers,
            [DamperSpec {
                elem: ElemId(2),
                total_weight: 4500.0,
            }]
        );
        assert_eq!(
            model.member_detail_attrs,
            [MemberDetailAttr {
                elem: ElemId(2),
                ..attr.clone()
            }]
        );
        assert_eq!(
            model.load_cases[0].member,
            [MemberLoad {
                elem: ElemId(2),
                ..load.clone()
            }]
        );
    }
}
