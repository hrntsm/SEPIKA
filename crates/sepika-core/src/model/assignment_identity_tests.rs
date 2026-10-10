use super::*;
use crate as core_fixture_api;
#[path = "../../tests/support/assignment_identity_model.rs"]
mod fixture;

fn boundary() -> Vec<SupportBoundary> {
    (0..4)
        .map(|id| SupportBoundary {
            support: SupportMemberId::Primary(ElemId(id)),
            span: [0., 1.],
        })
        .collect()
}

#[test]
fn 巡回逆順と符号付きゼロは同じ領域で隣接bitのspanは別領域() {
    for wall in [false, true] {
        let mut floor = FloorPlateAssignmentRegions::default();
        let mut walls = WallPlateAssignmentRegions::default();
        let mut variants = vec![boundary()];
        let mut cyclic = boundary();
        cyclic.rotate_left(2);
        variants.push(cyclic);
        variants.push(
            boundary()
                .into_iter()
                .rev()
                .map(|e| SupportBoundary {
                    span: [e.span[1], e.span[0]],
                    ..e
                })
                .collect(),
        );
        let mut zero = boundary();
        zero[0].span[0] = -0.;
        variants.push(zero);
        for (i, b) in variants.into_iter().enumerate() {
            let report = if wall {
                walls.replace_boundaries(vec![b], 0)
            } else {
                floor.replace_boundaries(vec![b], 0)
            };
            assert!(report.rejection.is_none());
            assert_eq!(report.created_unset, usize::from(i == 0));
            assert_eq!(report.preserved, usize::from(i > 0));
            if wall {
                assert_eq!(walls.regions[0].id, WallPlateAssignmentRegionId(0));
                walls.regions[0].assignment = PlateAssignment::NoPlate;
            } else {
                assert_eq!(floor.regions[0].id, FloorPlateAssignmentRegionId(0));
                floor.regions[0].assignment = PlateAssignment::NoPlate;
            }
        }
        let mut half = boundary();
        half[0].span[1] = 0.5;
        let mut adjacent = half.clone();
        adjacent[0].span[1] = 0.5000000000000001;
        for (i, b) in [half, adjacent].into_iter().enumerate() {
            let report = if wall {
                walls.replace_boundaries(vec![b], 0)
            } else {
                floor.replace_boundaries(vec![b], 0)
            };
            assert_eq!(
                (report.created_unset, report.preserved, report.removed),
                (1, 0, 1)
            );
            if wall {
                assert_eq!(
                    walls.regions[0].id,
                    WallPlateAssignmentRegionId(i as u32 + 1)
                );
                assert!(walls.regions[0].assignment.is_unset());
            } else {
                assert_eq!(
                    floor.regions[0].id,
                    FloorPlateAssignmentRegionId(i as u32 + 1)
                );
                assert!(floor.regions[0].assignment.is_unset());
            }
        }
    }
}

#[test]
fn 候補キー衝突と既存重複版は次idを含め原子的に拒否する() {
    let mut floor = FloorPlateAssignmentRegions::default();
    floor.replace_boundaries(vec![boundary()], 0);
    let before = floor.clone();
    let mut rotated = boundary();
    rotated.rotate_left(1);
    assert!(floor
        .replace_boundaries(vec![boundary(), rotated], 0)
        .rejection
        .unwrap()
        .contains("境界キー"));
    assert_eq!(floor, before);
    let mut other = boundary();
    for edge in &mut other {
        edge.support = SupportMemberId::Secondary(SecondaryMemberId(10));
    }
    floor.replace_boundaries(vec![boundary(), other.clone()], 0);
    for r in &mut floor.regions {
        r.assignment = PlateAssignment::Plate(SlabId(0));
    }
    let before = floor.clone();
    assert!(floor
        .replace_boundaries(vec![boundary()], 0)
        .rejection
        .unwrap()
        .contains("重複割当"));
    assert_eq!(floor, before);
    let mut walls = WallPlateAssignmentRegions::default();
    walls.replace_boundaries(vec![boundary(), other], 0);
    for r in &mut walls.regions {
        r.assignment = PlateAssignment::Plate(WallPlateId(0));
    }
    let before = walls.clone();
    assert!(walls
        .replace_boundaries(vec![], 0)
        .rejection
        .unwrap()
        .contains("重複割当"));
    assert_eq!(walls, before);
}

#[test]
fn 一般再構築は孤立版を診断し明示再構築だけが旧版を除去する() {
    for wall in [false, true] {
        let mut model = fixture::with_plate(wall);
        if wall {
            model.unassigned_posts.push(fixture::divider(true));
        } else {
            model.unassigned_beams.push(fixture::divider(false));
        }
        let before = model.clone();
        let reason = model.rebuild_assignment_regions().unwrap_err();
        assert!(reason.contains("孤立版"));
        assert!(reason.contains("0.0025"));
        assert!(reason.contains("1 件"));
        fixture::assert_inputs_eq(&model, &before);
        let report = model.rebuild_assignment_regions_dropping_orphan_plates();
        assert!(report.floor.rejection.is_none() && report.wall.rejection.is_none());
        assert_eq!(
            (report.removed_slabs, report.removed_wall_plates),
            if wall { (0, 1) } else { (1, 0) }
        );
        assert!(report.loss.description().contains("0.0025"));
        if wall {
            assert_eq!(
                model
                    .wall_assignment_regions
                    .regions
                    .iter()
                    .map(|r| r.id.0)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );
            assert!(model
                .wall_assignment_regions
                .regions
                .iter()
                .all(|r| r.assignment.is_unset()));
        } else {
            assert_eq!(
                model
                    .floor_assignment_regions
                    .regions
                    .iter()
                    .map(|r| r.id.0)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );
            assert!(model
                .floor_assignment_regions
                .regions
                .iter()
                .all(|r| r.assignment.is_unset()));
        }
    }
}

#[test]
fn 同じ支持の節点移動は版と荷重を保持して新しい幾何を返す() {
    for wall in [false, true] {
        let mut model = fixture::with_plate(wall);
        model.nodes[1].coord[0] = 5000.;
        model.nodes[2].coord[0] = 5000.;
        model.rebuild_assignment_regions().unwrap();
        if wall {
            assert_eq!(
                model.wall_assignment_regions.regions[0].id,
                WallPlateAssignmentRegionId(0)
            );
            assert_eq!(
                model.wall_assignment_regions.regions[0].assignment,
                PlateAssignment::Plate(WallPlateId(0))
            );
            assert_eq!(model.wall_plates[0].loads[0].value, 0.0025);
            assert!(model
                .wall_assignment_region_coords(WallPlateAssignmentRegionId(0))
                .unwrap()
                .iter()
                .any(|p| p[0] == 5000.));
        } else {
            assert_eq!(
                model.floor_assignment_regions.regions[0].id,
                FloorPlateAssignmentRegionId(0)
            );
            assert_eq!(
                model.floor_assignment_regions.regions[0].assignment,
                PlateAssignment::Plate(SlabId(0))
            );
            assert_eq!(model.slabs[0].plate.loads[0].value, 0.0025);
            assert!(model
                .floor_assignment_region_coords(FloorPlateAssignmentRegionId(0))
                .unwrap()
                .iter()
                .any(|p| p[0] == 5000.));
        }
        for _ in 0..5 {
            let before = model.clone();
            model.rebuild_assignment_regions().unwrap();
            fixture::assert_inputs_eq(&model, &before);
        }
    }
}

#[test]
fn 同重心同面積同親名でも別支持境界へ版を移さない() {
    for wall in [false, true] {
        let mut model = fixture::rectangle(wall);
        if wall {
            model.wall_regions[0].name = "共通名".into();
            model.wall_assignment_regions.regions[0].assignment = PlateAssignment::NoPlate;
        } else {
            model.floor_regions[0].name = "共通名".into();
            model.floor_assignment_regions.regions[0].assignment = PlateAssignment::NoPlate;
        }
        // 支持 ID だけを変更し、親名の推定対象の座標・面積は不変。
        model.elements.swap(0, 1);
        for (i, elem) in model.elements.iter_mut().enumerate() {
            elem.id = ElemId(i as u32);
        }
        model.rebuild_assignment_regions().unwrap();
        if wall {
            crate::wall_region_rebuild::rebuild_wall_regions(&mut model);
            assert_eq!(model.wall_regions[0].name, "共通名");
            assert_eq!(
                model.wall_assignment_regions.regions[0].id,
                WallPlateAssignmentRegionId(1)
            );
            assert!(model.wall_assignment_regions.regions[0]
                .assignment
                .is_unset());
        } else {
            crate::region_rebuild::rebuild_floor_regions(&mut model);
            assert_eq!(model.floor_regions[0].name, "共通名");
            assert_eq!(
                model.floor_assignment_regions.regions[0].id,
                FloorPlateAssignmentRegionId(1)
            );
            assert!(model.floor_assignment_regions.regions[0]
                .assignment
                .is_unset());
        }
    }
}

#[test]
fn 正常な面走査重複は一領域にまとめ既存衝突は修復せず拒否する() {
    let coords = [[0., 0.], [4., 0.], [4., 3.], [0., 3.]];
    let segments: Vec<_> = (0..4)
        .map(|i| SupportMemberSegment {
            support: SupportMemberId::Primary(ElemId(i as u32)),
            axis_span: [0., 1.],
            start: coords[i],
            end: coords[(i + 1) % 4],
        })
        .collect();
    let duplicated: Vec<_> = segments.iter().chain(&segments).copied().collect();
    let mut floor = FloorPlateAssignmentRegions::default();
    let report = floor.rebuild(&duplicated, &[]);
    assert!(report.rejection.is_none());
    assert_eq!(report.regions, 1);
    assert_eq!(floor.regions[0].id.0, 0);
    let mut corrupt = floor.regions[0].clone();
    corrupt.id = FloorPlateAssignmentRegionId(9);
    floor.regions.push(corrupt);
    let before = floor.clone();
    assert!(floor
        .rebuild(&segments, &[])
        .rejection
        .unwrap()
        .contains("境界キー"));
    assert_eq!(floor, before);
    let mut walls = WallPlateAssignmentRegions::default();
    walls.replace_boundaries(vec![boundary()], 0);
    let before = walls.clone();
    assert!(walls
        .replace_boundaries(vec![boundary(), boundary()], 0)
        .rejection
        .unwrap()
        .contains("境界キー"));
    assert_eq!(walls, before);
}

#[test]
fn 壁候補で孤立版が出ると床候補の新idも確定しない() {
    let mut model = fixture::with_plate(true);
    let mut floor = fixture::rectangle(false);
    for node in &mut floor.nodes {
        node.id.0 += 4;
        node.coord[0] += 10000.;
        node.coord[2] = 6000.;
    }
    for elem in &mut floor.elements {
        elem.id.0 += 4;
        for node in &mut elem.nodes {
            node.0 += 4;
        }
    }
    model.nodes.extend(floor.nodes);
    model.elements.extend(floor.elements);
    assert!(model.rebuild_floor_assignment_regions().rejection.is_none());
    assert_eq!(model.floor_assignment_regions.regions[0].id.0, 0);
    model.floor_assignment_regions.regions[0].assignment = PlateAssignment::NoPlate;
    model.elements.swap(4, 5);
    for (i, e) in model.elements.iter_mut().enumerate() {
        e.id = ElemId(i as u32);
    }
    model.unassigned_posts.push(fixture::divider(true));
    let before = model.clone();
    assert!(model
        .rebuild_assignment_regions()
        .unwrap_err()
        .contains("孤立版"));
    fixture::assert_inputs_eq(&model, &before);
    let report = model.rebuild_assignment_regions_dropping_orphan_plates();
    assert!(report.floor.rejection.is_none() && report.wall.rejection.is_none());
    assert_eq!(model.floor_assignment_regions.regions[0].id.0, 1);
    assert!(model.floor_assignment_regions.regions[0]
        .assignment
        .is_unset());
}
