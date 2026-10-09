//! 解析モデルの既知制限を知らせる共通通知。

use sepika_core::model::{ElementKind, Model};
use sepika_element::wall::misc_wall::{is_rc_wall, wall_is_seismic};

/// 壁展開済みモデルの鋼板耐震壁について、対象要素 ID と生成元の壁版 ID を集約した注意を返す。
/// 対象がない場合は `None`。線形解析では呼び出さないこと。
pub fn steel_seismic_wall_notice(
    model: &Model,
    index: &sepika_load::wall_expand::WallExpansionIndex,
) -> Option<String> {
    let mut ids: Vec<_> = model
        .elements
        .iter()
        .filter(|element| {
            element.kind == ElementKind::Wall
                && wall_is_seismic(element, model)
                && !is_rc_wall(element, model)
        })
        .map(|element| element.id.0)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return None;
    }
    let count = ids.len();
    let ids = ids
        .iter()
        .map(|&id| match index.plate_of(sepika_core::ids::ElemId(id)) {
            Some(plate) => format!("{id}（壁版 ID: {}）", plate.0),
            None => id.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "鋼板耐震壁 {count} 枚（要素 ID: {ids}）の面内せん断終局強度を、鋼板のせん断降伏 Qy=t·lw·F/√3 で評価します。\
         せん断座屈は考慮していないため、幅厚比が大きく補剛のない鋼板では耐力を過大評価し得ます。"
    ))
}
