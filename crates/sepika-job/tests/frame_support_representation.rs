#[path = "../../sepika-core/tests/support/frame_support_model.rs"]
mod fixture;
use sepika_core::model::*;
use sepika_job::{prepare::prepare_model_for_analysis, settings::AnalysisSettings};

fn resultant(model: &Model, nodal: &[NodalLoad], member: &[MemberLoad]) -> [f64; 4] {
    let mut sum = [0.; 4];
    for load in nodal {
        let point = model.node(load.node).unwrap().coord;
        let force = -load.values[2];
        sum[0] += force;
        for k in 0..3 {
            sum[k + 1] += force * point[k];
        }
    }
    for load in member {
        assert_eq!(load.dir, [0., 0., -1.]);
        let element = model.element(load.elem).unwrap();
        let start = model.node(element.nodes[0]).unwrap().coord;
        let end = model.node(element.nodes[1]).unwrap().coord;
        let length = model.member_length(element);
        let (force, first) = match load.kind {
            MemberLoadKind::Point { a, p } => (p, p * a),
            MemberLoadKind::Distributed { a, b, w1, w2 } => {
                let total = (b - a) * (w1 + w2) / 2.;
                (total, a * total + (b - a).powi(2) * (w1 + 2. * w2) / 6.)
            }
        };
        sum[0] += force;
        for k in 0..3 {
            sum[k + 1] += force * start[k] + first * (end[k] - start[k]) / length;
        }
    }
    sum
}

fn set_plate_section(model: &mut Model, wall: bool) {
    let plate_id = sepika_core::ids::SectionId(2);
    let shape = if wall {
        sepika_core::section_shape::SectionShape::RcWall {
            thickness: 100.,
            ps: 0.002,
            pwh_ratio: None,
            pwv_ratio: None,
        }
    } else {
        sepika_core::section_shape::SectionShape::RcSlab { thickness: 100. }
    };
    let mut section = shape.to_section(plate_id, "100mm版".into());
    let mut plate_material = model.materials[0].clone();
    plate_material.id = sepika_core::ids::MaterialId(1);
    plate_material.name = "板材".into();
    plate_material.density = 7.85e-9;
    model.materials.push(plate_material);
    section.material = Some(sepika_core::ids::MaterialId(1));
    section.shape = None;
    model.sections.push(section);
    if wall {
        for plate in &mut model.wall_plates {
            plate.section = Some(plate_id);
        }
    } else {
        model.stories.push(Story::new(
            sepika_core::ids::StoryId(0),
            "基準階".into(),
            Some(0.),
            Some(0.),
        ));
        model.stories.push(sepika_core::model::Story::new(
            sepika_core::ids::StoryId(1),
            "上階".into(),
            Some(3000.),
            Some(0.),
        ));
        for slab in &mut model.slabs {
            slab.reference_story = Some(sepika_core::ids::StoryId(0));
            slab.vertical_eccentricity_mm = Some(0.);
            slab.plate.section = Some(plate_id);
        }
    }
}

#[test]
fn 明示beam_fiber_ms混在は準備後も同じ版荷重と合力一次モーメントを持つ() {
    for wall in [false, true] {
        let mut reference_loads = None;
        let mut reference_prepared_loads = None;
        for kinds in fixture::representations() {
            let mut model = fixture::with_plate(wall, kinds);
            fixture::assert_geometry(&model, wall);
            set_plate_section(&mut model, wall);
            let before = (
                model.floor_assignment_regions.clone(),
                model.wall_assignment_regions.clone(),
            );
            let direct = if wall {
                sepika_load::wall_plate_load::distribute_enclosed_wall_plates(&model)
                    .unwrap()
                    .primary
            } else {
                sepika_load::slab_load::slab_beam_loads_with(
                    &model,
                    |_| 0.01035,
                    false,
                    &sepika_load::slab_load::beam_elem_map(&model),
                )
                .unwrap()
            };
            let (nodal, member) = sepika_load::slab_load::slab_load_case_content(&model, &direct);
            let expected = if wall {
                [124_200., 248_400_000., 0., 0.]
            } else {
                [165_600., 331_200_000., 331_200_000., 0.]
            };
            for (actual, expected) in resultant(&model, &nodal, &member).into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "wall={wall}, kinds={kinds:?}: {actual} != {expected}"
                );
            }
            if let Some((ref_nodes, ref_members)) = &reference_loads {
                assert_eq!(&nodal, ref_nodes);
                assert_eq!(&member, ref_members);
            } else {
                reference_loads = Some((nodal, member));
            }
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            model.validate_member_load_extents().unwrap();
            fixture::assert_geometry(&model, wall);
            assert_eq!(
                (
                    model.floor_assignment_regions.clone(),
                    model.wall_assignment_regions.clone()
                ),
                before
            );
            let dl = model
                .load_cases
                .iter()
                .find(|case| case.name == DL_CASE_NAME)
                .unwrap();
            if let Some((ref_nodes, ref_members)) = &reference_prepared_loads {
                assert_eq!(&dl.nodal, ref_nodes);
                assert_eq!(&dl.member, ref_members);
            } else {
                reference_prepared_loads = Some((dl.nodal.clone(), dl.member.clone()));
            }
            for (actual, expected) in resultant(&model, &dl.nodal, &dl.member)
                .into_iter()
                .zip(expected)
            {
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "準備 wall={wall}, kinds={kinds:?}: {actual} != {expected}"
                );
            }
        }
    }
}

#[test]
fn 中間アンカーの両端支持二次材と二分割版は全解析表現で準備後も同じ支持と荷重を保持する() {
    use sepika_core::ids::*;
    for wall in [false, true] {
        let mut reference_loads = None;
        for kinds in fixture::representations() {
            let mut model = fixture::with_plate(wall, kinds);
            let mut slabs = model.slabs.clone();
            let mut walls = model.wall_plates.clone();
            model.slabs.clear();
            model.wall_plates.clear();
            for region in &mut model.floor_assignment_regions.regions {
                region.assignment = PlateAssignment::Unset;
            }
            for region in &mut model.wall_assignment_regions.regions {
                region.assignment = PlateAssignment::Unset;
            }
            let anchors = [
                SecondaryMemberAnchor {
                    support: SupportMemberId::Primary(ElemId(0)),
                    position: 0.5,
                },
                SecondaryMemberAnchor {
                    support: SupportMemberId::Primary(ElemId(2)),
                    position: 0.5,
                },
            ];
            let sm = SecondaryMember {
                id: SecondaryMemberId(0),
                vertical_eccentricity: Some(0.0),
                kind: if wall {
                    SecondaryMemberKind::Post
                } else {
                    SecondaryMemberKind::Beam
                },
                ends: SecondaryMemberEnds::Supported(anchors),
                section: Some(SectionId(if wall { 1 } else { 0 })),
                ..Default::default()
            };
            if wall {
                model.unassigned_posts.push(sm);
            } else {
                model.unassigned_beams.push(sm);
            }
            model.rebuild_assignment_regions().unwrap();
            if wall {
                let mut second = walls[0].clone();
                second.id = WallPlateId(1);
                walls.push(second);
                model.wall_plates = walls;
                for (i, region) in model.wall_assignment_regions.regions.iter_mut().enumerate() {
                    region.assignment = PlateAssignment::Plate(WallPlateId(i as u32));
                }
            } else {
                let mut second = slabs[0].clone();
                second.id = SlabId(1);
                slabs.push(second);
                model.slabs = slabs;
                for (i, region) in model
                    .floor_assignment_regions
                    .regions
                    .iter_mut()
                    .enumerate()
                {
                    region.assignment = PlateAssignment::Plate(SlabId(i as u32));
                }
            }
            set_plate_section(&mut model, wall);
            let before = (
                model.floor_assignment_regions.clone(),
                model.wall_assignment_regions.clone(),
            );
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            model.validate_member_load_extents().unwrap();
            assert_eq!(
                model.secondary_member(SecondaryMemberId(0)).unwrap().ends,
                SecondaryMemberEnds::Supported(anchors)
            );
            assert_eq!(
                (
                    model.floor_assignment_regions.clone(),
                    model.wall_assignment_regions.clone()
                ),
                before
            );
            assert_eq!(
                if wall {
                    model.wall_assignment_regions.regions.len()
                } else {
                    model.floor_assignment_regions.regions.len()
                },
                2
            );
            let dl = model
                .load_cases
                .iter()
                .find(|case| case.name == DL_CASE_NAME)
                .unwrap();
            let expected = if wall {
                [124_200., 248_400_000., 0., 0.]
            } else {
                [165_600., 331_200_000., 331_200_000., 0.]
            };
            for (actual, expected) in resultant(&model, &dl.nodal, &dl.member)
                .into_iter()
                .zip(expected)
            {
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "wall={wall}, kinds={kinds:?}: {actual} != {expected}"
                );
            }
            if let Some((ref_nodes, ref_members)) = &reference_loads {
                assert_eq!(&dl.nodal, ref_nodes);
                assert_eq!(&dl.member, ref_members);
            } else {
                reference_loads = Some((dl.nodal.clone(), dl.member.clone()));
            }
        }
    }
}

fn attached_floor(split: bool, presplit: bool) -> Model {
    use sepika_core::ids::*;
    let mut model = fixture::rectangle(false, [ElementKind::Beam; 4]);
    model.nodes.truncate(2);
    model.elements.truncate(1);
    model.elements[0].local_axis.ref_vector = [0., 0., 1.];
    let points = if split {
        vec![[2000., 0., 0.], [2000., 800., 0.], [2000., 1500., 0.]]
    } else {
        vec![[2000., 0., 0.], [2000., 1500., 0.]]
    };
    for coord in points {
        let mut node = model.nodes[0].clone();
        node.id = NodeId(model.nodes.len() as u32);
        node.coord = coord;
        model.nodes.push(node);
    }
    for nodes in if split {
        vec![[2, 3], [3, 4]]
    } else {
        vec![[2, 3]]
    } {
        let mut element = model.elements[0].clone();
        element.id = ElemId(model.elements.len() as u32);
        element.nodes = nodes.map(NodeId).into_iter().collect();
        model.elements.push(element);
    }
    model.load_cases = default_load_cases();
    for (i, span) in if presplit {
        vec![[0., 0.5], [0.5, 1.]]
    } else {
        vec![[0., 1.]]
    }
    .into_iter()
    .enumerate()
    {
        model.slabs.push(Slab {
            id: SlabId(i as u32),
            shape: SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span,
                    transfer: LoadTransfer::Anchor,
                },
                extent: [1500.; 2],
            },
            plate: SlabPlate {
                loads: vec![AreaLoad {
                    kind: "仕上げ".into(),
                    value: 0.0025,
                }],
                ..Default::default()
            },
            reference_story: None,
            vertical_eccentricity_mm: None,
            tip_loads: vec![SlabTipLoad {
                case: LoadCaseId(0),
                intensity: 2.,
                direction: TipLoadDirection::NegZ,
            }],
        });
    }
    set_plate_section(&mut model, false);
    model
}

fn assert_attached_floor(model: &Model, original: &Slab, divided: bool) {
    use sepika_core::ids::*;
    let spans = if divided {
        vec![[0., 0.5], [0.5, 1.]]
    } else {
        vec![[0., 1.]]
    };
    assert_eq!(model.slabs.len(), spans.len());
    let mut total_area = 0.;
    for (slab, span) in model.slabs.iter().zip(spans) {
        assert_eq!(
            slab.shape,
            SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span,
                    transfer: LoadTransfer::Anchor,
                },
                extent: [1500.; 2],
            }
        );
        assert_eq!(slab.plate, original.plate);
        assert_eq!(slab.tip_loads, original.tip_loads);
        let area = sepika_core::geom::polygon::area_xy(&slab.boundary_coords(model).unwrap());
        assert_eq!(area, if divided { 3_000_000. } else { 6_000_000. });
        total_area += area;
        let supports = model.attached_slab_supports(slab).unwrap();
        assert_eq!(supports.len(), 1);
        assert_eq!(supports[0].elem, ElemId(0));
        assert_eq!(supports[0].span, span);
    }
    assert_eq!(total_area, 6_000_000.);
}

fn assert_attached_floor_loads(model: &Model) {
    let dl = model
        .load_cases
        .iter()
        .find(|case| case.name == DL_CASE_NAME)
        .unwrap();
    for (source, expected_force) in [(LoadSource::Auto, 62_100.), (LoadSource::SlabTip, 8_000.)] {
        let nodal: Vec<_> = dl
            .nodal
            .iter()
            .filter(|load| load.source == source)
            .cloned()
            .collect();
        let member: Vec<_> = dl
            .member
            .iter()
            .filter(|load| load.source == source)
            .cloned()
            .collect();
        assert!(member
            .iter()
            .all(|load| load.elem == sepika_core::ids::ElemId(0)));
        let expected = [expected_force, expected_force * 2000., 0., 0.];
        for (actual, expected) in resultant(model, &nodal, &member).into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1e-6,
                "{source:?}: {actual} != {expected}"
            );
        }
    }
    for (actual, expected) in
        resultant(model, &dl.nodal, &dl.member)
            .into_iter()
            .zip([70_100., 140_200_000., 0., 0.])
    {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }
}

#[test]
fn 取付き床の全長連結混在支持は分割と統合阻止と独立荷重を保持する() {
    use ElementKind::*;
    for (split, kinds) in [
        (false, vec![Beam]),
        (false, vec![Fiber]),
        (false, vec![MultiSpring]),
        (true, vec![Beam; 2]),
        (true, vec![Fiber; 2]),
        (true, vec![MultiSpring; 2]),
        (true, vec![Beam, Fiber]),
        (true, vec![Fiber, MultiSpring]),
        (true, vec![MultiSpring, Beam]),
    ] {
        for presplit in [false, true] {
            let mut model = attached_floor(split, presplit);
            for (element, kind) in model.elements.iter_mut().skip(1).zip(&kinds) {
                element.kind = *kind;
            }
            let original = model.slabs[0].clone();
            model.validate().unwrap();
            let input_slabs = model.slabs.clone();
            sepika_core::region_rebuild::rebuild_floor_regions(&mut model);
            assert_attached_floor(&model, &original, true);
            if presplit {
                assert_eq!(model.slabs, input_slabs, "既存の版IDと境界を保持する");
            } else {
                assert_eq!(model.slabs[0].id, original.id);
                assert_ne!(model.slabs[1].id, original.id);
            }
            let rebuilt = model.slabs.clone();
            sepika_core::region_rebuild::rebuild_floor_regions(&mut model);
            assert_eq!(model.slabs, rebuilt, "kinds={kinds:?}, presplit={presplit}");
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            model.validate().unwrap();
            assert_attached_floor(&model, &original, true);
            assert_eq!(model.slabs, rebuilt);
            assert_attached_floor_loads(&model);
            let first = (model.slabs.clone(), model.load_cases.clone());
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            assert_eq!((model.slabs.clone(), model.load_cases.clone()), first);
            assert_attached_floor_loads(&model);
        }
    }
}

#[test]
fn 取付き床の斜め未到達異レベルとパネルは分割支持にならない() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for end in [[2500., 1500., 0.], [2000., 1000., 0.], [2000., 1500., 100.]] {
            let mut model = attached_floor(false, false);
            model.elements[1].kind = kind;
            if end[2] != 0. {
                model.nodes[2].coord[2] = end[2];
            }
            model.nodes[3].coord = end;
            let original = model.slabs[0].clone();
            model.validate().unwrap();
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            model.validate().unwrap();
            assert_attached_floor(&model, &original, false);
            assert_attached_floor_loads(&model);
        }
    }
    let mut model = attached_floor(false, false);
    model.elements[1].kind = ElementKind::PanelZone;
    for (coord, column) in [([2000., -1000., 0.], false), ([2000., 0., -1000.], true)] {
        let mut far = model.nodes[0].clone();
        far.id = sepika_core::ids::NodeId(model.nodes.len() as u32);
        far.coord = coord;
        let mut member = model.elements[0].clone();
        member.id = sepika_core::ids::ElemId(model.elements.len() as u32);
        member.nodes = if column {
            [far.id, sepika_core::ids::NodeId(2)]
        } else {
            [sepika_core::ids::NodeId(2), far.id]
        }
        .into_iter()
        .collect();
        member.section = Some(sepika_core::ids::SectionId(u32::from(column)));
        if column {
            member.local_axis.ref_vector = [0., 1., 0.];
        }
        model.nodes.push(far);
        model.elements.push(member);
    }
    model.elements[2].rigid_zone.panel_offset_i = 200.;
    model.elements[3].rigid_zone.panel_offset_j = 200.;
    assert!(sepika_core::panel_zone::resolve_explicit_panel_joint(
        &model,
        sepika_core::ids::NodeId(2),
        &model.elements
    )
    .unwrap()
    .is_some());
    let original = model.slabs[0].clone();
    model.validate().unwrap();
    prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
    assert_attached_floor(&model, &original, false);
    assert_attached_floor_loads(&model);
}

#[test]
fn 取付き床のbrace用途不整合とwall準備拒否は正式受理対照に数えない() {
    let mut brace = attached_floor(false, false);
    brace.elements[1].kind = ElementKind::Brace {
        tension_only: false,
    };
    assert!(brace.validate().is_err());
    let mut wall = attached_floor(false, false);
    wall.elements[1].kind = ElementKind::Wall;
    assert!(prepare_model_for_analysis(&mut wall, &AnalysisSettings::default(), None).is_err());
}
