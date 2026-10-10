//! 全周支持・無開口の単純多角形床の有限線分最近接分配。

use sepika_core::geom::polygon as geom_polygon;

use super::fem::fem_uniform;
use super::geometry::edge_len;
use super::types::{push_edge, BeamLoad, LoadShape};

/// 格子寸法 [mm]、局所境界箱からの位相 [ピッチ比]、資源上限と辺別要求誤差 [mm²]。
#[derive(Clone, Copy, Debug)]
pub struct PolygonIntegrationOptions {
    pub cell_size_mm: f64,
    pub phase: [f64; 2],
    pub max_cells: usize,
    pub max_edge_error_mm2: Option<f64>,
}

impl Default for PolygonIntegrationOptions {
    fn default() -> Self {
        Self {
            cell_size_mm: 100.0,
            phase: [0.0; 2],
            max_cells: 4_000_000,
            max_edge_error_mm2: None,
        }
    }
}

/// 辺順の負担面積と絶対誤差上界 [mm²]。精度未指定時も上界を返す。
#[derive(Clone, Debug)]
pub struct PolygonDistribution {
    pub edge_areas_mm2: Vec<f64>,
    pub edge_error_bounds_mm2: Vec<f64>,
    pub polygon_area_mm2: f64,
    pub unallocated_area_mm2: f64,
    pub cell_size_mm: [f64; 2],
    pub grid_phase: [f64; 2],
    pub distance_epsilon_mm: f64,
    pub area_roundoff_bound_mm2: f64,
    pub rule: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PolygonDistributionError {
    InvalidInput(String),
    Unsupported(String),
    ResourceLimit {
        required_cells: f64,
        max_cells: usize,
    },
    PrecisionNotMet {
        bound_mm2: f64,
        requested_mm2: f64,
    },
    AreaMismatch {
        residual_mm2: f64,
        roundoff_bound_mm2: f64,
    },
}

impl std::fmt::Display for PolygonDistributionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(s) => write!(f, "多角形床の不正入力: {s}"),
            Self::Unsupported(s) => write!(f, "多角形床の未対応条件: {s}"),
            Self::ResourceLimit { required_cells, max_cells } => write!(f, "多角形床の格子資源上限: 必要 {required_cells} セル、上限 {max_cells} セル（粗格子への変更なし）"),
            Self::PrecisionNotMet { bound_mm2, requested_mm2 } => write!(f, "多角形床の要求精度未達: 辺別誤差上界 {bound_mm2} mm² > 要求 {requested_mm2} mm²。格子を細分化してください"),
            Self::AreaMismatch { residual_mm2, roundoff_bound_mm2 } => write!(f, "多角形床の面積保存未達: 残差 {residual_mm2} mm²、丸め上界 {roundoff_bound_mm2} mm²"),
        }
    }
}

impl std::error::Error for PolygonDistributionError {}

type Point = [f64; 2];

fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let on = |p: Point, q: Point, r: Point| {
        cross(p, q, r) == 0.0 && (0..2).all(|k| r[k] >= p[k].min(q[k]) && r[k] <= p[k].max(q[k]))
    };
    let ab_c = cross(a, b, c);
    let ab_d = cross(a, b, d);
    let cd_a = cross(c, d, a);
    let cd_b = cross(c, d, b);
    (ab_c.signum() != ab_d.signum()
        && cd_a.signum() != cd_b.signum()
        && ab_c != 0.0
        && ab_d != 0.0
        && cd_a != 0.0
        && cd_b != 0.0)
        || on(a, b, c)
        || on(a, b, d)
        || on(c, d, a)
        || on(c, d, b)
}

fn local_polygon(coords: &[[f64; 3]]) -> Result<Vec<Point>, PolygonDistributionError> {
    let invalid = |s: &str| PolygonDistributionError::InvalidInput(s.into());
    if coords.len() < 3 {
        return Err(invalid("境界頂点が3個未満"));
    }
    if coords.len() > 256 {
        return Err(PolygonDistributionError::Unsupported(
            "境界頂点数が256を超える".into(),
        ));
    }
    if coords.iter().flatten().any(|x| !x.is_finite()) {
        return Err(invalid("非有限座標"));
    }
    if coords.iter().any(|p| p[2] != coords[0][2]) {
        return Err(PolygonDistributionError::Unsupported("水平床以外".into()));
    }
    let origin = coords[0];
    let poly: Vec<_> = coords
        .iter()
        .map(|p| [p[0] - origin[0], p[1] - origin[1]])
        .collect();
    if poly.iter().flatten().any(|x| !x.is_finite()) {
        return Err(invalid("局所座標が非有限"));
    }
    let n = poly.len();
    for i in 0..n {
        let length_squared = (poly[i][0] - poly[(i + 1) % n][0]).powi(2)
            + (poly[i][1] - poly[(i + 1) % n][1]).powi(2);
        if !length_squared.is_finite() || length_squared < f64::MIN_POSITIVE {
            return Err(invalid("辺長の数値精度不足"));
        }
        if poly[i] == poly[(i + 1) % n] {
            return Err(invalid("退化辺"));
        }
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segments_intersect(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n]) {
                return Err(invalid(
                    "自己交差または非隣接辺の接触（開口を結ぶ境界は未対応）",
                ));
            }
        }
        let a = poly[(i + n - 1) % n];
        let b = poly[i];
        let c = poly[(i + 1) % n];
        if cross(a, b, c) == 0.0
            && (b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1]) <= 0.0
        {
            return Err(invalid("隣接辺の折返し"));
        }
    }
    let area = geom_polygon::area(&poly);
    if !area.is_finite() || area <= 0.0 {
        return Err(invalid("面積が非有限またはゼロ"));
    }
    Ok(poly)
}

fn triangulate(poly: &[Point]) -> Result<Vec<[Point; 3]>, PolygonDistributionError> {
    let mut indices: Vec<_> = (0..poly.len()).collect();
    if geom_polygon::signed_area(poly) < 0.0 {
        indices.reverse();
    }
    let mut triangles = Vec::new();
    while indices.len() > 3 {
        let mut found = false;
        for i in 0..indices.len() {
            let a = indices[(i + indices.len() - 1) % indices.len()];
            let b = indices[i];
            let c = indices[(i + 1) % indices.len()];
            if cross(poly[a], poly[b], poly[c]) == 0.0 {
                indices.remove(i);
                found = true;
                break;
            }
            if cross(poly[a], poly[b], poly[c]) <= 0.0 {
                continue;
            }
            if indices.iter().any(|&k| {
                k != a
                    && k != b
                    && k != c
                    && cross(poly[a], poly[b], poly[k]) >= 0.0
                    && cross(poly[b], poly[c], poly[k]) >= 0.0
                    && cross(poly[c], poly[a], poly[k]) >= 0.0
            }) {
                continue;
            }
            triangles.push([poly[a], poly[b], poly[c]]);
            indices.remove(i);
            found = true;
            break;
        }
        if !found {
            return Err(PolygonDistributionError::InvalidInput(
                "凸分割を確定できない数値精度".into(),
            ));
        }
    }
    triangles.push([poly[indices[0]], poly[indices[1]], poly[indices[2]]]);
    Ok(triangles)
}

fn clip_cell(triangle: &[Point; 3], lo: Point, hi: Point) -> Vec<Point> {
    let mut points = triangle.to_vec();
    for (axis, limit, keep_greater) in [
        (0, lo[0], true),
        (0, hi[0], false),
        (1, lo[1], true),
        (1, hi[1], false),
    ] {
        let input = std::mem::take(&mut points);
        if input.is_empty() {
            break;
        }
        let inside = |p: Point| {
            if keep_greater {
                p[axis] >= limit
            } else {
                p[axis] <= limit
            }
        };
        let mut a = *input.last().unwrap();
        for b in input {
            if inside(a) != inside(b) {
                let t = (limit - a[axis]) / (b[axis] - a[axis]);
                let mut p = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                p[axis] = limit;
                points.push(p);
            }
            if inside(b) {
                points.push(b);
            }
            a = b;
        }
    }
    points
}

fn projection(p: Point, a: Point, b: Point) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1])
}

fn endpoint_group(poly: &[Point], piece: &[Point], edge: usize) -> Vec<usize> {
    let n = poly.len();
    let prev = (edge + n - 1) % n;
    let next = (edge + 1) % n;
    if piece.iter().all(|&p| {
        projection(p, poly[edge], poly[next]) <= 0.0 && projection(p, poly[prev], poly[edge]) >= 1.0
    }) {
        vec![prev, edge]
    } else if piece.iter().all(|&p| {
        projection(p, poly[edge], poly[next]) >= 1.0
            && projection(p, poly[next], poly[(next + 1) % n]) <= 0.0
    }) {
        vec![edge, next]
    } else {
        vec![edge]
    }
}

fn nearest(poly: &[Point], p: Point, epsilon: f64) -> (Vec<f64>, Vec<usize>) {
    let distances: Vec<_> = (0..poly.len())
        .map(|e| {
            let a = poly[e];
            let b = poly[(e + 1) % poly.len()];
            let t = projection(p, a, b).clamp(0.0, 1.0);
            (p[0] - (a[0] + t * (b[0] - a[0]))).hypot(p[1] - (a[1] + t * (b[1] - a[1])))
        })
        .collect();
    let min = distances.iter().copied().fold(f64::INFINITY, f64::min);
    let winners = (0..poly.len())
        .filter(|&e| distances[e] - min <= epsilon)
        .collect();
    (distances, winners)
}

/// 座標 [mm] と全周支持辺のインデックスを受け、局所格子で辺負担面積を積分する。
/// 開口を含む複数輪郭・自由辺・支持欠落は未対応。要求精度未達はエラー。
pub fn integrate_polygon(
    coords: &[[f64; 3]],
    candidate_edges: &[usize],
    options: PolygonIntegrationOptions,
) -> Result<PolygonDistribution, PolygonDistributionError> {
    let poly = local_polygon(coords)?;
    let n = poly.len();
    if candidate_edges.is_empty() {
        return Err(PolygonDistributionError::InvalidInput(
            "支持候補なし".into(),
        ));
    }
    if candidate_edges.iter().any(|&e| e >= n) {
        return Err(PolygonDistributionError::InvalidInput(
            "支持辺参照不明".into(),
        ));
    }
    let mut candidates = candidate_edges.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    if candidates.len() != candidate_edges.len() {
        return Err(PolygonDistributionError::InvalidInput(
            "支持辺参照の重複".into(),
        ));
    }
    if candidates.len() != n {
        return Err(PolygonDistributionError::Unsupported(
            "自由辺・支持欠落（全周支持のみ対応、開口輪郭は入力不可）".into(),
        ));
    }
    let h = options.cell_size_mm;
    if !h.is_finite()
        || h <= 0.0
        || h > 100.0
        || options
            .phase
            .iter()
            .any(|p| !p.is_finite() || *p < 0.0 || *p >= 1.0)
        || options
            .max_edge_error_mm2
            .is_some_and(|x| !x.is_finite() || x < 0.0)
    {
        return Err(PolygonDistributionError::InvalidInput(
            "格子寸法・位相・要求誤差が不正".into(),
        ));
    }
    let (lo, hi) = geom_polygon::bounding_box(&poly);
    let epsilon = 64.0 * f64::EPSILON * (hi[0] - lo[0]).hypot(hi[1] - lo[1]).max(1.0);
    let start = [lo[0] - options.phase[0] * h, lo[1] - options.phase[1] * h];
    let nx = ((hi[0] - start[0]) / h).ceil();
    let ny = ((hi[1] - start[1]) / h).ceil();
    let required = nx * ny;
    if !required.is_finite() || required > options.max_cells as f64 {
        return Err(PolygonDistributionError::ResourceLimit {
            required_cells: required,
            max_cells: options.max_cells,
        });
    }
    let triangles = triangulate(&poly)?;
    // 頂点数に伴う凸片走査も資源消費に含め、細い高頂点数床を無制限に走査しない。
    if required * triangles.len() as f64 > 32_000_000.0 {
        return Err(PolygonDistributionError::ResourceLimit {
            required_cells: required * triangles.len() as f64,
            max_cells: 32_000_000,
        });
    }
    let area = geom_polygon::area(&poly);
    let mut result = PolygonDistribution {
        edge_areas_mm2: vec![0.0; n],
        edge_error_bounds_mm2: vec![0.0; n],
        polygon_area_mm2: area,
        unallocated_area_mm2: 0.0,
        cell_size_mm: [h; 2],
        grid_phase: options.phase,
        distance_epsilon_mm: epsilon,
        area_roundoff_bound_mm2: 0.0,
        rule: "有限線分最近接・全同距離辺等分（全周支持・無開口）",
    };
    let mut piece_count = 0usize;
    for iy in 0..ny as usize {
        for ix in 0..nx as usize {
            let cell_lo = [start[0] + ix as f64 * h, start[1] + iy as f64 * h];
            let cell_hi = [cell_lo[0] + h, cell_lo[1] + h];
            for triangle in &triangles {
                let piece = clip_cell(triangle, cell_lo, cell_hi);
                if piece.len() < 3 {
                    continue;
                }
                // 全体座標の大きな積の差は、小片の面積・重心を不安定にする。
                let anchor = piece[0];
                let shifted: Vec<_> = piece
                    .iter()
                    .map(|p| [p[0] - anchor[0], p[1] - anchor[1]])
                    .collect();
                let piece_area = geom_polygon::area(&shifted);
                if piece_area <= 0.0 {
                    continue;
                }
                let c = geom_polygon::centroid(&shifted);
                let centroid = [c[0] + anchor[0], c[1] + anchor[1]];
                let radius = piece
                    .iter()
                    .map(|p| (p[0] - centroid[0]).hypot(p[1] - centroid[1]))
                    .fold(0.0, f64::max);
                let (distances, winners) = nearest(&poly, centroid, epsilon);
                if winners.is_empty() {
                    return Err(PolygonDistributionError::InvalidInput(
                        "最近接候補なし（距離計算の精度不足）".into(),
                    ));
                }
                for &e in &winners {
                    result.edge_areas_mm2[e] += piece_area / winners.len() as f64;
                }
                let min = distances[winners[0]];
                let group = endpoint_group(&poly, &piece, winners[0]);
                let stable = winners.len() == group.len()
                    && winners.iter().all(|e| group.contains(e))
                    && (0..n)
                        .filter(|e| !group.contains(e))
                        .all(|e| distances[e] - min > 2.0 * radius + epsilon);
                if !stable {
                    for (e, &d) in distances.iter().enumerate() {
                        if d - min <= 2.0 * radius + epsilon {
                            result.edge_error_bounds_mm2[e] += piece_area;
                        }
                    }
                }
                piece_count += 1;
            }
        }
    }
    let total: f64 = result.edge_areas_mm2.iter().sum();
    let residual = area - total;
    result.area_roundoff_bound_mm2 = 128.0
        * f64::EPSILON
        * (piece_count + n) as f64
        * ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2));
    if residual.abs() > result.area_roundoff_bound_mm2 {
        return Err(PolygonDistributionError::AreaMismatch {
            residual_mm2: residual,
            roundoff_bound_mm2: result.area_roundoff_bound_mm2,
        });
    }
    result.unallocated_area_mm2 = residual.max(0.0);
    for bound in &mut result.edge_error_bounds_mm2 {
        *bound += result.area_roundoff_bound_mm2;
    }
    if let Some(requested) = options.max_edge_error_mm2 {
        let bound = result
            .edge_error_bounds_mm2
            .iter()
            .copied()
            .fold(0.0, f64::max);
        if bound > requested {
            return Err(PolygonDistributionError::PrecisionNotMet {
                bound_mm2: bound,
                requested_mm2: requested,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
pub(crate) fn polygon_edge_areas(coords: &[[f64; 3]], candidate_edges: &[usize]) -> Vec<f64> {
    integrate_polygon(
        coords,
        candidate_edges,
        PolygonIntegrationOptions::default(),
    )
    .unwrap()
    .edge_areas_mm2
}

pub(crate) fn distribute_polygon(
    coords: &[[f64; 3]],
    w: f64,
    loads: &mut Vec<BeamLoad>,
    options: PolygonIntegrationOptions,
) -> Result<PolygonDistribution, PolygonDistributionError> {
    if !w.is_finite() {
        return Err(PolygonDistributionError::InvalidInput(
            "非有限面荷重".into(),
        ));
    }
    let result = integrate_polygon(coords, &(0..coords.len()).collect::<Vec<_>>(), options)?;
    for (e, &a_e) in result.edge_areas_mm2.iter().enumerate() {
        if a_e <= 0.0 || w == 0.0 {
            continue;
        }
        let l_e = edge_len(coords, e);
        let w_line = w * a_e / l_e;
        if !w_line.is_finite() {
            return Err(PolygonDistributionError::InvalidInput(
                "辺荷重が非有限".into(),
            ));
        }
        let cmq = fem_uniform(w_line, l_e);
        if [cmq.c_i, cmq.c_j, cmq.q_i, cmq.q_j]
            .iter()
            .any(|x| !x.is_finite())
        {
            return Err(PolygonDistributionError::InvalidInput(
                "固定端荷重が非有限".into(),
            ));
        }
        push_edge(loads, e, LoadShape::Uniform { w: w_line }, cmq);
    }
    Ok(result)
}

#[cfg(test)]
mod verification;
