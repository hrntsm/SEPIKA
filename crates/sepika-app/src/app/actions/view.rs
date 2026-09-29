use super::*;

impl App {
    /// フレーム先頭で保留中の階編集を一件だけ適用する。
    pub(crate) fn apply_pending_story_command(&mut self) {
        if let Some(cmd) = self.ui.scoped.pending_story_cmds.pop_front() {
            self.core.scoped.undo.run(&mut self.core.model, cmd);
            self.core.scoped.staleness.mark_edited();
        }
    }

    pub(crate) fn undo_action(&mut self) {
        if self.core.scoped.undo.can_undo() {
            self.core.scoped.undo.undo(&mut self.core.model);
            self.core.scoped.staleness.mark_edited();
        }
    }

    pub(crate) fn redo_action(&mut self) {
        if self.core.scoped.undo.can_redo() {
            self.core.scoped.undo.redo(&mut self.core.model);
            self.core.scoped.staleness.mark_edited();
        }
    }

    pub(crate) fn clear_log_action(&mut self) {
        self.core.log.entries.clear();
    }

    pub(crate) fn select_diagnostic_target(&mut self, target: DiagTarget) {
        match target {
            DiagTarget::Member(id) => {
                self.ui.scoped.selection.members = vec![id];
                self.ui.scoped.selection.nodes.clear();
                self.ui.scoped.nav.focus_member = Some(id);
            }
            DiagTarget::Node(id) => {
                self.ui.scoped.selection.nodes = vec![id];
                self.ui.scoped.selection.members.clear();
                self.ui.scoped.nav.focus_node = Some(id);
            }
        }
    }
}
