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
    /// 仕口パネルの板厚 [mm]（ダイアフラム補強・ダブラープレートによる増厚の
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
        let _old_value = match self.field {
            SectionField::Area => {
                let o = sec.area;
                sec.area = self.value;
                o
            }
            SectionField::Iy => {
                let o = sec.iy;
                sec.iy = self.value;
                o
            }
            SectionField::Iz => {
                let o = sec.iz;
                sec.iz = self.value;
                o
            }
            SectionField::J => {
                let o = sec.j;
                sec.j = self.value;
                o
            }
            SectionField::Depth => {
                let o = sec.depth;
                sec.depth = self.value;
                o
            }
            SectionField::Width => {
                let o = sec.width;
                sec.width = self.value;
                o
            }
            SectionField::AsY => {
                let o = sec.as_y;
                sec.as_y = self.value;
                o
            }
            SectionField::AsZ => {
                let o = sec.as_z;
                sec.as_z = self.value;
                o
            }
            SectionField::PanelThickness => {
                let o = sec.panel_thickness.unwrap_or(0.0);
                sec.panel_thickness = (self.value > 0.0).then_some(self.value);
                o
            }
        };
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
    pub shape: sepika_section::shape::SectionShape,
    pub new_id: SectionId,
    pub name: String,
    pub floor: Option<String>,
    pub frame_use: Option<sepika_core::model::FrameSectionUse>,
}

impl EditCommand for AddSectionShape {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let non_frame = matches!(
            &self.shape,
            sepika_section::shape::SectionShape::RcWall { .. }
                | sepika_section::shape::SectionShape::RcSlab { .. }
        );
        if (!non_frame && self.frame_use.is_none()) || (non_frame && self.frame_use.is_some()) {
            return Box::new(Noop);
        }
        let Ok(mut candidate) = self.shape.try_to_section(self.new_id, self.name.clone()) else {
            return Box::new(Noop);
        };
        if self.shape.validate_surface_radius().is_err() {
            return Box::new(Noop);
        }
        candidate.frame_use = self.frame_use;
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
    pub new_shape: sepika_section::shape::SectionShape,
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
        if self.new_shape.validate_surface_radius().is_err() {
            return Box::new(Noop);
        }
        let non_frame = matches!(
            &self.new_shape,
            sepika_section::shape::SectionShape::RcWall { .. }
                | sepika_section::shape::SectionShape::RcSlab { .. }
        );
        let idx = self.section.index();
        if idx >= model.sections.len() || model.sections[idx].id != self.section {
            return Box::new(Noop);
        }
        if (non_frame && self.frame_use.is_some())
            || !section_use_is_valid(model, self.section, self.frame_use)
            || !(sepika_core::model::Section {
                shape: Some(self.new_shape.clone()),
                frame_use: self.frame_use,
                ..model.sections[idx].clone()
            })
            .cft_frame_use_allowed()
            || (matches!(
                &self.new_shape,
                sepika_section::shape::SectionShape::CftBox { .. }
                    | sepika_section::shape::SectionShape::CftPipe { .. }
            ) && !cft_section_references_are_valid(model, self.section))
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
        let Ok(mut new_sec) = self
            .new_shape
            .try_to_section(self.section, old.name.clone())
        else {
            return Box::new(Noop);
        };
        new_sec.floor = old.floor.clone();
        new_sec.frame_use = self.frame_use;
        new_sec.material = old.material;
        new_sec.rebar_material = old.rebar_material;
        new_sec.shear_rebar_material = old.shear_rebar_material;
        new_sec.steel_material = old.steel_material;
        model.sections[idx] = new_sec;
        Box::new(RestoreSection { old })
    }

    fn label(&self) -> &str {
        "断面形状変更"
    }
}

fn cft_section_references_are_valid(model: &Model, section_id: SectionId) -> bool {
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
            .all(|member| member.section != Some(section_id))
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
        let mut sec = self.section.clone();
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
        Box::new(RestoreElementSectionAndDeleteSection {
            elem: self.member,
            old_section: Some(sid),
            new_section: new_id,
        })
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
        Box::new(DuplicateSectionForMember { member: self.elem })
    }

    fn label(&self) -> &str {
        "部材断面複製解除"
    }
}

id_indexed_delete_insert!(
    /// 断面削除。部材から参照中の断面は削除すると参照が壊れるため Noop とする
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
    del_label = "断面削除",
    ins_label = "断面追加",
);

/// モデル内の全ての `SectionId` 参照（断面自身の ID を含む）に `f` を適用する。
/// 走査は core 側（[`Model::visit_section_ids`]）が単一情報源として持つ
/// （新フィールド追加時の追随漏れを防ぐ）。
fn shift_section_ids(model: &mut Model, f: impl FnMut(&mut SectionId)) {
    model.visit_section_ids(f);
}

/// 指定断面を参照している要素・床板・壁版・二次部材が存在するか（削除ガード用）。
fn section_in_use(model: &Model, id: SectionId) -> bool {
    model.elements.iter().any(|e| e.section == Some(id))
        || model.slabs.iter().any(|s| s.section() == Some(id))
        || model.wall_plates.iter().any(|p| p.section == Some(id))
        || model
            .beams()
            .chain(model.posts())
            .any(|sm| sm.section == Some(id))
}
