use std::f64::consts::PI;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::controller::{rounded_rect_core_torsion, CoreTorsionLevel, CoreTorsionSolution};
use super::mesh::{self, Mesh, MeshSettings, RoundedCore};
use super::p1;

struct Evidence {
    directory: PathBuf,
    rows: BufWriter<File>,
}

impl Evidence {
    fn new(name: &str) -> Self {
        let root = std::env::var_os("SEPIKA416_CALIBRATION_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("sepika416-numeric-calibration"));
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = root.join(format!("{name}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        println!("calibration evidence: {}", directory.display());
        let rows = BufWriter::new(File::create(directory.join("rows.jsonl")).unwrap());
        Self { directory, rows }
    }

    fn row(&mut self, value: Value) {
        serde_json::to_writer(&mut self.rows, &value).unwrap();
        writeln!(self.rows).unwrap();
        self.rows.flush().unwrap();
    }

    fn mesh(&self, name: &str, mesh: &Mesh) {
        let mut file =
            BufWriter::new(File::create(self.directory.join(format!("{name}.json"))).unwrap());
        serde_json::to_writer(
            &mut file,
            &json!({
                "points": mesh.points,
                "triangles": mesh.triangles,
                "boundary_vertices": mesh.boundary_vertices,
            }),
        )
        .unwrap();
        file.flush().unwrap();
    }
}

fn settings(n: usize, area: f64) -> MeshSettings {
    MeshSettings {
        quarter_arc_segments: n,
        max_triangle_area: area,
        max_vertices: 200_000,
        max_triangles: 400_000,
    }
}

fn change(previous: f64, current: f64) -> f64 {
    (current - previous).abs() / current.abs()
}

fn level_json(l: &CoreTorsionLevel) -> Value {
    json!({
        "n": l.attempt.quarter_arc_segments, "area_limit": l.attempt.max_triangle_area,
        "vertices": l.vertices, "triangles": l.triangles, "free_dofs": l.free_dofs,
        "nnz": l.nnz, "polygon_area": l.polygon_area, "triangle_area": l.triangle_area,
        "max_actual_area": l.max_actual_area, "min_signed_double_area": l.min_signed_double_area,
        "min_area_edge_ratio": l.min_area_edge_ratio, "min_angle_rad": l.min_angle_rad,
        "original_boundary_segments": l.original_boundary_segments, "boundary_edges": l.boundary_edges,
        "fingerprint": l.fingerprint, "relative_residual": l.relative_residual, "j_hat": l.j_hat,
        "min_phi": l.min_phi, "max_phi": l.max_phi,
        "mesh_seconds": l.mesh_seconds, "assembly_seconds": l.assembly_seconds,
        "factorization_seconds": l.factorization_seconds, "solve_seconds": l.solve_seconds,
    })
}

fn fresh(evidence: &mut Evidence, label: &str, b: f64, h: f64, r: f64) -> CoreTorsionSolution {
    match rounded_rect_core_torsion(b, h, r) {
        Ok(solution) => {
            for (index, level) in solution.levels.iter().enumerate() {
                evidence.row(json!({"label": label, "index": index, "level": level_json(level)}));
            }
            evidence.row(json!({
                "label": label, "status": "solved", "b": b, "h": h, "r": r,
                "j_mm4": solution.j_mm4, "j_hat": solution.j_hat,
                "length_scale_mm": solution.length_scale_mm, "accepted_level": solution.accepted_level,
                "internal_changes": solution.internal_changes, "boundary_changes": solution.boundary_changes,
                "total_seconds": solution.total_seconds,
            }));
            assert!(solution.internal_changes.iter().all(|v| *v <= 1e-3));
            assert!(solution.levels.iter().all(|v| v.relative_residual <= 1e-10));
            if r > 0.0 {
                assert!(solution
                    .boundary_changes
                    .unwrap()
                    .iter()
                    .all(|v| *v <= 1e-3));
            }
            solution
        }
        Err(error) => {
            for (index, level) in error.levels.iter().enumerate() {
                evidence.row(json!({"label": label, "index": index, "level": level_json(level)}));
            }
            evidence.row(json!({
                "label": label, "status": "unconfirmed", "reason": error.reason,
                "b": b, "h": h, "r": r, "total_seconds": error.total_seconds,
                "failed_attempt": error.failed_attempt.map(|a| json!({
                    "n": a.quarter_arc_segments, "area_limit": a.max_triangle_area,
                    "max_vertices": a.max_vertices, "max_triangles": a.max_triangles,
                })),
            }));
            panic!("{label}: {error}");
        }
    }
}

fn rectangle_series(w: f64, d: f64) -> (f64, f64, usize) {
    assert!(w >= d && d > 0.0);
    let leading = w * d.powi(3) / 3.0;
    let coefficient = leading * 192.0 * d / (PI.powi(5) * w);
    let mut sum = 0.0;
    for n in (1..100_000).step_by(2) {
        let nf = n as f64;
        sum += (nf * PI * w / (2.0 * d)).tanh() / nf.powi(5);
        let expected = leading - coefficient * sum;
        // tanh≤1なので、未加算の奇数項を全整数の積分tailで上から押さえる。
        let tail_bound = coefficient / (4.0 * nf.powi(4));
        if tail_bound <= expected * 1e-10 {
            return (expected, tail_bound, n);
        }
    }
    panic!("独立矩形級数の打切り誤差未達");
}

#[test]
fn ellipses_have_separate_boundary_and_internal_convergence_and_point_one_percent_accuracy() {
    let mut evidence = Evidence::new("ellipses");
    for ratio in [2.0, 4.0] {
        let a = 0.5_f64;
        let b = a / ratio;
        let exact = PI * a.powi(3) * b.powi(3) / (a * a + b * b);
        let mut grid: Vec<Vec<CoreTorsionLevel>> = (0..3).map(|_| Vec::new()).collect();
        let mut converged = false;
        for denominator in [2048.0, 8192.0, 32768.0, 131072.0, 524288.0, 2097152.0] {
            for (row_index, n) in [32, 64, 128].into_iter().enumerate() {
                let row = &mut grid[row_index];
                let area = 1.0 / denominator;
                let name = format!("ellipse-{ratio}-n{n}-d{denominator}");
                if row
                    .last()
                    .is_some_and(|previous| previous.polygon_area / area > 400_000.0)
                {
                    evidence.row(json!({"name": name, "status": "unconfirmed",
                        "reason": "輪郭面積/最大要素面積の下限がtriangle400000を超える"}));
                    panic!("{name}: 三角形数の面積下限が資源上限を超えます");
                }
                let started = Instant::now();
                let contour = (0..4 * n)
                    .map(|i| {
                        let angle = 2.0 * PI * i as f64 / (4 * n) as f64;
                        [a * angle.cos(), b * angle.sin()]
                    })
                    .collect();
                let mesh = match mesh::generate_contour(contour, settings(n, area)) {
                    Ok(mesh) => mesh,
                    Err(reason) => {
                        evidence
                            .row(json!({"name": name, "status": "mesh_failed", "reason": reason}));
                        panic!("{name}: {reason}");
                    }
                };
                let mesh_seconds = started.elapsed().as_secs_f64();
                evidence.mesh(&name, &mesh);
                let result = match p1::solve(&mesh) {
                    Ok(result) => result,
                    Err(reason) => {
                        evidence
                            .row(json!({"name": name, "status": "solve_failed", "reason": reason}));
                        panic!("{name}: {reason}");
                    }
                };
                let q = mesh.quality;
                let level = CoreTorsionLevel {
                    attempt: super::controller::CoreTorsionAttempt {
                        quarter_arc_segments: n,
                        max_triangle_area: area,
                        max_vertices: 200_000,
                        max_triangles: 400_000,
                    },
                    j_hat: result.j_hat,
                    relative_residual: result.relative_residual,
                    free_dofs: result.free_dofs,
                    nnz: result.nnz,
                    min_phi: result.min_phi,
                    max_phi: result.max_phi,
                    polygon_area: q.polygon_area,
                    triangle_area: q.triangle_area,
                    max_actual_area: q.max_actual_area,
                    min_signed_double_area: q.min_signed_double_area,
                    min_area_edge_ratio: q.min_area_edge_ratio,
                    min_angle_rad: q.min_angle_rad,
                    original_boundary_segments: q.original_boundary_segments,
                    boundary_edges: q.boundary_edges,
                    vertices: q.vertices,
                    triangles: q.triangles,
                    fingerprint: q.fingerprint,
                    mesh_seconds,
                    assembly_seconds: result.assembly_seconds,
                    factorization_seconds: result.factorization_seconds,
                    solve_seconds: result.solve_seconds,
                };
                evidence.row(json!({"name": name, "status": "solved", "ratio": ratio, "a": a, "b": b,
                    "exact": exact, "relative_error": change(exact, level.j_hat), "level": level_json(&level),
                    "internal_change": row.last().map(|old: &CoreTorsionLevel| change(old.j_hat, level.j_hat))}));
                row.push(level);
            }
            if grid.iter().all(|row| {
                row.len() >= 3
                    && row[row.len() - 3..].windows(2).all(|pair| {
                        change(pair[0].j_hat, pair[1].j_hat) <= 1e-3
                            && pair[1].vertices > pair[0].vertices
                            && pair[1].triangles > pair[0].triangles
                            && pair[1].free_dofs > pair[0].free_dofs
                            && pair[1].max_actual_area < pair[0].max_actual_area
                            && pair[1].fingerprint != pair[0].fingerprint
                    })
            }) && grid.windows(2).all(|pair| {
                change(pair[0].last().unwrap().j_hat, pair[1].last().unwrap().j_hat) <= 1e-3
            }) {
                converged = true;
                break;
            }
        }
        assert!(converged, "ratio={ratio}: 内部・境界の共通列品質未達");
        let mut internal_changes = Vec::new();
        for row in &grid {
            for pair in row.windows(2) {
                assert_eq!(pair[0].polygon_area, pair[1].polygon_area);
                assert!(
                    pair[1].vertices > pair[0].vertices && pair[1].triangles > pair[0].triangles
                );
                assert!(pair[1].free_dofs > pair[0].free_dofs);
                assert!(pair[1].max_actual_area < pair[0].max_actual_area);
                assert_ne!(pair[0].fingerprint, pair[1].fingerprint);
            }
            let last = &row[row.len() - 3..];
            internal_changes.push([
                change(last[0].j_hat, last[1].j_hat),
                change(last[1].j_hat, last[2].j_hat),
            ]);
        }
        let final_rows: Vec<_> = grid.iter().map(|row| row.last().unwrap()).collect();
        let boundary_changes = [
            change(final_rows[0].j_hat, final_rows[1].j_hat),
            change(final_rows[1].j_hat, final_rows[2].j_hat),
        ];
        let relative_error = (final_rows[2].j_hat / exact - 1.0).abs();
        evidence.row(json!({"ratio": ratio, "internal_changes": internal_changes,
            "boundary_changes": boundary_changes, "relative_error": relative_error, "exact": exact}));
        assert!(
            internal_changes.iter().flatten().all(|v| *v <= 1e-3),
            "{internal_changes:?}"
        );
        assert!(
            boundary_changes.iter().all(|v| *v <= 1e-3),
            "{boundary_changes:?}"
        );
        assert!(
            relative_error <= 1e-3,
            "ratio={ratio}, error={relative_error}"
        );
        for pair in final_rows.windows(2) {
            assert_eq!(
                pair[0].attempt.max_triangle_area,
                pair[1].attempt.max_triangle_area
            );
            assert!(pair[1].original_boundary_segments > pair[0].original_boundary_segments);
            assert!(pair[1].polygon_area > pair[0].polygon_area);
        }
        assert!(grid.iter().flatten().all(|l| l.relative_residual <= 1e-10));
    }
}

#[test]
fn elongated_rectangles_agree_with_tail_bounded_independent_series() {
    let mut evidence = Evidence::new("rectangle-series");
    let mut failures = Vec::new();
    for ratio in [4.0, 10.0] {
        let (expected, tail_bound, last_odd) = rectangle_series(1.0, 1.0 / ratio);
        evidence.row(json!({"ratio": ratio, "expected": expected, "tail_bound": tail_bound, "last_odd": last_odd}));
        let result = rounded_rect_core_torsion(1.0, 1.0 / ratio, 0.0);
        let (levels, status, reason) = match &result {
            Ok(solution) => (&solution.levels, "solved", None),
            Err(error) => (&error.levels, "unconfirmed", Some(&error.reason)),
        };
        for (index, level) in levels.iter().enumerate() {
            evidence.row(
                json!({"ratio": ratio, "index": index, "level": level_json(level),
                "relative_error": (level.j_hat / expected - 1.0).abs()}),
            );
        }
        evidence.row(json!({"ratio": ratio, "status": status, "reason": reason}));
        match result {
            Ok(solution) => {
                let relative_error = (solution.j_mm4 / expected - 1.0).abs();
                evidence.row(json!({"ratio": ratio, "relative_error": relative_error,
                    "internal_changes": solution.internal_changes, "total_seconds": solution.total_seconds}));
                if relative_error > 1e-3 {
                    failures.push(format!("ratio={ratio}, error={relative_error}"));
                }
            }
            Err(error) => failures.push(format!("ratio={ratio}: {error}")),
        }
        assert!(tail_bound / expected <= 1e-10);
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
fn positive_small_radii_approach_rectangle_and_half_side_radii_approach_circle() {
    let mut evidence = Evidence::new("radius-limits");
    let (rectangle, tail_bound, last_odd) = rectangle_series(1.0, 0.75);
    evidence.row(
        json!({"rectangle_expected": rectangle, "tail_bound": tail_bound, "last_odd": last_odd}),
    );
    for radius in [0.0, 0.75e-6, 0.75e-3] {
        let solution = fresh(
            &mut evidence,
            &format!("small-r-{radius}"),
            1.0,
            0.75,
            radius,
        );
        let relative_error = (solution.j_mm4 / rectangle - 1.0).abs();
        evidence.row(json!({"radius": radius, "relative_rectangle_error": relative_error}));
        assert!(relative_error <= 1e-3, "r={radius}, error={relative_error}");
        if radius > 0.0 {
            assert!(solution
                .levels
                .iter()
                .all(|l| l.original_boundary_segments > 4));
        }
    }
    let circle = PI / 32.0;
    let mut errors = Vec::new();
    for radius in [0.49, 0.499, 0.5] {
        let solution = fresh(
            &mut evidence,
            &format!("circle-limit-r-{radius}"),
            1.0,
            1.0,
            radius,
        );
        let error = (solution.j_mm4 / circle - 1.0).abs();
        evidence.row(
            json!({"radius": radius, "circle_exact": circle, "relative_circle_difference": error}),
        );
        errors.push(error);
    }
    assert!(errors[1] < errors[0], "{errors:?}");
    assert!(errors[2] < errors[1], "{errors:?}");
    assert!(errors[2] <= 1e-3, "{errors:?}");
}

#[test]
fn actual_fine_mesh_coordinates_preserve_j_under_rigid_motion_and_scale_to_fourth_power() {
    let mut evidence = Evidence::new("actual-transforms");
    let started = Instant::now();
    let mut mesh = mesh::generate(
        RoundedCore {
            b: 1.0,
            h: 0.75,
            r: 0.125,
        },
        settings(64, 1.0 / 32768.0),
    )
    .unwrap();
    let mesh_seconds = started.elapsed().as_secs_f64();
    let original_points = mesh.points.clone();
    let original = p1::solve(&mesh).unwrap();
    evidence.mesh("original", &mesh);
    evidence.row(json!({"label": "original", "j": original.j_hat, "n": 64, "area_limit": 1.0 / 32768.0,
        "relative_residual": original.relative_residual, "free_dofs": original.free_dofs, "nnz": original.nnz,
        "vertices": mesh.points.len(), "triangles": mesh.triangles.len(),
        "area_sum": mesh.quality.triangle_area, "polygon_area": mesh.quality.polygon_area,
        "min_signed_double_area": mesh.quality.min_signed_double_area,
        "min_area_edge_ratio": mesh.quality.min_area_edge_ratio, "min_angle_rad": mesh.quality.min_angle_rad,
        "max_actual_area": mesh.quality.max_actual_area, "original_boundary_segments": mesh.quality.original_boundary_segments,
        "boundary_edges": mesh.quality.boundary_edges, "relative_error": 0.0,
        "mesh_seconds": mesh_seconds, "assembly_seconds": original.assembly_seconds,
        "factorization_seconds": original.factorization_seconds, "solve_seconds": original.solve_seconds}));
    for (label, angle, translation, scale) in [
        ("rotate-pi-7", PI / 7.0, [0.0, 0.0], 1.0_f64),
        ("rotate-pi-2", PI / 2.0, [0.0, 0.0], 1.0),
        ("translate", 0.0, [13.0, -7.0], 1.0),
        ("combined", PI / 7.0, [13.0, -7.0], 1.0),
        ("scale-0.1", 0.0, [0.0, 0.0], 0.1),
        ("scale-10", 0.0, [0.0, 0.0], 10.0),
        ("scale-1000", 0.0, [0.0, 0.0], 1000.0),
    ] {
        let (sin, cos) = angle.sin_cos();
        for (point, p) in mesh.points.iter_mut().zip(&original_points) {
            *point = [
                scale * (p[0] * cos - p[1] * sin) + translation[0],
                scale * (p[0] * sin + p[1] * cos) + translation[1],
            ];
        }
        evidence.mesh(label, &mesh);
        let result = p1::solve(&mesh).unwrap_or_else(|reason| {
            evidence.row(json!({"label": label, "status": "solve_failed", "reason": reason}));
            panic!("{label}: {reason}");
        });
        let mut area_sum = 0.0;
        let mut min_double_area = f64::INFINITY;
        let mut max_area = 0.0_f64;
        let mut min_ratio = f64::INFINITY;
        let mut min_angle = f64::INFINITY;
        for triangle in &mesh.triangles {
            let [a, b, c] = triangle.map(|i| mesh.points[i]);
            let double_area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            min_double_area = min_double_area.min(double_area);
            max_area = max_area.max(double_area / 2.0);
            area_sum += double_area / 2.0;
            let points = [a, b, c];
            let mut longest_squared = 0.0_f64;
            for i in 0..3 {
                let u = [
                    points[(i + 1) % 3][0] - points[i][0],
                    points[(i + 1) % 3][1] - points[i][1],
                ];
                let v = [
                    points[(i + 2) % 3][0] - points[i][0],
                    points[(i + 2) % 3][1] - points[i][1],
                ];
                longest_squared = longest_squared.max(u[0] * u[0] + u[1] * u[1]);
                min_angle =
                    min_angle.min((u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]));
            }
            min_ratio = min_ratio.min(double_area / longest_squared);
        }
        let relative_error = (result.j_hat / original.j_hat / scale.powi(4) - 1.0).abs();
        evidence.row(json!({"label": label, "angle": angle, "translation": translation, "scale": scale,
            "status": "solved", "j": result.j_hat, "relative_error": relative_error,
            "relative_residual": result.relative_residual, "free_dofs": result.free_dofs, "nnz": result.nnz,
            "vertices": mesh.points.len(), "triangles": mesh.triangles.len(), "area_sum": area_sum,
            "min_signed_double_area": min_double_area, "max_actual_area": max_area,
            "n": 64, "area_limit": scale.powi(2) / 32768.0,
            "min_area_edge_ratio": min_ratio, "min_angle_rad": min_angle,
            "original_boundary_segments": mesh.quality.original_boundary_segments,
            "boundary_edges": mesh.quality.boundary_edges, "mesh_seconds": 0.0,
            "assembly_seconds": result.assembly_seconds, "factorization_seconds": result.factorization_seconds,
            "solve_seconds": result.solve_seconds}));
        assert!(min_double_area > 0.0);
        assert!(min_ratio > 128.0 * f64::EPSILON && min_angle > 0.0);
        assert!((area_sum / mesh.quality.triangle_area / scale.powi(2) - 1.0).abs() < 1e-10);
        assert!(relative_error < 1e-10, "{label}: {relative_error}");
        assert!(result.relative_residual <= 1e-10);
        assert_eq!(result.free_dofs, original.free_dofs);
        assert_eq!(result.nnz, original.nnz);
    }
}
