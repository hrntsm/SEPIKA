use std::collections::{BTreeMap, BTreeSet};

use spade::{
    AngleLimit, ConstrainedDelaunayTriangulation, Point2, RefinementParameters, Triangulation,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct RoundedCore {
    pub b: f64,
    pub h: f64,
    pub r: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MeshSettings {
    pub quarter_arc_segments: usize,
    pub max_triangle_area: f64,
    pub max_vertices: usize,
    pub max_triangles: usize,
}

#[derive(Debug)]
pub(super) struct Mesh {
    pub points: Vec<[f64; 2]>,
    pub triangles: Vec<[usize; 3]>,
    pub boundary_vertices: Vec<usize>,
    pub quality: MeshQuality,
}

#[derive(Debug)]
pub(super) struct MeshQuality {
    pub polygon_area: f64,
    pub triangle_area: f64,
    pub max_actual_area: f64,
    pub min_signed_double_area: f64,
    pub min_area_edge_ratio: f64,
    pub min_angle_rad: f64,
    pub original_boundary_segments: usize,
    pub boundary_edges: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub free_vertices: usize,
    pub fingerprint: u64,
}

pub(super) fn generate(core: RoundedCore, settings: MeshSettings) -> Result<Mesh, String> {
    let contour = rounded_contour(core, settings.quarter_arc_segments, settings.max_vertices)?;
    mesh_contour(contour, settings, Some([0.0, 0.0]))
}

fn rounded_contour(core: RoundedCore, n: usize, cap: usize) -> Result<Vec<[f64; 2]>, String> {
    let RoundedCore { b, h, r } = core;
    if !b.is_finite()
        || !h.is_finite()
        || !r.is_finite()
        || b <= 0.0
        || h <= 0.0
        || r < 0.0
        || r > b.min(h) / 2.0
        || b.max(h) != 1.0
    {
        return Err("コア輪郭: 長辺1の正規化寸法と0≤r≤min(b,h)/2が必要です".into());
    }
    if n == 0 {
        return Err("コア輪郭: 四分円分割数は正である必要があります".into());
    }
    let x = b / 2.0;
    let y = h / 2.0;
    if r == 0.0 {
        return Ok(vec![[-x, -y], [x, -y], [x, y], [-x, y]]);
    }
    let capacity = n
        .checked_add(1)
        .and_then(|v| v.checked_mul(4))
        .ok_or("コア輪郭: 分割数が整数上限を超えます")?;
    if capacity
        > cap
            .checked_add(4)
            .ok_or("コア輪郭: 頂点上限が整数上限を超えます")?
    {
        return Err("コア輪郭: 初期輪郭が頂点上限を超えます".into());
    }
    let mut points = Vec::with_capacity(capacity);
    for quadrant in 0..4 {
        for i in 0..=n {
            let (sin, cos) = if i == 0 {
                (0.0, 1.0)
            } else if i == n {
                (1.0, 0.0)
            } else {
                ((i as f64 / n as f64) * std::f64::consts::FRAC_PI_2).sin_cos()
            };
            let point = match quadrant {
                0 => [x - r * (1.0 - sin), -y + r * (1.0 - cos)],
                1 => [x - r * (1.0 - cos), y - r * (1.0 - sin)],
                2 => [-x + r * (1.0 - sin), y - r * (1.0 - cos)],
                3 => [-x + r * (1.0 - cos), -y + r * (1.0 - sin)],
                _ => unreachable!(),
            };
            let same_tangent =
                i == 0 && quadrant > 0 && if quadrant % 2 == 1 { r == y } else { r == x };
            if !same_tangent {
                points.push(point);
            }
        }
    }
    if r == x {
        points.pop();
    }

    Ok(points)
}

fn subtract(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn edge_key(a: usize, b: usize) -> [usize; 2] {
    [a.min(b), a.max(b)]
}

fn mesh_contour(
    contour: Vec<[f64; 2]>,
    settings: MeshSettings,
    interior_point: Option<[f64; 2]>,
) -> Result<Mesh, String> {
    if !settings.max_triangle_area.is_finite()
        || settings.max_triangle_area <= 0.0
        || settings.max_vertices < 3
        || settings.max_triangles == 0
    {
        return Err("コアメッシュ: 面積と資源上限が不正です".into());
    }
    if contour.len() < 3 || contour.len() > settings.max_vertices {
        return Err("コアメッシュ: 初期輪郭の頂点数が不正または上限超過です".into());
    }
    let mut unique = BTreeSet::new();
    for p in &contour {
        if p.iter().any(|v| !v.is_finite())
            || !unique.insert(p.map(|v| if v == 0.0 { 0 } else { v.to_bits() }))
        {
            return Err("コアメッシュ: 非有限座標または重複輪郭頂点です".into());
        }
    }
    let origin = contour[0];
    let polygon_area = (1..contour.len() - 1)
        .map(|i| {
            cross(
                subtract(contour[i], origin),
                subtract(contour[i + 1], origin),
            ) / 2.0
        })
        .sum::<f64>();
    if !polygon_area.is_finite() || polygon_area <= 0.0 {
        return Err("コアメッシュ: 正向きの有限な輪郭面積が必要です".into());
    }
    let edges = (0..contour.len())
        .map(|i| [i, (i + 1) % contour.len()])
        .collect();
    let mut conflicts = 0usize;
    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::try_bulk_load_cdt(
        contour.iter().map(|p| Point2::new(p[0], p[1])).collect(),
        edges,
        |_| conflicts += 1,
    )
    .map_err(|e| format!("コアメッシュ: CDT入力失敗: {e:?}"))?;
    if conflicts != 0 {
        return Err(format!("コアメッシュ: 制約辺競合{conflicts}件"));
    }
    refine(&mut cdt, settings)?;
    let mesh = audit_cdt(&cdt, &contour, polygon_area, settings)?;
    if let Some(point) = interior_point.filter(|_| mesh.quality.free_vertices == 0) {
        let previous_vertices = cdt.num_vertices();
        if previous_vertices >= settings.max_vertices {
            return Err("コアメッシュ: 内部節点の追加に必要な頂点余裕がありません".into());
        }
        cdt.insert(Point2::new(point[0], point[1]))
            .map_err(|e| format!("コアメッシュ: 内部節点の追加失敗: {e:?}"))?;
        if cdt.num_vertices() != previous_vertices + 1 {
            return Err("コアメッシュ: 内部節点の追加で頂点数が1増加しませんでした".into());
        }
        refine(&mut cdt, settings)?;
        let mesh = audit_cdt(&cdt, &contour, polygon_area, settings)?;
        if mesh.quality.free_vertices == 0 {
            return Err("コアメッシュ: 内部節点追加後も自由節点がありません".into());
        }
        return Ok(mesh);
    }
    Ok(mesh)
}

fn refine(
    cdt: &mut ConstrainedDelaunayTriangulation<Point2<f64>>,
    settings: MeshSettings,
) -> Result<(), String> {
    let result = cdt.refine(
        RefinementParameters::new()
            .exclude_outer_faces(true)
            .with_max_allowed_area(settings.max_triangle_area)
            .with_angle_limit(AngleLimit::from_deg(25.0))
            .with_max_additional_vertices(settings.max_vertices - cdt.num_vertices()),
    );
    if !result.refinement_complete {
        return Err("コアメッシュ: 頂点上限内でrefinementが完了しませんでした".into());
    }
    if cdt.num_vertices() > settings.max_vertices || cdt.num_inner_faces() > settings.max_triangles
    {
        return Err("コアメッシュ: 頂点または三角形上限超過です".into());
    }
    if !result.excluded_faces.is_empty() {
        return Err("コアメッシュ: 限定凸輪郭の外側に有限面が存在します".into());
    }
    Ok(())
}

fn audit_cdt(
    cdt: &ConstrainedDelaunayTriangulation<Point2<f64>>,
    contour: &[[f64; 2]],
    polygon_area: f64,
    settings: MeshSettings,
) -> Result<Mesh, String> {
    let points: Vec<_> = cdt
        .vertices()
        .map(|v| {
            let p = v.position();
            [p.x, p.y]
        })
        .collect();
    for p in contour {
        if !points.contains(p) {
            return Err("コアメッシュ: 元輪郭の接線点または弦端点が失われました".into());
        }
    }
    let triangles: Vec<_> = cdt
        .inner_faces()
        .map(|f| f.vertices().map(|v| v.index()))
        .collect();
    let constraints: BTreeSet<_> = cdt
        .undirected_edges()
        .filter(|e| e.is_constraint_edge())
        .map(|e| {
            let v = e.vertices();
            edge_key(v[0].index(), v[1].index())
        })
        .collect();
    audit(
        points,
        triangles,
        constraints,
        contour,
        polygon_area,
        settings,
    )
}

fn audit(
    points: Vec<[f64; 2]>,
    triangles: Vec<[usize; 3]>,
    constraints: BTreeSet<[usize; 2]>,
    contour: &[[f64; 2]],
    polygon_area: f64,
    settings: MeshSettings,
) -> Result<Mesh, String> {
    let mut incidence = BTreeMap::<[usize; 2], usize>::new();
    let mut area = 0.0f64;
    let mut max_area = 0.0f64;
    let mut min_d = f64::INFINITY;
    let mut min_ratio = f64::INFINITY;
    let mut min_angle = f64::INFINITY;
    let mut fingerprint = 0xcbf29ce484222325u64;
    for p in &points {
        for v in p {
            fingerprint = (fingerprint ^ v.to_bits()).wrapping_mul(0x100000001b3);
        }
    }
    for t in &triangles {
        let p = t.map(|i| points[i]);
        let d = cross(subtract(p[1], p[0]), subtract(p[2], p[0]));
        let lengths = [
            dot(subtract(p[1], p[0]), subtract(p[1], p[0])),
            dot(subtract(p[2], p[1]), subtract(p[2], p[1])),
            dot(subtract(p[0], p[2]), subtract(p[0], p[2])),
        ];
        let longest = lengths.into_iter().fold(0.0f64, f64::max);
        let ratio = d / longest;
        if !d.is_finite() || !ratio.is_finite() || d <= 0.0 || ratio <= 128.0 * f64::EPSILON {
            return Err("コアメッシュ: 反転・退化または数値的に特異な三角形です".into());
        }
        area += d / 2.0;
        max_area = max_area.max(d / 2.0);
        min_d = min_d.min(d);
        min_ratio = min_ratio.min(ratio);
        for i in 0..3 {
            let u = subtract(p[(i + 1) % 3], p[i]);
            let v = subtract(p[(i + 2) % 3], p[i]);
            min_angle = min_angle.min(cross(u, v).atan2(dot(u, v)));
            *incidence.entry(edge_key(t[i], t[(i + 1) % 3])).or_default() += 1;
            fingerprint = (fingerprint ^ t[i] as u64).wrapping_mul(0x100000001b3);
        }
    }
    if triangles.is_empty() || incidence.values().any(|n| *n > 2) {
        return Err("コアメッシュ: 空または非多様体のメッシュです".into());
    }
    if max_area > settings.max_triangle_area * (1.0 + 128.0 * f64::EPSILON) {
        return Err("コアメッシュ: 実最大三角形面積が指定値を超えます".into());
    }
    if !area.is_finite()
        || (area - polygon_area).abs()
            > polygon_area * 4096.0 * f64::EPSILON * (triangles.len() as f64).sqrt()
    {
        return Err("コアメッシュ: 輪郭面積と三角形面積和が一致しません".into());
    }
    let boundary: BTreeSet<_> = incidence
        .iter()
        .filter(|(_, n)| **n == 1)
        .map(|(e, _)| *e)
        .collect();
    if boundary != constraints {
        return Err("コアメッシュ: 境界辺と制約辺が一致しません".into());
    }
    let mut neighbors = BTreeMap::<usize, Vec<usize>>::new();
    let mut intervals = vec![Vec::<(f64, f64)>::new(); contour.len()];
    for [a, b] in &boundary {
        neighbors.entry(*a).or_default().push(*b);
        neighbors.entry(*b).or_default().push(*a);
        let mut found = None;
        for i in 0..contour.len() {
            let origin = contour[i];
            let v = subtract(contour[(i + 1) % contour.len()], origin);
            let len2 = dot(v, v);
            let len = len2.sqrt();
            let coord = origin
                .into_iter()
                .chain(points[*a])
                .chain(points[*b])
                .map(f64::abs)
                .fold(1.0, f64::max);
            let tolerance = 128.0 * f64::EPSILON * coord;
            let u = subtract(points[*a], origin);
            let w = subtract(points[*b], origin);
            let ta = dot(u, v) / len2;
            let tb = dot(w, v) / len2;
            if cross(u, v).abs() <= tolerance * len
                && cross(w, v).abs() <= tolerance * len
                && ta.min(tb) >= -tolerance / len
                && ta.max(tb) <= 1.0 + tolerance / len
            {
                if found.is_some() {
                    return Err("コアメッシュ: 境界辺の元区間帰属が曖昧です".into());
                }
                found = Some((i, ta.min(tb), ta.max(tb)));
            }
        }
        let (i, lo, hi) = found.ok_or("コアメッシュ: 境界辺が元の弦・直線区間から逸脱しました")?;
        intervals[i].push((lo, hi));
    }
    for (i, spans) in intervals.iter_mut().enumerate() {
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let len = dot(
            subtract(contour[(i + 1) % contour.len()], contour[i]),
            subtract(contour[(i + 1) % contour.len()], contour[i]),
        )
        .sqrt();
        let coord = contour[i].into_iter().map(f64::abs).fold(1.0, f64::max);
        let tolerance = 256.0 * f64::EPSILON * coord / len;
        let mut end = 0.0;
        for &(lo, hi) in spans.iter() {
            if (lo - end).abs() > tolerance || hi <= lo {
                return Err("コアメッシュ: 元境界区間の被覆に欠落・重複があります".into());
            }
            end = hi;
        }
        if spans.is_empty() || (end - 1.0).abs() > tolerance {
            return Err("コアメッシュ: 元境界区間が完全被覆されていません".into());
        }
    }
    if neighbors.values().any(|v| v.len() != 2)
        || neighbors.is_empty()
        || points.len().checked_add(triangles.len()) != incidence.len().checked_add(1)
    {
        return Err("コアメッシュ: 境界次数またはEuler関係が不正です".into());
    }
    let start = *neighbors.keys().next().unwrap();
    let mut visited = BTreeSet::new();
    let mut stack = vec![start];
    while let Some(v) = stack.pop() {
        if visited.insert(v) {
            stack.extend(&neighbors[&v]);
        }
    }
    if visited.len() != neighbors.len() {
        return Err("コアメッシュ: 境界が単一閉ループではありません".into());
    }
    let boundary_vertices: Vec<_> = neighbors.keys().copied().collect();
    let quality = MeshQuality {
        polygon_area,
        triangle_area: area,
        max_actual_area: max_area,
        min_signed_double_area: min_d,
        min_area_edge_ratio: min_ratio,
        min_angle_rad: min_angle,
        original_boundary_segments: contour.len(),
        boundary_edges: boundary.len(),
        vertices: points.len(),
        triangles: triangles.len(),
        free_vertices: points.len() - boundary_vertices.len(),
        fingerprint,
    };
    Ok(Mesh {
        points,
        triangles,
        boundary_vertices,
        quality,
    })
}

#[cfg(test)]
pub(super) fn generate_contour(
    contour: Vec<[f64; 2]>,
    settings: MeshSettings,
) -> Result<Mesh, String> {
    mesh_contour(contour, settings, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(area: f64) -> MeshSettings {
        MeshSettings {
            quarter_arc_segments: 8,
            max_triangle_area: area,
            max_vertices: 20_000,
            max_triangles: 40_000,
        }
    }

    fn core(r: f64) -> RoundedCore {
        RoundedCore { b: 1.0, h: 0.75, r }
    }

    #[test]
    fn existing_interior_node_meshes_keep_the_original_refinement() {
        for c in [
            core(0.0),
            core(0.125),
            RoundedCore {
                b: 1.0,
                h: 1.0,
                r: 0.5,
            },
        ] {
            let s = settings(1.0 / 512.0);
            let original =
                generate_contour(rounded_contour(c, 8, s.max_vertices).unwrap(), s).unwrap();
            let production = generate(c, s).unwrap();
            assert!(original.quality.free_vertices > 0);
            assert_eq!(production.points, original.points);
            assert_eq!(production.triangles, original.triangles);
            assert_eq!(production.boundary_vertices, original.boundary_vertices);
            assert_eq!(production.quality.fingerprint, original.quality.fingerprint);
            println!("baseline {c:?}: {:?}", original.quality);
        }
    }

    #[test]
    fn elongated_coarse_rectangle_gains_free_nodes_without_changing_its_domain() {
        let c = RoundedCore {
            b: 1.0,
            h: 0.1,
            r: 0.0,
        };
        let s = settings(1.0 / 128.0);
        let contour = rounded_contour(c, 8, s.max_vertices).unwrap();
        let original = generate_contour(contour.clone(), s).unwrap();
        assert_eq!(original.quality.free_vertices, 0);
        let mesh = generate(c, s).unwrap();
        let q = &mesh.quality;
        assert!(q.free_vertices > 0);
        assert!(mesh.points.contains(&[0.0, 0.0]));
        assert!(!mesh
            .boundary_vertices
            .iter()
            .any(|i| mesh.points[*i] == [0.0, 0.0]));
        assert_eq!(q.polygon_area, 0.1);
        assert_eq!(q.polygon_area, original.quality.polygon_area);
        assert!((q.triangle_area - 0.1).abs() < 1e-14);
        assert_eq!(q.original_boundary_segments, 4);
        assert_eq!(q.boundary_edges, mesh.boundary_vertices.len());
        assert!(q.max_actual_area <= s.max_triangle_area * (1.0 + 128.0 * f64::EPSILON));
        assert!(q.min_signed_double_area > 0.0 && q.min_angle_rad > 0.0);
        assert!(q.vertices <= s.max_vertices && q.triangles <= s.max_triangles);
        for point in &contour {
            assert!(mesh
                .boundary_vertices
                .iter()
                .any(|i| mesh.points[*i] == *point));
        }
        println!("completed coarse rect10: {q:?}");

        let cap = MeshSettings {
            max_vertices: original.quality.vertices + 1,
            ..s
        };
        assert!(generate_contour(contour.clone(), cap).is_ok());
        assert!(generate(c, cap).unwrap_err().contains("refinement"));
        let triangle_cap = MeshSettings {
            max_triangles: original.quality.triangles,
            ..s
        };
        assert!(generate_contour(contour.clone(), triangle_cap).is_ok());
        assert!(generate(c, triangle_cap)
            .unwrap_err()
            .contains("三角形上限"));

        let shifted = generate_contour(
            contour.iter().map(|p| [p[0] + 13.0, p[1] - 7.0]).collect(),
            s,
        )
        .unwrap();
        assert_eq!(shifted.quality.free_vertices, 0);
        assert!(shifted.points.iter().all(|p| p[0] >= 12.5 && p[1] <= -6.95));
    }

    #[test]
    fn rectangle_refinement_preserves_domain_and_changes_mesh() {
        let mut previous = None::<Mesh>;
        for area in [1.0 / 128.0, 1.0 / 512.0, 1.0 / 2048.0] {
            let mesh = generate(core(0.0), settings(area)).unwrap();
            let q = &mesh.quality;
            assert_eq!(q.polygon_area, 0.75);
            assert!((q.triangle_area - 0.75).abs() < 1e-12);
            assert!(q.max_actual_area <= area * (1.0 + 1e-13));
            assert!(q.min_signed_double_area > 0.0 && q.min_area_edge_ratio > 128.0 * f64::EPSILON);
            assert!(q.min_angle_rad.is_finite() && q.min_angle_rad > 0.0);
            assert_eq!(q.original_boundary_segments, 4);
            assert!(q.boundary_edges > 4);
            assert_eq!(q.boundary_edges, mesh.boundary_vertices.len());
            assert_eq!(q.vertices, mesh.points.len());
            assert_eq!(q.triangles, mesh.triangles.len());
            assert_eq!(q.free_vertices + q.boundary_edges, q.vertices);
            assert!(q.free_vertices > 0);
            if let Some(old) = previous {
                assert_eq!(q.polygon_area, old.quality.polygon_area);
                assert!(q.max_actual_area < old.quality.max_actual_area);
                assert!(q.free_vertices > old.quality.free_vertices);
                assert_ne!(q.fingerprint, old.quality.fingerprint);
            }
            previous = Some(mesh);
        }
    }

    #[test]
    fn arcs_retain_all_tangents_and_internal_refinement_keeps_chords() {
        let c = core(0.125);
        let expected = [
            [0.375, -0.375],
            [0.5, -0.25],
            [0.5, 0.25],
            [0.375, 0.375],
            [-0.375, 0.375],
            [-0.5, 0.25],
            [-0.5, -0.25],
            [-0.375, -0.375],
        ];
        let a = generate(c, settings(1.0 / 128.0)).unwrap();
        let b = generate(c, settings(1.0 / 2048.0)).unwrap();
        for mesh in [&a, &b] {
            for p in expected {
                assert!(mesh.boundary_vertices.iter().any(|i| mesh.points[*i] == p));
            }
            assert_eq!(mesh.quality.original_boundary_segments, 36);
            assert!(mesh.quality.polygon_area < 0.75);
        }
        assert_eq!(a.quality.polygon_area, b.quality.polygon_area);
        assert!(b.quality.boundary_edges > a.quality.original_boundary_segments);
        assert_ne!(a.quality.fingerprint, b.quality.fingerprint);
    }

    #[test]
    fn half_side_endpoints_remove_only_identical_tangencies() {
        for (c, count) in [
            (
                RoundedCore {
                    b: 1.0,
                    h: 1.0,
                    r: 0.5,
                },
                32,
            ),
            (core(0.375), 34),
            (
                RoundedCore {
                    b: 0.75,
                    h: 1.0,
                    r: 0.375,
                },
                34,
            ),
        ] {
            let contour = rounded_contour(c, 8, 100).unwrap();
            assert_eq!(contour.len(), count);
            let mesh = generate(c, settings(1.0 / 512.0)).unwrap();
            assert_eq!(mesh.quality.original_boundary_segments, count);
            assert!(mesh.quality.free_vertices > 0);
        }
    }

    #[test]
    fn positive_small_radius_is_retained_in_boundary_series() {
        for r in [1e-6, 1e-3] {
            let c = core(r);
            let contour = rounded_contour(c, 8, 100).unwrap();
            assert_eq!(contour.len(), 36);
            assert_ne!(contour, rounded_contour(core(0.0), 8, 100).unwrap());
            let mesh = generate(c, settings(1.0 / 128.0)).unwrap();
            assert_eq!(mesh.quality.original_boundary_segments, 36);
        }
        let mut coarse = settings(1.0 / 512.0);
        coarse.quarter_arc_segments = 4;
        let a = generate(core(0.25), coarse).unwrap();
        let b = generate(core(0.25), settings(1.0 / 512.0)).unwrap();
        assert!(b.quality.polygon_area > a.quality.polygon_area);
        assert!(b.quality.original_boundary_segments > a.quality.original_boundary_segments);
    }

    #[test]
    fn test_only_ellipse_and_actual_rigid_transform_use_the_same_mesher() {
        let ellipse: Vec<_> = (0..64)
            .map(|i| {
                let theta = 2.0 * std::f64::consts::PI * i as f64 / 64.0;
                [0.5 * theta.cos(), 0.25 * theta.sin()]
            })
            .collect();
        let angle = std::f64::consts::PI / 7.0;
        let (sin, cos) = angle.sin_cos();
        let transformed = ellipse
            .iter()
            .map(|p| {
                [
                    p[0] * cos - p[1] * sin + 13.0,
                    p[0] * sin + p[1] * cos - 7.0,
                ]
            })
            .collect();
        let a = generate_contour(ellipse, settings(1.0 / 512.0)).unwrap();
        let b = generate_contour(transformed, settings(1.0 / 512.0)).unwrap();
        assert!((a.quality.polygon_area - b.quality.polygon_area).abs() < 1e-13);
        assert!((a.quality.triangle_area - b.quality.triangle_area).abs() < 1e-12);
        assert!(b.points.iter().all(|p| p[0] > 12.0 && p[1] < -6.0));
        assert!(a.quality.free_vertices > 0 && b.quality.free_vertices > 0);
    }

    #[test]
    fn invalid_dimensions_and_settings_return_reasons() {
        for c in [
            core(-0.1),
            core(f64::NAN),
            core(0.4),
            RoundedCore {
                b: 0.0,
                h: 1.0,
                r: 0.0,
            },
            RoundedCore {
                b: 2.0,
                h: 1.0,
                r: 0.0,
            },
        ] {
            assert!(!generate(c, settings(0.01)).unwrap_err().is_empty());
        }
        for s in [
            MeshSettings {
                quarter_arc_segments: 0,
                ..settings(0.01)
            },
            settings(0.0),
            settings(f64::INFINITY),
            MeshSettings {
                max_vertices: 2,
                ..settings(0.01)
            },
        ] {
            assert!(!generate(core(0.1), s).unwrap_err().is_empty());
        }
        assert!(generate(
            core(0.1),
            MeshSettings {
                quarter_arc_segments: usize::MAX,
                ..settings(0.01)
            }
        )
        .unwrap_err()
        .contains("整数上限"));
    }

    #[test]
    fn incomplete_refinement_and_triangle_cap_are_errors() {
        let exhausted = generate(
            core(0.0),
            MeshSettings {
                max_vertices: 4,
                ..settings(1e-6)
            },
        )
        .unwrap_err();
        assert!(exhausted.contains("refinement"));
        let too_many = generate(
            core(0.0),
            MeshSettings {
                max_triangles: 1,
                ..settings(0.01)
            },
        )
        .unwrap_err();
        assert!(too_many.contains("三角形上限"));
        assert!(generate(
            core(0.2),
            MeshSettings {
                max_vertices: 8,
                ..settings(0.01)
            }
        )
        .unwrap_err()
        .contains("初期輪郭"));
    }

    #[test]
    fn duplicate_clockwise_and_conflicting_contours_are_rejected() {
        let square = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let mut clockwise = square.clone();
        clockwise.reverse();
        assert!(generate_contour(clockwise, settings(0.01))
            .unwrap_err()
            .contains("正向き"));
        let mut duplicate = square;
        duplicate.push([0.0, 0.0]);
        assert!(generate_contour(duplicate, settings(0.01))
            .unwrap_err()
            .contains("重複"));
        let crossed = vec![
            [0.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [1.0, 0.0],
            [2.0, 0.0],
            [2.0, 2.0],
            [0.0, 2.0],
        ];
        assert!(generate_contour(crossed, settings(0.01))
            .unwrap_err()
            .contains("競合"));
    }

    #[test]
    fn audit_rejects_reversed_degenerate_and_uncovered_meshes() {
        let points = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        for triangle in [[0, 2, 1], [0, 1, 1]] {
            assert!(audit(
                points.clone(),
                vec![triangle],
                BTreeSet::new(),
                &points,
                0.5,
                settings(1.0)
            )
            .unwrap_err()
            .contains("退化"));
        }
        assert!(audit(
            points.clone(),
            vec![[0, 1, 2]],
            BTreeSet::new(),
            &points,
            0.5,
            settings(1.0)
        )
        .unwrap_err()
        .contains("境界辺と制約辺"));
    }
}
