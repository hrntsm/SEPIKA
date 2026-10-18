use super::*;
use crate::section_shape::core_torsion::mesh::{
    generate, generate_contour, MeshQuality, MeshSettings, RoundedCore,
};

fn fixed_mesh(
    points: Vec<[f64; 2]>,
    triangles: Vec<[usize; 3]>,
    boundary_vertices: Vec<usize>,
) -> Mesh {
    let quality = MeshQuality {
        polygon_area: 1.0,
        triangle_area: 1.0,
        max_actual_area: 0.25,
        min_signed_double_area: 0.5,
        min_area_edge_ratio: 0.5,
        min_angle_rad: std::f64::consts::FRAC_PI_4,
        original_boundary_segments: 4,
        boundary_edges: boundary_vertices.len(),
        vertices: points.len(),
        triangles: triangles.len(),
        free_vertices: points.len() - boundary_vertices.len(),
        fingerprint: 0,
    };
    Mesh {
        points,
        triangles,
        boundary_vertices,
        quality,
    }
}

fn square_fan() -> Mesh {
    fixed_mesh(
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.5, 0.5]],
        vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]],
        vec![0, 1, 2, 3],
    )
}

fn settings(area: f64) -> MeshSettings {
    MeshSettings {
        quarter_arc_segments: 32,
        max_triangle_area: area,
        max_vertices: 20_000,
        max_triangles: 40_000,
    }
}

#[test]
fn square_center_has_fixed_discrete_stiffness_load_phi_and_j() {
    let mesh = square_fan();
    let assembly = assemble(&mesh).unwrap();
    assert_eq!(assembly.stiffness.nrows(), 1);
    assert_eq!(assembly.stiffness.val(), &[4.0]);
    assert!((assembly.load[0] - 2.0 / 3.0).abs() < 1e-15);
    let result = solve(&mesh).unwrap();
    assert!((result.j_hat - 1.0 / 9.0).abs() < 1e-15);
    assert!((result.max_phi - 1.0 / 6.0).abs() < 1e-15);
    assert_eq!(result.min_phi, 0.0);
    assert_eq!(result.free_dofs, 1);
    assert_eq!(result.nnz, 1);
    assert!(result.relative_residual <= 1e-10);
    let full = restore_phi(&assembly, &faer::Mat::from_fn(1, 1, |_, _| 1.0 / 6.0)).unwrap();
    assert_eq!(&full[..4], &[0.0; 4]);
    assert_eq!(full[4], 1.0 / 6.0);
    assert!([
        result.assembly_seconds,
        result.factorization_seconds,
        result.solve_seconds
    ]
    .into_iter()
    .all(|v| v.is_finite() && v >= 0.0));
}

#[test]
fn two_free_nodes_keep_both_off_diagonals_and_exact_discrete_integral() {
    let points = (0..3)
        .flat_map(|y| (0..4).map(move |x| [x as f64, y as f64]))
        .collect();
    let mut triangles = Vec::new();
    for y in 0..2 {
        for x in 0..3 {
            let a = y * 4 + x;
            triangles.push([a, a + 1, a + 5]);
            triangles.push([a, a + 5, a + 4]);
        }
    }
    let mesh = fixed_mesh(points, triangles, vec![0, 1, 2, 3, 4, 7, 8, 9, 10, 11]);
    let assembly = assemble(&mesh).unwrap();
    assert_eq!(assembly.stiffness.val_of_col(0), &[4.0, -1.0]);
    assert_eq!(assembly.stiffness.val_of_col(1), &[-1.0, 4.0]);
    assert!(assembly.load.iter().all(|f| (*f - 2.0).abs() < 1e-15));
    let result = solve(&mesh).unwrap();
    assert!((result.max_phi - 2.0 / 3.0).abs() < 1e-14);
    assert!((result.j_hat - 8.0 / 3.0).abs() < 1e-14);
    assert_eq!(result.nnz, 4);
}

#[test]
fn invalid_indices_coordinates_and_degenerate_elements_return_reasons() {
    let mut mesh = square_fan();
    mesh.boundary_vertices.push(usize::MAX);
    assert!(solve(&mesh).unwrap_err().contains("外周節点索引"));
    let mut mesh = square_fan();
    mesh.boundary_vertices.push(0);
    assert!(solve(&mesh).unwrap_err().contains("重複"));
    let mut mesh = square_fan();
    mesh.triangles[0][0] = usize::MAX;
    assert!(solve(&mesh).unwrap_err().contains("要素0の節点索引"));
    for coordinate in [f64::NAN, f64::INFINITY] {
        let mut mesh = square_fan();
        mesh.points[0][0] = coordinate;
        assert!(solve(&mesh).unwrap_err().contains("非有限"));
    }
    for triangle in [[0, 4, 1], [0, 0, 4]] {
        let mut mesh = square_fan();
        mesh.triangles[0] = triangle;
        assert!(solve(&mesh).unwrap_err().contains("反転・退化"));
    }
    let mut mesh = square_fan();
    mesh.points[4] = [0.5, 1e-20];
    assert!(solve(&mesh).unwrap_err().contains("数値的に特異"));
    assert!(zeros::<f64>(usize::MAX, "試験")
        .unwrap_err()
        .contains("資源確保"));
}

#[test]
fn missing_boundary_zero_dofs_and_isolated_free_nodes_are_not_zero_j_success() {
    let mut mesh = square_fan();
    mesh.boundary_vertices.clear();
    assert!(solve(&mesh).unwrap_err().contains("外周節点が必要"));
    let mut mesh = square_fan();
    mesh.boundary_vertices.push(4);
    assert!(solve(&mesh).unwrap_err().contains("自由度が0"));
    let mut mesh = square_fan();
    mesh.points.push([2.0, 2.0]);
    assert!(solve(&mesh).unwrap_err().contains("孤立"));
    let mut mesh = square_fan();
    mesh.triangles.clear();
    assert!(solve(&mesh).unwrap_err().contains("三角形"));
}

#[test]
fn non_spd_disconnected_component_has_no_regularization_or_fallback() {
    let mut mesh = square_fan();
    mesh.points.extend([[2.0, 0.0], [3.0, 0.0], [2.0, 1.0]]);
    mesh.triangles.push([5, 6, 7]);
    assert!(solve(&mesh).unwrap_err().contains("LLt数値分解失敗"));
}

#[test]
fn residual_uses_original_system_and_stable_norm_and_rejects_nonfinite_phi() {
    let assembly = assemble(&square_fan()).unwrap();
    assert!(relative_residual(&assembly, &faer::Mat::zeros(1, 1))
        .unwrap_err()
        .contains("相対残差"));
    let mut large = assembly;
    large.stiffness.val_mut()[0] = 1.0;
    large.load[0] = 1e200;
    assert_eq!(
        relative_residual(&large, &faer::Mat::from_fn(1, 1, |_, _| 1e200)).unwrap(),
        0.0
    );
    assert!(
        restore_phi(&large, &faer::Mat::from_fn(1, 1, |_, _| f64::NAN))
            .unwrap_err()
            .contains("Phiが非有限")
    );
}

#[test]
fn real_circle_rectangle_and_ellipse_refinement_approaches_independent_values() {
    let pi = std::f64::consts::PI;
    let expected = [
        pi / 32.0,
        0.028_585_209_639_946_346,
        pi * 0.5f64.powi(3) * 0.25f64.powi(3) / (0.5f64.powi(2) + 0.25f64.powi(2)),
    ];
    for (shape, exact) in expected.into_iter().enumerate() {
        let mut previous_error = f64::INFINITY;
        for area in [1.0 / 128.0, 1.0 / 512.0, 1.0 / 2048.0] {
            let mesh = match shape {
                0 => generate(
                    RoundedCore {
                        b: 1.0,
                        h: 1.0,
                        r: 0.5,
                    },
                    settings(area),
                ),
                1 => generate(
                    RoundedCore {
                        b: 1.0,
                        h: 0.5,
                        r: 0.0,
                    },
                    settings(area),
                ),
                _ => generate_contour(
                    (0..128)
                        .map(|i| {
                            let angle = 2.0 * pi * i as f64 / 128.0;
                            [0.5 * angle.cos(), 0.25 * angle.sin()]
                        })
                        .collect(),
                    settings(area),
                ),
            }
            .unwrap();
            let result = solve(&mesh).unwrap();
            let error = (result.j_hat / exact - 1.0).abs();
            assert!(result.j_hat > 0.0 && result.relative_residual <= 1e-10);
            assert!(
                error < previous_error,
                "shape={shape}, error={error}, previous={previous_error}"
            );
            previous_error = error;
        }
        assert!(
            previous_error < 0.02,
            "shape={shape}, error={previous_error}"
        );
    }
}

#[test]
fn actual_node_rigid_transform_preserves_the_discrete_j() {
    let mut mesh = generate(
        RoundedCore {
            b: 1.0,
            h: 0.75,
            r: 0.125,
        },
        settings(1.0 / 512.0),
    )
    .unwrap();
    let original = solve(&mesh).unwrap();
    let (sin, cos) = (std::f64::consts::PI / 7.0).sin_cos();
    for point in &mut mesh.points {
        *point = [
            point[0] * cos - point[1] * sin + 13.0,
            point[0] * sin + point[1] * cos - 7.0,
        ];
    }
    let transformed = solve(&mesh).unwrap();
    assert!((transformed.j_hat / original.j_hat - 1.0).abs() < 1e-12);
    assert!(transformed.relative_residual <= 1e-10);
}
