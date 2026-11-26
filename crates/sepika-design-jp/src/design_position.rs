//! 危険断面位置（断面検定を行う部材軸上の位置）。
//!
//! - [`crate::design_position::design_positions`] — 公称フェースに基づく危険断面位置（正規化座標 \[0,1\]）
//! - [`crate::design_position::is_near_design_position`] — 内力の評価位置が危険断面位置と一致するか
//!
//! 要素側の**内力の評価断面**（`sepika_element` の `eval_sections`）は、
//! ここに解析部材の両端（0.0 / 1.0）を加えたものになる。検定は危険断面位置の内力を使う。

use sepika_core::model::{ElementData, Model};

/// 公称フェースに基づく危険断面位置を正規化座標 \[0,1\] で返す。
/// 通常RC柱検定の実材端面や、RC梁QD1の専用検定位置は含まない。
///
/// 既定は**危険断面の端部フェース（i 端・j 端）と部材中央**の 3 点。フェース距離
/// （`RigidZone::face_i/face_j`）が未算定の端は解析部材の端（0.0 / 1.0）に一致する。
/// 部材付帯情報（ハンチ端・継手位置。剛性には影響しない）があれば、その
/// 追加検定位置も含める（「位置はユーザが追加・変更可能」）。
///
/// `geom_len` は部材の幾何長 \[mm\]（[`Model::member_length`]）。0 に縮退した
/// 部材は `[0.0, 0.5, 1.0]` を返す。返り値は昇順で、1e-9 以内の重複は畳む。
pub fn design_positions(elem: &ElementData, model: &Model, geom_len: f64) -> Vec<f64> {
    let [i, j] = prepared_end_positions(elem, geom_len);
    let mut xs = vec![i, 0.5, j];
    finish_positions(&mut xs, elem, model, geom_len);
    xs
}

pub(crate) fn prepared_end_positions(elem: &ElementData, geom_len: f64) -> [f64; 2] {
    if geom_len > 1e-12 {
        [
            (elem.rigid_zone.face_i_or_zero() / geom_len).clamp(0.0, 0.5 - 1e-9),
            (1.0 - elem.rigid_zone.face_j_or_zero() / geom_len).clamp(0.5 + 1e-9, 1.0),
        ]
    } else {
        [0.0, 1.0]
    }
}

/// 実材端を使用するQD1の検定位置。保存断面の選択には `same_qd1_station` を使う。
pub fn rc_beam_qd1_positions(
    model: &Model,
    elem: &ElementData,
    geometry: crate::rc::BeamQd1Geometry,
) -> Option<Vec<f64>> {
    if !geometry
        .uses_material_end
        .into_iter()
        .any(|uses_end| uses_end)
    {
        return None;
    }
    let [i, j] = geometry.end_positions;
    let mut positions = vec![0.5];
    finish_positions(&mut positions, elem, model, model.member_length(elem));
    positions.retain(|pos| [i, j].iter().all(|end| (pos - end).abs() >= 1e-9));
    positions.extend([i, j]);
    positions.sort_by(f64::total_cmp);
    Some(positions)
}

/// 実材端は厳密に一致させ、内部断面だけ正規化座標の丸め差を許す。
pub fn same_qd1_station(expected: f64, actual: f64) -> bool {
    if matches!(expected, 0.0 | 1.0) || matches!(actual, 0.0 | 1.0) {
        expected == actual
    } else {
        (expected - actual).abs() <= 32.0 * f64::EPSILON
    }
}

/// 指定の検定位置の一意かつ有限な保存断面力を選ぶ際の診断。
pub fn qd1_saved_station_issue(position: f64, forces: &[(f64, [f64; 6])]) -> Option<&'static str> {
    let mut rows = forces
        .iter()
        .filter(|(pos, _)| same_qd1_station(position, *pos));
    match (rows.next(), rows.next()) {
        (Some((_, force)), None) if force.iter().all(|value| value.is_finite()) => None,
        (None, _) => Some("RC梁の実材端または実柱面に対応する保存解析断面力がありません"),
        _ => Some("RC梁の実材端または実柱面の保存解析断面力が非有限、または重複しています"),
    }
}

pub(crate) fn finish_positions(
    xs: &mut Vec<f64>,
    elem: &ElementData,
    model: &Model,
    geom_len: f64,
) {
    if let Some(detail) = model.member_detail(elem.id) {
        xs.extend(detail.extra_check_positions(&elem.rigid_zone, geom_len));
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
}

/// 内力の評価位置 `pos` が危険断面位置 `positions` のいずれかと一致するか
/// （1e-6 以内）。要素が返す評価断面には解析部材の両端も含まれるため、検定側は
/// これで危険断面位置の行だけを拾う。
pub fn is_near_design_position(pos: f64, positions: &[f64]) -> bool {
    positions.iter().any(|p| (p - pos).abs() < 1e-6)
}
