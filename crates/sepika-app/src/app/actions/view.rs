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
            self.clear_geometry_selection();
            self.ui.scoped.boundary_node = None;
            self.core.scoped.staleness.mark_edited();
        }
    }

    pub(crate) fn redo_action(&mut self) {
        if self.core.scoped.undo.can_redo() {
            self.core.scoped.undo.redo(&mut self.core.model);
            self.clear_geometry_selection();
            self.ui.scoped.boundary_node = None;
            self.core.scoped.staleness.mark_edited();
        }
    }

    pub(crate) fn clear_log_action(&mut self) {
        self.core.log.entries.clear();
    }

    pub(crate) fn select_diagnostic_target(&mut self, target: DiagTarget) {
        match target {
            DiagTarget::Member(id) => self.select_member(id),
            DiagTarget::Node(id) => self.select_node(id),
        }
    }

    pub(crate) fn select_node(&mut self, id: NodeId) {
        self.select_nodes(vec![id], Some(id));
    }

    pub(crate) fn select_member(&mut self, id: ElemId) {
        self.select_members(vec![id], Some(id));
    }

    pub(crate) fn select_nodes(&mut self, ids: Vec<NodeId>, active: Option<NodeId>) {
        self.ui.scoped.selection.select_nodes(ids, active);
    }

    pub(crate) fn select_members(&mut self, ids: Vec<ElemId>, active: Option<ElemId>) {
        self.ui.scoped.selection.select_members(ids, active);
    }

    pub(crate) fn clear_geometry_selection(&mut self) {
        self.ui.scoped.selection = GeometrySelection::None;
    }

    pub(crate) fn clear_generated_member_selection(&mut self) {
        if self
            .ui
            .scoped
            .selection
            .members()
            .iter()
            .any(|id| self.core.model.element(*id).is_none())
        {
            self.clear_geometry_selection();
        }
    }
}
