use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::Model;

/// 編集適用前から適用後への材料・断面 ID の挿入・削除。列挙順に適用する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdChange {
    MaterialRemoved(MaterialId),
    MaterialInserted(MaterialId),
    SectionRemoved(SectionId),
    SectionInserted(SectionId),
}

/// 編集コマンド。`Send` を要求するのは、MCP サーバが `UndoStack` を
/// スレッド間で共有する(`rmcp::ServerHandler: Send + Sync`)ため。
pub trait EditCommand: Send {
    /// モデルと履歴の確定後にだけ呼ばれる通知。候補の検証中には呼ばれない。
    fn committed(&self) {}
    /// 解析・検定の計算入力を変更する編集か。
    fn affects_calculation(&self) -> bool {
        true
    }
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand>;
    fn label(&self) -> &str;
    /// 単一部材の断面割当である場合の対象と割当先。
    fn section_assignment(&self) -> Option<(sepika_core::ids::ElemId, Option<SectionId>)> {
        None
    }
    /// 全変更が断面割当だけである場合に、対象と割当先を返す。
    fn section_assignments(&self) -> Option<Vec<(sepika_core::ids::ElemId, Option<SectionId>)>> {
        self.section_assignment().map(|assignment| vec![assignment])
    }
    /// この逆コマンドを生成した編集の ID 変更を、適用順に返す。材料・断面の挿入・削除がなければ空。
    fn inverse_id_changes(&self) -> Vec<IdChange> {
        Vec::new()
    }
    /// 破棄可能な複合編集の途中候補へ適用する。拒否時は呼出し側が候補全体を破棄する。
    /// 座標と荷重区間の整合は複合編集の最終候補で検証する。
    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        self.apply(model)
    }
    /// 確定境界が所有する破棄可能な候補へ適用する。拒否時は呼出し側が候補を破棄する。
    /// `before` は適用前の確定モデル。複合編集の中間状態には `apply_candidate` を使う。
    fn apply_in_transaction(&self, _before: &Model, candidate: &mut Model) -> Box<dyn EditCommand> {
        self.apply(candidate)
    }
    /// 何も変更しないコマンド（適用失敗時の安全なフォールバック `Noop` 等）か。
    /// [`UndoStack::run`] は逆コマンドがこれに該当する場合、undo 履歴へ積まない。
    fn is_noop(&self) -> bool {
        false
    }
    fn rejection(&self) -> Option<&str> {
        None
    }
    /// 支持境界を変更し、旧版除去の事前確認を必要とする編集か。
    fn changes_assignment_boundaries(&self) -> bool {
        false
    }
}

struct CommandWithIdChanges {
    command: Box<dyn EditCommand>,
    changes: Vec<IdChange>,
}

struct RestoreStbNodeIdentities {
    command: Box<dyn EditCommand>,
    identities: Vec<sepika_core::model::StbNodeIdentity>,
}

impl EditCommand for RestoreStbNodeIdentities {
    fn section_assignments(&self) -> Option<Vec<(sepika_core::ids::ElemId, Option<SectionId>)>> {
        self.command.section_assignments()
    }
    fn committed(&self) {
        self.command.committed();
    }
    fn section_assignment(&self) -> Option<(sepika_core::ids::ElemId, Option<SectionId>)> {
        self.command.section_assignment()
    }
    fn affects_calculation(&self) -> bool {
        self.command.affects_calculation()
    }
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let identities = model.stb_node_ids.clone();
        let command = self.command.apply(model);
        model.stb_node_ids = self.identities.clone();
        Box::new(Self {
            command,
            identities,
        })
    }
    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let identities = model.stb_node_ids.clone();
        let command = self.command.apply_candidate(model);
        model.stb_node_ids = self.identities.clone();
        Box::new(Self {
            command,
            identities,
        })
    }
    fn apply_in_transaction(&self, before: &Model, model: &mut Model) -> Box<dyn EditCommand> {
        let identities = model.stb_node_ids.clone();
        let command = self.command.apply_in_transaction(before, model);
        model.stb_node_ids = self.identities.clone();
        Box::new(Self {
            command,
            identities,
        })
    }
    fn label(&self) -> &str {
        self.command.label()
    }
    fn is_noop(&self) -> bool {
        self.command.is_noop()
    }
    fn rejection(&self) -> Option<&str> {
        self.command.rejection()
    }
    fn inverse_id_changes(&self) -> Vec<IdChange> {
        self.command.inverse_id_changes()
    }
}

fn with_id_changes(command: Box<dyn EditCommand>, changes: Vec<IdChange>) -> Box<dyn EditCommand> {
    Box::new(CommandWithIdChanges { command, changes })
}

impl EditCommand for CommandWithIdChanges {
    fn section_assignments(&self) -> Option<Vec<(sepika_core::ids::ElemId, Option<SectionId>)>> {
        self.command.section_assignments()
    }
    fn committed(&self) {
        self.command.committed();
    }
    fn section_assignment(&self) -> Option<(sepika_core::ids::ElemId, Option<SectionId>)> {
        self.command.section_assignment()
    }
    fn affects_calculation(&self) -> bool {
        self.command.affects_calculation()
    }
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        self.command.apply(model)
    }

    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        self.command.apply_candidate(model)
    }

    fn apply_in_transaction(&self, before: &Model, model: &mut Model) -> Box<dyn EditCommand> {
        self.command.apply_in_transaction(before, model)
    }

    fn label(&self) -> &str {
        self.command.label()
    }

    fn inverse_id_changes(&self) -> Vec<IdChange> {
        self.changes.clone()
    }
}

impl<T: EditCommand + 'static> From<T> for Box<dyn EditCommand> {
    fn from(cmd: T) -> Self {
        Box::new(cmd)
    }
}

pub(crate) struct RejectedEdit(pub String);
impl EditCommand for RejectedEdit {
    fn apply(&self, _model: &mut Model) -> Box<dyn EditCommand> {
        Box::new(Noop)
    }
    fn label(&self) -> &str {
        "編集拒否"
    }
    fn is_noop(&self) -> bool {
        true
    }
    fn rejection(&self) -> Option<&str> {
        Some(&self.0)
    }
}

/// 編集を確定せず、失う版 ID・入力荷重を診断する。履歴は変更しない。
pub fn preview_plate_assignment_loss(
    model: &Model,
    command: &dyn EditCommand,
) -> Result<sepika_core::model::PlateAssignmentLoss, String> {
    if !command.changes_assignment_boundaries() {
        return Ok(Default::default());
    }
    model
        .validate_assignment_region_identity()
        .map_err(|reason| format!("モデル・履歴は未更新: {reason}"))?;
    let mut candidate = model.clone();
    let inverse = command.apply_in_transaction(model, &mut candidate);
    if let Some(reason) = inverse.rejection() {
        return Err(reason.to_owned());
    }
    candidate
        .validate_assignment_region_identity()
        .map_err(|reason| format!("モデル・履歴は未更新: {reason}"))?;
    Ok(model.assignment_plate_loss(&candidate))
}

/// 検討で割当可能な、鋼材と有限な正の剛性を持つ既存大梁断面か。
pub fn valid_study_section(model: &Model, section: &sepika_core::model::Section) -> bool {
    section.frame_use == Some(sepika_core::model::FrameSectionUse::Girder)
        && section
            .material
            .and_then(|id| model.materials.iter().find(|m| m.id == id))
            .is_some_and(|m| {
                m.category == sepika_core::model::MaterialCategory::Steel
                    && m.young.is_finite()
                    && m.young > 0.0
            })
        && [section.area, section.iy, section.iz, section.j]
            .iter()
            .all(|x| x.is_finite() && *x > 0.0)
        && section.ensure_properties_resolved().is_ok()
        && section.resolved_area().is_ok_and(|v| v > 0.0)
        && section.resolved_j().is_ok_and(|v| v > 0.0)
        && [section.property_basis.iy, section.property_basis.iz]
            .iter()
            .all(|v| *v != sepika_core::model::PropertyBasis::PendingShape)
}

pub struct UndoStack {
    done: Vec<Box<dyn EditCommand>>,
    undone: Vec<Box<dyn EditCommand>>,
    max_undo: usize,
    revision: u64,
    last_error: Option<String>,
    id_changes: Vec<IdChange>,
    affects_calculation: bool,
    restriction: Option<Vec<sepika_core::ids::ElemId>>,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            done: Vec::new(),
            undone: Vec::new(),
            max_undo: 100,
            revision: 0,
            last_error: None,
            id_changes: Vec::new(),
            affects_calculation: true,
            restriction: None,
        }
    }

    /// モデルを変更するたびに 1 つ増える通し番号。
    ///
    /// モデルを複製して試算する事前表示など、変更のたびに作り直したい派生データが
    /// 毎フレーム計算し直さずに済むよう、キャッシュのキーとして使う。
    /// undo・redo でも増えるため、このスタックを通した変更であれば、値が同じことが
    /// モデルが同じことを意味する。
    /// 空集合は読取専用。指定時は既存鉄骨大梁への有効断面割当だけを許可する。
    pub fn restrict_sections(&mut self, targets: Vec<sepika_core::ids::ElemId>) {
        self.restriction = Some(targets);
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// コマンドを適用し、モデルが変更されたか（適用に成功したか）を返す。
    ///
    /// 適用に失敗したコマンド（逆コマンドが `Noop`）は undo 履歴へ積まず、
    /// redo 履歴も消さない。かつては失敗時も `Noop` を積んでいたため、
    /// undo ラベルに「Noop」が表示される・undo が 1 段を無駄に消費する・
    /// 失敗した操作で redo 履歴が失われる、という不整合が生じていた。
    pub fn run(&mut self, model: &mut Model, cmd: Box<dyn EditCommand>) -> bool {
        self.last_error = None;
        self.id_changes.clear();
        if let Err(reason) = self.check_restriction(model, cmd.as_ref()) {
            self.last_error = Some(reason);
            return false;
        }
        if let Err(reason) = model.validate_assignment_region_identity() {
            self.last_error = Some(format!("モデルは未更新: {reason}"));
            return false;
        }
        let mut candidate = model.clone();
        if let Err(reason) = candidate.assign_stb_node_ids() {
            self.last_error = Some(reason);
            return false;
        }
        let affects_calculation = cmd.affects_calculation();
        let inv = cmd.apply_in_transaction(model, &mut candidate);
        if let Some(reason) = inv.rejection() {
            self.last_error = Some(reason.to_owned());
            return false;
        }
        if inv.is_noop() {
            return false;
        }
        if let Err(reason) = candidate.validate_assignment_region_identity() {
            self.last_error = Some(format!("モデルは未更新: {reason}"));
            return false;
        }
        if let Err(error) = candidate.validate_rc_section_usage() {
            self.last_error = Some(error.to_string());
            return false;
        }
        if let Err(error) = candidate.validate_attached_slabs() {
            self.last_error = Some(error.to_string());
            return false;
        }
        if let Err(reason) = candidate.assign_stb_node_ids() {
            self.last_error = Some(reason);
            return false;
        }
        let inv: Box<dyn EditCommand> = Box::new(RestoreStbNodeIdentities {
            command: inv,
            identities: model.stb_node_ids.clone(),
        });
        *model = candidate;
        self.affects_calculation = affects_calculation;
        self.id_changes = inv.inverse_id_changes();
        self.done.push(inv);
        if self.done.len() > self.max_undo {
            self.done.remove(0);
        }
        self.undone.clear();
        self.revision += 1;
        cmd.committed();
        true
    }

    fn check_restriction(&self, model: &Model, cmd: &dyn EditCommand) -> Result<(), String> {
        if let Some(targets) = &self.restriction {
            let allowed = cmd.section_assignments().is_some_and(|assignments| {
                !assignments.is_empty()
                    && assignments.into_iter().all(|(elem, section)| {
                        targets.contains(&elem)
                            && model.element(elem).is_some_and(|e| {
                                e.kind == sepika_core::model::ElementKind::Beam
                                    && e.section
                                        .and_then(|id| model.section(id))
                                        .is_some_and(|s| valid_study_section(model, s))
                            })
                            && section
                                .and_then(|id| model.section(id))
                                .is_some_and(|s| valid_study_section(model, s))
                    })
            });
            if !allowed {
                return Err(
                    "この案では固定対象の鉄骨大梁への既存有効断面割当だけを変更できます".into(),
                );
            }
        }
        Ok(())
    }

    /// 確認を行えない入口では、版・入力荷重が失われる編集を確定せず拒否する。
    pub fn run_preserving_plate_assignments(
        &mut self,
        model: &mut Model,
        cmd: Box<dyn EditCommand>,
    ) -> bool {
        match preview_plate_assignment_loss(model, cmd.as_ref()) {
            Err(reason) => self.last_error = Some(reason),
            Ok(loss) if !loss.is_empty() => {
                self.last_error = Some(format!(
                    "モデル・履歴は未更新。{}\n旧版の除去には確認が必要です。この入口では確認できないため編集を拒否します。確認可能な入口で適用した場合は、1 Undo で全 ID・割当・版・入力荷重を復元できます。",
                    loss.description()
                ));
            }
            Ok(_) => return self.run(model, cmd),
        }
        self.id_changes.clear();
        false
    }

    /// 直前の編集が拒否された理由。成功または通常の Noop では `None`。
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// 直前の run・undo・redo で確定した ID 変更。失敗または履歴なしでは空。
    pub fn id_changes(&self) -> &[IdChange] {
        &self.id_changes
    }

    pub fn undo(&mut self, model: &mut Model) {
        self.last_error = None;
        if let Some(cmd) = self.done.last() {
            if let Err(reason) = self.check_restriction(model, cmd.as_ref()) {
                self.last_error = Some(reason);
                return;
            }
        }
        self.id_changes.clear();
        if let Some(cmd) = self.done.pop() {
            self.affects_calculation = cmd.affects_calculation();
            let redo_cmd = cmd.apply(model);
            self.id_changes = redo_cmd.inverse_id_changes();
            self.undone.push(redo_cmd);
            self.revision += 1;
            cmd.committed();
        }
    }

    pub fn redo(&mut self, model: &mut Model) {
        self.last_error = None;
        self.id_changes.clear();
        let Some(cmd) = self.undone.last() else {
            return;
        };
        if let Err(reason) = self.check_restriction(model, cmd.as_ref()) {
            self.last_error = Some(reason);
            return;
        }
        if let Err(reason) = model.validate_assignment_region_identity() {
            self.last_error = Some(format!("モデル・履歴は未更新: {reason}"));
            return;
        }
        let mut candidate = model.clone();
        let affects_calculation = cmd.affects_calculation();
        let undo_cmd = cmd.apply_in_transaction(model, &mut candidate);
        if let Some(reason) = undo_cmd.rejection() {
            self.last_error = Some(format!("モデル・履歴は未更新: {reason}"));
            return;
        }
        if undo_cmd.is_noop() {
            return;
        }
        if let Err(reason) = candidate.validate_assignment_region_identity() {
            self.last_error = Some(format!("モデル・履歴は未更新: {reason}"));
            return;
        }
        if let Err(error) = candidate.validate_rc_section_usage() {
            self.last_error = Some(format!("モデル・履歴は未更新: {error}"));
            return;
        }
        if let Err(error) = candidate.validate_attached_slabs() {
            self.last_error = Some(format!("モデル・履歴は未更新: {error}"));
            return;
        }
        *model = candidate;
        self.affects_calculation = affects_calculation;
        let command = self.undone.pop().expect("Redo対象");
        self.id_changes = undo_cmd.inverse_id_changes();
        self.done.push(undo_cmd);
        self.revision += 1;
        command.committed();
    }

    /// 直前に確定した編集が解析・検定の計算入力を変更したか。
    pub fn affects_calculation(&self) -> bool {
        self.affects_calculation
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.done.last().map(|c| c.label())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.undone.last().map(|c| c.label())
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

/// ID＝配列添字エンティティの「削除／再挿入」コマンド対を生成するマクロ。
///
/// 材料・断面・荷重ケース・床で同一の定型（存在・同一性チェック → 使用中ガード →
/// remove → 後続 ID の繰り上げ → 逆操作の生成、およびその逆）が 4 通りに手書き
/// コピーされ、ID 繰り上げ漏れ（本クレートの過去の不具合の温床）になっていた
/// ため、機構部分を単一情報源化する。エンティティ固有の退避データを持つ
/// 節点・部材（`node_member.rs`）は対象外（個別実装のまま）。
///
/// - Delete 側: `pub struct $delete { pub id: $id }`。存在しない・ID 不一致・
///   `guard`（使用中）のときは `Noop` を返す。
/// - Insert 側: `pub struct $insert { pub index: usize, pub old: $entity }`。
///   挿入位置以降の ID を繰り下げてから、`old` の ID を振り直して挿入する。
/// - `shift`: モデル内の全 ID 参照へ `f` を適用する関数
///   （`fn(&mut Model, impl FnMut(&mut Id))`。走査本体は core の `visit_*_ids`）。
/// - `guard`: `fn(&Model, Id) -> bool`。真なら削除を拒否して `Noop`。
macro_rules! id_indexed_delete_insert {
    (
        $(#[$del_meta:meta])*
        $delete:ident,
        $(#[$ins_meta:meta])*
        $insert:ident,
        id = $id_ty:ident,
        entity = $ent_ty:ty,
        vec = $vecf:ident,
        shift = $shift:expr,
        guard = $guard:expr,
        $(remap = ($removed:ident, $inserted:ident),)?
        del_label = $dl:literal,
        ins_label = $il:literal $(,)?
    ) => {
        $(#[$del_meta])*
        pub struct $delete {
            pub id: $id_ty,
        }

        impl EditCommand for $delete {
            fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
                let idx = self.id.index();
                if idx >= model.$vecf.len() || model.$vecf[idx].id != self.id {
                    return Box::new(Noop);
                }
                #[allow(clippy::redundant_closure_call)]
                if ($guard)(model, self.id) {
                    return Box::new(Noop);
                }
                let removed = model.$vecf.remove(idx);
                let target = self.id.0;
                $shift(model, |id: &mut $id_ty| {
                    if id.0 > target {
                        id.0 -= 1;
                    }
                });
                Box::new($insert {
                    index: idx,
                    old: removed,
                })
            }

            fn label(&self) -> &str {
                $dl
            }
            $(fn inverse_id_changes(&self) -> Vec<IdChange> {
                vec![IdChange::$inserted(self.id)]
            })?
        }

        $(#[$ins_meta])*
        pub struct $insert {
            pub index: usize,
            pub old: $ent_ty,
        }

        impl EditCommand for $insert {
            fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
                if self.index > model.$vecf.len() {
                    return Box::new(Noop);
                }
                let id = $id_ty(self.index as u32);
                $shift(model, |x: &mut $id_ty| {
                    if x.0 >= id.0 {
                        x.0 += 1;
                    }
                });
                let mut entity = self.old.clone();
                entity.id = id;
                model.$vecf.insert(self.index, entity);
                Box::new($delete { id })
            }

            fn label(&self) -> &str {
                $il
            }
            $(fn inverse_id_changes(&self) -> Vec<IdChange> {
                vec![IdChange::$removed($id_ty(self.index as u32))]
            })?
        }
    };
}

mod axis;
mod beam_placement;
mod composite;
mod load_case;
mod member_detail;
mod node_member;
mod refs;
mod secondary;
pub use beam_placement::*;
mod section_material;
mod steel_design;
mod story;
mod story_copy;
mod wall_misc;

pub use axis::*;
pub use composite::*;
pub use load_case::*;
pub use member_detail::*;
pub use node_member::*;
pub use secondary::*;
pub use section_material::*;
pub use steel_design::*;
pub use story::*;
pub use story_copy::*;
pub use wall_misc::*;

#[cfg(test)]
mod tests;

mod strength;
pub use strength::{SetSourceStoryConcreteStrength, SetStbStrengths};

/// 数量設定の全置換。解析入力を変更せず、逆操作は以前の数量設定へ戻す。
pub struct SetQuantityCfg {
    pub cfg: sepika_core::model::QuantityCfg,
}
impl EditCommand for SetQuantityCfg {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let cfg = std::mem::replace(&mut model.quantity_cfg, self.cfg);
        Box::new(Self { cfg })
    }
    fn label(&self) -> &str {
        "数量設定の変更"
    }
    fn affects_calculation(&self) -> bool {
        false
    }
}
