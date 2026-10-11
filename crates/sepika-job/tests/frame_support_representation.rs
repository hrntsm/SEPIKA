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
        for slab in &mut model.slabs {
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
