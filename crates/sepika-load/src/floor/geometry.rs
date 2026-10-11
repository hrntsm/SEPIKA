//! 幾何ヘルパ（境界座標取得・距離・矩形判定・面積）。
//!
//! - [`boundary_coords`] — スラブ境界節点の座標列を取得する
//! - [`dist3`] — 3次元2点間のユークリッド距離
//! - [`slab_dimensions`] — 矩形判定と辺順の寸法 `(lx, ly)`
//! - [`edge_len`] — 多角形の辺 i の長さ
//! - [`point_in_slab_boundary`] — 点がスラブ境界多角形の内部または辺上にあるか

use sepika_core::model::{Model, Slab};

pub(crate) fn boundary_coords(model: &Model, slab: &Slab) -> Option<Vec<[f64; 3]>> {
    slab.boundary_coords(model)
}

/// 2 点間の距離 [mm]。算定の情報源は `sepika-core` に置く。
pub(crate) use sepika_core::geom::vec3::dist as dist3;

/// 矩形の寸法 [mm] を辺 0→1、0→3 の順で返す。閉合・直交・面積の相対許容は 1e-6。
/// 寸法を採用できない境界は `None`。水平性・単純性・支持参照の検査は呼出し側が行う。
pub fn slab_dimensions(model: &Model, slab: &Slab) -> Option<(f64, f64)> {
    slab_dimensions_of(&slab.boundary_coords(model)?)
}

/// [`slab_dimensions`] の座標列版（境界座標が手元にある場合はこちらを使う）。
pub fn slab_dimensions_of(coords: &[[f64; 3]]) -> Option<(f64, f64)> {
    if coords.len() != 4 {
        return None;
    }
    if !coords.iter().flatten().all(|x| x.is_finite()) {
        return None;
    }
    let local: [[f64; 3]; 4] =
        std::array::from_fn(|e| sepika_core::geom::vec3::sub(coords[e], coords[0]));
    let u = local[1];
    let v = local[3];
    let norm = |p: [f64; 3]| p[0].hypot(p[1]).hypot(p[2]);
    let lx = norm(u);
    let ly = norm(v);
    if !lx.is_finite() || !ly.is_finite() || lx <= 1e-9 || ly <= 1e-9 {
        return None;
    }
    let closure = norm(std::array::from_fn(|i| local[2][i] - u[i] - v[i])) / lx.max(ly);
    let dot = (0..3).map(|i| (u[i] / lx) * (v[i] / ly)).sum::<f64>().abs();
    let area_vector: [f64; 3] = std::array::from_fn(|i| {
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        (0..4)
            .map(|e| local[e][j] * local[(e + 1) % 4][k] - local[e][k] * local[(e + 1) % 4][j])
            .sum::<f64>()
            / 2.
    });
    let area = norm(area_vector);
    let rectangle_area = lx * ly;
    let area_error = (area - rectangle_area).abs() / area;
    if !closure.is_finite()
        || closure > 1e-6
        || !dot.is_finite()
        || dot > 1e-6
        || !area.is_finite()
        || area <= 0.
        || !rectangle_area.is_finite()
        || !area_error.is_finite()
        || area_error > 1e-6
    {
        return None;
    }
    Some((lx, ly))
}

pub(crate) fn edge_len(coords: &[[f64; 3]], i: usize) -> f64 {
    let n = coords.len();
    dist3(coords[i], coords[(i + 1) % n])
}

/// 点がスラブ境界多角形（XY 平面投影）の内部または辺上にあるか。
///
/// 床スラブは水平（Z 一定）を仮定し、境界節点の XY 座標のみで判定する。
/// レイキャストは辺上の点を内側にしないため、辺からの距離が 1 mm 以内なら辺上として含める。
/// 小梁中点がスラブ縁に乗っているとき、検定対象から落ちないようにするためである。
pub fn point_in_slab_boundary(model: &Model, slab: &Slab, p: [f64; 2]) -> bool {
    let Some(coords) = boundary_coords(model, slab) else {
        return false;
    };
    if coords.len() < 3 {
        return false;
    }
    let poly: Vec<[f64; 2]> = coords.iter().map(|c| [c[0], c[1]]).collect();
    sepika_core::geom::polygon::contains_including_boundary(&poly, p)
}
