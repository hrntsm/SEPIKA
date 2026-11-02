//! 断面の編集コマンド（追加・削除・複製・形状/プロパティ編集）。

use super::*;
use sepika_core::ids::*;

/// 断面プロパティ変更（名称・A・Iy・Iz・J 等）。
pub struct SetSectionField {
    pub id: SectionId,
    pub field: SectionField,
    pub value: f64,
}

/// 編集対象の断面プロパティ。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SectionField {
    Area,
    Iy,
    Iz,
    J,
    Depth,
    Width,
    AsY,
    AsZ,
    /// 仕口パネルの板厚 `[mm]`（ダイアフラム補強・ダブラープレートによる増厚の
    /// 明示指定）。0 以下は未入力として扱い、柱の断面形状から算定する。
    PanelThickness,
}

impl EditCommand for SetSectionField {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.id.index();
        if idx >= model.sections.len() || model.sections[idx].id != self.id {
            return Box::new(Noop);
        }
        let sec = &mut model.sections[idx];
        let old = sec.clone();
        match self.field {
            SectionField::Area => sec.area = self.value,
            SectionField::Iy => sec.iy = self.value,
            SectionField::Iz => sec.iz = self.value,
            SectionField::J => sec.j = self.value,
            SectionField::Depth => sec.depth = self.value,
            SectionField::Width => sec.width = self.value,
            SectionField::AsY => sec.as_y = self.value,
            SectionField::AsZ => sec.as_z = self.value,
            SectionField::PanelThickness => {
                sec.panel_thickness = (self.value > 0.0).then_some(self.value);
            }
        }
        use sepika_core::model::PropertyBasis;
        let basis = &mut sec.property_basis;
        match self.field {
            SectionField::Area => basis.area = PropertyBasis::Supplied,
            SectionField::Iy => basis.iy = PropertyBasis::Supplied,
            SectionField::Iz => basis.iz = PropertyBasis::Supplied,
            SectionField::J => basis.j = PropertyBasis::Supplied,
            SectionField::Depth => basis.depth = PropertyBasis::Supplied,
            SectionField::Width => basis.width = PropertyBasis::Supplied,
            SectionField::AsY => basis.as_y = PropertyBasis::Supplied,
            SectionField::AsZ => basis.as_z = PropertyBasis::Supplied,
            SectionField::PanelThickness => {}
        }
        Box::new(RestoreSection { old })
    }

    fn label(&self) -> &str {
        "断面プロパティ変更"
    }
}

/// 断面の符号と階の変更。
///
/// 断面の同一性キーは符号＋階なので、既存の断面と同じ組合せになる変更は
/// [`Noop`] として拒否する（モデル側の不変条件を GUI 以外の呼び出し元に対しても守る）。
/// 呼び出し側は事前に [`sepika_core::model::section_key_taken`] で判定し、
/// 利用者へ理由を示したうえでコマンドを発行しないことが望ましい。
pub struct SetSectionName {
    pub id: SectionId,
    pub name: String,
    pub floor: Option<String>,
}

impl EditCommand for SetSectionName {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.id.index();
        if idx >= model.sections.len() || model.sections[idx].id != self.id {
            return Box::new(Noop);
        }
        if sepika_core::model::section_key_taken(
            &model.sections,
            (self.name.as_str(), self.floor.as_deref()),
            Some(idx),
        ) {
            return Box::new(Noop);
        }
        let old_name = std::mem::replace(&mut model.sections[idx].name, self.name.clone());
        let old_floor = std::mem::replace(&mut model.sections[idx].floor, self.floor.clone());
        Box::new(SetSectionName {
            id: self.id,
            name: old_name,
            floor: old_floor,
        })
    }

    fn label(&self) -> &str {
        "断面符号・階変更"
    }
}

/// 断面形状を新規追加（UI-3 の新規断面作成）。
///
/// 符号＋階が既存の断面と衝突する追加は [`Noop`] として拒否する。
pub struct AddSectionShape {
    pub shape: sepika_core::section_shape::SectionShape,
    pub new_id: SectionId,
    pub name: String,
    pub floor: Option<String>,
    pub frame_use: Option<sepika_core::model::FrameSectionUse>,
}

impl EditCommand for AddSectionShape {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Err(reason) = self.shape.validate_corner_radius_input() {
            return Box::new(crate::RejectedEdit(reason));
        }
        let non_frame = matches!(
            &self.shape,
            sepika_core::section_shape::SectionShape::RcWall { .. }
                | sepika_core::section_shape::SectionShape::RcSlab { .. }
        );
        if (!non_frame && self.frame_use.is_none()) || (non_frame && self.frame_use.is_some()) {
            return Box::new(Noop);
        }
        let mut candidate = match if self.shape.has_unequal_box_thickness() {
            self.shape.input_section(self.new_id, self.name.clone())
        } else {
            self.shape.try_to_section(self.new_id, self.name.clone())
        } {
            Ok(candidate) => candidate,
            Err(reason) => return Box::new(crate::RejectedEdit(reason)),
        };
        if self.shape.validate_surface_radius().is_err() {
            return Box::new(Noop);
        }
        candidate.frame_use = self.frame_use;
        if let Err(reason) = candidate.validate_rc_frame_use() {
            return Box::new(crate::RejectedEdit(reason));
        }
        if !candidate.cft_frame_use_allowed() {
            return Box::new(Noop);
        }
        if sepika_core::model::section_key_taken(
            &model.sections,
            (self.name.as_str(), self.floor.as_deref()),
            None,
        ) {
            return Box::new(Noop);
        }
        let mut sec = candidate;
        sec.floor = self.floor.clone();
        sec.frame_use = self.frame_use;
        model.sections.push(sec);
        Box::new(DeleteSection { id: self.new_id })
    }

    fn label(&self) -> &str {
        "断面形状追加"
    }
}

/// 断面形状変更。逆操作は RestoreSection。
///
/// 符号と階は断面の同一性キーなので、形状を差し替えても維持する
/// （`to_section` は形状から決まらない階を `None` で返すため、明示的に引き継ぐ）。
pub struct EditSectionShape {
    pub section: SectionId,
    pub new_shape: sepika_core::section_shape::SectionShape,
    pub frame_use: Option<sepika_core::model::FrameSectionUse>,
}

/// フィレット半径・角Rを変更する。直接入力・カタログ性能と材料・用途は維持する。
pub struct SetSectionSurfaceRadius {
    pub section: SectionId,
    pub radius_mm: Option<f64>,
}

impl EditCommand for SetSectionSurfaceRadius {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let Some(section) = model.sections.get_mut(self.section.index()) else {
            return Box::new(Noop);
        };
        if section.id != self.section {
            return Box::new(Noop);
        }
        if self.radius_mm.is_none()
            && matches!(
                section.shape,
                Some(
                    sepika_core::section_shape::SectionShape::SteelBox { .. }
                        | sepika_core::section_shape::SectionShape::CftBox { .. }
                )
            )
        {
            return Box::new(crate::RejectedEdit(
                "角形断面の角部外半径（角R）が未設定です。溶接組立BOXの直角は0を明示してください"
                    .into(),
            ));
        }
        let old = section.clone();
        let Ok(updated) = section.with_surface_radius(self.radius_mm) else {
            return Box::new(Noop);
        };
        *section = updated;
        Box::new(RestoreSection { old })
    }

    fn label(&self) -> &str {
        "フィレット半径・角R変更"
    }
}

impl EditCommand for EditSectionShape {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Err(reason) = self.new_shape.validate_corner_radius_input() {
            return Box::new(crate::RejectedEdit(reason));
        }
        if self.new_shape.validate_surface_radius().is_err() {
            return Box::new(Noop);
        }
        let non_frame = matches!(
            &self.new_shape,
            sepika_core::section_shape::SectionShape::RcWall { .. }
                | sepika_core::section_shape::SectionShape::RcSlab { .. }
        );
        let idx = self.section.index();
        if idx >= model.sections.len() || model.sections[idx].id != self.section {
            return Box::new(Noop);
        }
        let candidate = sepika_core::model::Section {
            shape: Some(self.new_shape.clone()),
            frame_use: self.frame_use,
            ..model.sections[idx].clone()
        };
        if let Err(reason) = model.validate_rc_section_references(&candidate) {
            return Box::new(crate::RejectedEdit(reason));
        }
        if (non_frame && self.frame_use.is_some())
            || !section_use_is_valid(model, self.section, self.frame_use)
            || !candidate.cft_frame_use_allowed()
            || (candidate.is_cft() && !cft_section_references_are_valid(model, &candidate))
        {
            return Box::new(Noop);
        }
        let old = model.sections[idx].clone();
        let without_radius = |shape: &sepika_core::section_shape::SectionShape| {
            let mut dimensions = shape.clone();
            let radius = match &mut dimensions {
                sepika_core::section_shape::SectionShape::SteelH { root_r, .. } => root_r.take(),
                sepika_core::section_shape::SectionShape::SteelBox { corner_r, .. }
                | sepika_core::section_shape::SectionShape::CftBox { corner_r, .. } => {
                    corner_r.take()
                }
                _ => return None,
            };
            Some((dimensions, radius))
        };
        let radius_only = old
            .shape
            .as_ref()
            .and_then(without_radius)
            .zip(without_radius(&self.new_shape))
            .filter(|(old, new)| old.0 == new.0)
            .map(|(_, new)| new.1);
        if let Some(radius) = radius_only {
            let Ok(mut updated) = old.with_surface_radius(radius) else {
                return Box::new(Noop);
            };
            updated.frame_use = self.frame_use;
            model.sections[idx] = updated;
            return Box::new(RestoreSection { old });
        }
        let mut new_sec = match if self.new_shape.has_unequal_box_thickness() {
            self.new_shape.input_section(self.section, old.name.clone())
        } else {
            self.new_shape
                .try_to_section(self.section, old.name.clone())
        } {
            Ok(section) => section,
            Err(reason) => return Box::new(crate::RejectedEdit(reason)),
        };
        new_sec.floor = old.floor.clone();
        new_sec.frame_use = self.frame_use;
        new_sec.material = old.material;
        new_sec.rebar_material = old.rebar_material;
        new_sec.shear_rebar_material = old.shear_rebar_material;
        new_sec.steel_material = old.steel_material;
        let input = model.stb_strengths.clone();
        use sepika_core::section_shape::SectionShape;
        let entered = match &self.new_shape {
            SectionShape::RcBeamRect { rebar, .. } | SectionShape::SrcBeamRect { rebar, .. } => {
                !rebar.is_unset()
                    || [
                        rebar.main_dia,
                        rebar.cover,
                        rebar.stirrup.dia,
                        rebar.stirrup.pitch,
                    ]
                    .iter()
                    .any(|n| *n != 0.0)
                    || rebar.stirrup.legs != 0
            }
            SectionShape::RcColumnRect { rebar, .. }
            | SectionShape::SrcColumnRect { rebar, .. } => {
                !rebar.is_unset()
                    || [
                        rebar.main_dia,
                        rebar.cover,
                        rebar.hoop.dia,
                        rebar.hoop.pitch,
                    ]
                    .iter()
                    .any(|n| *n != 0.0)
                    || rebar.hoop.legs_x != 0
                    || rebar.hoop.legs_y != 0
            }
            SectionShape::RcColumnCircle { rebar, .. } => {
                !rebar.is_unset()
                    || [
                        rebar.main_dia,
                        rebar.cover,
                        rebar.hoop.dia,
                        rebar.hoop.pitch,
                    ]
                    .iter()
                    .any(|n| *n != 0.0)
            }
            _ => false,
        };
        if let Some(source) = model
            .stb_strengths
            .sections
            .iter_mut()
            .find(|s| s.section == self.section && (s.rebar.is_some() || entered))
        {
            source.rebar = None;
            source.native_rebar = true;
            source.reinforcement.clear();
        }
        model.sections[idx] = new_sec;
        Box::new(crate::strength::RestoreStrengthInput {
            input,
            inverse: Box::new(RestoreSection { old }),
            stories: model.source_stories.clone(),
        })
    }

    fn label(&self) -> &str {
        "断面形状変更"
    }
}

fn cft_section_references_are_valid(model: &Model, section: &sepika_core::model::Section) -> bool {
    let section_id = section.id;
    model
        .elements
        .iter()
        .filter(|element| element.section == Some(section_id))
        .all(|element| {
            matches!(
                element.kind,
                sepika_core::model::ElementKind::Beam
                    | sepika_core::model::ElementKind::Fiber
                    | sepika_core::model::ElementKind::MultiSpring
            )
        })
        && model
            .beams()
            .chain(model.posts())
            .filter(|member| member.section == Some(section_id))
            .all(|member| Model::validate_secondary_section_use(member, section).is_ok())
        && !model
            .slabs
            .iter()
            .any(|slab| slab.section() == Some(section_id))
        && !model
            .wall_plates
            .iter()
            .any(|plate| plate.section == Some(section_id))
}

fn section_use_is_valid(
    model: &Model,
    section_id: SectionId,
    frame_use: Option<sepika_core::model::FrameSectionUse>,
) -> bool {
    let element_valid = model
        .elements
        .iter()
        .filter(|element| element.section == Some(section_id))
        .filter(|element| {
            matches!(
                element.kind,
                sepika_core::model::ElementKind::Beam
                    | sepika_core::model::ElementKind::Fiber
                    | sepika_core::model::ElementKind::MultiSpring
                    | sepika_core::model::ElementKind::Brace { .. }
            )
        })
        .all(|element| {
            matches!(
                (element.kind, frame_use),
                (
                    sepika_core::model::ElementKind::Brace { .. },
                    Some(sepika_core::model::FrameSectionUse::Brace),
                ) | (
                    sepika_core::model::ElementKind::Beam
                        | sepika_core::model::ElementKind::Fiber
                        | sepika_core::model::ElementKind::MultiSpring,
                    Some(
                        sepika_core::model::FrameSectionUse::Girder
                            | sepika_core::model::FrameSectionUse::Column,
                    ),
                )
            )
        });
    let secondary_valid = model
        .beams()
        .chain(model.posts())
        .filter(|member| member.section == Some(section_id))
        .all(|member| {
            matches!(
                (member.kind, frame_use),
                (_, None)
                    | (
                        sepika_core::model::SecondaryMemberKind::Beam,
                        Some(sepika_core::model::FrameSectionUse::Girder),
                    )
                    | (
                        sepika_core::model::SecondaryMemberKind::Post,
                        Some(sepika_core::model::FrameSectionUse::Column),
                    )
            )
        });
    element_valid && secondary_valid
}

/// 断面データを指定した Section で復元する（EditSectionShape の逆操作）。
pub struct RestoreSection {
    pub old: sepika_core::model::Section,
}

impl EditCommand for RestoreSection {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let idx = self.old.id.index();
        if idx >= model.sections.len() || model.sections[idx].id != self.old.id {
            return Box::new(Noop);
        }
        let replaced = std::mem::replace(&mut model.sections[idx], self.old.clone());
        Box::new(RestoreSection { old: replaced })
    }

    fn label(&self) -> &str {
        "断面復元"
    }
}

/// カタログから選んだ断面（数値直入力）を末尾へ追加する。
///
/// 断面性能はカタログの表値をそのまま用いるため形状からは作れず、
/// [`AddSectionShape`] とは別の経路になる。符号＋階が既存の断面と衝突する追加は
/// [`Noop`] として拒否する。逆操作の [`AddSection`] は削除の取り消し専用で
/// 衝突判定を持たないため、新規追加はこちらを使う。
pub struct AddCatalogSection {
    pub section: sepika_core::model::Section,
}

impl EditCommand for AddCatalogSection {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Some(shape) = &self.section.shape {
            if let Err(reason) = shape.validate_corner_radius_input() {
                return Box::new(crate::RejectedEdit(reason));
            }
        }
        let mut sec = self.section.clone();
        if let Err(reason) = sec.validate_rc_frame_use() {
            return Box::new(crate::RejectedEdit(reason));
        }
        if !sec.cft_frame_use_allowed()
            || sepika_core::model::section_key_taken(&model.sections, sec.key(), None)
        {
            return Box::new(Noop);
        }
        let new_id = SectionId(model.sections.len() as u32);
        sec.id = new_id;
        model.sections.push(sec);
        Box::new(DeleteSection { id: new_id })
    }

    fn label(&self) -> &str {
        "カタログ断面追加"
    }
}

/// 複製断面の符号を、符号＋階が既存の断面と衝突しないように決める。
/// `C1(複製)` を起点に、埋まっていれば `C1(複製2)`・`C1(複製3)`… と後ろへ送る。
fn unique_duplicate_name(
    sections: &[sepika_core::model::Section],
    base: &str,
    floor: Option<&str>,
) -> String {
    let first = format!("{base}(複製)");
    if !sepika_core::model::section_key_taken(sections, (first.as_str(), floor), None) {
        return first;
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}(複製{n})");
        if !sepika_core::model::section_key_taken(sections, (candidate.as_str(), floor), None) {
            return candidate;
        }
        n += 1;
    }
}

/// 部材が参照する断面を複製し、部材に新断面を割り当てる。
pub struct DuplicateSectionForMember {
    pub member: ElemId,
}

impl EditCommand for DuplicateSectionForMember {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let elem_idx = self.member.index();
        if elem_idx >= model.elements.len() || model.elements[elem_idx].id != self.member {
            return Box::new(Noop);
        }
        let sid = match model.elements[elem_idx].section {
            Some(s) => s,
            None => return Box::new(Noop),
        };
        let sec_idx = sid.index();
        if sec_idx >= model.sections.len() || model.sections[sec_idx].id != sid {
            return Box::new(Noop);
        }
        let orig = &model.sections[sec_idx];
        if !crate::refs::frame_element_section_ref_ok(model, &model.elements[elem_idx], Some(sid)) {
            return Box::new(Noop);
        }
        let new_id = SectionId(model.sections.len() as u32);
        let mut new_sec = orig.clone();
        new_sec.id = new_id;
        new_sec.name = unique_duplicate_name(&model.sections, &orig.name, orig.floor.as_deref());
        model.sections.push(new_sec);
        model.elements[elem_idx].section = Some(new_id);
        with_id_changes(
            Box::new(RestoreElementSectionAndDeleteSection {
                elem: self.member,
                old_section: Some(sid),
                new_section: new_id,
            }),
            vec![IdChange::SectionInserted(new_id)],
        )
    }

    fn label(&self) -> &str {
        "部材断面複製"
    }
}

/// DuplicateSectionForMember の逆操作。
pub struct RestoreElementSectionAndDeleteSection {
    pub elem: ElemId,
    pub old_section: Option<SectionId>,
    pub new_section: SectionId,
}

impl EditCommand for RestoreElementSectionAndDeleteSection {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let elem_idx = self.elem.index();
        if elem_idx >= model.elements.len() || model.elements[elem_idx].id != self.elem {
            return Box::new(Noop);
        }
        let new_idx = self.new_section.index();
        if new_idx >= model.sections.len() || model.sections[new_idx].id != self.new_section {
            return Box::new(Noop);
        }
        model.elements[elem_idx].section = self.old_section;
        model.sections.remove(new_idx);
        let removed_id = self.new_section;
        shift_section_ids(model, |sid| {
            if sid.0 > removed_id.0 {
                sid.0 -= 1;
            }
        });
        with_id_changes(
            Box::new(DuplicateSectionForMember { member: self.elem }),
            vec![IdChange::SectionRemoved(self.new_section)],
        )
    }

    fn label(&self) -> &str {
        "部材断面複製解除"
    }
}

id_indexed_delete_insert!(
    /// 断面削除。部材・床板・壁版・ST-Bridge断面強度入力から参照中は Noop とする
    /// （先に割当を解除するか、UI 側でボタンを無効化する）。
    /// ID＝配列インデックスの不変条件を保つため、削除後は後続の断面 ID と
    /// 部材からの参照を 1 つずつ繰り上げる。
    DeleteSection,
    /// 断面追加（DeleteSection の逆操作）。後続の断面 ID・参照を 1 つ繰り下げてから挿入する。
    AddSection,
    id = SectionId,
    entity = sepika_core::model::Section,
    vec = sections,
    shift = shift_section_ids,
    guard = section_in_use,
    remap = (SectionRemoved, SectionInserted),
    del_label = "断面削除",
    ins_label = "断面追加",
);

/// モデル内の全ての `SectionId` 参照（断面自身の ID を含む）に `f` を適用する。
/// 走査は core 側（[`Model::visit_section_ids`]）が単一情報源として持つ
/// （新フィールド追加時の追随漏れを防ぐ）。
fn shift_section_ids(model: &mut Model, f: impl FnMut(&mut SectionId)) {
    model.visit_section_ids(f);
}

fn section_in_use(model: &Model, id: SectionId) -> bool {
    DeleteSection { id }.blocked_reason(model).is_some()
}

impl DeleteSection {
    /// 参照中で削除できない場合は理由を返す。参照がなければ `None` を返す。
    /// 断面 ID の実在は検証しない。
    pub fn blocked_reason(&self, model: &Model) -> Option<&'static str> {
        let strength = model
            .stb_strengths
            .sections
            .iter()
            .any(|s| s.section == self.id);
        let member = model.elements.iter().any(|e| e.section == Some(self.id))
            || model.slabs.iter().any(|s| s.section() == Some(self.id))
            || model.wall_plates.iter().any(|p| p.section == Some(self.id))
            || model
                .beams()
                .chain(model.posts())
                .any(|sm| sm.section == Some(self.id));
        match (member, strength) {
            (false, false) => None,
            (true, false) => Some("部材・小梁・二次部材から参照中のため削除できません"),
            (false, true) => Some("ST-Bridge断面強度入力から参照中のため削除できません"),
            (true, true) => {
                Some("部材・小梁・二次部材とST-Bridge断面強度入力から参照中のため削除できません")
            }
        }
    }
}

#[cfg(test)]
mod corner_radius_input_tests {
    use super::*;
    use sepika_core::{model::FrameSectionUse, section_shape::SectionShape};
    #[test]
    fn box_radius_missing_rejects_all_setting_commands_without_changing_model_or_history() {
        for cft in [false, true] {
            let shape = if cft {
                SectionShape::CftBox {
                    height: 300.0,
                    width: 300.0,
                    thick: 12.0,
                    horizontal_thick: 12.0,
                    corner_r: Some(0.0),
                }
            } else {
                SectionShape::SteelBox {
                    height: 300.0,
                    width: 300.0,
                    thick: 12.0,
                    horizontal_thick: 12.0,
                    corner_r: Some(0.0),
                }
            };
            let mut m = Model::default();
            let mut history = crate::UndoStack::new();
            assert!(history.run(
                &mut m,
                Box::new(AddSectionShape {
                    shape: shape.clone(),
                    new_id: SectionId(0),
                    name: "BOX".into(),
                    floor: None,
                    frame_use: Some(FrameSectionUse::Column)
                })
            ));
            assert!(history.run(
                &mut m,
                Box::new(SetSectionName {
                    id: SectionId(0),
                    name: "変更".into(),
                    floor: None
                })
            ));
            history.undo(&mut m);
            let mut unknown = shape;
            match &mut unknown {
                SectionShape::SteelBox { corner_r, .. } | SectionShape::CftBox { corner_r, .. } => {
                    *corner_r = None
                }
                _ => unreachable!(),
            }
            let mut catalog = m.sections[0].clone();
            catalog.id = SectionId(1);
            catalog.name = "CAT".into();
            catalog.shape = Some(unknown.clone());
            let commands: Vec<Box<dyn EditCommand>> = vec![
                Box::new(AddSectionShape {
                    shape: unknown.clone(),
                    new_id: SectionId(1),
                    name: "追加".into(),
                    floor: None,
                    frame_use: Some(FrameSectionUse::Column),
                }),
                Box::new(EditSectionShape {
                    section: SectionId(0),
                    new_shape: unknown,
                    frame_use: Some(FrameSectionUse::Column),
                }),
                Box::new(SetSectionSurfaceRadius {
                    section: SectionId(0),
                    radius_mm: None,
                }),
                Box::new(AddCatalogSection { section: catalog }),
            ];
            for command in commands {
                let before = m.clone();
                let revision = history.revision();
                let states = (history.can_undo(), history.can_redo());
                assert!(!history.run(&mut m, command));
                assert!(history.last_error().unwrap().contains("角部外半径"));
                assert!(m.eq_ignoring_dofmap(&before));
                assert_eq!(history.revision(), revision);
                assert_eq!((history.can_undo(), history.can_redo()), states);
            }
            history.redo(&mut m);
            assert_eq!(m.sections[0].name, "変更");
            history.undo(&mut m);
            assert_eq!(m.sections[0].name, "BOX");
        }
    }
    #[test]
    fn explicit_zero_box_and_shapes_without_corner_radius_remain_settable() {
        assert!(SectionShape::SteelH {
            height: 300.0,
            width: 150.0,
            web_thick: 8.0,
            flange_thick: 12.0,
            root_r: None
        }
        .validate_corner_radius_input()
        .is_ok());
        let shapes = [
            SectionShape::SteelBox {
                height: 300.0,
                width: 300.0,
                thick: 12.0,
                horizontal_thick: 12.0,
                corner_r: Some(0.0),
            },
            SectionShape::SteelPipe {
                outer_dia: 216.3,
                thick: 8.2,
            },
        ];
        for shape in shapes {
            let mut m = Model::default();
            let mut history = crate::UndoStack::new();
            assert!(history.run(
                &mut m,
                Box::new(AddSectionShape {
                    shape: shape.clone(),
                    new_id: SectionId(0),
                    name: "入力".into(),
                    floor: None,
                    frame_use: Some(FrameSectionUse::Column)
                })
            ));
            history.undo(&mut m);
            assert!(m.sections.is_empty());
            history.redo(&mut m);
            assert_eq!(m.sections[0].shape, Some(shape));
        }
    }
}
