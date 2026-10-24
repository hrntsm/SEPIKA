use std::collections::HashSet;

use sepika_core::{ids::*, model::*};
use sepika_job::{prepare::prepare_model_for_analysis, settings::AnalysisSettings};
use sepika_load::{floor, slab_load};

fn representations() -> [ElementKind; 3] {
    [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ]
}

fn anchors() -> SecondaryMemberEnds {
    SecondaryMemberEnds::Supported([
        SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(1)),
            position: 0.,
        },
        SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(2)),
            position: 0.,
        },
    ])
}

fn parallel_beam_model(kind: ElementKind) -> Model {
    let mut model = Model {
        panel_zone: PanelZoneMode::None,
        ..Default::default()
    };
    model.materials.push(Material {
        id: MaterialId(0),
        name: "自重なし鋼材".into(),
        category: MaterialCategory::Steel,
        young: 205_000.,
        poisson: 0.3,
        density: 0.,
        shear: None,
        fc: None,
        fy: Some(235.),
        concrete_class: Default::default(),
        strength_factor: None,
    });
    let mut section = sepika_core::section_shape::SectionShape::SteelH {
        height: 400.,
        width: 200.,
        web_thick: 8.,
        flange_thick: 13.,
        root_r: Some(0.),
    }
    .to_section(SectionId(0), "H400".into());
    section.frame_use = Some(FrameSectionUse::Girder);
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    let mut plate_material = model.materials[0].clone();
    plate_material.id = MaterialId(1);
    plate_material.name = "板材".into();
    plate_material.density = 7.85e-9;
    model.materials.push(plate_material);
    let mut plate_section = sepika_core::section_shape::SectionShape::RcSlab { thickness: 100. }
        .to_section(SectionId(1), "100mm版".into());
    plate_section.shape = None;
    plate_section.material = Some(MaterialId(1));
    model.sections.push(plate_section);
    // 最初の近傍節点を選ぶ実部材化判定で、5mm離れた下梁端点を拾わせない。
    for (i, coord) in [
        [0., 5., 0.],
        [4000., 5., 0.],
        [0., 0., 0.],
        [4000., 0., 0.],
        [4000., 4000., 0.],
        [0., 4000., 0.],
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
    for (i, (a, b)) in [(2, 3), (0, 5), (1, 4), (3, 1), (2, 0), (5, 4)]
        .into_iter()
        .enumerate()
    {
        model.elements.push(ElementData {
            id: ElemId(i as u32),
            kind: if i == 0 { kind } else { ElementKind::Beam },
            nodes: vec![NodeId(a), NodeId(b)].into(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0., 0., 1.],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        });
    }
    model.unassigned_beams.push(SecondaryMember {
        id: SecondaryMemberId(0),
        kind: SecondaryMemberKind::Beam,
        ends: anchors(),
        section: Some(SectionId(0)),
        ..Default::default()
    });
    model
        .stories
        .push(Story::new(StoryId(0), "基準階".into(), Some(0.), Some(0.)));
    model
        .stories
        .push(Story::new(StoryId(1), "上階".into(), Some(3000.), Some(0.)));
    model.rebuild_assignment_regions().unwrap();
    model.slabs.push(Slab {
        id: SlabId(0),
        shape: SlabShape::Enclosed,
        plate: SlabPlate {
            section: Some(SectionId(1)),
            method: DistributionMethod::OneWay,
            one_way: Some(OneWayDir::Y),
            ..Default::default()
        },
        reference_story: Some(StoryId(0)),
        vertical_eccentricity_mm: Some(0.),
        tip_loads: vec![],
    });
    assert_eq!(model.floor_assignment_regions.regions.len(), 2);
    for region in &mut model.floor_assignment_regions.regions {
        region.assignment = if region
            .boundary
            .iter()
            .any(|edge| edge.support == SupportMemberId::Primary(ElemId(0)))
        {
            PlateAssignment::Plate(SlabId(0))
        } else {
            PlateAssignment::NoPlate
        };
    }
    sepika_core::region_rebuild::rebuild_floor_regions(&mut model);
    model
}

fn assert_input_geometry(model: &Model, kind: ElementKind) {
    assert_eq!(model.elements[0].kind, kind);
    assert_eq!(model.nodes[0].coord, [0., 5., 0.]);
    assert_eq!(model.nodes[1].coord, [4000., 5., 0.]);
    assert_eq!(model.nodes[0].id, NodeId(0));
    assert_eq!(model.nodes[1].id, NodeId(1));
    let member = model.secondary_member(SecondaryMemberId(0)).unwrap();
    assert_eq!(member.ends, anchors());
    assert_eq!(member.kind, SecondaryMemberKind::Beam);
    assert_eq!(member.section, Some(SectionId(0)));
    assert_eq!(
        model.secondary_member_axis(member),
        Some(([0., 5., 0.], [4000., 5., 0.], 4000.))
    );
    assert!(!model.secondary_member_materialized(member));
    assert!(model.unassigned_beams.is_empty());
    assert_eq!(model.floor_regions.len(), 1);
    let owner = model.floor_region_of_beam(member.id).unwrap();
    assert_eq!(owner.id, FloorRegionId(0));
    assert_eq!(owner.slab_ids, vec![SlabId(0)]);
    assert_eq!(owner.secondary_beams.len(), 1);
    assert_eq!(model.floor_assignment_regions.regions.len(), 2);
    for region in &model.floor_assignment_regions.regions {
        let mut coords = model.floor_assignment_region_coords(region.id).unwrap();
        coords.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let (expected, area) = match region.assignment {
            PlateAssignment::Plate(SlabId(0)) => (
                vec![[0., 0., 0.], [0., 5., 0.], [4000., 0., 0.], [4000., 5., 0.]],
                20_000.,
            ),
            PlateAssignment::NoPlate => (
                vec![
                    [0., 5., 0.],
                    [0., 4000., 0.],
                    [4000., 5., 0.],
                    [4000., 4000., 0.],
                ],
                15_980_000.,
            ),
            _ => panic!("床板あり・なしの明示割当を保持する"),
        };
        assert_eq!(coords, expected);
        close(
            (coords[2][0] - coords[0][0]) * (coords[1][1] - coords[0][1]),
            area,
        );
    }
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

fn force_and_first(load: &MemberLoadKind) -> (f64, f64) {
    match *load {
        MemberLoadKind::Point { a, p } => (p, p * a),
        MemberLoadKind::Distributed { a, b, w1, w2 } => {
            let force = (b - a) * (w1 + w2) / 2.;
            (force, a * force + (b - a).powi(2) * (w1 + 2. * w2) / 6.)
        }
    }
}

fn resultant(model: &Model, nodal: &[NodalLoad], member: &[MemberLoad]) -> [f64; 3] {
    let mut result = [0.; 3];
    for load in nodal {
        let point = model.node(load.node).unwrap().coord;
        let force = -load.values[2];
        result[0] += force;
        result[1] += force * point[0];
        result[2] += force * point[1];
    }
    for load in member {
        assert_eq!(load.dir, [0., 0., -1.]);
        let element = model.element(load.elem).unwrap();
        let start = model.node(element.nodes[0]).unwrap().coord;
        let end = model.node(element.nodes[1]).unwrap().coord;
        let length = model.member_length(element);
        let (force, first) = force_and_first(&load.kind);
        result[0] += force;
        for k in 0..2 {
            result[k + 1] += force * start[k] + first * (end[k] - start[k]) / length;
        }
    }
    result
}

fn assert_supply_distribution(model: &Model) {
    let (split, leftover) = floor::secondary_beam_distribution_split(model, |_| 0.005).unwrap();
    assert_eq!(split.len(), 1);
    let entry = &split[&SecondaryMemberId(0)];
    assert_eq!(entry.rep_slab_id, Some(SlabId(0)));
    assert_eq!(entry.expected_slab_ids, HashSet::from([SlabId(0)]));
    assert_eq!(entry.contributed_slab_ids, HashSet::from([SlabId(0)]));
    assert_eq!(entry.member_loads.len(), 1);
    assert_eq!(
        entry.member_loads[0],
        MemberLoadKind::Distributed {
            a: 0.,
            b: 4000.,
            w1: 0.0125,
            w2: 0.0125
        }
    );
    close(
        entry
            .member_loads
            .iter()
            .map(|load| force_and_first(load).0)
            .sum(),
        50.,
    );
    close(
        floor::covered_length_of_loads(&entry.member_loads, 4000.),
        4000.,
    );
    assert!(floor::beam_distribution_is_ready(entry, 4000.));
    assert_eq!(
        floor::secondary_beam_distribution_gaps(model).unwrap(),
        Default::default()
    );
    assert_eq!(leftover.len(), 1);
    let (nodes, t) = match leftover[0].target {
        floor::LoadTarget::Span { nodes, t } => (nodes, t),
        other => panic!("主架構の下辺へ残るはず: {other:?}"),
    };
    assert!(nodes == [NodeId(2), NodeId(3)] || nodes == [NodeId(3), NodeId(2)]);
    assert_eq!(t, [0., 1.]);
    assert!(
        matches!(leftover[0].shape, floor::LoadShape::Uniform { w } if (w - 0.0125).abs() < 1e-12)
    );
    let (nodes, members) = slab_load::slab_load_case_content(model, &leftover);
    assert!(nodes.is_empty());
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].elem, ElemId(0));
    close(force_and_first(&members[0].kind).0, 50.);
    let loads =
        slab_load::slab_beam_loads_with(model, |_| 0.005, false, &slab_load::beam_elem_map(model))
            .unwrap();
    let (nodal, member) = slab_load::slab_load_case_content(model, &loads);
    assert_primary_loads(model, &nodal, &member, 50., 25., [100., 200_000., 250.]);
}

fn assert_primary_loads(
    model: &Model,
    nodal: &[NodalLoad],
    member: &[MemberLoad],
    bottom_force_n: f64,
    reaction_n: f64,
    expected: [f64; 3],
) {
    assert_eq!(nodal.len(), 2);
    for id in [NodeId(0), NodeId(1)] {
        let load = nodal.iter().find(|load| load.node == id).unwrap();
        close(-load.values[2], reaction_n);
        for k in [0, 1, 3, 4, 5] {
            close(load.values[k], 0.);
        }
    }
    assert_eq!(member.len(), 1);
    assert_eq!(member[0].elem, ElemId(0));
    close(force_and_first(&member[0].kind).0, bottom_force_n);
    for (actual, expected) in resultant(model, nodal, member).into_iter().zip(expected) {
        close(actual, expected);
    }
}

#[test]
fn supported並走小梁は主架構表現だけを変えても所属と供給荷重の位置一次を保持する() {
    let reference = bincode::serialize(&parallel_beam_model(ElementKind::Beam)).unwrap();
    for kind in representations() {
        let mut model = parallel_beam_model(kind);
        let mut normalized = model.clone();
        normalized.elements[0].kind = ElementKind::Beam;
        assert_eq!(bincode::serialize(&normalized).unwrap(), reference);
        model.validate().unwrap();
        assert_input_geometry(&model, kind);
        assert_supply_distribution(&model);
        let before = (
            model.floor_assignment_regions.clone(),
            model.floor_regions.clone(),
            model.slabs.clone(),
        );
        for _ in 0..2 {
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            model.validate().unwrap();
            assert_input_geometry(&model, kind);
            assert_supply_distribution(&model);
            assert_eq!(
                (
                    model.floor_assignment_regions.clone(),
                    model.floor_regions.clone(),
                    model.slabs.clone()
                ),
                before
            );
        }
    }
}

#[test]
fn 百mm版の実準備dlは供給分配とは別に総百五十七nと端反力を保持する() {
    for kind in representations() {
        let mut model = parallel_beam_model(kind);
        model.validate().unwrap();
        close(
            model.slab_self_weight_intensity(&model.slabs[0]).unwrap(),
            0.00785,
        );
        close(model.slabs[0].plate.finish_intensity(), 0.);
        assert_eq!(
            floor::beam_self_weight_udl(
                &model,
                model.secondary_member(SecondaryMemberId(0)).unwrap()
            )
            .unwrap(),
            None
        );
        let mut previous = None;
        for _ in 0..2 {
            prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
            model.validate().unwrap();
            assert_input_geometry(&model, kind);
            let dl = model
                .load_cases
                .iter()
                .find(|case| case.name == DL_CASE_NAME)
                .unwrap();
            // 設計重量 78.5kN/m³、板厚100mm、面積20000mm²なので157N。
            assert_primary_loads(
                &model,
                &dl.nodal,
                &dl.member,
                78.5,
                39.25,
                [157., 314_000., 392.5],
            );
            if let Some(previous) = &previous {
                assert_eq!(&(dl.nodal.clone(), dl.member.clone()), previous);
            }
            previous = Some((dl.nodal.clone(), dl.member.clone()));
        }
    }
}

#[test]
fn 両側床版なしのsupported小梁は全表現で分配なし理由を保持する() {
    for kind in representations() {
        let mut model = parallel_beam_model(kind);
        model.slabs.clear();
        for region in &mut model.floor_assignment_regions.regions {
            region.assignment = PlateAssignment::NoPlate;
        }
        sepika_core::region_rebuild::rebuild_floor_regions(&mut model);
        model.validate().unwrap();
        prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
        model.validate().unwrap();
        let (split, leftover) =
            floor::secondary_beam_distribution_split(&model, |_| 0.005).unwrap();
        assert!(split.is_empty());
        assert!(leftover.is_empty());
        assert_eq!(
            floor::secondary_beam_distribution_gaps(&model).unwrap(),
            floor::SecondaryBeamDistributionGaps {
                missing_expected_slabs: 0,
                short_cover: 0,
                no_distribution: 1
            }
        );
    }
}
