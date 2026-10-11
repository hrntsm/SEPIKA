use sepika_core::ids::*;
use sepika_core::model::*;

pub fn representations() -> Vec<[ElementKind; 4]> {
    use ElementKind::*;
    vec![
        [Beam; 4],
        [Fiber; 4],
        [MultiSpring; 4],
        [Fiber, Beam, Beam, Beam],
        [MultiSpring, Beam, Beam, Beam],
        [Beam, Fiber, MultiSpring, Fiber],
        [Beam, Fiber, Beam, MultiSpring],
    ]
}

pub fn rectangle(wall: bool, kinds: [ElementKind; 4]) -> Model {
    let mut model = Model {
        panel_zone: PanelZoneMode::None,
        ..Default::default()
    };
    model.materials.push(Material {
        id: MaterialId(0),
        name: "自重なし鋼材".into(),
        category: MaterialCategory::Steel,
        young: 205000.,
        poisson: 0.3,
        density: 0.,
        shear: None,
        fc: None,
        fy: Some(235.),
        concrete_class: Default::default(),
        strength_factor: None,
    });
    for (i, usage) in [FrameSectionUse::Girder, FrameSectionUse::Column]
        .into_iter()
        .enumerate()
    {
        let mut section = sepika_core::section_shape::SectionShape::SteelH {
            height: 400.,
            width: 200.,
            web_thick: 8.,
            flange_thick: 13.,
            root_r: Some(0.),
        }
        .to_section(SectionId(i as u32), format!("H{i}"));
        section.frame_use = Some(usage);
        section.material = Some(MaterialId(0));
        model.sections.push(section);
    }
    let height = if wall { 3000. } else { 4000. };
    for (i, [x, y]) in [[0., 0.], [4000., 0.], [4000., height], [0., height]]
        .into_iter()
        .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord: if wall { [x, 0., y] } else { [x, y, 0.] },
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, [a, b]) in [[0, 1], [1, 2], [2, 3], [3, 0]].into_iter().enumerate() {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: kinds[i],
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: Some(SectionId(if wall && i % 2 == 1 { 1 } else { 0 })),
            local_axis: LocalAxis {
                ref_vector: [0., 1., 0.],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    model
}

pub fn with_plate(wall: bool, kinds: [ElementKind; 4]) -> Model {
    let mut model = rectangle(wall, kinds);
    model.rebuild_assignment_regions().unwrap();
    sepika_core::region_rebuild::rebuild_floor_regions(&mut model);
    sepika_core::wall_region_rebuild::rebuild_wall_regions(&mut model);
    let loads = vec![AreaLoad {
        kind: "仕上げ".into(),
        value: 0.0025,
    }];
    if wall {
        model.wall_plates.push(WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: None,
            opening_area: 0.,
            opening_weight: 0.,
            openings: vec![],
            loads,
            slit: Default::default(),
            self_weight_shares: vec![],
            dl_support: Some(WallDlSupport::LowerBeam),
        });
        model.wall_assignment_regions.regions[0].assignment =
            PlateAssignment::Plate(WallPlateId(0));
        model.wall_regions[0].wall_plate_ids = vec![WallPlateId(0)];
    } else {
        model.slabs.push(Slab {
            id: SlabId(0),
            shape: SlabShape::Enclosed,
            plate: SlabPlate {
                loads,
                ..Default::default()
            },
            tip_loads: vec![],
        });
        model.floor_assignment_regions.regions[0].assignment = PlateAssignment::Plate(SlabId(0));
        model.floor_regions[0].slab_ids = vec![SlabId(0)];
    }
    model
}

pub fn assert_geometry(model: &Model, wall: bool) {
    let (area, mut edges) = if wall {
        let scan = sepika_core::region_gen::wall::scan_wall_region_boundaries(model);
        assert_eq!(scan.boundaries.len(), 1);
        assert_eq!(model.wall_regions.len(), 1);
        (
            scan.boundaries[0].area(model),
            scan.boundaries[0].edges.clone(),
        )
    } else {
        let scan = sepika_core::region_gen::floor::scan_region_boundaries(model);
        assert_eq!(scan.boundaries.len(), 1);
        assert_eq!(model.floor_regions.len(), 1);
        (
            scan.boundaries[0].area(model),
            scan.boundaries[0].edges.clone(),
        )
    };
    assert_eq!(area, if wall { 12_000_000. } else { 16_000_000. });
    edges.sort();
    assert_eq!(edges, (0..4).map(ElemId).collect::<Vec<_>>());
    let boundary = if wall {
        assert_eq!(model.wall_assignment_regions.regions.len(), 1);
        assert_eq!(
            model.wall_assignment_regions.regions[0].assignment,
            PlateAssignment::Plate(WallPlateId(0))
        );
        &model.wall_assignment_regions.regions[0].boundary
    } else {
        assert_eq!(model.floor_assignment_regions.regions.len(), 1);
        assert_eq!(
            model.floor_assignment_regions.regions[0].assignment,
            PlateAssignment::Plate(SlabId(0))
        );
        &model.floor_assignment_regions.regions[0].boundary
    };
    let mut supports: Vec<_> = boundary.iter().map(|b| b.support).collect();
    supports.sort();
    assert_eq!(
        supports,
        (0..4)
            .map(|i| SupportMemberId::Primary(ElemId(i)))
            .collect::<Vec<_>>()
    );
    assert!(boundary
        .iter()
        .all(|b| b.span[0].min(b.span[1]) == 0. && b.span[0].max(b.span[1]) == 1.));
    model.validate_assignment_region_identity().unwrap();
    model.floor_assignment_regions.validate(model).unwrap();
    model.wall_assignment_regions.validate(model).unwrap();
}
