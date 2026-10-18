use super::*;

fn assert_quality(solution: &CoreTorsionSolution, rounded: bool) {
    assert!(solution.j_mm4.is_finite() && solution.j_mm4 > 0.0);
    assert!(solution.internal_changes.iter().all(|d| *d <= 1e-3));
    assert_eq!(solution.boundary_changes.is_some(), rounded);
    if let Some(changes) = solution.boundary_changes {
        assert!(changes.iter().all(|d| *d <= 1e-3));
    }
    for level in &solution.levels {
        assert!(level.relative_residual <= 1e-10);
        assert!(
            level.max_actual_area <= level.attempt.max_triangle_area * (1.0 + 128.0 * f64::EPSILON)
        );
        assert!(level.min_signed_double_area > 0.0);
        assert!(level.min_angle_rad > 0.0);
        assert!(level.free_dofs > 0 && level.nnz >= level.free_dofs);
    }
    let final_n = solution.levels[solution.accepted_level]
        .attempt
        .quarter_arc_segments;
    let final_area = solution.levels[solution.accepted_level]
        .attempt
        .max_triangle_area;
    let rows: Vec<_> = solution
        .levels
        .iter()
        .filter(|l| l.attempt.max_triangle_area == final_area)
        .collect();
    if rounded {
        assert!(rows.len() >= 3);
        assert!(rows
            .iter()
            .any(|l| l.attempt.quarter_arc_segments == final_n));
    }
    println!(
        "J={} Jhat={} levels={} n={} a={} time={} internal={:?} boundary={:?}",
        solution.j_mm4,
        solution.j_hat,
        solution.levels.len(),
        final_n,
        final_area,
        solution.total_seconds,
        solution.internal_changes,
        solution.boundary_changes
    );
}

fn assert_six_column_success_unchanged(
    b_mm: f64,
    h_mm: f64,
    r_mm: f64,
    current: &CoreTorsionSolution,
) {
    let original = run(b_mm, h_mm, r_mm, 200_000, 400_000, 6, 6).unwrap();
    assert_eq!(current.levels.len(), original.levels.len());
    assert_eq!(current.accepted_level, original.accepted_level);
    let accepted = &current.levels[current.accepted_level];
    let original_accepted = &original.levels[original.accepted_level];
    assert_eq!(
        accepted.attempt.quarter_arc_segments,
        original_accepted.attempt.quarter_arc_segments
    );
    assert_eq!(
        accepted.attempt.max_triangle_area,
        original_accepted.attempt.max_triangle_area
    );
    assert_eq!(current.j_hat, original.j_hat);
    assert_eq!(current.j_mm4, original.j_mm4);
}

#[test]
fn rectangle_and_circle_agree_with_independent_exact_values_within_point_one_percent() {
    for (b, h, r, expected) in [
        (1.0, 1.0, 0.0, 0.1405770149551536),
        (1.0, 0.5, 0.0, 0.028585209639946346),
        (2.0, 2.0, 1.0, std::f64::consts::FRAC_PI_2),
    ] {
        let solution = rounded_rect_core_torsion(b, h, r).unwrap();
        assert_quality(&solution, r > 0.0);
        assert_six_column_success_unchanged(b, h, r, &solution);
        assert!(
            (solution.j_mm4 / expected - 1.0).abs() <= 1e-3,
            "{} vs {}",
            solution.j_mm4,
            expected
        );
    }
}

#[test]
fn rounded_rectangle_has_separate_internal_and_boundary_histories() {
    let solution = rounded_rect_core_torsion(376.0, 276.0, 18.0).unwrap();
    assert_quality(&solution, true);
    assert_six_column_success_unchanged(376.0, 276.0, 18.0, &solution);
    let final_level = &solution.levels[solution.accepted_level];
    assert_eq!(solution.j_hat, final_level.j_hat);
    for n in [
        final_level.attempt.quarter_arc_segments / 4,
        final_level.attempt.quarter_arc_segments / 2,
        final_level.attempt.quarter_arc_segments,
    ] {
        let mut row: Vec<_> = solution
            .levels
            .iter()
            .filter(|l| {
                l.attempt.quarter_arc_segments == n
                    && l.attempt.max_triangle_area >= final_level.attempt.max_triangle_area
            })
            .collect();
        row.sort_by(|a, b| {
            b.attempt
                .max_triangle_area
                .total_cmp(&a.attempt.max_triangle_area)
        });
        assert!(row.len() >= 3);
        assert_eq!(
            row.last().unwrap().attempt.max_triangle_area,
            final_level.attempt.max_triangle_area
        );
        for pair in row[row.len() - 3..].windows(2) {
            assert!((pair[1].j_hat - pair[0].j_hat).abs() / pair[1].j_hat <= 1e-3);
            assert!(pair[1].free_dofs > pair[0].free_dofs);
            assert!(pair[1].max_actual_area < pair[0].max_actual_area);
            assert_ne!(pair[1].fingerprint, pair[0].fingerprint);
            assert_eq!(pair[1].polygon_area, pair[0].polygon_area);
        }
    }
}

#[test]
fn fourth_aspect_rectangle_converges_on_seventh_stage_without_relaxing_quality() {
    let six_stages = run(1.0, 0.25, 0.0, 200_000, 400_000, 6, 6).unwrap_err();
    assert!(six_stages.reason.contains("内部細分化上限"));
    assert_eq!(six_stages.levels.len(), 6);
    assert!(six_stages.failed_attempt.is_none());

    let solution = rounded_rect_core_torsion(1.0, 0.25, 0.0).unwrap();
    assert_quality(&solution, false);
    assert_eq!(solution.levels.len(), 7);
    assert_eq!(solution.accepted_level, 6);
    let accepted = &solution.levels[solution.accepted_level];
    assert_eq!(accepted.attempt.max_triangle_area, 1.0 / 524288.0);
    assert!(accepted.vertices <= 200_000 && accepted.triangles <= 400_000);
    assert!((solution.j_mm4 / 0.004387702473557458 - 1.0).abs() <= 1e-3);
    for pair in solution.levels[4..].windows(2) {
        assert!((pair[1].j_hat - pair[0].j_hat).abs() / pair[1].j_hat <= 1e-3);
        assert!(pair[1].free_dofs > pair[0].free_dofs);
        assert!(pair[1].max_actual_area < pair[0].max_actual_area);
        assert_ne!(pair[1].fingerprint, pair[0].fingerprint);
    }
}

#[test]
fn tenth_aspect_rectangle_converges_with_actual_free_dofs_and_bounded_history() {
    let solution = rounded_rect_core_torsion(1.0, 0.1, 0.0).unwrap();
    assert_quality(&solution, false);
    let expected = 0.00031232503747163996;
    let series_tail_bound = 2.989367149994701e-14;
    assert!((solution.j_mm4 - expected).abs() + series_tail_bound <= expected * 1e-3);
    assert!(solution.levels.len() >= 3 && solution.levels.len() <= 8);
    assert_eq!(solution.accepted_level, solution.levels.len() - 1);
    assert_eq!(solution.levels[0].attempt.max_triangle_area, 1.0 / 128.0);
    for level in &solution.levels {
        assert_eq!(level.attempt.max_vertices, 200_000);
        assert_eq!(level.attempt.max_triangles, 400_000);
        assert!(level.vertices <= 200_000 && level.triangles <= 400_000);
        assert_eq!(level.polygon_area, 0.1);
        assert!((level.triangle_area - level.polygon_area).abs() < 1e-12);
        assert_eq!(level.original_boundary_segments, 4);
        println!("rect10 level: {level:?}");
    }
    for pair in solution.levels[solution.levels.len() - 3..].windows(2) {
        assert!((pair[1].j_hat - pair[0].j_hat).abs() / pair[1].j_hat <= 1e-3);
        assert!(pair[1].free_dofs > pair[0].free_dofs);
        assert!(pair[1].vertices > pair[0].vertices);
        assert!(pair[1].triangles > pair[0].triangles);
        assert!(pair[1].max_actual_area < pair[0].max_actual_area);
        assert_ne!(pair[1].fingerprint, pair[0].fingerprint);
    }
    let original = mesh::generate_contour(
        vec![[-0.5, -0.05], [0.5, -0.05], [0.5, 0.05], [-0.5, 0.05]],
        MeshSettings {
            quarter_arc_segments: 16,
            max_triangle_area: 1.0 / 128.0,
            max_vertices: 200_000,
            max_triangles: 400_000,
        },
    )
    .unwrap();
    assert_eq!(original.quality.free_vertices, 0);
    let error = run(1.0, 0.1, 0.0, original.quality.vertices + 1, 400_000, 6, 8).unwrap_err();
    assert!(error.reason.contains("refinement"));
    assert!(error.levels.is_empty());
    let failed = error.failed_attempt.unwrap();
    assert_eq!(failed.max_vertices, original.quality.vertices + 1);
    assert_eq!(failed.max_triangle_area, 1.0 / 128.0);
}

#[test]
fn resource_failure_on_added_stage_keeps_original_six_completed_stages() {
    let error = run(1.0, 0.25, 0.0, 30_000, 60_000, 6, 8).unwrap_err();
    assert_eq!(error.levels.len(), 6);
    let failed = error.failed_attempt.unwrap();
    assert_eq!(failed.max_triangle_area, 1.0 / 524288.0);
    assert_eq!(failed.max_vertices, 30_000);
    assert_eq!(failed.max_triangles, 60_000);
    assert!(error.levels.iter().all(|l| l.relative_residual <= 1e-10));
    assert!(error
        .levels
        .iter()
        .all(|l| l.j_hat.is_finite() && l.j_hat > 0.0));
    assert!(!error.reason.is_empty());
}

#[test]
fn invalid_dimensions_and_normalization_underflow_preserve_failure_contract() {
    for (b, h, r) in [
        (0.0, 1.0, 0.0),
        (1.0, -1.0, 0.0),
        (1.0, 1.0, -0.1),
        (1.0, 1.0, 0.6),
        (f64::NAN, 1.0, 0.0),
        (1.0, f64::INFINITY, 0.0),
        (1e308, 1e-308, 0.0),
        (1e308, 1.0, f64::from_bits(1)),
    ] {
        let error = rounded_rect_core_torsion(b, h, r).unwrap_err();
        assert!(!error.reason.is_empty());
        assert!(error.levels.is_empty());
        assert!(error.failed_attempt.is_none());
        assert_eq!(error.to_string(), error.reason);
    }
}

#[test]
fn finite_limits_return_unconfirmed_with_completed_history_and_failed_settings() {
    let resource = run(1.0, 1.0, 0.0, 10, 20, 6, 6).unwrap_err();
    assert!(resource.failed_attempt.is_some());
    assert!(resource.levels.is_empty());
    let partial = run(1.0, 1.0, 0.0, 1000, 2000, 6, 6).unwrap_err();
    assert!(!partial.levels.is_empty());
    assert!(partial.failed_attempt.is_some());
    let internal = run(1.0, 1.0, 0.0, 200_000, 400_000, 6, 2).unwrap_err();
    assert_eq!(internal.levels.len(), 2);
    assert!(internal.reason.contains("内部細分化上限"));
    let boundary = run(1.0, 1.0, 0.5, 200_000, 400_000, 2, 6).unwrap_err();
    assert!(boundary.levels.len() >= 6);
    assert!(boundary.reason.contains("境界細分化上限"));
    assert!(boundary.levels.iter().all(|l| l.relative_residual <= 1e-10));
}

#[test]
fn physical_scaling_is_fourth_power_and_restoration_failure_retains_history() {
    let base = rounded_rect_core_torsion(1.0, 0.5, 0.0).unwrap();
    for scale in [0.1, 10.0, 1000.0] {
        let scaled = rounded_rect_core_torsion(scale, scale / 2.0, 0.0).unwrap();
        assert!((scaled.j_mm4 / base.j_mm4 / scale.powi(4) - 1.0).abs() < 1e-12);
        assert_eq!(scaled.j_hat, base.j_hat);
    }
    for scale in [1e100, 1e-100] {
        let error = rounded_rect_core_torsion(scale, scale / 2.0, 0.0).unwrap_err();
        assert!(error.reason.contains("尺度復元"));
        assert!(error.levels.len() >= 3);
        assert!(error.failed_attempt.is_none());
    }
}
