use super::*;

fn u_shape() -> Vec<[f64; 3]> {
    [
        (0., 0.),
        (12., 0.),
        (12., 12.),
        (9., 12.),
        (9., 4.),
        (3., 4.),
        (3., 12.),
        (0., 12.),
    ]
    .iter()
    .map(|&(x, y)| [1000. * x, 1000. * y, 0.])
    .collect()
}

#[test]
fn local_u_tie_and_finite_segment_fixture() {
    let poly = local_polygon(&u_shape()).unwrap();
    let epsilon = 64. * f64::EPSILON * 12000_f64.hypot(12000.);
    let (distances, winners) = nearest(&poly, [2000., 3000.], epsilon);
    assert_eq!(winners, vec![4, 5]);
    assert!((distances[4] - 2000_f64.sqrt() * 1000_f64.sqrt()).abs() < 1e-9);
    assert_eq!(nearest(&poly, [4000., 3000.], epsilon).1, vec![4]);
    // 延長直線は辺5への距離1mを返し、有限線分の√2mと異なる。
    assert_eq!((2000_f64 - 3000.).abs(), 1000.);
    let piece = [
        [1900., 2900.],
        [2100., 2900.],
        [2100., 3100.],
        [1900., 3100.],
    ];
    assert_eq!(endpoint_group(&poly, &piece, 4), vec![4, 5]);
}

fn integrate(coords: &[[f64; 3]], h: f64, phase: f64) -> PolygonDistribution {
    integrate_polygon(
        coords,
        &(0..coords.len()).collect::<Vec<_>>(),
        PolygonIntegrationOptions {
            cell_size_mm: h,
            phase: [phase; 2],
            ..Default::default()
        },
    )
    .unwrap()
}

fn trapezoid_reference_m2() -> [f64; 4] {
    // d0=d2=1.5、d0=d3(L)、d0=d1(R)の半平面境界の交点。
    let l = [(1. + 10_f64.sqrt()) / 2., 1.5];
    let r = [5. - 13_f64.sqrt() / 2., 1.5];
    let vertices = [[0., 0.], [6., 0.], [4., 3.], [1., 3.]];
    let regions = [
        vec![vertices[0], vertices[1], r, l],
        vec![vertices[1], vertices[2], r],
        vec![vertices[2], vertices[3], l, r],
        vec![vertices[3], vertices[0], l],
    ];
    let distances = |p: Point| {
        [
            p[1],
            (18. - 3. * p[0] - 2. * p[1]) / 13_f64.sqrt(),
            3. - p[1],
            (3. * p[0] - p[1]) / 10_f64.sqrt(),
        ]
    };
    let areas = std::array::from_fn(|e| {
        for &p in &regions[e] {
            let d = distances(p);
            assert!(d.iter().all(|&other| d[e] <= other + 1e-14));
        }
        (0..regions[e].len())
            .map(|i| {
                let a = regions[e][i];
                let b = regions[e][(i + 1) % regions[e].len()];
                a[0] * b[1] - a[1] * b[0]
            })
            .sum::<f64>()
            .abs()
            / 2.
    });
    let a = 3. * 13_f64.sqrt();
    let b = 3. * 10_f64.sqrt();
    let analytic = [(63. - a - b) / 8., a / 4., (45. - a - b) / 8., b / 4.];
    for e in 0..4 {
        assert!((areas[e] - analytic[e]).abs() < 1e-10);
    }
    assert!((areas.iter().sum::<f64>() - 13.5).abs() < 1e-12);
    areas
}

#[test]
fn convex_trapezoid_independent_integral() {
    let expected_m2 = trapezoid_reference_m2();
    let base = [
        [0., 0., 0.],
        [6000., 0., 0.],
        [4000., 3000., 0.],
        [1000., 3000., 0.],
    ];
    for h in [100., 50., 25., 12.5] {
        for phase in [0., 0.5] {
            for shift in 0..4 {
                for reverse in [false, true] {
                    let coords: Vec<_> = (0..4)
                        .map(|i| {
                            base[if reverse {
                                (shift + 4 - i) % 4
                            } else {
                                (shift + i) % 4
                            }]
                        })
                        .collect();
                    let result = integrate(&coords, h, phase);
                    assert!((result.polygon_area_mm2 - 13.5e6).abs() < 1e-6);
                    assert!(
                        (result.edge_areas_mm2.iter().sum::<f64>() - 13.5e6).abs()
                            <= result.area_roundoff_bound_mm2
                    );
                    let mut loads = Vec::new();
                    let actual = distribute_polygon(
                        &coords,
                        0.003,
                        &mut loads,
                        PolygonIntegrationOptions {
                            cell_size_mm: h,
                            phase: [phase; 2],
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    assert_eq!(actual.edge_areas_mm2, result.edge_areas_mm2);
                    let mut forces_n = [0.; 4];
                    for load in loads {
                        let super::super::types::LoadTarget::Edge(e) = load.target else {
                            panic!("辺荷重を期待")
                        };
                        assert!(load.cmq.q_i > 0. && load.cmq.q_j > 0.);
                        forces_n[e] += load.cmq.q_i + load.cmq.q_j;
                    }
                    assert!((forces_n.iter().sum::<f64>() - 40500.).abs() < 1e-6);
                    let mut areas = [0.; 4];
                    let mut errors = [0.; 4];
                    let mut bounds = [0.; 4];
                    let mut mapping = [0; 4];
                    for e in 0..4 {
                        let original = if reverse {
                            (shift + 3 - e) % 4
                        } else {
                            (shift + e) % 4
                        };
                        mapping[e] = original;
                        areas[original] = result.edge_areas_mm2[e] / 1e6;
                        errors[original] = (areas[original] - expected_m2[original]).abs();
                        bounds[original] = (result.edge_error_bounds_mm2[e]
                            - result.area_roundoff_bound_mm2)
                            / 1e6;
                        assert!(
                            errors[original]
                                <= bounds[original] + result.area_roundoff_bound_mm2 / 1e6
                        );
                        assert!((forces_n[e] - result.edge_areas_mm2[e] * 0.003).abs() < 1e-6);
                    }
                    println!("trapezoid h={h} phase={phase} shift={shift} reverse={reverse} mapping={mapping:?} areas_m2={areas:?} errors_m2={errors:?} B_m2={bounds:?} roundoff_m2={}", result.area_roundoff_bound_mm2 / 1e6);
                }
            }
        }
    }
}

#[test]
fn trapezoid_partial_pieces_partition_and_radius_bound() {
    let coords = [
        [0., 0., 0.],
        [6000., 0., 0.],
        [4000., 3000., 0.],
        [1000., 3000., 0.],
    ];
    let poly = local_polygon(&coords).unwrap();
    let triangles = triangulate(&poly).unwrap();
    for h in [100., 50., 25., 12.5] {
        for phase in [0., 0.5] {
            let result = integrate(&coords, h, phase);
            let mut clipped_area = 0.;
            let mut ambiguous_area = 0.;
            let mut bounds = [0.; 4];
            let mut partial_pieces = 0;
            for iy in 0..((3000. + phase * h) / h).ceil() as usize {
                for ix in 0..((6000. + phase * h) / h).ceil() as usize {
                    let lo = [(ix as f64 - phase) * h, (iy as f64 - phase) * h];
                    let hi = [lo[0] + h, lo[1] + h];
                    for triangle in &triangles {
                        let piece = clip_cell(triangle, lo, hi);
                        if piece.len() < 3 {
                            continue;
                        }
                        let anchor = piece[0];
                        let shifted: Vec<_> = piece
                            .iter()
                            .map(|p| [p[0] - anchor[0], p[1] - anchor[1]])
                            .collect();
                        let area = geom_polygon::area(&shifted);
                        if area <= 0. {
                            continue;
                        }
                        clipped_area += area;
                        if area < h * h - 1e-6 {
                            partial_pieces += 1;
                        }
                        let c = geom_polygon::centroid(&shifted);
                        let c = [c[0] + anchor[0], c[1] + anchor[1]];
                        let r = piece
                            .iter()
                            .map(|p| (p[0] - c[0]).hypot(p[1] - c[1]))
                            .fold(0., f64::max);
                        let (d, winners) = nearest(&poly, c, result.distance_epsilon_mm);
                        assert!(!winners.is_empty());
                        assert!(
                            (winners
                                .iter()
                                .map(|_| 1. / winners.len() as f64)
                                .sum::<f64>()
                                - 1.)
                                .abs()
                                < 1e-14
                        );
                        let min = d[winners[0]];
                        let stable = winners.len() == 1
                            && (0..4)
                                .filter(|&e| e != winners[0])
                                .all(|e| d[e] - min > 2. * r + result.distance_epsilon_mm);
                        if stable {
                            for &p in &piece {
                                assert_eq!(
                                    nearest(&poly, p, result.distance_epsilon_mm).1,
                                    winners
                                );
                            }
                        } else {
                            ambiguous_area += area;
                            for e in 0..4 {
                                if d[e] - min <= 2. * r + result.distance_epsilon_mm {
                                    bounds[e] += area;
                                }
                            }
                        }
                    }
                }
            }
            assert!(partial_pieces > 0);
            assert!((clipped_area - 13.5e6).abs() < 1e-5);
            for (e, bound) in bounds.iter().enumerate() {
                assert!(
                    (result.edge_error_bounds_mm2[e] - result.area_roundoff_bound_mm2 - bound)
                        .abs()
                        < 1e-5
                );
            }
            println!("trapezoid pieces h={h} phase={phase} partial_pieces={partial_pieces} total_m2={} ambiguous_B_m2={} edge_B_m2={:?}", clipped_area / 1e6, ambiguous_area / 1e6, bounds.map(|b| b / 1e6));
        }
    }
}

#[test]
fn u_exact_integral_grid_matrix() {
    // 底左区画の直線/凹端点境界の解析積分。m単位。
    let s = 7. - 24_f64.sqrt();
    let t = (6. * s - 9.).powf(1.5) / 9. + 2. * (3. - s) - (3. - s).powi(3) / 24.;
    let b = s * s / 2. + 2. * (3. - s) + (3. - s).powi(3) / 24.;
    let expected = [
        12. + 2. * b,
        22.875 - b - t,
        2.25,
        10.875 + t / 2.,
        12. + t,
        10.875 + t / 2.,
        2.25,
        22.875 - b - t,
    ];
    for h in [100., 50., 25., 12.5] {
        for phase in [0., 0.25, 0.5] {
            let result = integrate(&u_shape(), h, phase);
            assert!(
                (result.edge_areas_mm2.iter().sum::<f64>() - 96e6).abs()
                    < result.area_roundoff_bound_mm2
            );
            assert!((result.edge_areas_mm2.iter().sum::<f64>() * 0.003 - 288000.).abs() < 1e-6);
            for (e, &a) in expected.iter().enumerate() {
                assert!(
                    (result.edge_areas_mm2[e] - a * 1e6).abs() <= result.edge_error_bounds_mm2[e]
                );
            }
            for (a, b) in [(1, 7), (2, 6), (3, 5)] {
                assert!(
                    (result.edge_areas_mm2[a] - result.edge_areas_mm2[b]).abs()
                        <= result.edge_error_bounds_mm2[a] + result.edge_error_bounds_mm2[b]
                );
            }
            println!(
                "U h={h} phase={phase} areas_m2={:?} bounds_m2={:?}",
                result
                    .edge_areas_mm2
                    .iter()
                    .map(|x| x / 1e6)
                    .collect::<Vec<_>>(),
                result
                    .edge_error_bounds_mm2
                    .iter()
                    .map(|x| x / 1e6)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn polygon_errors_do_not_become_zero_success() {
    let coords = u_shape();
    let all: Vec<_> = (0..8).collect();
    assert!(matches!(
        integrate_polygon(&coords, &[], Default::default()),
        Err(PolygonDistributionError::InvalidInput(_))
    ));
    assert!(matches!(
        integrate_polygon(&coords, &[0, 100], Default::default()),
        Err(PolygonDistributionError::InvalidInput(_))
    ));
    assert!(matches!(
        integrate_polygon(&coords, &[4, 5], Default::default()),
        Err(PolygonDistributionError::Unsupported(_))
    ));
    let options = PolygonIntegrationOptions {
        max_cells: 1,
        ..Default::default()
    };
    assert!(matches!(
        integrate_polygon(&coords, &all, options),
        Err(PolygonDistributionError::ResourceLimit { .. })
    ));
    let options = PolygonIntegrationOptions {
        max_edge_error_mm2: Some(0.),
        ..Default::default()
    };
    assert!(matches!(
        integrate_polygon(&coords, &all, options),
        Err(PolygonDistributionError::PrecisionNotMet { .. })
    ));
    let mut bad = coords.clone();
    bad[1][0] = f64::NAN;
    assert!(integrate_polygon(&bad, &all, Default::default()).is_err());
    bad = coords.clone();
    bad[1] = bad[0];
    assert!(integrate_polygon(&bad, &all, Default::default()).is_err());
    let bowtie = [
        [0., 0., 0.],
        [1000., 1000., 0.],
        [0., 1000., 0.],
        [1000., 0., 0.],
    ];
    assert!(integrate_polygon(&bowtie, &[0, 1, 2, 3], Default::default()).is_err());
    let mut loads = Vec::new();
    assert!(distribute_polygon(&coords, f64::NAN, &mut loads, Default::default()).is_err());
    assert!(distribute_polygon(&bad, 0., &mut loads, Default::default()).is_err());
    assert!(distribute_polygon(&coords, 1e300, &mut loads, Default::default()).is_err());
    let square = [[0., 0.], [100., 0.], [100., 100.], [0., 100.]];
    assert_eq!(nearest(&square, [50., 50.], 1e-12).1, vec![0, 1, 2, 3]);
    let bridged_opening = [
        [0., 0., 0.],
        [100., 0., 0.],
        [100., 100., 0.],
        [0., 100., 0.],
        [0., 0., 0.],
        [20., 20., 0.],
        [20., 80., 0.],
        [80., 80., 0.],
        [80., 20., 0.],
        [20., 20., 0.],
    ];
    assert!(integrate_polygon(
        &bridged_opening,
        &(0..10).collect::<Vec<_>>(),
        Default::default()
    )
    .unwrap_err()
    .to_string()
    .contains("開口"));
}

fn c_shape() -> Vec<[f64; 3]> {
    [
        (0., 0.),
        (12., 0.),
        (12., 2.),
        (2., 2.),
        (2., 10.),
        (12., 10.),
        (12., 12.),
        (0., 12.),
    ]
    .iter()
    .map(|&(x, y)| [1000. * x, 1000. * y, 0.])
    .collect()
}

#[test]
fn deep_c_independent_integral_and_transform_matrix() {
    // 2m角の角領域。共有端点の距離がx,yより小さい領域を対角線で二分して積分する。
    let s = 4. - 8_f64.sqrt();
    let t = -s * s + 2. * s - (2. - s).powi(3) / 6.;
    let b = (4. - t) / 2.;
    let expected = [
        9.5 + b,
        1.,
        9.5 + t / 2.,
        8. + t,
        9.5 + t / 2.,
        1.,
        9.5 + b,
        8. + 2. * b,
    ];
    for h in [100., 50., 25., 12.5] {
        for phase in [0., 0.25, 0.5] {
            let result = integrate(&c_shape(), h, phase);
            assert!((result.polygon_area_mm2 - 64e6).abs() < 1e-6);
            assert!(
                (result.edge_areas_mm2.iter().sum::<f64>() - 64e6).abs()
                    < result.area_roundoff_bound_mm2
            );
            for (e, &a) in expected.iter().enumerate() {
                assert!(
                    (result.edge_areas_mm2[e] - a * 1e6).abs() <= result.edge_error_bounds_mm2[e]
                );
            }
            println!(
                "C h={h} phase={phase} areas_m2={:?} bounds_m2={:?}",
                result
                    .edge_areas_mm2
                    .iter()
                    .map(|x| x / 1e6)
                    .collect::<Vec<_>>(),
                result
                    .edge_error_bounds_mm2
                    .iter()
                    .map(|x| x / 1e6)
                    .collect::<Vec<_>>()
            );
        }
    }
    let u_expected_m2 = [
        20.07074872440124,
        16.311054705156383,
        2.25,
        12.139285466321498,
        14.528570932642996,
        12.139285466321498,
        2.25,
        16.311054705156383,
    ];
    for (base, analytic_m2) in [(u_shape(), u_expected_m2), (c_shape(), expected)] {
        for (h, phase) in [(100., 0.), (50., 0.25), (25., 0.5), (12.5, 0.25)] {
            let reference = integrate(&base, h, phase);
            for angle in [0., 0.37, std::f64::consts::FRAC_PI_2] {
                for (shift, reverse) in [(0, false), (3, false), (0, true), (3, true)] {
                    let n = base.len();
                    let transformed: Vec<_> = (0..n)
                        .map(|i| {
                            let k = if reverse {
                                (shift + n - i) % n
                            } else {
                                (shift + i) % n
                            };
                            let p = base[k];
                            [
                                1e9 + angle.cos() * p[0] - angle.sin() * p[1],
                                -1e9 + angle.sin() * p[0] + angle.cos() * p[1],
                                0.,
                            ]
                        })
                        .collect();
                    let result = integrate(&transformed, h, phase);
                    for e in 0..n {
                        let original = if reverse {
                            (shift + n - e - 1) % n
                        } else {
                            (shift + e) % n
                        };
                        let error_mm2 =
                            (result.edge_areas_mm2[e] - analytic_m2[original] * 1e6).abs();
                        assert!(
                            error_mm2 <= result.edge_error_bounds_mm2[e],
                            "h={h} phase={phase} angle={angle} shift={shift} reverse={reverse} edge={e}"
                        );
                        assert!(
                            (result.edge_areas_mm2[e] - reference.edge_areas_mm2[original]).abs()
                                <= result.edge_error_bounds_mm2[e]
                                    + reference.edge_error_bounds_mm2[original]
                        );
                    }
                    println!(
                        "transform area_m2={} h={h} phase={phase} angle={angle} shift={shift} reverse={reverse} areas_mm2={:?} bounds_mm2={:?}",
                        analytic_m2.iter().sum::<f64>(),
                        result.edge_areas_mm2,
                        result.edge_error_bounds_mm2
                    );
                }
            }
        }
    }
}

#[test]
fn clipped_disconnected_boundary_and_precision_contract() {
    let thin_u = [
        [0., 0., 0.],
        [90., 0., 0.],
        [90., 90., 0.],
        [70., 90., 0.],
        [70., 20., 0.],
        [20., 20., 0.],
        [20., 90., 0.],
        [0., 90., 0.],
    ];
    let result = integrate(&thin_u, 100., 0.5);
    assert!((result.edge_areas_mm2.iter().sum::<f64>() - 4600.).abs() < 1e-8);
    let generous = PolygonIntegrationOptions {
        max_edge_error_mm2: Some(1e7),
        ..Default::default()
    };
    assert!(integrate_polygon(&u_shape(), &(0..8).collect::<Vec<_>>(), generous).is_ok());
    let options = PolygonIntegrationOptions {
        cell_size_mm: 101.,
        ..Default::default()
    };
    assert!(integrate_polygon(&u_shape(), &(0..8).collect::<Vec<_>>(), options).is_err());
}

#[test]
fn actual_slab_entry_diagnostics_and_support_errors() {
    use crate::floor::{
        distribute_region, distribute_slab_resolved, distribute_slab_w_with_diagnostics,
        FloorDistributionError,
    };
    use sepika_core::ids::ElemId;
    use sepika_core::model::{DistributionMethod, SupportMemberId};
    let pts: Vec<_> = u_shape().iter().map(|p| (p[0], p[1])).collect();
    let (mut model, slab) =
        crate::floor::tests::polygon_slab_model(&pts, DistributionMethod::TriTrapezoid, 0.003);
    let result =
        distribute_slab_w_with_diagnostics(&model, &slab, 0.003, Default::default()).unwrap();
    let diagnostics = result.polygon.unwrap();
    assert!((diagnostics.polygon_area_mm2 - 96e6).abs() < 1e-6);
    assert!(
        (result
            .loads
            .iter()
            .map(|l| l.cmq.q_i + l.cmq.q_j)
            .sum::<f64>()
            - 288000.)
            .abs()
            < 1e-6
    );
    assert!(distribute_slab_resolved(&model, &slab, 0.003)
        .unwrap()
        .iter()
        .all(|l| !matches!(l.target, crate::floor::LoadTarget::Edge(_))));
    let options = PolygonIntegrationOptions {
        max_cells: 1,
        ..Default::default()
    };
    assert!(matches!(
        distribute_slab_w_with_diagnostics(&model, &slab, 0., options),
        Err(FloorDistributionError::Polygon(
            PolygonDistributionError::ResourceLimit { .. }
        ))
    ));
    let options = PolygonIntegrationOptions {
        max_edge_error_mm2: Some(0.),
        ..Default::default()
    };
    assert!(matches!(
        distribute_slab_w_with_diagnostics(&model, &slab, 0.003, options),
        Err(FloorDistributionError::Polygon(
            PolygonDistributionError::PrecisionNotMet { .. }
        ))
    ));
    assert!(
        distribute_slab_w_with_diagnostics(&model, &slab, f64::INFINITY, Default::default())
            .is_err()
    );
    let mut missing = model.clone();
    missing.floor_assignment_regions.regions[0].boundary.pop();
    assert!(distribute_slab_resolved(&missing, &slab, 0.).is_err());
    let mut nonfinite = model.clone();
    nonfinite.nodes[0].coord[0] = f64::NAN;
    assert!(distribute_slab_resolved(&nonfinite, &slab, 0.).is_err());
    let mut invalid_span = model.clone();
    invalid_span.floor_assignment_regions.regions[0].boundary[0].span = [0., 0.];
    assert!(distribute_slab_resolved(&invalid_span, &slab, 0.).is_err());
    let mut gap = model.clone();
    gap.floor_assignment_regions.regions[0].boundary[0].span[1] = 0.99999999;
    assert!(distribute_slab_resolved(&gap, &slab, 0.)
        .unwrap_err()
        .to_string()
        .contains("閉じていない"));
    model.floor_assignment_regions.regions[0].boundary[0].support =
        SupportMemberId::Primary(ElemId(u32::MAX));
    assert!(distribute_slab_resolved(&model, &slab, 0.).is_err());
    let mut region =
        sepika_core::model::FloorRegion::new(sepika_core::ids::FloorRegionId(0), Vec::new());
    region.slab_ids = vec![sepika_core::ids::SlabId(u32::MAX)];
    assert!(distribute_region(&model, &region, |_| 0.).is_err());
}

#[test]
fn enclosed_geometry_errors_precede_rectangle_and_zero_load_dispatch() {
    use crate::floor::{distribute_slab_resolved, distribute_slab_w, FloorDistributionError};
    use sepika_core::model::DistributionMethod;
    let cases = [
        (
            vec![
                [0., 0., 0.],
                [1000., 0., 0.],
                [3000., 0., 0.],
                [2000., 0., 0.],
            ],
            false,
        ),
        (
            vec![
                [0., 0., 0.],
                [1000., 0., 0.],
                [1000., 1000., 500.],
                [0., 1000., 500.],
            ],
            true,
        ),
    ];
    for (coords, unsupported) in cases {
        assert!(crate::floor::slab_dimensions_of(&coords).is_some());
        let direct = integrate_polygon(&coords, &[0, 1, 2, 3], Default::default()).unwrap_err();
        assert_eq!(
            matches!(direct, PolygonDistributionError::Unsupported(_)),
            unsupported
        );
        assert_eq!(
            matches!(direct, PolygonDistributionError::InvalidInput(_)),
            !unsupported
        );
        let pts: Vec<_> = coords.iter().map(|p| (p[0], p[1])).collect();
        for method in [
            DistributionMethod::TriTrapezoid,
            DistributionMethod::OneWay,
            DistributionMethod::TributaryArea,
        ] {
            let (mut model, slab) = crate::floor::tests::polygon_slab_model(&pts, method, 0.003);
            for (node, coord) in model.nodes.iter_mut().zip(&coords) {
                node.coord = *coord;
            }
            for w in [0., 0.003] {
                assert_eq!(
                    distribute_slab_w(&model, &slab, w).unwrap_err(),
                    FloorDistributionError::Polygon(direct.clone())
                );
                assert_eq!(
                    distribute_slab_resolved(&model, &slab, w).unwrap_err(),
                    FloorDistributionError::Polygon(direct.clone())
                );
            }
        }
    }
}
