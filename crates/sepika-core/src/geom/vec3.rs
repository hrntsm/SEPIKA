//! 3 次元ベクトルの基本演算。
//!
//! 節点座標・部材軸・面法線の扱いはクレートを横断して現れるため、内積・外積・
//! ノルム・単位化といった最小限の演算をここに集約する（各クレートが同じ数式を
//! 私的なヘルパとして持ち直すのを防ぎ、零ベクトル判定の閾値も 1 か所に保つ）。
//!
//! 座標系は常に mm。ベクトルは `[f64; 3]` の値型で受け渡す（要素数 3 の
//! 固定長配列はコピーが安く、参照を持ち回るより呼び出し側が読みやすい）。

/// 数値的に零ベクトルとみなすノルムの上限 `[mm]`。
///
/// 座標が mm であるため、これを下回る長さの部材軸・面法線は縮退している
/// （方向を定義できない）とみなす。
pub const ZERO_TOL: f64 = 1e-9;

/// 差 `a − b`。
pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// 和 `a + b`。
pub fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// スカラ倍 `s・a`。
pub fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// 2 点の中点。
pub fn midpoint(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        0.5 * (a[0] + b[0]),
        0.5 * (a[1] + b[1]),
        0.5 * (a[2] + b[2]),
    ]
}

/// 内積。
pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 外積。
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// ノルム（長さ）。
pub fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// 2 点間の距離。
pub fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(sub(a, b))
}

/// 単位ベクトル。長さが [`ZERO_TOL`] 以下（縮退）なら `None`。
pub fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = norm(a);
    (l > ZERO_TOL).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

/// `a` から `b` へ向かう単位ベクトル。2 点が一致（縮退）なら `None`。
pub fn unit_from(a: [f64; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    unit(sub(b, a))
}

/// i→j を局所 x、断面参照方向の直交成分を局所 y とする単位直交基底を返す。
/// 有限な零・平行参照方向は全体 X/Y 方向で補い、非有限入力・基底不能は理由付きエラーとする。
pub fn local_frame_axes(
    p_i: [f64; 3],
    p_j: [f64; 3],
    reference: [f64; 3],
) -> Result<[[f64; 3]; 3], &'static str> {
    local_frame_axes_with_fallback(p_i, p_j, reference).map(|(axes, _)| axes)
}

pub(crate) fn local_frame_axes_with_fallback(
    p_i: [f64; 3],
    p_j: [f64; 3],
    reference: [f64; 3],
) -> Result<([[f64; 3]; 3], bool), &'static str> {
    if reference.iter().any(|value| !value.is_finite()) {
        return Err("断面参照方向に非有限値があります");
    }
    let d = sub(p_j, p_i);
    if d.iter().any(|value| !value.is_finite()) {
        return Err("材軸を有限値で解決できません");
    }
    let d_max = d.iter().fold(0.0_f64, |max, value| max.max(value.abs()));
    if d_max == 0.0 {
        return Err("材軸が零長です");
    }
    let d_scaled = d.map(|value| value / d_max);
    let d_length = norm(d);
    let ex = if d_length.is_finite() && d_length > 0.0 {
        d.map(|value| value / d_length)
    } else {
        let scaled_length = norm(d_scaled);
        d_scaled.map(|value| value / scaled_length)
    };
    let reject_ex = |v: [f64; 3]| sub(v, scale(ex, dot(v, ex)));
    let reference_max = reference
        .iter()
        .fold(0.0_f64, |max, value| max.max(value.abs()));
    let reference_scale = if norm(reject_ex(reference)).is_finite() {
        1.0
    } else {
        reference_max
    };
    let reference_scaled = reference.map(|value| value / reference_scale);
    let mut ey = reject_ex(reference_scaled);
    let mut ey_length = norm(ey);
    let used_fallback =
        cross(d_scaled, reference_scaled) == [0.0; 3] || ey_length <= 1e-12 / reference_scale;
    if used_fallback {
        ey = reject_ex(if ex[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        });
        ey_length = norm(ey);
    }
    if !ey_length.is_finite() || ey_length == 0.0 {
        return Err("断面参照方向から単位直交基底を解決できません");
    }
    ey = ey.map(|value| value / ey_length);
    let ez = cross(ex, ey);
    let axes = [ex, ey, ez];
    let is_unit = |axis: [f64; 3]| (dot(axis, axis) - 1.0).abs() <= 1e-12;
    if !axes.into_iter().all(is_unit)
        || dot(ex, ey).abs() > 1e-12
        || dot(ex, ez).abs() > 1e-12
        || dot(ey, ez).abs() > 1e-12
    {
        return Err("断面参照方向から単位直交基底を解決できません");
    }
    Ok((axes, used_fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_and_direction_reject_degenerate_vectors() {
        for (vector, expected) in [
            ([0.0; 3], None),
            ([ZERO_TOL, 0.0, 0.0], None),
            ([0.0, 3.0, 4.0], Some([0.0, 0.6, 0.8])),
        ] {
            assert_eq!(unit(vector), expected);
            assert_eq!(unit_from([0.0; 3], vector), expected);
        }
        assert_eq!(unit_from([3.0, 4.0, 0.0], [3.0, 4.0, 0.0]), None);
    }

    #[test]
    fn local_frame_axes_reject_each_nonfinite_reference_component() {
        for component in 0..3 {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut reference = [1.0, 0.0, 0.0];
                reference[component] = value;
                assert_eq!(
                    local_frame_axes([0.0; 3], [0.0, 0.0, 3000.0], reference),
                    Err("断面参照方向に非有限値があります")
                );
            }
        }
    }

    #[test]
    fn local_frame_axes_preserve_rotation_fallback_reversal_and_large_finite_direction() {
        let i = [0.0; 3];
        let j = [0.0, 0.0, 3000.0];
        assert_eq!(
            local_frame_axes(i, j, [1.0, 0.0, 0.0]),
            Ok([[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
        );
        assert_eq!(
            local_frame_axes(i, j, [0.0, 1.0, 0.0]),
            Ok([[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]])
        );
        assert_eq!(
            local_frame_axes(j, i, [1.0, 0.0, 0.0]),
            Ok([[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]])
        );
        for reference in [[0.0; 3], [0.0, 0.0, 1.0], [1e-308, 0.0, 0.0]] {
            assert_eq!(
                local_frame_axes(i, j, reference),
                local_frame_axes(i, j, [1.0, 0.0, 0.0])
            );
        }
        assert_eq!(
            local_frame_axes(i, j, [1e308, 0.0, 0.0]),
            local_frame_axes(i, j, [1.0, 0.0, 0.0])
        );
        assert_eq!(
            local_frame_axes(i, j, [1e-8, 0.0, 1e6]),
            local_frame_axes(i, j, [1.0, 0.0, 0.0])
        );
        assert_eq!(
            local_frame_axes([0.0; 3], [1.0; 3], [1e308; 3]),
            local_frame_axes([0.0; 3], [1.0; 3], [0.0; 3])
        );
        assert_eq!(
            local_frame_axes([0.0; 3], [3000.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            Ok([[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]])
        );
    }

    #[test]
    fn local_frame_axes_reject_finite_reference_with_unresolvable_orthogonal_direction() {
        let reference: [f64; 3] = [1e308, 1e308, 1e308 - 1e292];
        assert!(reference.iter().all(|value| value.is_finite()));
        assert_eq!(
            local_frame_axes([0.0; 3], [1.0; 3], reference),
            Err("断面参照方向から単位直交基底を解決できません")
        );
    }

    #[test]
    fn local_frame_axes_preserves_small_perpendicular_direction_beside_huge_parallel_component() {
        let axes = local_frame_axes([0.0; 3], [0.0, 0.0, 3000.0], [0.0, 1e-5, 1e308]).unwrap();
        assert_eq!(axes[1], [0.0, 1.0, 0.0]);
        assert_eq!(axes[2], [-1.0, 0.0, 0.0]);

        let axes =
            local_frame_axes([0.0; 3], [0.0, 0.0, 3000.0], [1.1e-12, 2.3e-12, 1e308]).unwrap();
        assert!((axes[1][0] - 1.1 / (1.1_f64.powi(2) + 2.3_f64.powi(2)).sqrt()).abs() < 1e-12);
        assert!((axes[1][1] - 2.3 / (1.1_f64.powi(2) + 2.3_f64.powi(2)).sqrt()).abs() < 1e-12);
    }
}
