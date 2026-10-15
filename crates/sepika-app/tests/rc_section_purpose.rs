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
