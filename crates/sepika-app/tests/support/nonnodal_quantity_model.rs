use sepika_core::{ids::*, model::*, section_shape::*};

pub fn model() -> Model {
    let mut beam = SectionShape::RcBeamRect {
        b: 300.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: 0.0,
            top: vec![],
            bottom: vec![],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 0.0,
                pitch: 0.0,
                legs: 0,
            },
        },
    }
    .to_section(SectionId(0), "B300x600".into());
    beam.material = Some(MaterialId(0));
    let mut steel = SectionShape::SteelFlatBar {
        width: 100.0,
        thick: 100.0,
    }
    .to_section(SectionId(1), "支持鋼材".into());
    steel.material = Some(MaterialId(1));
    steel.frame_use = Some(FrameSectionUse::Girder);
    Model {
        nodes: [
            [0.0, 0.0, 3000.0],
            [3000.0, 0.0, 3000.0],
            [3000.0, 3000.0, 3000.0],
            [0.0, 3000.0, 3000.0],
        ]
        .into_iter()
        .enumerate()
        .map(|(id, coord)| Node {
            id: NodeId(id as u32),
            coord,
            restraint: Default::default(),
            mass: None,
            story: Some(StoryId(0)),
            support_spring: None,
        })
        .collect(),
        elements: [[0, 1], [2, 3], [1, 2], [3, 0]]
            .into_iter()
            .enumerate()
            .map(|(id, ns)| ElementData {
                id: ElemId(id as u32),
                kind: ElementKind::Beam,
                nodes: ns.into_iter().map(NodeId).collect(),
                section: Some(SectionId(1)),
                local_axis: LocalAxis {
                    ref_vector: [0.0, 0.0, 1.0],
                },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                plastic_zone: None,
                spring: None,
            })
            .collect(),
        sections: vec![beam, steel],
        materials: vec![
            Material {
                id: MaterialId(0),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 22700.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
                concrete_class: Default::default(),
                strength_factor: None,
            },
            Material {
                id: MaterialId(1),
                name: "SS400".into(),
                category: MaterialCategory::Steel,
                young: 205000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(235.0),
                concrete_class: Default::default(),
                strength_factor: None,
            },
        ],
        stories: vec![Story {
            id: StoryId(0),
            name: "1F".into(),
            elevation: Some(3000.0),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: (0..4).map(NodeId).collect(),
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
            wall_weights: vec![],
        }],
        floor_regions: vec![FloorRegion::new(
            FloorRegionId(0),
            (0..4).map(NodeId).collect(),
        )],
        ..Default::default()
    }
}

pub fn ends(position: f64) -> SecondaryMemberEnds {
    SecondaryMemberEnds::Supported([
        SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(0)),
            position: 0.5,
        },
        SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(1)),
            position,
        },
    ])
}

pub fn placed() -> Model {
    let mut model = model();
    model.floor_regions[0]
        .secondary_beams
        .push(SecondaryMember {
            id: SecondaryMemberId(557),
            kind: SecondaryMemberKind::Beam,
            section: Some(SectionId(0)),
            ends: ends(0.5),
            name: "非節点小梁".into(),
            gravity_end_shares: None,
        });
    model.next_secondary_member_id = 558;
    model
}

pub fn assert_quantity(model: &Model, id: SecondaryMemberId, volume_m3: f64, formwork_m2: f64) {
    let q = sepika_design_jp::quantity::try_compute_quantity_takeoff(model, &model.quantity_cfg)
        .unwrap();
    let rows: Vec<_> = q
        .items
        .iter()
        .filter(|item| item.secondary == Some(id))
        .collect();
    assert_eq!(rows.len(), 1);
    let item = rows[0];
    assert_eq!(item.elem, None);
    assert_eq!(item.slab, None);
    assert_eq!(item.story, "1F");
    assert!(
        (item.concrete_m3 - volume_m3).abs() < 1e-12,
        "{}",
        item.concrete_m3
    );
    assert!(
        (item.formwork_m2 - formwork_m2).abs() < 1e-12,
        "{}",
        item.formwork_m2
    );
}

pub fn edit_model() -> Model {
    let mut model = model();
    model.elements.truncate(2);
    model.floor_regions.clear();
    model
}

pub fn moved_ends() -> SecondaryMemberEnds {
    SecondaryMemberEnds::Supported([
        SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(0)),
            position: 0.0,
        },
        SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(1)),
            position: 0.25,
        },
    ])
}

pub fn with_stb_strengths(mut model: Model, id: SecondaryMemberId) -> Model {
    model.stb_strengths.sections.push(StbSectionStrength {
        section: SectionId(0),
        concrete: Some("Fc24".into()),
        native_material: None,
        native_rebar: false,
        rebar: None,
        reinforcement: vec![],
        steel: vec![],
    });
    model.stb_strengths.members.push(StbMemberStrength {
        target: StrengthTarget::Secondary(id),
        node: NodeId(0),
        node_order: vec![],
        concrete: Some("Fc30".into()),
    });
    model.prepare_stb_strength_materials();
    model
}
