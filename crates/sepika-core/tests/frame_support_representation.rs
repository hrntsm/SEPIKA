#[path = "support/frame_support_model.rs"]
mod fixture;
use sepika_core::model::*;

#[test]
fn 明示解析表現を変えても床壁の物理境界と割当支持idを保持する() {
    for wall in [false, true] {
        let reference = fixture::with_plate(wall, [ElementKind::Beam; 4]);
        for kinds in fixture::representations() {
            let mut model = fixture::with_plate(wall, kinds);
            fixture::assert_geometry(&model, wall);
            assert_eq!(
                model.floor_assignment_regions,
                reference.floor_assignment_regions
            );
            assert_eq!(
                model.wall_assignment_regions,
                reference.wall_assignment_regions
            );
            model.rebuild_assignment_regions().unwrap();
            fixture::assert_geometry(&model, wall);
        }
    }
}

#[test]
fn ブレース壁パネルは床壁の閉領域を支持しない() {
    for wall in [false, true] {
        for excluded in [
            ElementKind::Brace {
                tension_only: false,
            },
            ElementKind::Wall,
            ElementKind::PanelZone,
        ] {
            let mut extra = fixture::with_plate(wall, [ElementKind::Fiber; 4]);
            let mut diagonal = extra.elements[0].clone();
            diagonal.id = sepika_core::ids::ElemId(4);
            diagonal.kind = excluded;
            diagonal.section = None;
            diagonal.nodes = [sepika_core::ids::NodeId(0), sepika_core::ids::NodeId(2)]
                .into_iter()
                .collect();
            extra.elements.push(diagonal);
            extra.rebuild_assignment_regions().unwrap();
            fixture::assert_geometry(&extra, wall);
            let mut model = fixture::rectangle(wall, [ElementKind::Fiber; 4]);
            model.elements[1].kind = excluded;
            assert!(
                sepika_core::region_gen::floor::scan_region_boundaries(&model)
                    .boundaries
                    .is_empty()
            );
            assert!(
                sepika_core::region_gen::wall::scan_wall_region_boundaries(&model)
                    .boundaries
                    .is_empty()
            );
            model.rebuild_assignment_regions().unwrap();
            assert!(model.floor_assignment_regions.regions.is_empty());
            assert!(model.wall_assignment_regions.regions.is_empty());
            let mut assigned = fixture::with_plate(wall, [ElementKind::Fiber; 4]);
            assigned.elements[1].kind = excluded;
            assert!(if wall {
                assigned.wall_assignment_regions.validate(&assigned)
            } else {
                assigned.floor_assignment_regions.validate(&assigned)
            }
            .is_err());
        }
    }
}

#[test]
fn 床同一レベルと壁同一面と二節点制限は全表現で維持する() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut floor = fixture::rectangle(false, [kind; 4]);
        floor.nodes[1].coord[2] = 1000.;
        assert!(
            sepika_core::region_gen::floor::scan_region_boundaries(&floor)
                .boundaries
                .is_empty()
        );
        floor.rebuild_assignment_regions().unwrap();
        assert!(floor.floor_assignment_regions.regions.is_empty());
        let mut wall = fixture::rectangle(true, [kind; 4]);
        wall.nodes[2].coord[1] = 1000.;
        assert!(
            sepika_core::region_gen::wall::scan_wall_region_boundaries(&wall)
                .boundaries
                .is_empty()
        );
        wall.rebuild_assignment_regions().unwrap();
        assert!(wall.wall_assignment_regions.regions.is_empty());
        for vertical in [false, true] {
            let mut model = fixture::with_plate(vertical, [kind; 4]);
            model.elements[0].nodes.push(sepika_core::ids::NodeId(2));
            assert!(if vertical {
                model.wall_assignment_regions.validate(&model)
            } else {
                model.floor_assignment_regions.validate(&model)
            }
            .is_err());
        }
    }
}

#[test]
fn 未割当断面でも幾何列挙に新しい用途必須条件を加えない() {
    for wall in [false, true] {
        for kinds in fixture::representations() {
            let mut model = fixture::with_plate(wall, kinds);
            for element in &mut model.elements {
                element.section = None;
            }
            model.rebuild_assignment_regions().unwrap();
            fixture::assert_geometry(&model, wall);
        }
    }
}

#[test]
fn fiber_ms主架構でも二次材の両端支持だけが割当領域を分割する() {
    use sepika_core::ids::*;
    for wall in [false, true] {
        for kinds in fixture::representations() {
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
            let height = if wall { 3000. } else { 4000. };
            let points = if wall {
                [[2000., 0., 0.], [2000., 0., height]]
            } else {
                [[2000., 0., 0.], [2000., height, 0.]]
            };
            for ends in [
                SecondaryMemberEnds::Supported(anchors),
                SecondaryMemberEnds::Detached(points),
                SecondaryMemberEnds::Cantilever {
                    support: anchors[0],
                    free_end_vector: [0., height],
                },
            ] {
                let mut model = fixture::rectangle(wall, kinds);
                let secondary = SecondaryMember {
                    id: SecondaryMemberId(0),
                    kind: if wall {
                        SecondaryMemberKind::Post
                    } else {
                        SecondaryMemberKind::Beam
                    },
                    ends,
                    ..Default::default()
                };
                if wall {
                    model.unassigned_posts.push(secondary);
                } else {
                    model.unassigned_beams.push(secondary);
                }
                model.rebuild_assignment_regions().unwrap();
                let expected = if matches!(ends, SecondaryMemberEnds::Supported(_)) {
                    2
                } else {
                    1
                };
                assert_eq!(
                    if wall {
                        model.wall_assignment_regions.regions.len()
                    } else {
                        model.floor_assignment_regions.regions.len()
                    },
                    expected
                );
            }
        }
    }
}

#[test]
fn 明示解析表現を変えても二次材の主架構支持アンカー再推定で両端支持を保持する() {
    use sepika_core::ids::*;
    for wall in [false, true] {
        for kinds in fixture::representations() {
            let mut model = fixture::rectangle(wall, kinds);
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
            let secondary = SecondaryMember {
                id: SecondaryMemberId(0),
                kind: if wall {
                    SecondaryMemberKind::Post
                } else {
                    SecondaryMemberKind::Beam
                },
                ends: SecondaryMemberEnds::Supported(anchors),
                ..Default::default()
            };
            if wall {
                model.unassigned_posts.push(secondary);
            } else {
                model.unassigned_beams.push(secondary);
            }
            let report = model.anchorize_secondary_members();
            assert!(
                report.unresolved.is_empty(),
                "wall={wall}, kinds={kinds:?}: {:?}",
                report.unresolved
            );
            let sm = model.secondary_member(SecondaryMemberId(0)).unwrap();
            assert_eq!(sm.ends, SecondaryMemberEnds::Supported(anchors));
            model.rebuild_assignment_regions().unwrap();
            assert_eq!(
                if wall {
                    model.wall_assignment_regions.regions.len()
                } else {
                    model.floor_assignment_regions.regions.len()
                },
                2
            );
        }
    }
}
