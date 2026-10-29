//! 複合コマンド。複数の編集コマンドを undo/redo 1 回の単位にまとめる。

use super::*;

/// 子コマンド列を順に適用し、逆操作を「逆順の逆コマンド列」として返す複合コマンド。
///
/// グリッド操作のペースト・範囲クリア・行削除など、複数セル（＋行追加・行削除）に
/// またがる変更を undo 1 回で丸ごと戻すための基盤。
/// 行削除は [`DeleteNode`] 等を行番号の降順に並べて構成すること
/// （昇順だと先行する削除で後続の ID がずれる）。
pub struct CompositeCommand {
    pub label: String,
    pub children: Vec<Box<dyn EditCommand>>,
}

impl EditCommand for CompositeCommand {
    fn section_assignments(&self) -> Option<Vec<(sepika_core::ids::ElemId, Option<SectionId>)>> {
        self.children
            .iter()
            .filter(|child| !child.is_noop())
            .map(|child| child.section_assignments())
            .collect::<Option<Vec<_>>>()
            .map(|assignments| assignments.into_iter().flatten().collect())
    }
    fn affects_calculation(&self) -> bool {
        self.children
            .iter()
            .any(|child| child.affects_calculation())
    }
    fn changes_assignment_boundaries(&self) -> bool {
        self.children
            .iter()
            .any(|child| child.changes_assignment_boundaries())
    }
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut candidate = model.clone();
        let inverse = self.apply_in_transaction(model, &mut candidate);
        if inverse.rejection().is_none() {
            *model = candidate;
        }
        inverse
    }

    fn apply_in_transaction(&self, before: &Model, model: &mut Model) -> Box<dyn EditCommand> {
        let inverse = self.apply_candidate(model);
        if inverse.rejection().is_some() {
            return inverse;
        }
        if let Err(error) = model.validate_rc_section_usage() {
            return Box::new(crate::RejectedEdit(error.to_string()));
        }
        if let Err(error) = model.validate_attached_slabs() {
            return Box::new(crate::RejectedEdit(error.to_string()));
        }
        if let Err(reason) = crate::node_member::validate_coordinate_loads(before, model) {
            return Box::new(crate::RejectedEdit(reason));
        }
        if self.changes_assignment_boundaries() {
            let report = model.rebuild_assignment_regions_dropping_orphan_plates();
            if let Some(reason) = report.floor.rejection.or(report.wall.rejection) {
                return Box::new(crate::RejectedEdit(reason));
            }
        }
        inverse
    }

    fn apply_candidate(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut inverses = Vec::new();
        for child in &self.children {
            let inverse = child.apply_candidate(model);
            if inverse.rejection().is_some() {
                return inverse;
            }
            inverses.push(inverse);
        }
        Box::new(CompositeCommand {
            label: self.label.clone(),
            children: inverses.into_iter().rev().collect(),
        })
    }

    fn inverse_id_changes(&self) -> Vec<IdChange> {
        self.children
            .iter()
            .rev()
            .flat_map(|child| child.inverse_id_changes())
            .collect()
    }

    fn label(&self) -> &str {
        &self.label
    }

    /// 全子コマンドが Noop（＝1 件も適用されなかった）なら複合全体も Noop。
    /// 空の複合コマンドも何も変更しないため Noop 扱いとする。
    fn is_noop(&self) -> bool {
        self.children.iter().all(|c| c.is_noop())
    }
}
