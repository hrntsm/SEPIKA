use crate::app::App;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::SrcJointHoops;
use sepika_core::section_shape::StandardRebar;
use sepika_design_jp::srrc::joint_hoops::{src_joint_hoop_input_statuses, SrcJointHoopInputStatus};
use sepika_edit::SetSrcJointHoops;

use super::finite_input::{parse_finite, EmptyInput};

pub(crate) struct Draft {
    node: NodeId,
    axis: [f64; 3],
    revision: u64,
    target: String,
    effective_legs: String,
    diameter: Option<StandardRebar>,
    spacing_mm: String,
    width_mm: String,
    grade: String,
}

impl Draft {
    fn new(status: &SrcJointHoopInputStatus<'_>, target: String, revision: u64) -> Self {
        let mut draft = Self {
            node: status.node,
            axis: status.axis,
            revision,
            target,
            effective_legs: String::new(),
            diameter: None,
            spacing_mm: String::new(),
            width_mm: String::new(),
            grade: String::new(),
        };
        if let [entry] = status.assignments.as_slice() {
            let hoops = &entry.hoops;
            draft.effective_legs = hoops.effective_legs.to_string();
            draft.diameter = Some(hoops.diameter);
            draft.spacing_mm = hoops.spacing_mm.to_string();
            draft.width_mm = hoops.width_mm.to_string();
            draft.grade.clone_from(&hoops.grade);
        }
        draft
    }

    fn input(&self) -> Result<SrcJointHoops, String> {
        let millimeters = |label: &str, text: &str| {
            parse_finite(text, EmptyInput::Optional)
                .map_err(|reason| format!("{label}: {reason}"))?
                .ok_or_else(|| format!("{label}を入力してください"))
        };
        let input = SrcJointHoops {
            effective_legs: self
                .effective_legs
                .trim()
                .parse()
                .map_err(|_| "有効脚本数には正の整数が必要です")?,
            diameter: self.diameter.ok_or("帯筋の呼び径を選択してください")?,
            spacing_mm: millimeters("間隔", &self.spacing_mm)?,
            width_mm: millimeters("対象幅", &self.width_mm)?,
            grade: self.grade.clone(),
        };
        input.ratio()?;
        Ok(input)
    }

    fn form(&mut self, ui: &mut egui::Ui) -> Option<SetSrcJointHoops> {
        ui.strong(&self.target);
        egui::Grid::new("src_joint_hoops_form").show(ui, |ui| {
            for (id, label, value) in [
                (
                    "src_joint_hoop_legs",
                    "有効脚本数",
                    &mut self.effective_legs,
                ),
                ("src_joint_hoop_spacing", "間隔 [mm]", &mut self.spacing_mm),
                ("src_joint_hoop_width", "対象幅 [mm]", &mut self.width_mm),
            ] {
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .id(egui::Id::new(id))
                        .desired_width(100.0),
                );
                ui.end_row();
            }
            ui.label("呼び径");
            egui::ComboBox::from_id_salt("src_joint_hoop_diameter")
                .selected_text(
                    self.diameter
                        .map_or_else(|| "呼び径を選択".into(), |size| size.to_string()),
                )
                .show_ui(ui, |ui| {
                    for diameter in StandardRebar::ALL {
                        ui.selectable_value(
                            &mut self.diameter,
                            Some(diameter),
                            diameter.to_string(),
                        );
                    }
                });
            ui.end_row();
            ui.label("帯筋材種");
            egui::ComboBox::from_id_salt("src_joint_hoop_grade")
                .selected_text(if self.grade.is_empty() {
                    "材種を選択"
                } else {
                    &self.grade
                })
                .show_ui(ui, |ui| {
                    for grade in SrcJointHoops::GRADES {
                        if ui.selectable_label(self.grade == grade, grade).clicked() {
                            self.grade = grade.into();
                        }
                    }
                });
            ui.end_row();
        });

        let input = self.input();
        if let Err(reason) = &input {
            ui.label(reason);
        }
        if ui
            .add_enabled(input.is_ok(), egui::Button::new("帯筋を登録"))
            .clicked()
        {
            return Some(SetSrcJointHoops {
                node: self.node,
                axis: self.axis,
                hoops: input.ok(),
            });
        }
        None
    }
}

pub(crate) fn table(ui: &mut egui::Ui, app: &mut App) {
    let statuses = src_joint_hoop_input_statuses(&app.core.model);
    if statuses.is_empty() {
        ui.label("SRC柱梁接合部の構面がありません。");
        return;
    }
    ui.label("構面ごとに、有効と確認した帯筋と対象幅を登録してください。表示値は短期せん断検定の入力値です。");
    let revision = app.core.scoped.undo.revision();
    let mut draft = app
        .ui
        .scoped
        .src_joint_hoop_draft
        .take()
        .filter(|draft| draft.revision == revision);
    let mut command = None;
    if let Some(form) = &mut draft {
        command = form.form(ui);
        ui.separator();
    }
    for status in statuses {
        let ids = |members: &[ElemId]| {
            members
                .iter()
                .map(|member| member.0.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let beams = ids(&status.beam_ids);
        let [x, y, z] = status.axis;
        let connection = if beams.is_empty() {
            "構面なし".into()
        } else {
            format!("柱 {} / 梁 {beams}", ids(&status.column_ids))
        };
        let target = format!(
            "節点 {} / 構面 [{x:.6}, {y:.6}, {z:.6}] / {connection}",
            status.node.0
        );
        ui.horizontal_wrapped(|ui| {
            ui.strong(&target);
            if !status.beam_ids.is_empty()
                && ui.button(format!("帯筋を編集（梁 {beams}）")).clicked()
            {
                draft = Some(Draft::new(&status, target.clone(), revision));
            }
            if !status.assignments.is_empty() && ui.button("帯筋を解除").clicked() {
                command = Some(SetSrcJointHoops {
                    node: status.node,
                    axis: status.axis,
                    hoops: None,
                });
            }
        });
        for entry in &status.assignments {
            let h = &entry.hoops;
            ui.label(format!(
                "{}-{} @{} mm / 対象幅 {} mm / {}",
                h.effective_legs, h.diameter, h.spacing_mm, h.width_mm, h.grade
            ));
        }
        if let Some(prepared) = status.prepared {
            ui.label(format!(
                "帯筋比 {:.8e} / 短期許容引張応力度 {} N/mm²",
                prepared.ratio, prepared.allowable_tension_n_per_mm2
            ));
        }
        if let Some(reason) = status.reason {
            ui.label(reason);
        }
        ui.separator();
    }
    if command.is_some_and(|command| app.apply_model_edit(Box::new(command))) {
        draft = None;
    }
    app.ui.scoped.src_joint_hoop_draft = draft;
}
#[cfg(test)]
mod tests {
    use super::*;
    mod fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-design-jp/tests/fixtures/src_joint_hoops_model.rs"
        ));
    }

    fn frame(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1300.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.model_tab_panel(ui),
        )
    }

    fn text_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output
            .shapes
            .iter()
            .rev()
            .find_map(|item| match &item.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
    }

    fn click(ctx: &egui::Context, app: &mut App, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    fn click_text(ctx: &egui::Context, app: &mut App, label: &str) {
        frame(ctx, app, vec![]);
        let output = frame(ctx, app, vec![]);
        click(
            ctx,
            app,
            text_position(&output, label).unwrap_or_else(|| panic!("表示なし: {label}")),
        );
    }

    fn register(ctx: &egui::Context, app: &mut App, label: &str, input: &SrcJointHoops) {
        click_text(ctx, app, label);
        frame(ctx, app, vec![]);
        for (id, value) in [
            ("src_joint_hoop_legs", input.effective_legs.to_string()),
            ("src_joint_hoop_spacing", input.spacing_mm.to_string()),
            ("src_joint_hoop_width", input.width_mm.to_string()),
        ] {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(id)));
            frame(ctx, app, vec![egui::Event::Text(value)]);
        }
        click_text(ctx, app, "呼び径を選択");
        click_text(ctx, app, &input.diameter.to_string());
        click_text(ctx, app, "材種を選択");
        click_text(ctx, app, &input.grade);
        assert_eq!(
            app.ui.scoped.src_joint_hoop_draft.as_ref().unwrap().input(),
            Ok(input.clone())
        );
        let revision = app.core.scoped.undo.revision();
        click_text(ctx, app, "帯筋を登録");
        assert_eq!(
            app.core.scoped.undo.revision(),
            revision + 1,
            "登録されていません: {:?}",
            app.core.scoped.last_error
        );
    }

    #[test]
    fn src_joint_hoops_gui_edits_both_planes_and_keeps_their_identity_after_save_and_renumber() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        assert!(app.load_model(fixture::frame()));
        click_text(&ctx, &mut app, "SRC接合部帯筋");
        let calculation_key =
            |app: &App| rmp_serde::to_vec_named(&app.core.model.calculation_model()).unwrap();
        let weight_key = |app: &App| {
            sepika_job::weight_preparation::weight_input_key(
                &app.core.model,
                app.core.model.mass_method,
            )
        };
        let calculation_before = calculation_key(&app);
        let weight_before = weight_key(&app);
        app.core.scoped.staleness.results_stale = false;
        app.core.scoped.staleness.design_stale = false;
        app.core.scoped.staleness.preparation_stale = false;
        let x_hoops = fixture::example_hoops();
        register(&ctx, &mut app, "帯筋を編集（梁 1, 3）", &x_hoops);
        assert_eq!(app.core.model.src_joint_hoops[0].hoops, x_hoops);
        let y_hoops = SrcJointHoops {
            effective_legs: 4,
            diameter: StandardRebar::D10,
            spacing_mm: 250.0,
            width_mm: 650.0,
            grade: "SD345".into(),
        };
        register(&ctx, &mut app, "帯筋を編集（梁 2）", &y_hoops);
        assert_eq!(app.core.model.src_joint_hoops[1].hoops, y_hoops);
        assert_eq!(calculation_key(&app), calculation_before);
        assert_eq!(weight_key(&app), weight_before);
        assert!(!app.core.scoped.staleness.results_stale);
        assert!(!app.core.scoped.staleness.design_stale);
        assert!(!app.core.scoped.staleness.preparation_stale);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(text_position(
            &output,
            "帯筋比 7.45294118e-4 / 短期許容引張応力度 295 N/mm²"
        )
        .is_some());
        assert!(text_position(
            &output,
            "帯筋比 1.75581538e-3 / 短期許容引張応力度 345 N/mm²"
        )
        .is_some());

        assert!(app.apply_model_edit(Box::new(sepika_edit::DeleteNode { id: NodeId(0) })));
        let path = std::env::temp_dir().join(format!(
            "sepika-src-joint-hoops-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        click_text(&ctx, &mut loaded, "SRC接合部帯筋");
        let rows = src_joint_hoop_input_statuses(&loaded.core.model);
        assert!(rows.iter().all(|row| row.node == NodeId(0)));
        assert_eq!(rows[0].assignments[0].hoops, x_hoops);
        assert_eq!(rows[1].assignments[0].hoops, y_hoops);
        assert!((rows[0].prepared.unwrap().ratio - 0.0007452941176470588).abs() < 1e-15);

        assert!(loaded.apply_model_edit(Box::new(sepika_edit::DeleteMember { id: ElemId(1) })));
        let rows = src_joint_hoop_input_statuses(&loaded.core.model);
        assert!(rows[0].reason.as_ref().unwrap().contains("再登録"));
        assert_eq!(rows[1].prepared.unwrap().allowable_tension_n_per_mm2, 345.0);
        loaded.undo_action();
        assert!(src_joint_hoop_input_statuses(&loaded.core.model)
            .iter()
            .all(|row| row.prepared.is_some()));
        loaded.redo_action();
        let rows = src_joint_hoop_input_statuses(&loaded.core.model);
        assert!(rows[0].prepared.is_none());
        assert_eq!(rows[1].beam_ids, vec![ElemId(1)]);
    }

    #[test]
    fn src_joint_hoops_require_reconfirmation_after_column_replacement() {
        let mut app = App::default();
        assert!(app.load_model(fixture::frame()));
        for axis in [fixture::X, fixture::Y] {
            assert!(app.apply_model_edit(Box::new(SetSrcJointHoops {
                node: fixture::NODE,
                axis,
                hoops: Some(fixture::example_hoops()),
            })));
        }
        let prepared_planes = |app: &App| {
            src_joint_hoop_input_statuses(&app.core.model)
                .iter()
                .filter(|row| row.prepared.is_some())
                .count()
        };
        assert_eq!(prepared_planes(&app), 2);

        assert!(app.apply_model_edit(Box::new(sepika_edit::SetNodeCoord {
            node: NodeId(0),
            coord: [0.0, 0.0, 7_000.0],
        })));
        let mut upper_column = app.core.model.elements[0].clone();
        upper_column.id = ElemId(4);
        upper_column.nodes = [fixture::NODE, NodeId(0)].into_iter().collect();
        assert!(app.apply_model_edit(Box::new(sepika_edit::AddMember { elem: upper_column })));
        let rows = src_joint_hoop_input_statuses(&app.core.model);
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row.column_ids, vec![ElemId(0), ElemId(4)]);
            assert!(row.prepared.is_none());
            assert!(row
                .reason
                .as_ref()
                .unwrap()
                .contains("接続柱が登録時と異なります"));
        }
        app.undo_action();
        assert_eq!(prepared_planes(&app), 2);
        app.redo_action();
        assert_eq!(prepared_planes(&app), 0);
        app.undo_action();
        assert_eq!(prepared_planes(&app), 2);

        let mut replacement = app.core.model.elements[0].clone();
        assert!(app.apply_model_edit(Box::new(sepika_edit::DeleteMember { id: ElemId(0) })));
        assert_eq!(prepared_planes(&app), 0);
        app.undo_action();
        assert_eq!(prepared_planes(&app), 2);
        app.redo_action();
        assert_eq!(prepared_planes(&app), 0);

        replacement.id = ElemId(3);
        assert!(app.apply_model_edit(Box::new(sepika_edit::AddMember { elem: replacement })));
        let rows = src_joint_hoop_input_statuses(&app.core.model);
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row.node, fixture::NODE);
            assert!(!row.beam_ids.is_empty());
            assert_eq!(row.column_ids, vec![ElemId(3)]);
            assert_eq!(row.assignments.len(), 1);
            assert!(row.prepared.is_none());
            assert!(row.reason.as_ref().unwrap().contains("柱・梁との対応"));
        }

        assert!(app.apply_model_edit(Box::new(SetSrcJointHoops {
            node: fixture::NODE,
            axis: fixture::X,
            hoops: Some(fixture::example_hoops()),
        })));
        assert_eq!(prepared_planes(&app), 1);
    }

    #[test]
    fn src_joint_hoops_project_read_retains_invalid_and_duplicate_saved_assignments() {
        use sepika_core::model::SrcJointHoopAssignment;

        let mut model = fixture::frame();
        let initial = fixture::example_hoops();
        model.src_joint_hoops = vec![
            SrcJointHoopAssignment {
                node: fixture::NODE,
                axis: fixture::X,
                columns: vec![ElemId(0)],
                beams: vec![ElemId(1), ElemId(3)],
                hoops: SrcJointHoops {
                    spacing_mm: 0.0,
                    ..initial.clone()
                },
            },
            SrcJointHoopAssignment {
                node: fixture::NODE,
                axis: fixture::Y,
                columns: vec![ElemId(0)],
                beams: vec![ElemId(2)],
                hoops: initial,
            },
        ];
        model.src_joint_hoops.push(model.src_joint_hoops[1].clone());
        let path = std::env::temp_dir().join(format!(
            "sepika-src-joint-raw-{}.ovika",
            uuid::Uuid::new_v4()
        ));
        sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
        let mut opened = App::default();
        opened.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(
            opened.core.scoped.last_error.is_none(),
            "{:?}",
            opened.core.scoped.last_error
        );
        let statuses = src_joint_hoop_input_statuses(&opened.core.model);
        assert!(statuses.iter().all(|row| row.prepared.is_none()));
        assert!(statuses[0].reason.as_ref().unwrap().contains("間隔"));
        assert!(statuses[1].reason.as_ref().unwrap().contains("重複"));
        assert_eq!(statuses[1].assignments.len(), 2);
    }
}
