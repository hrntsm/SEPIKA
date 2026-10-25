use sepika_core::{dof::Dof6Mask, ids::*, model::*, section_shape::*};
use sepika_load::self_weight::{beam_upstand_segments, self_weight_case_content};

fn model() -> Model {
    let shape = SectionShape::RcBeamRect {
        b: 400.,
        d: 600.,
        rebar: RcBeamRebar {
            main_dia: 25.,
            top: vec![3],
            bottom: vec![3],
            cover: 50.,
            stirrup: BeamStirrup {
                dia: 10.,
                pitch: 150.,
                legs: 2,
            },
        },
    };
    let mut beam = shape.to_section(SectionId(0), "梁".into());
    beam.frame_use = Some(FrameSectionUse::Girder);
    beam.material = Some(MaterialId(0));
    let mut slab = SectionShape::RcSlab { thickness: 150. }.to_section(SectionId(1), "床".into());
    slab.material = Some(MaterialId(1));
    let mut material = Material {
        id: MaterialId(0),
        name: "梁".into(),
        category: MaterialCategory::Concrete,
        young: 24000.,
        poisson: 0.2,
        density: 0.,
        shear: None,
        fc: Some(24.),
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    };
    let mut body = material.clone();
    body.density = 24e-6 / sepika_core::units::GRAVITY_MM_S2;
    material.id = MaterialId(1);
    material.name = "床".into();
    material.density = 24e-6 / sepika_core::units::GRAVITY_MM_S2;
    let mut model = Model {
        sections: vec![beam, slab],
        materials: vec![body, material],
        stories: vec![
            Story::new(StoryId(0), "基部".into(), Some(-3500.), Some(0.)),
            Story::new(StoryId(1), "床".into(), Some(0.), Some(-500.)),
        ],
        ..Default::default()
    };
    for (i, [x, y]) in [
        [0., 0.],
        [6000., 0.],
        [6000., 2000.],
        [0., 2000.],
        [6000., -2000.],
        [0., -2000.],
        [0., 0.],
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(i as u32),
            coord: [x, y, if i == 6 { -3500. } else { -500. }],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for (i, (a, b)) in [(0, 1), (1, 2), (2, 3), (3, 0), (1, 4), (4, 5), (5, 0)]
        .into_iter()
        .enumerate()
    {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: ElementKind::Beam,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0., 0., 1.],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.),
            plastic_zone: None,
            spring: None,
        });
    }
    for nodes in [[0, 1, 2, 3], [0, 5, 4, 1]] {
        model.add_enclosed_slab_from_nodes(
            &nodes.map(NodeId),
            SlabPlate {
                section: Some(SectionId(1)),
                ..Default::default()
            },
        );
    }
    for slab in &mut model.slabs {
        slab.reference_story = Some(StoryId(1));
        slab.vertical_eccentricity_mm = Some(0.);
    }
    model
}

fn weight(model: &Model) -> f64 {
    beam_upstand_segments(model, SupportMemberId::Primary(ElemId(0)))
        .unwrap()
        .iter()
        .map(|s| s.design_weight_n)
        .sum()
}

fn steel_shape(model: &mut Model, shape: SectionShape) {
    model.sections[0] = shape.to_section(SectionId(0), "S梁".into());
    model.sections[0].frame_use = Some(FrameSectionUse::Girder);
    model.sections[0].material = Some(MaterialId(0));
    model.materials[0].category = MaterialCategory::Steel;
    model.materials[0].fc = None;
    model.materials[0].density = 7.85e-9;
}

fn reverse_target(model: &mut Model) {
    model.elements[0].nodes.reverse();
    for region in &mut model.floor_assignment_regions.regions {
        for edge in &mut region.boundary {
            if edge.support == SupportMemberId::Primary(ElemId(0)) {
                edge.span = edge.span.map(|t| 1. - t);
            }
        }
    }
}

fn target_dl_increment(model: &Model) -> f64 {
    let total = |m: &Model| {
        let (_, loads) = self_weight_case_content(m, &LoadCfg::default()).unwrap();
        loads
            .iter()
            .filter(|load| load.elem == ElemId(0))
            .map(|load| match load.kind {
                MemberLoadKind::Distributed { a, b, w1, w2 } => (b - a) * (w1 + w2) / 2.,
                MemberLoadKind::Point { p, .. } => p,
            })
            .sum::<f64>()
    };
    let mut bare = model.clone();
    for region in &mut bare.floor_assignment_regions.regions {
        region.assignment = PlateAssignment::NoPlate;
    }
    bare.slabs.clear();
    bare.floor_regions.clear();
    total(model) - total(&bare)
}

fn assert_dynamic_increment(
    model: &Model,
    baseline: &Model,
    design_n: f64,
    physical_n: f64,
    mass_t: f64,
) {
    let sync = |source: &Model| {
        let mut m = source.clone();
        let (mut nodal, mut member) = self_weight_case_content(&m, &LoadCfg::default()).unwrap();
        let loads = sepika_load::slab_load::slab_beam_loads_with(
            &m,
            |s| m.slab_dead_intensity(s),
            true,
            &sepika_load::slab_load::beam_elem_map(&m),
        )
        .unwrap();
        let (sn, sm) = sepika_load::slab_load::slab_load_case_content(&m, &loads);
        nodal.extend(sn);
        member.extend(sm);
        m.load_cases = vec![LoadCase {
            id: LoadCaseId(0),
            kind: LoadCaseKind::Dead,
            name: "DL".into(),
            nodal,
            member,
        }];
        m
    };
    for method in [MassMethod::CorrectedLumped, MassMethod::LumpedOnly] {
        for synced in [false, true] {
            let generate = |m: &Model| {
                if synced {
                    sepika_load::story_gen::generate_stories_with_synced_self_weight(
                        &sync(m),
                        &[LoadCaseId(0)],
                        method,
                    )
                    .unwrap()
                } else {
                    sepika_load::story_gen::generate_stories_with_opts(m, &[], true, method)
                        .unwrap()
                }
            };
            let result = generate(model);
            let base = generate(baseline);
            let design = |r: &sepika_load::story_gen::StoryGenResult| {
                r.calculated_weights.iter().map(|(_, v)| v).sum::<f64>()
            };
            let physical = |r: &sepika_load::story_gen::StoryGenResult| {
                r.stories
                    .iter()
                    .filter_map(|s| s.dynamic_mass)
                    .map(|s| s.mass_equiv_weight_n)
                    .sum::<f64>()
            };
            let lumped = |r: &sepika_load::story_gen::StoryGenResult| {
                r.rep_nodes
                    .iter()
                    .filter_map(|n| n.mass)
                    .map(|m| m[0])
                    .sum::<f64>()
            };
            assert!(
                (design(&result) - design(&base) - design_n).abs() < 1e-6,
                "{method:?} synced={synced}"
            );
            assert!(
                (physical(&result) - physical(&base) - physical_n).abs() < 1e-6,
                "{method:?} synced={synced}"
            );
            assert!(
                (lumped(&result) - lumped(&base) - mass_t).abs() < 1e-9,
                "{method:?} synced={synced}"
            );
        }
    }
}

#[test]
fn 実高さと左右材料から独立期待重量を得る() {
    let mut m = model();
    assert!((weight(&m) - 2880.).abs() < 1e-7);
    m.slabs[0].vertical_eccentricity_mm = Some(50.);
    assert!((weight(&m) - 4320.).abs() < 1e-7);
    let mut material = m.materials[1].clone();
    material.id = MaterialId(2);
    material.density = 18e-6 / sepika_core::units::GRAVITY_MM_S2;
    m.materials.push(material);
    let mut section = m.sections[1].clone();
    section.id = SectionId(2);
    section.material = Some(MaterialId(2));
    m.sections.push(section);
    m.slabs[1].plate.section = Some(SectionId(2));
    assert!((weight(&m) - 3960.).abs() < 1e-7);
    m.slabs[1].vertical_eccentricity_mm = Some(-50.);
    assert!((weight(&m) - 2880.).abs() < 1e-7);
    m.floor_assignment_regions.regions[1].assignment = PlateAssignment::NoPlate;
    assert!((weight(&m) - 5760.).abs() < 1e-7);
    m.floor_assignment_regions.regions[1].assignment = PlateAssignment::Unset;
    assert!(
        beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0)))
            .unwrap_err()
            .contains("未設定")
    );
}

#[test]
fn 鋼材床の設計重量と物理質量を独立にdlと層質量へ渡す() {
    let mut m = model();
    m.materials[1].category = MaterialCategory::Steel;
    m.materials[1].fc = None;
    m.materials[1].density = 7.85e-9;
    m.slabs[0].vertical_eccentricity_mm = Some(50.);
    m.floor_assignment_regions.regions[1].assignment = PlateAssignment::NoPlate;
    m.slabs.pop();
    let segments = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap();
    assert!((segments.iter().map(|s| s.volume_mm3).sum::<f64>() - 240e6).abs() < 1e-7);
    assert!((segments.iter().map(|s| s.design_weight_n).sum::<f64>() - 18840.).abs() < 1e-7);
    assert!((segments.iter().map(|s| s.physical_mass_t).sum::<f64>() - 1.884).abs() < 1e-12);
    assert!(
        (segments
            .iter()
            .map(|s| s.mass_equivalent_weight_n)
            .sum::<f64>()
            - 18475.7286)
            .abs()
            < 1e-7
    );
    let (_, loads) = self_weight_case_content(&m, &LoadCfg::default()).unwrap();
    let total: f64 = loads
        .iter()
        .filter(|l| l.elem == ElemId(0))
        .map(|l| match l.kind {
            MemberLoadKind::Distributed { a, b, w1, w2 } => (b - a) * (w1 + w2) / 2.,
            _ => 0.,
        })
        .sum();
    assert!((total - 53400.).abs() < 1e-7);
    let mut baseline = m.clone();
    baseline.elements[0].vertical_eccentricity = Some(100.);
    assert_dynamic_increment(&m, &baseline, 18840., 18475.7286, 1.884);
}

#[test]
fn 右前半床だけの区間分布と反力を保持し控除ゼロの部分床をdlへ渡す() {
    let mut m = model();
    m.slabs[0].vertical_eccentricity_mm = Some(50.);
    let mut material = m.materials[1].clone();
    material.id = MaterialId(2);
    material.density = 18e-6 / sepika_core::units::GRAVITY_MM_S2;
    m.materials.push(material);
    let mut section = m.sections[1].clone();
    section.id = SectionId(2);
    section.material = Some(MaterialId(2));
    m.sections.push(section);
    m.slabs[1].plate.section = Some(SectionId(2));
    for (id, coord) in [(7, [3000., 0., -500.]), (8, [3000., -2000., -500.])] {
        let mut node = m.nodes[0].clone();
        node.id = NodeId(id);
        node.coord = coord;
        m.nodes.push(node);
    }
    let mut cross = m.elements[0].clone();
    cross.id = ElemId(7);
    cross.nodes = [NodeId(7), NodeId(8)].into_iter().collect();
    m.elements.push(cross);
    m.floor_assignment_regions.regions[1].boundary = vec![
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(6)),
            span: [1., 0.],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(5)),
            span: [1., 0.5],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(7)),
            span: [1., 0.],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(0)),
            span: [0.5, 0.],
        },
    ];
    assert!((weight(&m) - 4860.).abs() < 1e-7);
    let segments = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap();
    let first: f64 = segments
        .iter()
        .map(|s| match s.design_load {
            MemberLoadKind::Distributed { a, b, w1, w2 } => {
                a * (b - a) * (w1 + w2) / 2. + (b - a).powi(2) * (w1 + 2. * w2) / 6.
            }
            _ => 0.,
        })
        .sum();
    assert!((first / 6000. - 2655.).abs() < 1e-7);
    let (_, loads) = self_weight_case_content(&m, &LoadCfg::default()).unwrap();
    let target: f64 = loads
        .iter()
        .filter(|s| s.elem == ElemId(0))
        .map(|s| match s.kind {
            MemberLoadKind::Distributed { a, b, w1, w2 } => (b - a) * (w1 + w2) / 2.,
            _ => 0.,
        })
        .sum();
    assert!((target - 39420.).abs() < 1e-7);
}

#[test]
fn 床が接しても材料参照切れと不正密度を拒否し密度ゼロを許容する() {
    let mut m = model();
    for slab in &mut m.slabs {
        slab.vertical_eccentricity_mm = Some(-50.);
    }
    assert_eq!(weight(&m), 0.);
    for density in [f64::NAN, f64::INFINITY, -1.] {
        m.materials[1].density = density;
        let error = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap_err();
        assert!(error.contains("材料 1") && error.contains("床板"));
    }
    m.materials[1].density = 0.;
    assert_eq!(weight(&m), 0.);
    m.sections[1].material = Some(MaterialId(99));
    assert!(beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).is_err());
}

#[test]
fn 二次小梁は内法区間の嵩上げを設計荷重と支持反力へ一度だけ渡す() {
    let mut m = model();
    m.elements.remove(0);
    for (i, e) in m.elements.iter_mut().enumerate() {
        e.id = ElemId(i as u32);
    }
    for region in &mut m.floor_assignment_regions.regions {
        for edge in &mut region.boundary {
            if let SupportMemberId::Primary(id) = edge.support {
                edge.support = if id.0 == 0 {
                    SupportMemberId::Secondary(SecondaryMemberId(0))
                } else {
                    SupportMemberId::Primary(ElemId(id.0 - 1))
                };
            }
        }
    }
    m.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        section: Some(SectionId(0)),
        vertical_eccentricity: Some(0.),
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(2)),
                position: 1.,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(0)),
                position: 0.,
            },
        ]),
        ..Default::default()
    });
    let pieces =
        beam_upstand_segments(&m, SupportMemberId::Secondary(SecondaryMemberId(0))).unwrap();
    assert!((pieces.iter().map(|s| s.design_weight_n).sum::<f64>() - 2688.).abs() < 1e-7);
    assert!(pieces.iter().all(|s| s.interval_mm == [200., 5800.]));
    let transfer = sepika_load::cascade::solve(&m, |_| 0., true).unwrap();
    let beam = &transfer.members[&SecondaryMemberId(0)];
    assert!((beam.reactions.iter().sum::<f64>() - 34944.).abs() < 1e-7);
    assert_eq!(beam.member_loads.len(), 3);
    let mut baseline = m.clone();
    baseline.unassigned_beams[0].vertical_eccentricity = Some(50.);
    let base = sepika_load::cascade::solve(&baseline, |_| 0., true).unwrap();
    assert!(
        (beam.reactions.iter().sum::<f64>()
            - base.members[&SecondaryMemberId(0)]
                .reactions
                .iter()
                .sum::<f64>()
            - 2688.)
            .abs()
            < 1e-7
    );
    m.materials[1].category = MaterialCategory::Steel;
    m.materials[1].fc = None;
    m.materials[1].density = 7.85e-9;
    let mut baseline = m.clone();
    baseline.unassigned_beams[0].vertical_eccentricity = Some(50.);
    assert_dynamic_increment(&m, &baseline, 8792., 8622.00668, 0.8792);
}

#[test]
fn 実部材化済み小梁は主架構でのみ嵩上げを持つ() {
    let mut m = model();
    m.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        section: Some(SectionId(0)),
        vertical_eccentricity: Some(0.),
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(3)),
                position: 1.,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(1)),
                position: 0.,
            },
        ]),
        ..Default::default()
    });
    assert!(m.secondary_member_materialized(&m.unassigned_beams[0]));
    assert!(
        beam_upstand_segments(&m, SupportMemberId::Secondary(SecondaryMemberId(0)))
            .unwrap()
            .is_empty()
    );
    assert!((weight(&m) - 2880.).abs() < 1e-7);
    let transfer = sepika_load::cascade::solve(&m, |_| 0., true).unwrap();
    assert!(!transfer.members.contains_key(&SecondaryMemberId(0)));
}

#[test]
fn stb床板材料を断面材料より優先し採用元の変更と密度変更に追随する() {
    let mut m = model();
    m.floor_assignment_regions.regions[1].assignment = PlateAssignment::NoPlate;
    m.slabs.pop();
    let mut alternate = m.materials[1].clone();
    alternate.id = MaterialId(2);
    alternate.density = 18e-6 / sepika_core::units::GRAVITY_MM_S2;
    m.materials.push(alternate);
    m.stb_strengths.members.push(StbMemberStrength {
        target: StrengthTarget::Slab(SlabId(0)),
        node: NodeId(0),
        node_order: vec![],
        concrete: Some("Fc24".into()),
    });
    m.stb_strengths.materials.push(StbGradeMaterial {
        grade: "Fc24".into(),
        material: MaterialId(2),
    });
    assert!((weight(&m) - 2160.).abs() < 1e-7);
    assert!(
        beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0)))
            .unwrap()
            .iter()
            .all(|s| s.material == MaterialId(2))
    );
    m.materials[2].density = 12e-6 / sepika_core::units::GRAVITY_MM_S2;
    assert!((weight(&m) - 1440.).abs() < 1e-7);
    m.stb_strengths.materials[0].material = MaterialId(1);
    assert!((weight(&m) - 2880.).abs() < 1e-7);
    m.stb_strengths.materials[0].material = MaterialId(99);
    assert!(beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).is_err());
}

#[test]
fn 斜梁は実上面の鉛直空きと水平投影長で線形100からゼロを積分する() {
    let mut m = model();
    m.floor_assignment_regions.regions[1].assignment = PlateAssignment::NoPlate;
    m.slabs.pop();
    let plan_length = 6000. / 0.9;
    let cosine = 1. / (1f64 + 1. / 3600.).sqrt();
    let start_x = plan_length * 0.05;
    let z = |x: f64| -250. - 300. / cosine + (x - start_x) / 60.;
    for node in &mut m.nodes[..6] {
        if node.coord[0] > 0. {
            node.coord[0] = plan_length;
        }
        node.coord[2] = z(node.coord[0]);
    }
    for (id, x, y) in [
        (7, start_x, 0.),
        (8, start_x, 2000.),
        (9, plan_length * 0.95, 0.),
        (10, plan_length * 0.95, 2000.),
    ] {
        let mut node = m.nodes[0].clone();
        node.id = NodeId(id);
        node.coord = [x, y, z(x)];
        m.nodes.push(node);
    }
    for (id, nodes) in [(7, [7, 8]), (8, [9, 10])] {
        let mut e = m.elements[0].clone();
        e.id = ElemId(id);
        e.nodes = nodes.map(NodeId).into_iter().collect();
        m.elements.push(e);
    }
    m.floor_assignment_regions.regions[0].boundary = vec![
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(0)),
            span: [0.05, 0.95],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(8)),
            span: [0., 1.],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(2)),
            span: [0.05, 0.95],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(7)),
            span: [1., 0.],
        },
    ];
    let pieces = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap();
    assert!((pieces.iter().map(|s| s.design_weight_n).sum::<f64>() - 2880.).abs() < 1e-6);
    assert!((pieces[0].gap_mm[0] - 100.).abs() < 1e-9);
    assert!(pieces.last().unwrap().gap_mm[1].abs() < 1e-9);
    let (_, loads) = self_weight_case_content(&m, &LoadCfg::default()).unwrap();
    assert!(loads.iter().any(|l|l.elem==ElemId(0) && matches!(l.kind,MemberLoadKind::Distributed{a,b,w1,w2} if a>0. && b< m.member_length(&m.elements[0]) && w1>w2)));
    let crossing_cosine = 1. / (1f64 + 1. / 900.).sqrt();
    for (i, node) in m.nodes.iter_mut().enumerate() {
        if i != 6 {
            node.coord[2] +=
                (node.coord[0] - start_x) / 60. + 300. / cosine - 300. / crossing_cosine;
        }
    }
    let crossing = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap();
    assert!((crossing.iter().map(|s| s.design_weight_n).sum::<f64>() - 1440.).abs() < 1e-6);
    assert!(crossing.iter().any(|s| s.volume_mm3 < 1e-3));
    assert!(crossing
        .iter()
        .any(|s| s.gap_mm[0] > 99. && s.gap_mm[1] < 1e-9));
}

#[test]
fn 左右交換と重複候補は総量を変えず同側の異床板候補を拒否する() {
    let mut m = model();
    m.slabs[0].vertical_eccentricity_mm = Some(50.);
    let original = weight(&m);
    for n in &mut m.nodes {
        n.coord[1] = -n.coord[1];
    }
    assert!((weight(&m) - original).abs() < 1e-7);
    m.floor_assignment_regions
        .regions
        .push(m.floor_assignment_regions.regions[0].clone());
    assert!((weight(&m) - original).abs() < 1e-7);
    let mut slab = m.slabs[0].clone();
    slab.id = SlabId(2);
    m.slabs.push(slab);
    let last = m.floor_assignment_regions.regions.last_mut().unwrap();
    last.assignment = PlateAssignment::Plate(SlabId(2));
    let error = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap_err();
    assert!(error.contains("床板 0 と 2") && error.contains("区間"));
    let dl_error = self_weight_case_content(&m, &LoadCfg::default()).unwrap_err();
    assert!(dl_error.contains("矛盾") && dl_error.contains("0") && dl_error.contains("2"));
    m.floor_assignment_regions.regions.pop();
    m.floor_assignment_regions.regions[1].assignment = PlateAssignment::Plate(SlabId(0));
    assert!((weight(&m) - 5760.).abs() < 1e-7);
}

#[test]
fn 線取付き床板も既存支持区間の空きを加算し床板本体を重ねない() {
    let mut m = model();
    for r in &mut m.floor_assignment_regions.regions {
        r.assignment = PlateAssignment::NoPlate;
    }
    m.slabs.truncate(1);
    m.slabs[0].shape = SlabShape::Attached {
        anchor: RegionAnchor::Line {
            nodes: [NodeId(0), NodeId(1)],
            span: [0., 1.],
            transfer: LoadTransfer::Anchor,
        },
        extent: [2000., 2000.],
    };
    assert!((weight(&m) - 2880.).abs() < 1e-7);
    let (_, loads) = self_weight_case_content(&m, &LoadCfg::default()).unwrap();
    let target: f64 = loads
        .iter()
        .filter(|s| s.elem == ElemId(0))
        .map(|s| match s.kind {
            MemberLoadKind::Distributed { a, b, w1, w2 } => (b - a) * (w1 + w2) / 2.,
            _ => 0.,
        })
        .sum();
    assert!((target - 37440.).abs() < 1e-7);
}

#[test]
fn 同じ実配置の対応梁材種と要素表現で嵩上げ増分を変えない() {
    let original = model();
    for shape in [
        SectionShape::SteelBox {
            height: 600.,
            width: 400.,
            thick: 10.,
            corner_r: Some(0.),
        },
        SectionShape::SteelH {
            height: 600.,
            width: 400.,
            web_thick: 10.,
            flange_thick: 20.,
            root_r: Some(0.),
        },
    ] {
        let mut m = original.clone();
        m.sections[0] = shape.to_section(SectionId(0), "梁".into());
        m.sections[0].material = Some(MaterialId(0));
        m.sections[0].frame_use = Some(FrameSectionUse::Girder);
        m.materials[0].category = MaterialCategory::Steel;
        m.materials[0].fc = None;
        m.materials[0].density = 7.85e-9;
        m.load_cfg = Some(LoadCfg {
            steel_weight_factor: 1.3,
            ..Default::default()
        });
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            m.elements[0].kind = kind;
            assert!((weight(&m) - 2880.).abs() < 1e-7);
        }
    }
}

#[test]
fn 柱控除の異なる材種は既存区間を保持し共通床区間では同じ増分となる() {
    let mut rc = model();
    for slab in &mut rc.slabs {
        slab.vertical_eccentricity_mm = Some(50.);
    }
    let mut column = SectionShape::RcColumnRect {
        b: 600.,
        d: 600.,
        rebar: RcRectColumnRebar {
            main_dia: 22.,
            x: vec![],
            y: vec![],
            cover: 40.,
            hoop: RectColumnHoop {
                dia: 10.,
                pitch: 100.,
                legs_x: 2,
                legs_y: 2,
            },
        },
    }
    .to_section(SectionId(2), "柱".into());
    column.frame_use = Some(FrameSectionUse::Column);
    column.material = Some(MaterialId(0));
    rc.sections.push(column);
    let mut bottom = rc.nodes[6].clone();
    bottom.id = NodeId(7);
    bottom.coord[0] = 6000.;
    rc.nodes.push(bottom);
    for (id, nodes) in [(7, [6, 0]), (8, [7, 1])] {
        let mut e = rc.elements[0].clone();
        e.id = ElemId(id);
        e.nodes = nodes.map(NodeId).into_iter().collect();
        e.section = Some(SectionId(2));
        e.local_axis.ref_vector = [1., 0., 0.];
        rc.elements.push(e);
    }
    let as_steel = |source: &Model| {
        let mut m = source.clone();
        let mut material = m.materials[0].clone();
        material.id = MaterialId(2);
        material.category = MaterialCategory::Steel;
        material.fc = None;
        material.density = 7.85e-9;
        m.materials.push(material);
        m.sections[0] = SectionShape::SteelH {
            height: 600.,
            width: 400.,
            web_thick: 10.,
            flange_thick: 20.,
            root_r: Some(0.),
        }
        .to_section(SectionId(0), "S梁".into());
        m.sections[0].frame_use = Some(FrameSectionUse::Girder);
        m.sections[0].material = Some(MaterialId(2));
        m
    };
    assert!((weight(&rc) - 5184.).abs() < 1e-7);
    assert!((weight(&as_steel(&rc)) - 5760.).abs() < 1e-7);
    rc.floor_assignment_regions.regions[1].assignment = PlateAssignment::NoPlate;
    rc.slabs.pop();
    for (id, coord) in [
        (8, [300., 0., -500.]),
        (9, [300., 2000., -500.]),
        (10, [5700., 0., -500.]),
        (11, [5700., 2000., -500.]),
    ] {
        let mut n = rc.nodes[0].clone();
        n.id = NodeId(id);
        n.coord = coord;
        rc.nodes.push(n);
    }
    for (id, nodes) in [(9, [8, 9]), (10, [10, 11])] {
        let mut e = rc.elements[0].clone();
        e.id = ElemId(id);
        e.nodes = nodes.map(NodeId).into_iter().collect();
        rc.elements.push(e);
    }
    rc.floor_assignment_regions.regions[0].boundary = vec![
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(0)),
            span: [0.05, 0.95],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(10)),
            span: [0., 1.],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(2)),
            span: [0.05, 0.95],
        },
        SupportBoundary {
            support: SupportMemberId::Primary(ElemId(9)),
            span: [1., 0.],
        },
    ];
    assert!((weight(&rc) - 5184.).abs() < 1e-7);
    assert!((weight(&as_steel(&rc)) - 5184.).abs() < 1e-7);
}

#[test]
fn 円管と回転矩形は中心線上の実上端と水平投影幅をdlへ用い端順交換で総量を保つ() {
    for (shape, reference, expected_width, expected_gap, expected_weight) in [
        (
            SectionShape::SteelPipe {
                outer_dia: 600.,
                thick: 20.,
            },
            [0., 0., 1.],
            600.,
            50.,
            4320.,
        ),
        (
            SectionShape::SteelPipe {
                outer_dia: 600.,
                thick: 20.,
            },
            [0., 1., 1.],
            600.,
            50.,
            4320.,
        ),
        (
            SectionShape::SteelFlatBar {
                width: 400.,
                thick: 600.,
            },
            [0., 1., 1.],
            707.1067811865476,
            67.15728752538098,
            6838.181771801398,
        ),
    ] {
        let mut m = model();
        steel_shape(&mut m, shape);
        m.elements[0].local_axis.ref_vector = reference;
        for _ in 0..2 {
            let pieces = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap();
            assert!(
                (pieces.iter().map(|s| s.design_weight_n).sum::<f64>() - expected_weight).abs()
                    < 1e-6
            );
            assert!((target_dl_increment(&m) - expected_weight).abs() < 1e-6);
            assert!(pieces
                .iter()
                .all(|s| (s.width_mm - expected_width / 2.).abs() < 1e-9
                    && s.gap_mm.iter().all(|h| (*h - expected_gap).abs() < 1e-9)));
            reverse_target(&mut m);
        }
    }
    let mut rc = model();
    rc.elements[0].local_axis.ref_vector = [0., 1., 1.];
    for slab in &mut rc.slabs {
        slab.vertical_eccentricity_mm = Some(150.);
    }
    for _ in 0..2 {
        assert!((weight(&rc) - 22111.68824543142).abs() < 1e-6);
        assert!((target_dl_increment(&rc) - 22111.68824543142).abs() < 1e-6);
        reverse_target(&mut rc);
    }
}

#[test]
fn 斜中空梁の端capで上側管壁が消える不連続を左右極限の別区間で保持する() {
    for (shape, expected_weight) in [
        (
            SectionShape::SteelBox {
                height: 600.,
                width: 400.,
                thick: 20.,
                corner_r: Some(0.),
            },
            166770.72,
        ),
        (
            SectionShape::SteelPipe {
                outer_dia: 600.,
                thick: 20.,
            },
            250156.08,
        ),
    ] {
        let mut m = model();
        steel_shape(&mut m, shape);
        m.nodes[1].coord[2] += 4500.;
        for slab in &mut m.slabs {
            slab.vertical_eccentricity_mm = Some(5150.);
        }
        let pieces = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap();
        assert!(
            (pieces.iter().map(|s| s.design_weight_n).sum::<f64>() - expected_weight).abs() < 1e-6
        );
        assert!((target_dl_increment(&m) - expected_weight).abs() < 1e-6);
        let before = pieces
            .iter()
            .find(|s| (s.interval_mm[0] - 7275.).abs() < 1e-8)
            .unwrap();
        let after = pieces
            .iter()
            .find(|s| (s.interval_mm[0] - 7290.).abs() < 1e-8)
            .unwrap();
        assert!((before.gap_mm[0] - 760.).abs() < 1e-8 && (before.gap_mm[1] - 776.).abs() < 1e-8);
        assert!((after.gap_mm[0] - 1476.).abs() < 1e-8 && (after.gap_mm[1] - 1350.).abs() < 1e-8);
        reverse_target(&mut m);
        let reversed = beam_upstand_segments(&m, SupportMemberId::Primary(ElemId(0))).unwrap();
        assert!(
            (reversed.iter().map(|s| s.design_weight_n).sum::<f64>() - expected_weight).abs()
                < 1e-6
        );
        assert!((target_dl_increment(&m) - expected_weight).abs() < 1e-6);
        for original in &pieces {
            let reflected=reversed.iter().find(|s|s.slab==original.slab && (s.interval_mm[0]-(7500.-original.interval_mm[1])).abs()<1e-8 && (s.interval_mm[1]-(7500.-original.interval_mm[0])).abs()<1e-8).unwrap();
            assert!((reflected.gap_mm[0]-original.gap_mm[1]).abs()<1e-8);
            assert!((reflected.gap_mm[1]-original.gap_mm[0]).abs()<1e-8);
        }
    }
}
