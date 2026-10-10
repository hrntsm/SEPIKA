use crate::app::App;
use sepika_core::material_grade::{material_presets, MaterialPreset};
use sepika_core::model::MaterialCategory;
use sepika_core::units::{
    concrete_unit_weight_kn_m3, to_internal::mass_density_from_unit_weight_kn_m3, ConcreteClass,
    ConcreteComposition,
};
use sepika_edit::{
    AddMaterial, DeleteMaterial, MaterialField, SetMaterialCategory, SetMaterialField,
    SetMaterialName,
};

/// プリセット追加 UI の選択状態（区分・グレード名・SRC造トグル）。
/// `ui.data`/`data_mut` の temp storage に保持する。
#[derive(Clone, Debug)]
struct PresetDraft {
    category: MaterialCategory,
    name: String,
    /// コンクリート区分のみ有効。ON のとき密度を γSRC 由来に差し替える。
    src: bool,
}

impl PresetDraft {
    fn new(presets: &[MaterialPreset], category: MaterialCategory) -> Self {
        Self {
            category,
            name: first_name_in(presets, category),
            src: false,
        }
    }
}

fn first_name_in(presets: &[MaterialPreset], category: MaterialCategory) -> String {
    presets
        .iter()
        .find(|p| p.category == category)
        .map(|p| p.name.to_string())
        .unwrap_or_default()
}

/// プリセットのグレード選択に添えるホバーテキスト（主要値の要約）。
fn preset_hover_text(p: &MaterialPreset) -> String {
    match p.category {
        MaterialCategory::Steel => format!("F={} (t≤40)", p.fy.unwrap_or_default()),
        MaterialCategory::Rebar => format!("降伏点 {}", p.fy.unwrap_or_default()),
        MaterialCategory::Concrete => format!("Fc={}, Ec={:.0}", p.fc.unwrap_or_default(), p.young),
    }
}

/// SRC造（鉄骨鉄筋コンクリート）トグル適用時の材料名・密度を計算する。
///
/// `fc` は元プリセットのコンクリート設計基準強度。密度は単位体積重量表の
/// γSRC（鉄骨鉄筋込み。普通コンクリート・Fc≤36 帯で 25.0 kN/m³）から導出する。
fn apply_src_toggle(name: &str, fc: f64) -> (String, f64) {
    let gamma = concrete_unit_weight_kn_m3(fc, ConcreteClass::Normal, ConcreteComposition::Src);
    let rho = mass_density_from_unit_weight_kn_m3(gamma);
    (format!("{name}(SRC)"), rho)
}

fn young_editable(category: MaterialCategory, concrete_class: ConcreteClass) -> bool {
    !(category == MaterialCategory::Concrete && concrete_class == ConcreteClass::Normal)
}

/// 材料タブ：プリセット追加・カスタム追加・一覧編集・削除。
pub fn materials_table(ui: &mut egui::Ui, app: &mut App) {
    use crate::table_util::{self, Col};

    let presets = material_presets();
    let id_preset_draft = egui::Id::new("material_preset_draft");
    let mut draft = ui
        .data(|d| d.get_temp::<PresetDraft>(id_preset_draft))
        .unwrap_or_else(|| PresetDraft::new(&presets, MaterialCategory::Steel));

    ui.horizontal(|ui| {
        ui.label("プリセット追加:");
        for cat in [
            MaterialCategory::Steel,
            MaterialCategory::Rebar,
            MaterialCategory::Concrete,
        ] {
            if ui
                .selectable_label(draft.category == cat, cat.label())
                .clicked()
                && draft.category != cat
            {
                draft.category = cat;
                draft.name = first_name_in(&presets, cat);
                draft.src = false;
            }
        }
    });

    let grades: Vec<&MaterialPreset> = presets
        .iter()
        .filter(|p| p.category == draft.category)
        .collect();
    if !grades.iter().any(|p| p.name == draft.name) {
        draft.name = grades
            .first()
            .map(|p| p.name.to_string())
            .unwrap_or_default();
    }

    ui.horizontal(|ui| {
        ui.label("グレード:");
        egui::ComboBox::from_id_salt("material_preset_select")
            .selected_text(&draft.name)
            .show_ui(ui, |ui| {
                for p in &grades {
                    let hover = preset_hover_text(p);
                    if ui
                        .selectable_label(draft.name == p.name, p.name)
                        .on_hover_text(hover)
                        .clicked()
                    {
                        draft.name = p.name.to_string();
                    }
                }
            });
        if draft.category == MaterialCategory::Concrete {
            ui.checkbox(&mut draft.src, "SRC造(γSRC)");
        }

        let selected = grades.iter().find(|p| p.name == draft.name).copied();
        if let Some(preset) = selected {
            let (name, density) = if draft.category == MaterialCategory::Concrete && draft.src {
                apply_src_toggle(preset.name, preset.fc.unwrap_or_default())
            } else {
                (preset.name.to_string(), preset.density)
            };
            if ui.button("+ 追加").clicked() {
                app.core.scoped.undo.run(
                    &mut app.core.model,
                    Box::new(AddMaterial {
                        name,
                        category: draft.category,
                        young: preset.young,
                        poisson: preset.poisson,
                        density,
                        fc: preset.fc,
                        fy: preset.fy,
                        strength_factor: None,
                        concrete_class: Default::default(),
                    }),
                );
                app.core.scoped.staleness.mark_edited();
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id_preset_draft, draft));

    let id_draft = egui::Id::new("material_custom_draft");
    let mut draft: [String; 7] = ui
        .data(|d| d.get_temp::<[String; 7]>(id_draft))
        .unwrap_or_else(|| {
            [
                "新規材料".into(),
                "205000".into(),
                "0.3".into(),
                format!("{:.4e}", sepika_core::units::STEEL_MASS_DENSITY_TON_MM3),
                String::new(),
                String::new(),
                String::new(),
            ]
        });
    let mut do_add_custom = false;
    let id_cat = ui.id().with("custom_material_category");
    let mut custom_category: MaterialCategory = ui
        .data_mut(|d| d.get_temp(id_cat))
        .unwrap_or(MaterialCategory::Steel);
    ui.horizontal(|ui| {
        ui.label("直接入力:");
        ui.add(egui::TextEdit::singleline(&mut draft[0]).desired_width(80.0))
            .on_hover_text("名称");
        egui::ComboBox::from_id_salt(id_cat)
            .selected_text(custom_category.label())
            .width(100.0)
            .show_ui(ui, |ui| {
                for cat in [
                    MaterialCategory::Steel,
                    MaterialCategory::Rebar,
                    MaterialCategory::Concrete,
                ] {
                    ui.selectable_value(&mut custom_category, cat, cat.label());
                }
            })
            .response
            .on_hover_text("区分。S 造 / RC 造の判定と検定式の選択に用います");
        for (k, label) in [(1, "E"), (2, "ν"), (3, "ρ"), (4, "Fc"), (5, "Fy")] {
            ui.label(label);
            ui.add(egui::TextEdit::singleline(&mut draft[k]).desired_width(60.0));
        }
        ui.label("割増");
        ui.add(egui::TextEdit::singleline(&mut draft[6]).desired_width(50.0))
            .on_hover_text(
                "保有水平耐力計算（増分解析）の材料強度割増係数。\
                 空欄=自動（鋼材1.1、590N級1.05、RC主筋1.1）",
            );
        let parsed_e = draft[1].parse::<f64>();
        let parsed_nu = draft[2].parse::<f64>();
        let parsed_rho = draft[3].parse::<f64>();
        let ok = parsed_e.is_ok() && parsed_nu.is_ok() && parsed_rho.is_ok();
        if ui
            .add_enabled(ok, egui::Button::new("+ 追加"))
            .on_hover_text("E・ν・ρ は必須。Fc・Fy・割増は空欄可")
            .clicked()
        {
            do_add_custom = true;
        }
    });
    if do_add_custom {
        let fc = draft[4].parse::<f64>().ok();
        let fy = draft[5].parse::<f64>().ok();
        let strength_factor = draft[6].parse::<f64>().ok();
        if let (Ok(e), Ok(nu), Ok(rho)) = (
            draft[1].parse::<f64>(),
            draft[2].parse::<f64>(),
            draft[3].parse::<f64>(),
        ) {
            app.core.scoped.undo.run(
                &mut app.core.model,
                Box::new(AddMaterial {
                    name: draft[0].clone(),
                    category: custom_category,
                    young: e,
                    poisson: nu,
                    density: rho,
                    fc,
                    fy,
                    strength_factor,
                    concrete_class: if custom_category == MaterialCategory::Concrete {
                        ConcreteClass::UserDefined
                    } else {
                        Default::default()
                    },
                }),
            );
            app.core.scoped.staleness.mark_edited();
        }
    }
    ui.data_mut(|d| d.insert_temp(id_cat, custom_category));
    ui.data_mut(|d| d.insert_temp(id_draft, draft));
    ui.separator();

    let n = app.core.model.materials.len();
    ui.label(format!("材料一覧（{} 件）", n));
    let mut pending_name: Option<(u32, String)> = None;
    let mut pending_category: Option<(u32, MaterialCategory)> = None;
    let mut pending_field: Option<(u32, MaterialField, Option<f64>)> = None;
    let mut pending_delete: Option<u32> = None;
    let mut pending_focus: Option<sepika_core::ids::MaterialId> = None;

    table_util::standard_table(
        ui,
        "materials_tbl",
        &[
            Col::id(),
            Col::name("名称"),
            Col::name("区分").hover(
                "S 造 / RC 造の判定に用います。剛域長・仕口パネルの対象・\
                 断面検定の式・数量集計がこの値で変わります",
            ),
            Col::num("E [N/mm²]"),
            Col::num("ν"),
            Col::num("ρ [t/mm³]"),
            Col::num("Fc"),
            Col::num("Fy"),
            Col::num("割増").hover(
                "保有水平耐力計算（増分解析）の材料強度割増係数。\
                 空欄=自動（鋼材1.1、590N級1.05、RC主筋1.1）",
            ),
            Col::actions(),
        ],
        n,
        |row| {
            let idx = row.index();
            let mat = &app.core.model.materials[idx];
            let mat_id = mat.id;
            row.col(|ui| {
                let is_sel = app.ui.scoped.nav.focus_material == Some(mat_id);
                if table_util::id_cell(ui, is_sel, mat_id.0, "クリックで選択") {
                    pending_focus = Some(mat_id);
                }
            });
            row.col(|ui| {
                let mut name = mat.name.clone();
                if table_util::cell_text_edit(ui, &mut name).lost_focus() && name != mat.name {
                    pending_name = Some((mat_id.0, name));
                }
            });
            row.col(|ui| {
                let mut category = mat.category;
                table_util::cell_combo(ui, ("mat_category", mat_id.0), category.label(), |ui| {
                    for cat in [
                        MaterialCategory::Steel,
                        MaterialCategory::Rebar,
                        MaterialCategory::Concrete,
                    ] {
                        ui.selectable_value(&mut category, cat, cat.label());
                    }
                });
                if category != mat.category {
                    pending_category = Some((mat_id.0, category));
                }
            });
            let cells: [(MaterialField, String, bool); 6] = [
                (
                    MaterialField::Young,
                    format!("{}", mat.young),
                    young_editable(mat.category, mat.concrete_class),
                ),
                (MaterialField::Poisson, format!("{}", mat.poisson), true),
                (MaterialField::Density, format!("{:.3e}", mat.density), true),
                (
                    MaterialField::Fc,
                    mat.fc.map(|v| format!("{}", v)).unwrap_or_default(),
                    false,
                ),
                (
                    MaterialField::Fy,
                    mat.fy.map(|v| format!("{}", v)).unwrap_or_default(),
                    false,
                ),
                (
                    MaterialField::StrengthFactor,
                    mat.strength_factor
                        .map(|v| format!("{}", v))
                        .unwrap_or_default(),
                    false,
                ),
            ];
            for (field, current, required) in cells {
                row.col(|ui| {
                    let editable = field != MaterialField::Young
                        || young_editable(mat.category, mat.concrete_class);
                    let cell_id = egui::Id::new(("mat_cell", mat_id.0, field as u8));
                    let mut buf = ui
                        .data(|d| d.get_temp::<String>(cell_id))
                        .unwrap_or_else(|| current.clone());
                    let resp = ui
                        .add_enabled_ui(editable, |ui| table_util::cell_text_edit(ui, &mut buf))
                        .inner;
                    if resp.lost_focus() {
                        let parsed = buf.trim().parse::<f64>().ok();
                        let changed = buf.trim() != current.trim();
                        if changed && (parsed.is_some() || !required) {
                            pending_field = Some((mat_id.0, field, parsed));
                        }
                        ui.data_mut(|d| d.remove::<String>(cell_id));
                    } else if resp.has_focus() {
                        ui.data_mut(|d| d.insert_temp(cell_id, buf));
                    }
                });
            }
            row.col(|ui| {
                let in_use = app.core.model.sections.iter().any(|s| {
                    [
                        s.material,
                        s.rebar_material,
                        s.shear_rebar_material,
                        s.steel_material,
                    ]
                    .contains(&Some(mat_id))
                });
                let blocked = in_use.then_some("断面から参照中のため削除できません");
                if table_util::delete_cell(ui, "この材料を削除", blocked) {
                    pending_delete = Some(mat_id.0);
                }
            });
        },
    );

    let mut edited = false;
    if let Some((id, name)) = pending_name {
        edited |= app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(SetMaterialName {
                id: sepika_core::ids::MaterialId(id),
                name,
            }),
        );
    }
    if let Some((id, category)) = pending_category {
        edited |= app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(SetMaterialCategory {
                id: sepika_core::ids::MaterialId(id),
                category,
            }),
        );
    }
    if let Some((id, field, value)) = pending_field {
        edited |= app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(SetMaterialField {
                id: sepika_core::ids::MaterialId(id),
                field,
                value,
            }),
        );
    }
    apply_pending_actions(app, pending_focus, pending_delete);
    if edited {
        app.core.scoped.staleness.mark_edited();
    }
    ui.separator();
    ui.label("材料定数のグリッド編集（E・ν・ρ・割増、TSVコピー／貼り付け）");
    material_constants_grid(ui, app);
}

fn material_constants_grid(ui: &mut egui::Ui, app: &mut App) {
    let edited = {
        let mut adapter = crate::app::material_grid::MaterialGridAdapter {
            model: &mut app.core.model,
            undo: &mut app.core.scoped.undo,
            edited: false,
        };
        ui.push_id("material_constants_grid", |ui| {
            app.ui.scoped.material_grid.show(
                ui,
                &mut adapter,
                &["E [N/mm²]", "ν [-]", "ρ [t/mm³]", "割増 [-]"],
            );
        });
        adapter.edited
    };
    for (message, is_error) in app.ui.scoped.material_grid.take_log() {
        app.core.log.push(
            if is_error {
                crate::app::LogLevel::Error
            } else {
                crate::app::LogLevel::Info
            },
            message,
        );
    }
    let _ = app.ui.scoped.material_grid.take_row_selection();
    if edited {
        app.core.scoped.staleness.mark_edited();
    }
}
pub(crate) fn apply_pending_actions(
    app: &mut App,
    pending_focus: Option<sepika_core::ids::MaterialId>,
    pending_delete: Option<u32>,
) {
    if let Some(mid) = pending_focus {
        app.ui.scoped.nav.focus_material = Some(mid);
    }
    if let Some(id) = pending_delete {
        app.apply_model_edit(Box::new(DeleteMaterial {
            id: sepika_core::ids::MaterialId(id),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SRC造トグル適用時の密度が γSRC=25.0 kN/m³ 由来であることを確認する
    /// （Fc≤36 帯。`apply_src_toggle` に切り出したロジックを直接検証する）。
    #[test]
    fn test_apply_src_toggle_uses_gamma_src() {
        let src_density = mass_density_from_unit_weight_kn_m3(25.0);
        let (name, density) = apply_src_toggle("Fc24", 24.0);
        assert_eq!(name, "Fc24(SRC)");
        assert!(
            (density - src_density).abs() < 1e-18,
            "density={density} expected={src_density}"
        );
    }

    #[test]
    fn standard_rc_and_src_have_same_concrete_young_modulus() {
        let preset = material_presets()
            .into_iter()
            .find(|preset| preset.name == "Fc36")
            .unwrap();
        let fc = preset.fc.unwrap();
        let rc_gamma =
            concrete_unit_weight_kn_m3(fc, ConcreteClass::Normal, ConcreteComposition::Rc);
        let src_gamma =
            concrete_unit_weight_kn_m3(fc, ConcreteClass::Normal, ConcreteComposition::Src);
        let rc = sepika_core::section_shape::concrete_young_modulus_gamma(fc, rc_gamma - 1.0);
        let src = sepika_core::section_shape::concrete_young_modulus_gamma(fc, src_gamma - 2.0);
        assert_eq!(rc, src);
        assert_eq!(preset.young, rc);
    }

    #[test]
    fn standard_concrete_young_is_not_editable_but_direct_input_is() {
        assert!(!young_editable(
            MaterialCategory::Concrete,
            ConcreteClass::Normal
        ));
        assert!(young_editable(
            MaterialCategory::Concrete,
            ConcreteClass::UserDefined
        ));
        assert!(young_editable(
            MaterialCategory::Steel,
            ConcreteClass::Normal
        ));
    }

    #[test]
    fn imported_standard_concrete_young_is_not_editable() {
        let preset = material_presets()
            .into_iter()
            .find(|p| p.name == "Fc24")
            .unwrap();
        assert!(!young_editable(preset.category, ConcreteClass::Normal));
        assert!(young_editable(
            MaterialCategory::Concrete,
            ConcreteClass::UserDefined,
        ));
    }
    fn paste_frame(app: &mut App, text: &str) {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 1200.0),
                )),
                events: vec![egui::Event::Paste(text.into())],
                ..Default::default()
            },
            |ui| materials_table(ui, app),
        );
    }

    fn grid_app() -> App {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.select_node(sepika_core::ids::NodeId(0));
        app.ui.scoped.material_grid.grid =
            crate::grid::GridState::new(app.core.model.materials.len(), 4);
        app.ui
            .scoped
            .material_grid
            .grid
            .click(crate::grid::CellRef { row: 0, col: 0 }, false);
        app
    }

    #[test]
    fn material_table_paste_entry_updates_stale_atomically_and_keeps_geometry() {
        let mut app = grid_app();
        let geometry = format!("{:?}", app.ui.scoped.selection);
        let original = rmp_serde::to_vec_named(&app.core.model).unwrap();
        paste_frame(&mut app, "210000\t0.28\t7.85e-9\t1.05\r\n");
        assert_eq!(app.core.model.materials[0].young, 210000.0);
        assert_eq!(app.core.model.materials[0].density, 7.85e-9);
        assert_eq!(app.core.model.materials[0].strength_factor, Some(1.05));
        assert!(app.core.scoped.staleness.results_stale);
        assert!(app.core.scoped.staleness.design_stale);
        assert!(app.core.scoped.staleness.unsaved_changes);
        assert_eq!(format!("{:?}", app.ui.scoped.selection), geometry);
        assert_eq!(app.core.scoped.undo.revision(), 1);
        let changed = rmp_serde::to_vec_named(&app.core.model).unwrap();
        app.core.scoped.undo.undo(&mut app.core.model);
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), original);
        assert!(!app.core.scoped.undo.can_undo());
        app.core.scoped.undo.redo(&mut app.core.model);
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), changed);
    }

    #[test]
    fn material_table_rejects_invalid_dimensions_and_values_without_stale_or_history() {
        let invalid_blocks = [
            "1\t2\t3\t4\t",
            "1\n\t",
            "1\tNaN",
            "1\tinf",
            "1\t1e309",
            "1\tabc",
            "1\t=1+2",
            "1\t1,000",
            "1\t10mm",
        ];
        for text in invalid_blocks {
            let mut app = grid_app();
            let geometry = format!("{:?}", app.ui.scoped.selection);
            let original = rmp_serde::to_vec_named(&app.core.model).unwrap();
            paste_frame(&mut app, text);
            assert_eq!(
                rmp_serde::to_vec_named(&app.core.model).unwrap(),
                original,
                "{text:?}"
            );
            assert!(!app.core.scoped.staleness.results_stale, "{text:?}");
            assert!(!app.core.scoped.staleness.unsaved_changes, "{text:?}");
            assert!(!app.core.scoped.undo.can_undo(), "{text:?}");
            assert_eq!(format!("{:?}", app.ui.scoped.selection), geometry);
            assert!(
                app.core
                    .log
                    .entries
                    .iter()
                    .any(|e| e.level == crate::app::LogLevel::Error),
                "{text:?}"
            );
        }
    }

    #[test]
    fn material_table_blank_paste_keeps_redo_and_geometry() {
        let mut app = grid_app();
        paste_frame(&mut app, "210000");
        app.core.scoped.undo.undo(&mut app.core.model);
        app.core.scoped.staleness = Default::default();
        let revision = app.core.scoped.undo.revision();
        let geometry = format!("{:?}", app.ui.scoped.selection);
        for text in ["\t\t\t", "NaN"] {
            paste_frame(&mut app, text);
            assert_eq!(app.core.scoped.undo.revision(), revision);
            assert!(app.core.scoped.undo.can_redo());
            assert!(!app.core.scoped.staleness.results_stale);
            assert_eq!(format!("{:?}", app.ui.scoped.selection), geometry);
        }
    }
    #[test]
    fn focused_external_text_edit_does_not_paste_into_active_material_grid() {
        let mut app = grid_app();
        let original = rmp_serde::to_vec_named(&app.core.model).unwrap();
        let ctx = egui::Context::default();
        let mut text = String::new();
        let id = egui::Id::new("external_material_text");
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(id))
                .request_focus();
            materials_table(ui, &mut app);
        });
        assert!(ctx.text_edit_focused());
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Paste("210000".into())],
                ..Default::default()
            },
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).id(id));
                materials_table(ui, &mut app);
            },
        );
        assert_eq!(text, "210000");
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), original);
        assert!(!app.core.scoped.undo.can_undo());
    }
    #[test]
    fn material_table_copy_emits_canonical_density_before_display_rounding() {
        let mut app = grid_app();
        app.core.model.materials[0].density = 7.851234567890123e-9;
        app.ui
            .scoped
            .material_grid
            .grid
            .click(crate::grid::CellRef { row: 0, col: 2 }, false);
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Copy],
                ..Default::default()
            },
            |ui| materials_table(ui, &mut app),
        );
        let text = output
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            text.parse::<f64>().unwrap().to_bits(),
            app.core.model.materials[0].density.to_bits()
        );
        assert_ne!(
            *text,
            format!("{:.3e}", app.core.model.materials[0].density)
        );
        assert!(!app.core.scoped.undo.can_undo());
        assert!(!app.core.scoped.staleness.results_stale);
    }
}
