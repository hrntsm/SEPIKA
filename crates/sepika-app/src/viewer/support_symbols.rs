//! 支点ばねの 3D シンボル描画。
//!
//! 支持記号（矢印＝並進固定・円弧＝回転固定、[`super::support::draw_support_symbol`]）とは
//! 別種の支持条件を区別できるよう、専用の記号を追加する。
//! - 支点ばね（[`sepika_core::model::Node::support_spring`]）: 並進はジグザグ（コイル）線、
//!   回転は渦巻線。拘束（`restraint`）で固定済みの成分は矢印・円弧のまま
//!   （呼び出し側で判定し、ばね記号を描かない）。
//!
//! 記号の点列生成（ジグザグ・渦巻）は egui の `Painter` に依存しない純関数へ切り出し、
//! 単体テストで形状を検証する。

use crate::theme;
use sepika_core::dof::{Dof, Dof6Mask};

use super::Projector;

/// 節点間ジグザグ（コイル）線の点列をスクリーン座標で生成する（純関数）。
///
/// `from`→`to` を `coils` 周期でジグザグさせ、両端は `from`/`to` そのもの。
/// `coils == 0` または `from`/`to` が重なる場合は 2 点（直線扱い）を返す。
pub(super) fn zigzag_points(
    from: egui::Pos2,
    to: egui::Pos2,
    coils: usize,
    amplitude: f32,
) -> Vec<egui::Pos2> {
    let dir = to - from;
    let len = dir.length();
    if len < 1e-3 || coils == 0 {
        return vec![from, to];
    }
    let ux = dir.x / len;
    let uy = dir.y / len;
    let nx = -uy;
    let ny = ux;
    let n = coils * 2;
    let mut pts = Vec::with_capacity(n + 2);
    pts.push(from);
    for i in 1..=n {
        let t = i as f32 / (n + 1) as f32;
        let base_x = from.x + dir.x * t;
        let base_y = from.y + dir.y * t;
        let side = if i % 2 == 1 { 1.0 } else { -1.0 };
        pts.push(egui::pos2(
            base_x + nx * amplitude * side,
            base_y + ny * amplitude * side,
        ));
    }
    pts.push(to);
    pts
}

/// 並進ばねのジグザグ（コイル）線を描画する。
pub(super) fn draw_translational_spring(
    painter: &egui::Painter,
    from: egui::Pos2,
    to: egui::Pos2,
    color: egui::Color32,
) {
    /// コイルの周期数（多すぎず視認できる程度）
    const COILS: usize = 4;
    /// ジグザグの振幅 [px]（非テキスト形状のため固定値）。
    const AMPLITUDE: f32 = 3.0;
    let stroke = egui::Stroke::new(2.0_f32, color);
    let pts = zigzag_points(from, to, COILS, AMPLITUDE);
    painter.add(egui::Shape::line(pts, stroke));
}

/// 渦巻線の (半径比, 角度[rad]) 列を生成する（純関数）。
///
/// 半径比は 0.0→1.0 へ、角度は 0→`turns`*2π へ、ともに単調に増加する
/// （アルキメデス螺旋。回転固定の単一円弧と視覚的に区別するための形状）。
/// `n == 0` の場合は空を返す。
pub(super) fn spiral_fracs(turns: f64, n: usize) -> Vec<(f64, f64)> {
    if n == 0 {
        return Vec::new();
    }
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            (t, t * turns * std::f64::consts::TAU)
        })
        .collect()
}

/// 渦巻の周回数（回転固定の単一円弧と区別できる程度）。
const SPIRAL_TURNS: f64 = 1.5;
/// 渦巻の分割数。
const SPIRAL_SEGMENTS: usize = 40;

/// スクリーン平面上に渦巻アイコンを描く（凡例など、3D 投影を経ない用途）。
pub(super) fn draw_spiral_icon_2d(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.5_f32, color);
    let pts: Vec<egui::Pos2> = spiral_fracs(SPIRAL_TURNS, SPIRAL_SEGMENTS)
        .into_iter()
        .map(|(r_frac, theta)| {
            let r = radius * r_frac as f32;
            egui::pos2(
                center.x + r * (theta.cos() as f32),
                center.y + r * (theta.sin() as f32),
            )
        })
        .collect();
    painter.add(egui::Shape::line(pts, stroke));
}

/// 回転ばねの渦巻線を 3D 空間（節点まわり、`axis` に直交する面内）に描く。
/// 基底の作り方は [`super::support::draw_rotation_arc`] と同じ（[`super::support::axis_basis`]）。
pub(super) fn draw_rotational_spring(
    painter: &egui::Painter,
    proj: &Projector,
    center_world: [f64; 3],
    axis: [f64; 3],
    radius_world: f64,
    color: egui::Color32,
) {
    let Some((u, v)) = super::support::axis_basis(axis) else {
        return;
    };
    let stroke = egui::Stroke::new(1.5_f32, color);
    let mut prev: Option<egui::Pos2> = None;
    for (r_frac, theta) in spiral_fracs(SPIRAL_TURNS, SPIRAL_SEGMENTS) {
        let r = radius_world * r_frac;
        let c = theta.cos();
        let s = theta.sin();
        let pt = [
            center_world[0] + r * (c * u[0] + s * v[0]),
            center_world[1] + r * (c * u[1] + s * v[1]),
            center_world[2] + r * (c * u[2] + s * v[2]),
        ];
        let cur = proj.project(pt);
        if let Some(p0) = prev {
            painter.line_segment([p0, cur], stroke);
        }
        prev = Some(cur);
    }
}

/// 支点ばねシンボルを 3D ビューに描画する。
///
/// 拘束（`restraint`）で固定済みの成分は矢印・円弧表示に委ねるため、
/// ここでは非固定かつばね値が非ゼロの成分のみジグザグ・渦巻を描く。
/// 軸色は X=赤 / Y=緑 / Z=青で [`super::support::draw_support_symbol`] と揃える。
pub(super) fn draw_spring_symbol(
    painter: &egui::Painter,
    proj: &Projector,
    node_coord: [f64; 3],
    restraint: Dof6Mask,
    spring: &[f64; 6],
    arrow_px: f32,
    arc_px: f32,
) {
    let arrow_world = arrow_px as f64 / proj.scale() as f64;
    let arc_world = arc_px as f64 / proj.scale() as f64;
    let origin = proj.project(node_coord);

    let translational: [(Dof, [f64; 3], egui::Color32); 3] = [
        (Dof::Ux, [1.0, 0.0, 0.0], theme::AXIS_X),
        (Dof::Uy, [0.0, 1.0, 0.0], theme::AXIS_Y),
        (Dof::Uz, [0.0, 0.0, 1.0], theme::AXIS_Z),
    ];
    for (i, (dof, dir, color)) in translational.into_iter().enumerate() {
        if restraint.is_fixed(dof) || spring[i] == 0.0 {
            continue;
        }
        let end = [
            node_coord[0] + dir[0] * arrow_world,
            node_coord[1] + dir[1] * arrow_world,
            node_coord[2] + dir[2] * arrow_world,
        ];
        draw_translational_spring(painter, origin, proj.project(end), color);
    }

    let rotational: [(Dof, [f64; 3], egui::Color32); 3] = [
        (Dof::Rx, [1.0, 0.0, 0.0], theme::AXIS_X),
        (Dof::Ry, [0.0, 1.0, 0.0], theme::AXIS_Y),
        (Dof::Rz, [0.0, 0.0, 1.0], theme::AXIS_Z),
    ];
    for (i, (dof, axis, color)) in rotational.into_iter().enumerate() {
        let spring_idx = i + 3;
        if restraint.is_fixed(dof) || spring[spring_idx] == 0.0 {
            continue;
        }
        draw_rotational_spring(painter, proj, node_coord, axis, arc_world, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_points_preserve_endpoints_and_remain_drawable() {
        let from = egui::pos2(0.0, 0.0);
        let to = egui::pos2(20.0, 0.0);
        for (end, coils, bends) in [(to, 2, true), (to, 0, false), (from, 4, false)] {
            let pts = zigzag_points(from, end, coils, 3.0);
            assert_eq!(pts.first().copied(), Some(from));
            assert_eq!(pts.last().copied(), Some(end));
            assert!(pts.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
            assert_eq!(pts.iter().any(|p| p.y > from.y), bends);
            assert_eq!(pts.iter().any(|p| p.y < from.y), bends);
        }
    }

    #[test]
    fn spiral_fracs_remain_finite_and_cover_radius() {
        for segments in [0, 10] {
            let fracs = spiral_fracs(1.5, segments);
            if segments == 0 {
                assert!(fracs.is_empty());
                continue;
            }
            assert_eq!(fracs.first().copied(), Some((0.0, 0.0)));
            let (radius, angle) = *fracs.last().unwrap();
            assert!((radius - 1.0).abs() < 1e-12 && angle > 0.0);
            assert!(fracs
                .iter()
                .all(|(r, theta)| (0.0..=1.0).contains(r) && theta.is_finite()));
            assert!(fracs
                .windows(2)
                .all(|w| w[1].0 >= w[0].0 && w[1].1 >= w[0].1));
        }
    }
}
