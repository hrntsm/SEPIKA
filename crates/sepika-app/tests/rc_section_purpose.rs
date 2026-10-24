use sepika_core::{dof::Dof6Mask, ids::*, model::*, section_shape::*};
use sepika_edit::{
    EditSectionShape, PlaceSecondaryMember, SecondaryParent, SetPostGravityEndShares, UndoStack,
};
use sepika_io::ovika::{load_ovika, save_ovika, OvikaExtras};
use sepika_solver::statics::analysis::{
    precheck::{model_issues, IssueSeverity},
    Analysis,
};

#[derive(Clone, Copy)]
enum RcKind {
    Beam,
    RectColumn,
    CircleColumn,
}

impl RcKind {
    fn shape(self, extra_depth_mm: f64) -> SectionShape {
        match self {
            Self::Beam => SectionShape::RcBeamRect {
                b: 300.0,
                d: 500.0 + extra_depth_mm,
                rebar: RcBeamRebar {
                    main_dia: 22.0,
                    top: vec![3],
                    bottom: vec![4],
                    cover: 40.0,
                    stirrup: BeamStirrup {
                        dia: 10.0,
                        pitch: 150.0,
                        legs: 2,
                    },
                },
            },
            Self::RectColumn => SectionShape::RcColumnRect {
                b: 600.0,
                d: 600.0 + extra_depth_mm,
                rebar: RcRectColumnRebar {
                    main_dia: 19.0,
                    x: vec![4],
                    y: vec![4],
                    cover: 40.0,
                    hoop: RectColumnHoop {
                        dia: 10.0,
                        pitch: 150.0,
                        legs_x: 2,
                        legs_y: 2,
                    },
                },
            },
            Self::CircleColumn => SectionShape::RcColumnCircle {
                d: 600.0 + extra_depth_mm,
                rebar: RcCircleColumnRebar {
                    main_dia: 19.0,
                    count: 12,
                    cover: 40.0,
                    hoop: CircleColumnHoop {
                        dia: 10.0,
                        pitch: 150.0,
                    },
                },
            },
        }
    }

    fn secondary_kind(self) -> SecondaryMemberKind {
        match self {
            Self::Beam => SecondaryMemberKind::Beam,
            _ => SecondaryMemberKind::Post,
        }
    }

    fn frame_use(self) -> FrameSectionUse {
        match self {
            Self::Beam => FrameSectionUse::Girder,
            _ => FrameSectionUse::Column,
        }
    }

    fn expected_area_mm2(self) -> f64 {
        match self {
            Self::Beam => 150_000.0,
            Self::RectColumn => 360_000.0,
            Self::CircleColumn => 90_000.0 * std::f64::consts::PI,
        }
    }

    fn expected_formwork_m2(self) -> f64 {
        match self {
            Self::Beam => 3.9,
            Self::RectColumn => 7.2,
            Self::CircleColumn => 1.8 * std::f64::consts::PI,
        }
    }

    fn standard_figure(self) -> &'static str {
        match self {
            Self::Beam => "<StbSecBeam_RC_Straight width=\"300\" depth=\"500\"/>",
            Self::RectColumn => "<StbSecColumn_RC_Rect width_X=\"600\" width_Y=\"600\"/>",
            Self::CircleColumn => "<StbSecColumn_RC_Circle D=\"600\"/>",
        }
    }
}

fn material(id: u32, concrete: bool) -> Material {
    Material {
        id: MaterialId(id),
        name: if concrete { "Fc24" } else { "SD345" }.into(),
        category: if concrete {
            MaterialCategory::Concrete
        } else {
            MaterialCategory::Rebar
        },
        young: if concrete { 25_000.0 } else { 205_000.0 },
        poisson: if concrete { 0.2 } else { 0.3 },
        density: if concrete { 2.4e-9 } else { 7.85e-9 },
        shear: None,
        fc: concrete.then_some(24.0),
        fy: (!concrete).then_some(345.0),
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

fn section(
    kind: RcKind,
    id: SectionId,
    extra_depth_mm: f64,
    usage: Option<FrameSectionUse>,
) -> Section {
    let mut section = kind
        .shape(extra_depth_mm)
        .try_to_section(id, format!("RC-{}", id.0))
        .unwrap();
    section.frame_use = usage;
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section
}

fn normal_model(kind: RcKind, shared: bool) -> (Model, SectionId) {
    let horizontal = matches!(kind, RcKind::Beam);
    let primary_section = SectionId(if horizontal { 1 } else { 0 });
    let secondary_section = if shared {
        primary_section
    } else {
        SectionId(2)
    };
    let coords = if horizontal {
        [
            [0.0, 0.0, 3000.0],
            [3000.0, 0.0, 3000.0],
            [3000.0, 3000.0, 3000.0],
            [0.0, 3000.0, 3000.0],
        ]
    } else {
        [
            [0.0, 0.0, 0.0],
            [3000.0, 0.0, 0.0],
            [3000.0, 0.0, 3000.0],
            [0.0, 0.0, 3000.0],
        ]
    };
    let mut model = Model {
        materials: vec![material(0, true), material(1, false)],
        sections: vec![
            section(
                RcKind::RectColumn,
                SectionId(0),
                0.0,
                Some(FrameSectionUse::Column),
            ),
            section(
                RcKind::Beam,
                SectionId(1),
                0.0,
                Some(FrameSectionUse::Girder),
            ),
        ],
        girder_torsion: GirderTorsionMode::Keep,
        ..Default::default()
    };
    if shared {
        model.sections[primary_section.index()] =
            section(kind, primary_section, 20.0, Some(kind.frame_use()));
    } else {
        model.sections[primary_section.index()] =
            section(kind, primary_section, 0.0, Some(kind.frame_use()));
        model
            .sections
            .push(section(kind, secondary_section, 20.0, None));
    }
    model.nodes = coords
        .into_iter()
        .enumerate()
        .map(|(i, coord)| Node {
            id: NodeId(i as u32),
            coord,
            restraint: if i == 0 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        })
        .collect();
    model.elements = (0..4)
        .map(|i| ElementData {
            id: ElemId(i),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(i), NodeId((i + 1) % 4)],
            section: Some(SectionId(if !horizontal && i % 2 == 1 { 0 } else { 1 })),
            local_axis: LocalAxis {
                ref_vector: if horizontal {
                    [0.0, 0.0, 1.0]
                } else {
                    [0.0, 1.0, 0.0]
                },
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        })
        .collect();
    let boundary = vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)];
    let parent = if horizontal {
        model
            .floor_regions
            .push(FloorRegion::new(FloorRegionId(0), boundary));
        SecondaryParent::Floor(FloorRegionId(0))
    } else {
        model
            .wall_regions
            .push(WallRegion::new(WallRegionId(0), boundary));
        SecondaryParent::Wall(WallRegionId(0))
    };
    let mut stack = UndoStack::new();
    assert!(
        stack.run(
            &mut model,
            Box::new(PlaceSecondaryMember {
                parent,
                kind: kind.secondary_kind(),
                ends: SecondaryMemberEnds::Supported([
                    SecondaryMemberAnchor {
                        support: SupportMemberId::Primary(ElemId(0)),
                        position: 0.5
                    },
                    SecondaryMemberAnchor {
                        support: SupportMemberId::Primary(ElemId(2)),
                        position: 0.5
                    },
                ]),
                section: Some(secondary_section),
                name: "正常二次部材".into(),
            })
        ),
        "{:?}",
        stack.last_error()
    );
    if !horizontal {
        let member = model.posts().next().unwrap().id;
        assert!(stack.run(
            &mut model,
            Box::new(SetPostGravityEndShares {
                member,
                shares: Some([1.0, 0.0])
            })
        ));
    }
    let before_area = model.sections[secondary_section.index()].area;
    assert!(
        stack.run(
            &mut model,
            Box::new(EditSectionShape {
                section: secondary_section,
                new_shape: kind.shape(0.0),
                frame_use: shared.then_some(kind.frame_use()),
            })
        ),
        "{:?}",
        stack.last_error()
    );
    assert_ne!(model.sections[secondary_section.index()].area, before_area);
    model.rebuild_assignment_regions().unwrap();
    for region in &mut model.floor_assignment_regions.regions {
        region.assignment = PlateAssignment::NoPlate;
    }
    for region in &mut model.wall_assignment_regions.regions {
        region.assignment = PlateAssignment::NoPlate;
    }
    let member = model.beams().chain(model.posts()).next().unwrap();
    let (start, end) = model.secondary_member_end_points(member).unwrap();
    for coord in [start, end] {
        model.nodes.push(Node {
            id: NodeId(model.nodes.len() as u32),
            coord,
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    (model, secondary_section)
}

fn assert_normal_path(kind: RcKind, shared: bool, label: &str) {
    let (model, section_id) = normal_model(kind, shared);
    model.validate().unwrap();
    let secondary = model.beams().chain(model.posts()).next().unwrap();
    assert_eq!(secondary.section, Some(section_id));
    let section = &model.sections[section_id.index()];
    assert_eq!(section.frame_use, shared.then_some(kind.frame_use()));
    assert_eq!(section.shape, Some(kind.shape(0.0)));
    assert!((section.area - kind.expected_area_mm2()).abs() < 1e-8);
    assert_eq!(
        model
            .elements
            .iter()
            .any(|element| element.section == Some(section_id)),
        shared
    );
    let path = std::env::temp_dir().join(format!(
        "sepika-rc-purpose-{}-{label}.ovika",
        std::process::id()
    ));
    save_ovika(&path, &model, OvikaExtras::default()).unwrap();
    let loaded = load_ovika(&path).unwrap().model;
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        serde_json::to_value(&model).unwrap(),
        serde_json::to_value(&loaded).unwrap()
    );
    let errors: Vec<_> = model_issues(&loaded)
        .into_iter()
        .filter(|issue| issue.severity == IssueSeverity::Error)
        .map(|issue| issue.message)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    Analysis::prepare(&loaded).unwrap();
    let quantity =
        sepika_design_jp::quantity::try_compute_quantity_takeoff(&loaded, &Default::default())
            .unwrap();
    let secondary_quantity = quantity
        .items
        .iter()
        .find(|item| item.secondary == Some(secondary.id))
        .unwrap();
    assert_eq!(secondary_quantity.section, Some(section_id));
    assert!(
        (secondary_quantity.concrete_m3 - kind.expected_area_mm2() * 3000.0 / 1e9).abs() < 1e-10
    );
    assert!((secondary_quantity.formwork_m2 - kind.expected_formwork_m2()).abs() < 1e-10);
    let (stb, _) = sepika_io::stbridge::export_stbridge_with_report(&loaded).unwrap();
    assert!(stb.contains(kind.standard_figure()), "{stb}");
    assert!(!stb.contains("<StbSecRaw"));
}

#[test]
fn rc_beam_section_is_shared_by_girder_and_secondary_beam() {
    assert_normal_path(RcKind::Beam, true, "beam-shared");
}

#[test]
fn rc_beam_secondary_section_keeps_no_frame_use() {
    assert_normal_path(RcKind::Beam, false, "beam-none");
}

#[test]
fn rc_rect_column_section_is_shared_by_column_and_post() {
    assert_normal_path(RcKind::RectColumn, true, "rect-shared");
}

#[test]
fn rc_rect_column_post_section_keeps_no_frame_use() {
    assert_normal_path(RcKind::RectColumn, false, "rect-none");
}

#[test]
fn rc_circle_column_section_is_shared_by_column_and_post() {
    assert_normal_path(RcKind::CircleColumn, true, "circle-shared");
}

#[test]
fn rc_circle_column_post_section_keeps_no_frame_use() {
    assert_normal_path(RcKind::CircleColumn, false, "circle-none");
}

fn plate_model(wall: bool, numeric: bool) -> Model {
    let (mut model, _) = normal_model(
        if wall {
            RcKind::RectColumn
        } else {
            RcKind::Beam
        },
        false,
    );
    model
        .floor_regions
        .iter_mut()
        .for_each(|region| region.secondary_beams.clear());
    model
        .wall_regions
        .iter_mut()
        .for_each(|region| region.posts.clear());
    model.rebuild_assignment_regions().unwrap();
    let mut plate_section = if wall {
        SectionShape::RcWall {
            thickness: 180.0,
            ps: 0.0025,
            pwh_ratio: Some(0.0025),
            pwv_ratio: Some(0.0025),
        }
        .to_section(SectionId(3), "板".into())
    } else {
        SectionShape::RcSlab { thickness: 180.0 }.to_section(SectionId(3), "板".into())
    };
    plate_section.material = Some(MaterialId(0));
    if numeric {
        plate_section.shape = None;
    }
    model.sections.push(plate_section);
    let mut stack = UndoStack::new();
    let command: Box<dyn sepika_edit::EditCommand> = if wall {
        Box::new(sepika_edit::AssignWallPlateToRegion {
            region: model.wall_assignment_regions.regions[0].id,
            section: Some(SectionId(3)),
            opening_area: 0.0,
            opening_weight: 0.0,
        })
    } else {
        Box::new(sepika_edit::AssignSlabToFloorPlateRegion {
            region: model.floor_assignment_regions.regions[0].id,
            plate: SlabPlate {
                section: Some(SectionId(3)),
                ..Default::default()
            },
        })
    };
    assert!(stack.run(&mut model, command), "{:?}", stack.last_error());
    if wall {
        model.wall_plates[0].self_weight_shares = vec![1.0, 0.0, 0.0, 0.0];
    }
    if !wall {
        use sepika_core::{ids::StoryId, model::Story};
        model.stories = vec![
            Story::new(StoryId(0), "基準階".into(), Some(0.), Some(0.)),
            Story::new(StoryId(1), "上階".into(), Some(3000.), Some(0.)),
        ];
        model.slabs[0].reference_story = Some(StoryId(1));
        model.slabs[0].vertical_eccentricity_mm = Some(0.);
    }
    model.validate().unwrap();
    model
}

fn plate_section_command(wall: bool, id: Option<SectionId>) -> Box<dyn sepika_edit::EditCommand> {
    if wall {
        Box::new(sepika_edit::SetWallPlateSection {
            id: WallPlateId(0),
            section: id,
        })
    } else {
        Box::new(sepika_edit::SetSlabSection {
            id: SlabId(0),
            section: id,
        })
    }
}

fn assert_plate_reason(error: &str, wall: bool, kind: RcKind, section_id: u32) {
    let plate = if wall { "WallPlateId(0)" } else { "SlabId(0)" };
    let reason = if matches!(kind, RcKind::Beam) {
        "RC 梁用断面は大梁・小梁専用"
    } else {
        "RC 柱用断面は柱・間柱専用"
    };
    assert!(
        error.contains(plate)
            && error.contains(&format!("SectionId({section_id})"))
            && error.contains(reason),
        "{error}"
    );
}

#[test]
fn normal_rc_and_numeric_plates_keep_edit_roundtrip_prepare_and_quantity() {
    for wall in [false, true] {
        for numeric in [false, true] {
            let mut model = plate_model(wall, numeric);
            let mut stack = UndoStack::new();
            assert!(stack.run(&mut model, plate_section_command(wall, None)));
            model.validate().unwrap();
            let missing_errors: Vec<_> = model_issues(&model)
                .into_iter()
                .filter(|i| i.severity == IssueSeverity::Error)
                .map(|i| i.message)
                .collect();
            if wall {
                assert!(missing_errors.is_empty(), "{missing_errors:?}");
                Analysis::prepare(&model).unwrap();
            } else {
                assert!(!missing_errors.is_empty());
                assert!(Analysis::prepare(&model).is_err());
            }
            let unassigned_path = std::env::temp_dir().join(format!(
                "sepika677-unassigned-{}-{wall}-{numeric}.ovika",
                std::process::id()
            ));
            save_ovika(&unassigned_path, &model, OvikaExtras::default()).unwrap();
            let unassigned = load_ovika(&unassigned_path).unwrap().model;
            assert!(unassigned.eq_ignoring_dofmap(&model));
            std::fs::remove_file(unassigned_path).unwrap();
            assert!(stack.run(&mut model, plate_section_command(wall, Some(SectionId(3)))));
            model.validate().unwrap();
            let path = std::env::temp_dir().join(format!(
                "sepika677-normal-{}-{wall}-{numeric}.ovika",
                std::process::id()
            ));
            save_ovika(&path, &model, OvikaExtras::default()).unwrap();
            let loaded = load_ovika(&path).unwrap().model;
            std::fs::remove_file(path).unwrap();
            assert_eq!(
                serde_json::to_value(&model).unwrap(),
                serde_json::to_value(&loaded).unwrap()
            );
            let errors: Vec<_> = model_issues(&loaded)
                .into_iter()
                .filter(|i| i.severity == IssueSeverity::Error)
                .map(|i| i.message)
                .collect();
            assert!(
                errors.is_empty(),
                "wall={wall} numeric={numeric} {errors:?}"
            );
            Analysis::prepare(&loaded).unwrap();
            sepika_design_jp::quantity::try_compute_quantity_takeoff(&loaded, &Default::default())
                .unwrap();
            let thickness = if wall {
                loaded.sections[3].thickness.unwrap()
            } else {
                loaded.slab_plate_thickness(&loaded.slabs[0]).unwrap()
            };
            assert_eq!(thickness, 180.0);
        }
    }
}

#[test]
fn rc_frame_sections_reject_plate_assignment_and_shape_edit_without_state_changes() {
    for wall in [false, true] {
        for numeric in [false, true] {
            for kind in [RcKind::Beam, RcKind::RectColumn, RcKind::CircleColumn] {
                let mut model = plate_model(wall, numeric);
                model.sections.push(section(kind, SectionId(4), 0.0, None));
                for (command, expected_id) in [
                    (plate_section_command(wall, Some(SectionId(4))), 4),
                    (
                        Box::new(EditSectionShape {
                            section: SectionId(3),
                            new_shape: kind.shape(0.0),
                            frame_use: None,
                        }) as Box<dyn sepika_edit::EditCommand>,
                        3,
                    ),
                ] {
                    let mut actual = model.clone();
                    let mut direct = model.clone();
                    let inverse = command.apply(&mut direct);
                    assert_plate_reason(inverse.rejection().unwrap(), wall, kind, expected_id);
                    assert!(direct.eq_ignoring_dofmap(&model));
                    let mut stack = UndoStack::new();
                    assert!(stack.run(
                        &mut actual,
                        Box::new(sepika_edit::SetSectionName {
                            id: SectionId(3),
                            name: "履歴".into(),
                            floor: None
                        })
                    ));
                    stack.undo(&mut actual);
                    let before = actual.clone();
                    let revision = stack.revision();
                    let redo = stack.redo_label().map(str::to_owned);
                    assert!(!stack.run(&mut actual, command));
                    assert_plate_reason(stack.last_error().unwrap(), wall, kind, expected_id);
                    assert!(actual.eq_ignoring_dofmap(&before));
                    assert_eq!(stack.revision(), revision);
                    assert_eq!(stack.redo_label(), redo.as_deref());
                    assert!(!stack.can_undo());
                    assert!(stack.id_changes().is_empty());
                }
                let mut malformed = model.clone();
                if wall {
                    malformed.wall_plates[0].section = Some(SectionId(4));
                } else {
                    malformed.slabs[0].plate.section = Some(SectionId(4));
                }
                assert_plate_reason(
                    &malformed.validate().unwrap_err().to_string(),
                    wall,
                    kind,
                    4,
                );
                let issues = model_issues(&malformed);
                assert!(issues
                    .iter()
                    .any(|issue| issue.severity == IssueSeverity::Error
                        && issue.message.contains("専用")));
                assert_plate_reason(
                    &Analysis::prepare(&malformed).err().unwrap().to_string(),
                    wall,
                    kind,
                    4,
                );
                assert_plate_reason(
                    &sepika_design_jp::quantity::try_compute_quantity_takeoff(
                        &malformed,
                        &Default::default(),
                    )
                    .unwrap_err(),
                    wall,
                    kind,
                    4,
                );
                let path = std::env::temp_dir().join(format!(
                    "sepika677-reject-{}-{wall}-{numeric}.ovika",
                    std::process::id()
                ));
                save_ovika(&path, &model, OvikaExtras::default()).unwrap();
                let original = std::fs::read(&path).unwrap();
                assert_plate_reason(
                    &save_ovika(&path, &malformed, OvikaExtras::default())
                        .unwrap_err()
                        .to_string(),
                    wall,
                    kind,
                    4,
                );
                assert_eq!(std::fs::read(&path).unwrap(), original);
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}
