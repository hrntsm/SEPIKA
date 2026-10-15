use super::{area, signed_area};

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

/// 境界順の無開口多角形（mm）の単純性と正面積を検査する。
/// 自己交差・非隣接接触・重複点・折返し・非有限座標を拒否する。
pub fn validate_simple(poly: &[[f64; 2]]) -> Result<(), &'static str> {
    if poly.len() < 3 {
        return Err("境界頂点が3個未満");
    }
    if poly.iter().flatten().any(|x| !x.is_finite()) {
        return Err("非有限座標");
    }
    let n = poly.len();
    for i in 0..n {
        let length_squared = (poly[i][0] - poly[(i + 1) % n][0]).powi(2)
            + (poly[i][1] - poly[(i + 1) % n][1]).powi(2);
        if !length_squared.is_finite() || length_squared < f64::MIN_POSITIVE {
            return Err("辺長の数値精度不足");
        }
        if poly[i] == poly[(i + 1) % n] {
            return Err("退化辺");
        }
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segments_intersect(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n]) {
                return Err("自己交差または非隣接辺の接触（開口を結ぶ境界は未対応）");
            }
        }
        let a = poly[(i + n - 1) % n];
        let b = poly[i];
        let c = poly[(i + 1) % n];
        if cross(a, b, c) == 0.0
            && (b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1]) <= 0.0
        {
            return Err("隣接辺の折返し");
        }
    }
    let area = area(poly);
    if !area.is_finite() || area <= 0.0 {
        return Err("面積が非有限またはゼロ");
    }
    Ok(())
}

/// 単純多角形の三角形を元頂点インデックスで返す。座標は mm、穴なし・境界順。
/// 自己交差・接触・重複点・非有限・非正面積や分割不能時はエラーを返し、入力を修復しない。
pub fn triangulate_simple(poly: &[[f64; 2]]) -> Result<Vec<[usize; 3]>, &'static str> {
    validate_simple(poly)?;
    let mut indices: Vec<_> = (0..poly.len()).collect();
    if signed_area(poly) < 0.0 {
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
            triangles.push([a, b, c]);
            indices.remove(i);
            found = true;
            break;
        }
        if !found {
            return Err("凸分割を確定できない数値精度");
        }
    }
    triangles.push([indices[0], indices[1], indices[2]]);
    Ok(triangles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_triangulation_preserves_original_vertices_and_concave_coverage() {
        let points = [
            [0.0, 0.0],
            [4000.0, 0.0],
            [4000.0, 1000.0],
            [1000.0, 1000.0],
            [1000.0, 4000.0],
            [0.0, 4000.0],
        ];
        for poly in [points.to_vec(), points.into_iter().rev().collect()] {
            let triangles = triangulate_simple(&poly).unwrap();
            assert_eq!(triangles.len(), 4);
            let mut area_mm2 = 0.0;
            for indices in triangles {
                assert!(indices.iter().all(|&i| i < poly.len()));
                let triangle = indices.map(|i| poly[i]);
                area_mm2 += area(&triangle);
                assert!(!super::super::contains_including_boundary(
                    &triangle,
                    [2000.0, 2000.0]
                ));
            }
            assert_eq!(area_mm2, 7_000_000.0);
        }
    }

    #[test]
    fn simple_triangulation_rejects_crossings_contact_backtracking_and_nonfinite_input() {
        for poly in [
            vec![[0.0, 0.0], [4000.0, 3000.0], [0.0, 4000.0], [3000.0, 0.0]],
            vec![[0.0, 0.0], [4000.0, 0.0], [4000.0, 0.0], [0.0, 4000.0]],
            vec![[0.0, 0.0], [2000.0, 0.0], [4000.0, 0.0]],
            vec![[0.0, 0.0], [4000.0, 0.0], [2000.0, 0.0], [0.0, 4000.0]],
            vec![
                [0.0, 0.0],
                [4000.0, 0.0],
                [2000.0, 2000.0],
                [2000.0, 0.0],
                [0.0, 4000.0],
            ],
            vec![[0.0, 0.0], [f64::NAN, 0.0], [0.0, 4000.0]],
            vec![[0.0, 0.0], [f64::INFINITY, 0.0], [0.0, 4000.0]],
        ] {
            assert!(triangulate_simple(&poly).is_err());
        }
    }
}
