//! RC部材の終局検定に用いる内法長さ。

use sepika_core::geom::vec3::dist;
use sepika_core::model::{ElementData, Model};

/// 両端フェースが算定済みで、内法長さ [mm] が有限な正値の場合に返す。
pub(super) fn clear_span(elem: &ElementData, model: &Model) -> Result<f64, String> {
    let faces = (elem.rigid_zone.face_i, elem.rigid_zone.face_j);
    let (Some(face_i_mm), Some(face_j_mm)) = faces else {
        let missing = match faces {
            (None, None) => "両端",
            (None, _) => "i端",
            _ => "j端",
        };
        return Err(format!(
            "ultimate_clear_span_uncomputed: {missing}のフェース距離が未算定のため内法長さを確認できません"
        ));
    };
    for (end, distance) in [("i端", face_i_mm), ("j端", face_j_mm)] {
        if !distance.is_finite() || distance < 0.0 {
            return Err(format!(
                "ultimate_clear_span_invalid: {end}のフェース距離 {distance} mm が有限な非負値ではありません"
            ));
        }
    }
    let invalid_axis = |reason: &str| {
        format!("ultimate_clear_span_invalid: 部材の材軸長を確認できません（{reason}）")
    };
    let axis_ends = model
        .primary_frame_axis_ends(elem)
        .map_err(|reason| invalid_axis(&reason))?;
    let [i, j] = match axis_ends {
        Some(ends) => ends,
        None => {
            let [i, j] = elem.nodes.as_slice() else {
                return Err(invalid_axis("両端に二節点が必要です"));
            };
            let (Some(i), Some(j)) = (model.node(*i), model.node(*j)) else {
                return Err(invalid_axis("材端節点が見つかりません"));
            };
            [i.coord, j.coord]
        }
    };
    let axis_mm = dist(i, j);
    if !axis_mm.is_finite() || axis_mm <= 0.0 {
        return Err(invalid_axis("有限な正値ではありません"));
    }
    let span_mm = axis_mm - face_i_mm - face_j_mm;
    if !span_mm.is_finite() || span_mm <= 0.0 {
        return Err(format!(
            "ultimate_clear_span_invalid: 材軸長 {axis_mm} mm、i端フェース距離 {face_i_mm} mm、j端フェース距離 {face_j_mm} mm では正の内法長さが成立しません"
        ));
    }
    Ok(span_mm)
}
