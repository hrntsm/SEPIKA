use sepika_core::{ids::*, model::*, section_shape::SectionShape};

fn enclosed() -> Model {
    let mut model = Model::default();
    model
        .stories
        .push(Story::new(StoryId(0), "2F".into(), Some(0.0), Some(-300.0)));
    model
        .sections
        .push(SectionShape::RcSlab { thickness: 150.0 }.to_section(SectionId(0), "S15".into()));
    for (i, coord) in [
        [0., 0., -300.],
        [4000., 0., -300.],
        [4000., 3000., -300.],
        [0., 3000., -300.],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord,
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    model.add_enclosed_slab_from_nodes(
        &[NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        SlabPlate {
            section: Some(SectionId(0)),
            ..Default::default()
        },
    );
    model.slabs[0].reference_story = Some(StoryId(0));
    model.slabs[0].vertical_eccentricity_mm = Some(0.0);
    model
}

#[test]
fn fl基準の上面と実厚から下面を解決する() {
    let mut model = enclosed();
    for (eccentricity, top, bottom) in [(0., 0., -150.), (100., 100., -50.), (-100., -100., -250.)]
    {
        model.slabs[0].vertical_eccentricity_mm = Some(eccentricity);
        let placement = model.resolved_slab_placement(SlabId(0)).unwrap();
        assert_eq!(
            (
                placement.top_z_mm,
                placement.bottom_z_mm,
                placement.thickness_mm
            ),
            (top, bottom, 150.)
        );
        assert!(placement.top_boundary.iter().all(|p| p[2] == top));
    }
    model.slabs[0].vertical_eccentricity_mm = Some(100.);
    model.stories[0].elevation = Some(200.);
    let placement = model.resolved_slab_placement(SlabId(0)).unwrap();
    assert_eq!((placement.top_z_mm, placement.bottom_z_mm), (300., 150.));
    model.sections[0].thickness = Some(200.);
    let placement = model.resolved_slab_placement(SlabId(0)).unwrap();
    assert_eq!((placement.top_z_mm, placement.bottom_z_mm), (300., 100.));
}

#[test]
fn 支持節点z梁偏心梁せいslは床板の実配置と支持関係を変えない() {
    let mut model = enclosed();
    model
        .stories
        .push(Story::new(StoryId(1), "上階".into(), Some(3000.), Some(0.)));
    let beam_section = |height| {
        SectionShape::SteelH {
            height,
            width: 200.,
            web_thick: 8.,
            flange_thick: 13.,
            root_r: Some(0.),
        }
        .to_section(SectionId(1), "大梁".into())
    };
    model.sections.push(beam_section(400.));
    model.elements[0].section = Some(SectionId(1));
    let expected = model.resolved_slab_placement(SlabId(0)).unwrap();
    for step in 0..4 {
        match step {
            0 => model.stories[0].sl_offset_mm = Some(-800.),
            1 => model.elements[0].vertical_eccentricity = Some(500.),
            2 => model.nodes[0].coord[2] = 5000.,
            _ => model.sections[1] = beam_section(800.),
        }
        model.rebuild_floor_assignment_regions();
        assert_eq!(model.resolved_slab_placement(SlabId(0)).unwrap(), expected);
    }
    model.nodes[0].coord[0] = 100.;
    assert_eq!(
        model
            .resolved_slab_placement(SlabId(0))
            .unwrap()
            .top_boundary[0],
        [100., 0., 0.]
    );
}

#[test]
fn 実配置の不足不正は床板idと項目を示して拒否する() {
    let baseline = enclosed();
    for (item, mutate) in [
        (
            "基準階",
            (|m: &mut Model| m.slabs[0].reference_story = None) as fn(&mut Model),
        ),
        ("基準階", |m| {
            m.slabs[0].reference_story = Some(StoryId(99))
        }),
        ("鉛直偏心", |m| {
            m.slabs[0].vertical_eccentricity_mm = None
        }),
        ("鉛直偏心", |m| {
            m.slabs[0].vertical_eccentricity_mm = Some(f64::NAN)
        }),
        ("FL", |m| m.stories[0].elevation = None),
        ("FL", |m| m.stories[0].elevation = Some(f64::INFINITY)),
        ("床断面", |m| m.slabs[0].plate.section = None),
        ("床断面", |m| {
            m.slabs[0].plate.section = Some(SectionId(99))
        }),
        ("実板厚", |m| m.sections[0].thickness = Some(0.)),
        ("実板厚", |m| m.sections[0].thickness = Some(-1.)),
        ("実板厚", |m| {
            m.sections[0].thickness = Some(f64::INFINITY)
        }),
        ("平面位置", |m| m.nodes[0].coord[0] = f64::NAN),
        ("上面・下面Z", |m| {
            m.stories[0].elevation = Some(f64::MAX);
            m.slabs[0].vertical_eccentricity_mm = Some(f64::MAX);
        }),
    ] {
        let mut model = baseline.clone();
        mutate(&mut model);
        let reason = model.validate_slab_placements().unwrap_err();
        assert!(
            reason.contains("床板 0") && reason.contains(item),
            "{reason}"
        );
        assert!(model.slabs[0].surface_coords(&model).is_none());
    }
    let mut model = baseline;
    model.slabs.clear();
    model.floor_assignment_regions.regions[0].assignment = PlateAssignment::NoPlate;
    assert_eq!(model.validate_slab_placements(), Ok(()));
    model.floor_assignment_regions.regions[0].assignment = PlateAssignment::Unset;
    assert!(model.unset_plate_assignment_warning().is_some());
}

#[test]
fn 表示座標は未変形上面に支持変位を一度加える() {
    let mut model = enclosed();
    model.slabs[0].vertical_eccentricity_mm = Some(100.);
    let slab = &model.slabs[0];
    let undeformed = slab
        .surface_coords_with(&model, |id| Some(model.node(id)?.coord))
        .unwrap();
    assert_eq!(undeformed[0], [0., 0., 100.]);
    for scale in [1., 10., -2.] {
        let points = slab
            .surface_coords_with(&model, |id| {
                let mut p = model.node(id)?.coord;
                p[0] += 2. * scale;
                p[2] -= 5. * scale;
                Some(p)
            })
            .unwrap();
        assert_eq!(points[0], [2. * scale, 0., 100. - 5. * scale]);
    }
}

#[test]
fn 支持節点のない小梁アンカー頂点にも変位を内挿する() {
    let mut model = enclosed();
    let beam = SecondaryMember {
        id: SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(0)),
                position: 0.5,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(2)),
                position: 0.5,
            },
        ]),
        ..Default::default()
    };
    model.unassigned_beams.push(beam);
    let region = &mut model.floor_assignment_regions.regions[0];
    region.boundary = vec![
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(0)),
            span: [0., 0.5],
        },
        SupportBoundary {
            support: SupportMemberId::Secondary(SecondaryMemberId(0)),
            span: [0., 1.],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(2)),
            span: [0.5, 1.],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(3)),
            span: [0., 1.],
        },
    ];
    let points = model.slabs[0]
        .surface_coords_with(&model, |id| {
            let mut p = model.node(id)?.coord;
            p[2] += if id == NodeId(1) { 20. } else { 0. };
            Some(p)
        })
        .unwrap();
    assert_eq!(points[1], [2000., 0., 10.]);
    assert_eq!(points[2], [2000., 3000., 0.]);
}

#[test]
fn 取付き床の不正区間はclampせず実配置と表示を拒否する() {
    let mut model = enclosed();
    model.floor_assignment_regions = Default::default();
    for span in [
        [-0.2, 0.5],
        [f64::NEG_INFINITY, 1.],
        [0., f64::INFINITY],
        [0.8, 0.2],
        [0., 0.],
    ] {
        model.slabs[0].shape = SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span,
                transfer: LoadTransfer::Anchor,
            },
            extent: [1000.; 2],
        };
        let reason = model.resolved_slab_placement(SlabId(0)).unwrap_err();
        assert!(
            reason.contains("床板 0") && reason.contains("span"),
            "{reason}"
        );
        assert!(model.slabs[0].surface_coords(&model).is_none());
        assert!(model.slabs[0]
            .surface_coords_with(&model, |id| Some(model.node(id)?.coord))
            .is_none());
    }
}
