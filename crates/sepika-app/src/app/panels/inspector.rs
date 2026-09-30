//! 右ドック インスペクタ。
//!
//! `panels` からの構造分割。アルゴリズム変更は行わない。

use super::*;
use crate::table_util::fmt_section_prop;
use sepika_core::units::to_display::{area_cm2, inertia_cm4};

pub(crate) fn section_member_ids(
    model: &sepika_core::model::Model,
    sec_id: SectionId,
) -> Vec<ElemId> {
    model
        .elements
        .iter()
        .filter(|e| e.section == Some(sec_id))
        .map(|e| e.id)
        .collect()
}

impl App {
    /// 右ペイン：選択要素のインスペクタ。
    pub(crate) fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        let mut duplicate_member = None;
        let mut highlight_section_members: Option<Vec<ElemId>> = None;
        let display_model = if self.ui.scoped.selection.active_member().is_some()
            || self.ui.scoped.nav.focus_section.is_some()
        {
            crate::viewer::wall_expanded_view_model(&self.core.model)
        } else {
            std::borrow::Cow::Borrowed(&self.core.model)
        };
        ui.group(|ui| {
            ui.strong("インスペクタ");
            ui.separator();

            if let Some(id) = self.ui.scoped.nav.focus_vibration_case {
                if let Some(case) = self.core.model.vibration_cases.iter().find(|c| c.id == id) {
                    ui.strong("表示中の立体振動ケース");
                    ui.label(format!("名称: {}", case.name));
                    ui.label(format!("波形: {}", case.wave_name));
                    ui.label(format!(
                        "方向: {}",
                        match case.dir {
                            sepika_core::model::VibrationThDir::X => "X",
                            sepika_core::model::VibrationThDir::Y => "Y",
                            sepika_core::model::VibrationThDir::Xy => "X+Y",
                        }
                    ));
                    ui.label(format!(
                        "解析: {}",
                        if case.nonlinear {
                            "非線形"
                        } else {
                            "線形"
                        }
                    ));
                    ui.separator();
                }
            } else if let Some(id) = self.ui.scoped.nav.focus_lumped_vibration_case {
                if let Some(case) = self
                    .core
                    .model
                    .lumped_vibration_cases
                    .iter()
                    .find(|c| c.id == id)
                {
                    ui.strong("表示中の質点系振動ケース");
                    ui.label(format!("名称: {}", case.name));
                    ui.label(format!("波形: {}", case.wave_name));
                    ui.label(format!(
                        "方向: {}",
                        match case.dir {
                            sepika_core::model::LumpedVibrationDir::X => "X",
                            sepika_core::model::LumpedVibrationDir::Y => "Y",
                        }
                    ));
                    ui.label(format!(
                        "解析: {}・{}",
                        if case.nonlinear {
                            "非線形"
                        } else {
                            "線形"
                        },
                        match case.dim {
                            sepika_core::model::LumpedVibrationDim::Planar => "2次元",
                            sepika_core::model::LumpedVibrationDim::Spatial => "3次元",
                        }
                    ));
                    ui.separator();
                }
            }

            let count = self
                .ui
                .scoped
                .selection
                .members()
                .len()
                .max(self.ui.scoped.selection.nodes().len());
            if count > 0 {
                ui.label(format!("幾何選択: {count} 件"));
            }
            if count > 1
                && self.ui.scoped.selection.active_member().is_none()
                && self.ui.scoped.selection.active_node().is_none()
            {
                ui.label("複数選択中（個別の注目対象なし）");
            }
            if let Some(elem_id) = self.ui.scoped.selection.active_member() {
                if let Some(e) = display_model.element(elem_id) {
                    ui.label(format!("部材 ID: {}", e.id.0));
                    let n0 = e.nodes.first().map(|n| n.0).unwrap_or(0);
                    let n1 = e.nodes.get(1).map(|n| n.0).unwrap_or(0);
                    ui.label(format!("節点 I/J: {} / {}", n0, n1));
                    if let Some(sec_id) = e.section {
                        if let Some(sec) = self
                            .core
                            .model
                            .sections
                            .get(sec_id.index())
                            .filter(|s| s.id == sec_id)
                        {
                            ui.label(format!("断面: {} ({})", sec.name, sec_id.0));
                            ui.label(format!(
                                "  A = {} cm²",
                                fmt_section_prop(area_cm2(sec.area))
                            ));
                            ui.label(format!(
                                "  Iy= {} cm⁴",
                                fmt_section_prop(inertia_cm4(sec.iy))
                            ));
                            ui.label(format!(
                                "  Iz= {} cm⁴",
                                fmt_section_prop(inertia_cm4(sec.iz))
                            ));
                            let n_used = section_member_ids(&display_model, sec_id).len();
                            ui.colored_label(
                                crate::theme::BLUE_500,
                                format!("この断面を使う {} 部材に影響", n_used),
                            );
                            if self.core.model.element(elem_id).is_some()
                                && ui.button("📋 複製してこの部材だけ別断面に").clicked()
                            {
                                duplicate_member = Some(elem_id);
                            }
                        }
                    } else {
                        ui.label("断面: 未割当");
                    }
                    if let Some(mat) = self.core.model.element_material(e) {
                        ui.label(format!("材料: {} ({})", mat.name, mat.id.0));
                        ui.label(format!("  E = {:.1} N/mm²", mat.young));
                        if let Some(fc) = mat.fc {
                            ui.label(format!("  Fc = {:.1} N/mm²", fc));
                        }
                    }
                    ui.separator();
                    if let Some(r) = &self.core.scoped.results {
                        let positions = r
                            .member_checks
                            .iter()
                            .find(|m| m.elem == elem_id)
                            .map(|m| m.positions.as_slice())
                            .unwrap_or(&[]);
                        ui.label(format!("検定結果（{} 位置）", positions.len()));
                        for p in positions.iter().take(8) {
                            match &p.outcome {
                                sepika_design_jp::CheckOutcome::Checked(cr) => {
                                    let ratio = cr.ratio();
                                    let color = crate::theme::status_color(ratio);
                                    ui.colored_label(
                                        color,
                                        format!("  pos={:.2} 検定比={:.3}", p.xi, ratio),
                                    );
                                }
                                sepika_design_jp::CheckOutcome::Skipped { reason } => {
                                    ui.colored_label(
                                        crate::theme::GRAY_600,
                                        format!("  pos={:.2} 検定不能（{reason}）", p.xi),
                                    );
                                }
                            }
                        }
                        if positions.len() > 8 {
                            ui.label(format!("  ... 他 {} 件", positions.len() - 8));
                        }
                    }
                } else {
                    ui.colored_label(crate::theme::GRAY_600, "部材を選択してください");
                }
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(150, 150, 150),
                    "部材を選択してください",
                );
            }

            if let Some(sec_id) = self.ui.scoped.nav.focus_section {
                if let Some(sec) = self.core.model.section(sec_id) {
                    ui.separator();
                    ui.strong("表示中の断面");
                    ui.label(format!("名前: {} ({})", sec.name, sec_id.0));
                    ui.label(format!(
                        "  A = {} cm²",
                        fmt_section_prop(area_cm2(sec.area))
                    ));
                    ui.label(format!(
                        "  Iy= {} cm⁴",
                        fmt_section_prop(inertia_cm4(sec.iy))
                    ));
                    ui.label(format!(
                        "  Iz= {} cm⁴",
                        fmt_section_prop(inertia_cm4(sec.iz))
                    ));
                    let used = section_member_ids(&display_model, sec_id);
                    ui.label(format!("使用部材数: {}", used.len()));
                    if ui.button("🔍 使用部材を3Dハイライト").clicked() {
                        highlight_section_members = Some(used);
                    }
                }
            }

            ui.separator();
            if let Some(node_id) = self.ui.scoped.selection.active_node() {
                if let Some(node) = self.core.model.node(node_id) {
                    ui.label(format!("節点 ID: {}", node.id.0));
                    ui.label(format!(
                        "座標: ({:.3}, {:.3}, {:.3})",
                        node.coord[0], node.coord[1], node.coord[2]
                    ));
                    let is_fixed = node.restraint.0 != 0;
                    if is_fixed {
                        ui.label("拘束: あり");
                    } else {
                        ui.label("拘束: なし");
                    }
                }
            }
        });

        if let Some(member) = duplicate_member {
            self.core.scoped.undo.run(
                &mut self.core.model,
                Box::new(sepika_edit::DuplicateSectionForMember { member }),
            );
            self.core.scoped.staleness.mark_edited();
        }
        if let Some(members) = highlight_section_members {
            self.select_members(members, None);
        }
    }
}
