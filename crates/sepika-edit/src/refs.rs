//! 編集コマンドが書き込む ID 参照の存在検証。
//!
//! # 規約
//!
//! **モデルへ ID 参照を書き込むコマンドは、書き込む前に参照先の存在を確認する。**
//! 確認に落ちたコマンドは [`Noop`](crate::Noop) を返し、モデルを一切変更しない。
//!
//! ここで確認するのは [`Model::validate`](sepika_core::model::Model::validate) が
//! 弾く 2 種類の壊れ方である。
//!
//! - **ダングリング参照**: 実在しない節点・部材・断面・材料を指す ID
//! - **ID ＝配列添字の破れ**: 末尾以外の添字を持つ実体の追加
//!
//! 壊れたモデルを作ってから `validate` で落とす形にすると、診断・解析・保存の
//! すべてが同時に止まり、どの操作が原因かを利用者が追えなくなる。書き込む側で
//! 止めれば、失敗するのはその 1 操作だけで済み、undo 履歴も汚れない
//! （[`UndoStack::run`](crate::UndoStack::run) は `Noop` を積まない）。
//!
//! 参照先の実体そのものを操作するコマンド（対象の断面が無ければ何もしない等）の
//! 「対象の存在確認」は各コマンドが従来どおり自前で行う。本モジュールが受け持つのは
//! **書き込む値の側**の確認である。

use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
use sepika_core::model::{ElementData, ElementKind, FrameSectionUse, Model};

/// 節点が実在するか（ID ＝配列添字の規約込み）。
pub(crate) fn node_exists(model: &Model, id: NodeId) -> bool {
    model.nodes.get(id.index()).is_some_and(|n| n.id == id)
}

/// 部材が実在するか（ID ＝配列添字の規約込み）。
pub(crate) fn elem_exists(model: &Model, id: ElemId) -> bool {
    model.elements.get(id.index()).is_some_and(|e| e.id == id)
}

/// 断面が実在するか（ID ＝配列添字の規約込み）。
pub(crate) fn section_exists(model: &Model, id: SectionId) -> bool {
    model.sections.get(id.index()).is_some_and(|s| s.id == id)
}

/// 材料が実在するか（ID ＝配列添字の規約込み）。
pub(crate) fn material_exists(model: &Model, id: MaterialId) -> bool {
    model.materials.get(id.index()).is_some_and(|m| m.id == id)
}

/// 未割当（`None`）も可の断面参照が妥当か。
pub(crate) fn section_ref_ok(model: &Model, id: Option<SectionId>) -> bool {
    id.is_none_or(|s| section_exists(model, s))
}

pub(crate) fn frame_element_section_ref_ok(
    model: &Model,
    element: &ElementData,
    id: Option<SectionId>,
) -> bool {
    section_ref_ok(model, id)
        && id
            .and_then(|sid| model.sections.get(sid.index()))
            .is_none_or(|section| {
                if section.validate_rc_element_use(element.kind).is_err() {
                    return false;
                }
                if !section.is_cft() {
                    return true;
                }
                if section.frame_use != Some(FrameSectionUse::Column) {
                    return false;
                }
                if !matches!(
                    element.kind,
                    ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
                ) {
                    return false;
                }
                true
            })
}

pub(crate) fn plate_section_ref_ok(model: &Model, id: Option<SectionId>) -> bool {
    section_ref_ok(model, id)
        && id
            .and_then(|sid| model.sections.get(sid.index()))
            .is_none_or(|section| !section.is_cft())
}

pub(crate) fn rc_plate_section_error(
    model: &Model,
    id: Option<SectionId>,
    plate: impl std::fmt::Debug,
) -> Option<String> {
    id.and_then(|sid| model.section(sid))
        .and_then(|section| section.validate_rc_plate_use().err())
        .map(|reason| format!("{plate:?}: {reason}"))
}

pub(crate) fn secondary_section_ref_ok(model: &Model, id: Option<SectionId>) -> bool {
    section_ref_ok(model, id)
        && id
            .and_then(|sid| model.sections.get(sid.index()))
            .is_none_or(|section| section.secondary_member_use_allowed())
}

/// 未割当（`None`）も可の材料参照が妥当か。
pub(crate) fn material_ref_ok(model: &Model, id: Option<MaterialId>) -> bool {
    id.is_none_or(|m| material_exists(model, m))
}

/// 追加する部材が [`Model::validate`](sepika_core::model::Model::validate) を
/// 通る形か（ID が末尾の添字と一致し、参照する節点・断面が実在するか）。
pub(crate) fn new_elem_ok(model: &Model, elem: &sepika_core::model::ElementData) -> bool {
    elem.validate_frame_node_count().is_ok()
        && elem.id.index() == model.elements.len()
        && elem.nodes.iter().all(|&n| node_exists(model, n))
        && section_ref_ok(model, elem.section)
}

pub(crate) fn rc_element_section_error(
    model: &Model,
    element: &ElementData,
    id: Option<SectionId>,
) -> Option<String> {
    id.and_then(|sid| model.section(sid))
        .and_then(|section| section.validate_rc_element_use(element.kind).err())
        .map(|reason| format!("ElemId({}): {reason}", element.id.0))
}

pub(crate) fn rc_secondary_section_error(
    model: &Model,
    kind: sepika_core::model::SecondaryMemberKind,
    id: Option<SectionId>,
) -> Option<String> {
    id.and_then(|sid| model.section(sid))
        .and_then(|section| section.validate_rc_secondary_use(kind).err())
}
