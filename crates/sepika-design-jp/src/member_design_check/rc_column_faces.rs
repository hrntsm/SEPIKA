use std::collections::HashMap;

use sepika_core::face_distance::{RcColumnGirderFace, RcColumnGirderFaces};
use sepika_core::ids::ElemId;
use sepika_core::model::{ElementData, Model};
use sepika_solver::statics::linear::RcColumnFaceEnds;

use crate::design_position::{finish_positions, prepared_end_positions};

pub(super) fn response_index(
    rows: &[(ElemId, RcColumnFaceEnds)],
) -> HashMap<ElemId, Option<&RcColumnFaceEnds>> {
    let mut index = HashMap::new();
    for (id, ends) in rows {
        index
            .entry(*id)
            .and_modify(|entry| *entry = None)
            .or_insert(Some(ends));
    }
    index
}

pub(super) fn response(
    table: &HashMap<ElemId, Option<&RcColumnFaceEnds>>,
    id: ElemId,
    side: usize,
    face: RcColumnGirderFace,
) -> Result<[f64; 6], String> {
    let ends = table
        .get(&id)
        .ok_or("同じ解析結果に対応する実材端面の断面力がありません")?
        .ok_or("同じ柱の実材端面の断面力が重複しています")?;
    let saved = ends[side]
        .as_ref()
        .ok_or("同じ解析結果に対応する実材端面の断面力がありません")?
        .as_ref()
        .map_err(Clone::clone)?;
    if saved.face != face {
        return Err("解析結果と検定対象の実材端面位置または材軸長が一致しません".into());
    }
    saved.forces.as_ref().copied().map_err(Clone::clone)
}

pub(super) fn regular_positions(
    geometry: &RcColumnGirderFaces,
    elem: &ElementData,
    model: &Model,
    length_mm: f64,
) -> Vec<f64> {
    if !geometry.has_positive_span {
        return Vec::new();
    }
    let mut result = vec![0.5];
    for (side, xi) in prepared_end_positions(elem, length_mm)
        .into_iter()
        .enumerate()
    {
        if geometry.ends[side].is_none() {
            result.push(xi);
        }
    }
    finish_positions(&mut result, elem, model, length_mm);
    let limits = geometry.ends.each_ref().map(|end| {
        end.as_ref()
            .and_then(|end| end.as_ref().ok())
            .map(|face| face.position_mm / face.length_mm)
    });
    result.retain(|&xi| {
        limits[0].is_none_or(|bound| xi > bound) && limits[1].is_none_or(|bound| xi < bound)
    });
    result
}
