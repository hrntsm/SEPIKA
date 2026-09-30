//! 左ドック ナビゲータ。
//!
//! `panels` からの構造分割。アルゴリズム変更は行わない。

use super::*;
use sepika_core::units::to_display::force_kn;

impl App {
    /// 左ペイン：ナビゲータ（階/部材群/断面・材料/荷重ケース/結果ケースのツリー）。
    pub(crate) fn navigator_panel(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.strong("ナビゲータ");
            ui.separator();

            let header = egui::CollapsingHeader::new("部材グループ")
                .default_open(true)
                .id_salt("nav_groups");
            header.show(ui, |ui| {
                let (steel_ids, rc_ids) = member_material_groups(&self.core.model);
                let is_steel_sel = member_group_selected(&self.ui.scoped.selection, &steel_ids);
                if ui
                    .selectable_label(is_steel_sel, format!("鋼材部材 ({})", steel_ids.len()))
                    .on_hover_text("クリックで3Dビューにハイライト")
                    .clicked()
                {
                    self.select_members(steel_ids.clone(), None);
                }
                let is_rc_sel = member_group_selected(&self.ui.scoped.selection, &rc_ids);
                if ui
                    .selectable_label(is_rc_sel, format!("RC部材 ({})", rc_ids.len()))
                    .on_hover_text("クリックで3Dビューにハイライト")
                    .clicked()
                {
                    self.select_members(rc_ids.clone(), None);
                }
            });

            self.nav_load_cases(ui);

            self.nav_vibration_cases(ui);

            let header = egui::CollapsingHeader::new("部材一覧")
                .default_open(false)
                .id_salt("nav_members");
            header.show(ui, |ui| {
                use crate::table_util::{self, Col};
                let n = self.core.model.elements.len();
                table_util::standard_table(
                    ui,
                    "nav_members_tbl",
                    &[Col::id(), Col::label("種別")],
                    n,
                    |row| {
                        let idx = row.index();
                        let elem = self.core.model.elements[idx].clone();
                        let is_focus = self.ui.scoped.selection.active_member() == Some(elem.id);
                        row.col(|ui| {
                            if table_util::id_cell(ui, is_focus, elem.id.0, "クリックで部材を選択")
                            {
                                self.select_member(elem.id);
                            }
                        });
                        row.col(|ui| {
                            ui.label(format!("{:?}", elem.kind));
                        });
                    },
                );
            });

            self.nav_sections(ui);
            self.nav_materials(ui);

            self.nav_result_cases(ui);

            let _ = ui.collapsing("階/レベル", |ui| {
                if self.core.model.stories.is_empty() {
                    ui.colored_label(crate::theme::GRAY_600, "未定義");
                    if ui.small_button("🏢 解析タブで自動生成").clicked() {
                        self.ui.view.active_tab = Tab::Analysis;
                    }
                } else {
                    for s in self.core.model.stories.iter().rev() {
                        ui.label(format!(
                            "{}  Z={:.0}mm  W={:.1}kN",
                            s.name,
                            s.elevation,
                            force_kn(s.seismic_weight.unwrap_or(0.0))
                        ));
                    }
                }
            });
        });
    }
}

fn member_group_selected(selection: &GeometrySelection, group: &[ElemId]) -> bool {
    let mut ids = group.to_vec();
    ids.sort_unstable();
    ids.dedup();
    !ids.is_empty() && selection.members() == ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_selection_requires_whole_nonempty_set() {
        let mut selection = GeometrySelection::None;
        selection.select_members(vec![ElemId(2)], Some(ElemId(2)));
        assert!(!member_group_selected(&selection, &[ElemId(2), ElemId(3)]));
        assert!(!member_group_selected(&selection, &[]));
        selection.select_members(vec![ElemId(3), ElemId(2), ElemId(2)], None);
        assert!(member_group_selected(
            &selection,
            &[ElemId(3), ElemId(2), ElemId(3)]
        ));
        assert!(!member_group_selected(&selection, &[ElemId(2)]));
    }
}
