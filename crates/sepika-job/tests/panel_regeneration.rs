use sepika_core::ids::{
    ElemId, LoadCaseId, MaterialId, NodeId, SectionId, SlabId, StoryId, WallPlateId,
};
use sepika_core::model::{
    AreaLoad, DistributionMethod, ElementKind, MaterialCategory, OneWayDir, PlateAssignment,
    SecondaryMember, SecondaryMemberAnchor, SecondaryMemberEnds, SecondaryMemberKind, Slab,
    SlabPlate, SlabShape, SupportMemberId, WallDlSupport, WallPlate, WallPlateShape,
};
use sepika_core::model::{
    BrbAttr, DamperSpec, LoadCase, LoadCaseKind, LoadCfg, MemberDetailAttr, MemberJoint,
    MemberLoad, MemberLoadKind, Model, PanelZoneMode, DL_CASE_NAME,
};
use sepika_core::section_shape::SectionShape;
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

fn frame_with_panel_before_supports(wall: bool) -> Model {
    let mut model = steel_panel_model();
    model.materials[0].density = 0.0;
    for column in &mut model.sections[..2] {
        let mut section = SectionShape::SteelPipe {
            outer_dia: 400.0,
            thick: 16.0,
        }
        .to_section(column.id, column.name.clone());
        section.material = column.material;
        section.frame_use = column.frame_use;
        *column = section;
    }
    let mut back = model.nodes[3].clone();
    back.id = NodeId(4);
    back.coord = if wall {
        [6000., 0., 0.]
    } else {
        [6000., 4000., 3000.]
    };
    model.nodes.push(back);
    let far_corner = if wall {
        NodeId(1)
    } else {
        let mut far = model.nodes[0].clone();
        far.id = NodeId(5);
        far.coord[1] = 4000.;
        model.nodes.push(far);
        NodeId(5)
    };
    for (nodes, column) in [
        ([NodeId(4), NodeId(3)], wall),
        ([far_corner, NodeId(4)], false),
    ] {
        let mut element = model.elements[if column { 0 } else { 2 }].clone();
        element.id = ElemId(model.elements.len() as u32);
        element.nodes = nodes.into_iter().collect();
        model.elements.push(element);
    }
    if !wall {
        let mut element = model.elements[2].clone();
        element.id = ElemId(5);
        element.nodes = [NodeId(0), far_corner].into_iter().collect();
        model.elements.push(element);
    }
    assert!(!apply_rigid_zones_and_panels(&mut model).unwrap().is_empty());
    let panel_index = model
        .elements
        .iter()
        .position(|element| element.kind == ElementKind::PanelZone)
        .unwrap();
    model.elements[2..=panel_index].rotate_right(1);
    for (index, element) in model.elements.iter_mut().enumerate() {
        element.id = ElemId(index as u32);
    }

    let secondary = SecondaryMember {
        id: model.alloc_secondary_member_id(),
        kind: if wall {
            SecondaryMemberKind::Post
        } else {
            SecondaryMemberKind::Beam
        },
        ends: SecondaryMemberEnds::Supported([ElemId(3), ElemId(5)].map(|elem| {
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(elem),
                position: 0.5,
            }
        })),
        section: Some(SectionId(2)),
        name: if wall { "間柱P1" } else { "小梁B1" }.into(),
        ..Default::default()
    };
    if wall {
        model.unassigned_posts.push(secondary);
    } else {
        model.unassigned_beams.push(secondary);
    }
    model.rebuild_assignment_regions().unwrap();
    let mut concrete = model.materials[0].clone();
    concrete.id = MaterialId(1);
    concrete.name = "Fc24".into();
    concrete.category = MaterialCategory::Concrete;
    concrete.young = 23_000.;
    concrete.density = if wall { 2.4e-9 } else { 0. };
    concrete.fc = Some(24.);
    concrete.fy = None;
    model.materials.push(concrete);
    let shape = if wall {
        SectionShape::RcWall {
            thickness: 150.,
            ps: 0.002,
            pwh_ratio: None,
            pwv_ratio: None,
        }
    } else {
        SectionShape::RcSlab { thickness: 100. }
    };
    let mut section = shape.to_section(SectionId(3), "版断面".into());
    section.material = Some(MaterialId(1));
    section.shape = None;
    section.property_basis.area = sepika_core::model::PropertyBasis::Supplied;
    model.sections.push(section);
    if wall {
        model.wall_plates.push(WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(3)),
            opening_area: 0.,
            opening_weight: 0.,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
            self_weight_shares: vec![],
            dl_support: Some(WallDlSupport::UpperBeam),
        });
        assert_eq!(model.wall_assignment_regions.regions.len(), 2);
        for region in &mut model.wall_assignment_regions.regions {
            region.assignment = if region
                .boundary
                .iter()
                .any(|edge| edge.support == SupportMemberId::Primary(ElemId(0)))
            {
                PlateAssignment::Plate(WallPlateId(0))
            } else {
                PlateAssignment::NoPlate
            };
        }
    } else {
        model.slabs.push(Slab {
            id: SlabId(0),
            shape: SlabShape::Enclosed,
            plate: SlabPlate {
                section: Some(SectionId(3)),
                loads: vec![AreaLoad {
                    kind: "仕上げ".into(),
                    value: 0.002,
                }],
                method: DistributionMethod::OneWay,
                one_way: Some(OneWayDir::X),
                ..Default::default()
            },
            reference_story: Some(StoryId(1)),
            vertical_eccentricity_mm: Some(0.),
            tip_loads: vec![],
        });
        assert_eq!(model.floor_assignment_regions.regions.len(), 2);
        for region in &mut model.floor_assignment_regions.regions {
            region.assignment = if region
                .boundary
                .iter()
                .any(|edge| edge.support == SupportMemberId::Primary(ElemId(6)))
            {
                PlateAssignment::Plate(SlabId(0))
            } else {
                PlateAssignment::NoPlate
            };
        }
    }
    model
}

#[test]
fn repeated_preparation_keeps_secondary_supports_plate_assignments_and_floor_load_path() {
    for wall in [false, true] {
        let input = frame_with_panel_before_supports(wall);
        let panel_count = input
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::PanelZone)
            .count();
        let member = input.beams().chain(input.posts()).next().unwrap();
        let expected_ends = [
            [3000., 0., 3000.],
            if wall {
                [3000., 0., 0.]
            } else {
                [3000., 4000., 3000.]
            },
        ];
        for (mode, num_panels) in [
            (PanelZoneMode::Model, panel_count),
            (PanelZoneMode::None, 0),
        ] {
            let mut model = input.clone();
            model.panel_zone = mode;
            for _ in 0..2 {
                let report =
                    prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None)
                        .unwrap();
                assert_eq!(report.panels.len(), num_panels);
                let kept = model.secondary_member(member.id).unwrap();
                let ends = kept.ends.anchors();
                assert_eq!(
                    [ends[0].support, ends[1].support],
                    [ElemId(2), ElemId(4)].map(SupportMemberId::Primary)
                );
                assert_eq!(
                    model.secondary_member_end_points(kept),
                    Some((expected_ends[0], expected_ends[1]))
                );
                if wall {
                    assert_eq!(model.wall_assignment_regions.regions.len(), 2);
                    for original in &input.wall_assignment_regions.regions {
                        let region = model.wall_assignment_region(original.id).unwrap();
                        assert_eq!(region.assignment, original.assignment);
                        assert_eq!(
                            model.wall_assignment_region_coords(region.id),
                            input.wall_assignment_region_coords(original.id)
                        );
                    }
                } else {
                    assert_eq!(model.floor_assignment_regions.regions.len(), 2);
                    for original in &input.floor_assignment_regions.regions {
                        let region = model.floor_assignment_region(original.id).unwrap();
                        assert_eq!(region.assignment, original.assignment);
                        assert_eq!(
                            model.floor_assignment_region_coords(region.id),
                            input.floor_assignment_region_coords(original.id)
                        );
                    }
                    let dl = model
                        .load_cases
                        .iter()
                        .find(|case| case.kind == LoadCaseKind::Dead)
                        .unwrap();
                    for support in [ElemId(2), ElemId(4)] {
                        // 3 m × 4 m の片側床板24 kNの半分を小梁が受け、両端が等分する。
                        let transferred: f64 = dl
                            .member
                            .iter()
                            .filter_map(|load| match load.kind {
                                MemberLoadKind::Point { a, p } if load.elem == support => {
                                    assert!((a - 3000.).abs() < 1e-6);
                                    assert_eq!(load.dir, [0., 0., -1.]);
                                    Some(p)
                                }
                                _ => None,
                            })
                            .sum();
                        assert!(
                            (transferred - 6_000.).abs() < 1e-6,
                            "{support:?}: {transferred} N"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn preparation_identifies_panel_support_owners_and_allows_retry_after_repair() {
    fn support_to_change(
        model: &mut Model,
        wall: bool,
        boundary: bool,
    ) -> (&mut SupportMemberId, String) {
        match (wall, boundary) {
            (false, true) => {
                let region = &mut model.floor_assignment_regions.regions[0];
                (
                    &mut region.boundary[0].support,
                    format!("床板割当領域 ID {}", region.id.0),
                )
            }
            (true, true) => {
                let region = &mut model.wall_assignment_regions.regions[0];
                (
                    &mut region.boundary[0].support,
                    format!("壁版割当領域 ID {}", region.id.0),
                )
            }
            (_, false) => {
                let member = if wall {
                    &mut model
                        .wall_regions
                        .iter_mut()
                        .find(|r| !r.posts.is_empty())
                        .unwrap()
                        .posts[0]
                } else {
                    &mut model
                        .floor_regions
                        .iter_mut()
                        .find(|r| !r.secondary_beams.is_empty())
                        .unwrap()
                        .secondary_beams[0]
                };
                let SecondaryMemberEnds::Supported(anchors) = &mut member.ends else {
                    panic!("両端支持の入力が必要");
                };
                let kind = if wall { "間柱" } else { "小梁" };
                (
                    &mut anchors[1].support,
                    format!("{kind} ID {}（{}）のj端", member.id.0, member.name),
                )
            }
        }
    }

    let panel = SupportMemberId::Primary(ElemId(2));
    for wall in [false, true] {
        let mut input = frame_with_panel_before_supports(wall);
        if wall {
            sepika_core::wall_region_rebuild::rebuild_wall_regions(&mut input);
        } else {
            sepika_core::region_rebuild::rebuild_floor_regions(&mut input);
        }
        for mode in [PanelZoneMode::Model, PanelZoneMode::None] {
            for boundary in [false, true] {
                let mut model = input.clone();
                model.panel_zone = mode;
                let (support, owner) = support_to_change(&mut model, wall, boundary);
                let correct_support = *support;
                *support = panel;
                let before = bincode::serialize(&model).unwrap();
                let mut rigid_zones = model.clone();
                let diagnostic =
                    prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None)
                        .err()
                        .expect("削除するパネルを支持先として採用できない")
                        .to_string();
                assert!(diagnostic.contains(&owner), "{diagnostic}");
                assert!(
                    diagnostic.contains("削除対象の仕口パネル要素 2"),
                    "{diagnostic}"
                );
                assert!(diagnostic.contains("参照先を修正"), "{diagnostic}");
                assert_eq!(bincode::serialize(&model).unwrap(), before);
                let error = apply_rigid_zones_and_panels(&mut rigid_zones).unwrap_err();
                assert!(diagnostic.contains(&error), "{error}");
                assert_eq!(bincode::serialize(&rigid_zones).unwrap(), before);

                *support_to_change(&mut model, wall, boundary).0 = correct_support;
                prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            }
        }
    }
}
