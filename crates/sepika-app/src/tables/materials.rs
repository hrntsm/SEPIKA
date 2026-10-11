use super::finite_input::{parse_finite, EmptyInput};
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

    if app.ui.scoped.reset_material_inputs {
        clear_material_inputs(ui);
        app.ui.scoped.reset_material_inputs = false;
    }
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
            let resp = ui.add(egui::TextEdit::singleline(&mut draft[k]).desired_width(60.0));
            if let Err(reason) = parse_finite(
                &draft[k],
                if k <= 3 {
                    EmptyInput::Required
                } else {
                    EmptyInput::Optional
                },
            ) {
                resp.on_hover_text(format!("材料追加 {label}: {reason}"));
            }
        }
        ui.label("割増");
        ui.add(egui::TextEdit::singleline(&mut draft[6]).desired_width(50.0))
            .on_hover_text(
                "保有水平耐力計算（増分解析）の材料強度割増係数。\
                 空欄=自動（鋼材1.1、590N級1.05、RC主筋1.1）",
            );
        if let Err(reason) = parse_material_draft(&draft) {
            ui.colored_label(crate::theme::ERROR_RED, reason);
        }
        let ok = parse_material_draft(&draft).is_ok();
        if ui
            .add_enabled(ok, egui::Button::new("+ 追加"))
            .on_hover_text("E・ν・ρ は必須。Fc・Fy・割増は空欄可")
            .clicked()
        {
            do_add_custom = true;
        }
    });
    if do_add_custom {
        match parse_material_draft(&draft) {
            Ok(values) => {
                app.apply_model_edit(Box::new(AddMaterial {
                    name: draft[0].clone(),
                    category: custom_category,
                    young: values[0].unwrap(),
                    poisson: values[1].unwrap(),
                    density: values[2].unwrap(),
                    fc: values[3],
                    fy: values[4],
                    strength_factor: values[5],
                    concrete_class: if custom_category == MaterialCategory::Concrete {
                        ConcreteClass::UserDefined
                    } else {
                        Default::default()
                    },
                }));
            }
            Err(reason) => app.report_error(reason),
        }
    }
    ui.data_mut(|d| d.insert_temp(id_cat, custom_category));
    ui.data_mut(|d| d.insert_temp(id_draft, draft));
    ui.separator();

    let active_id = egui::Id::new("material_active_field");
    let material_id_revision = app.ui.scoped.material_id_revision;
    if let Some((id, field, revision)) =
        ui.data(|d| d.get_temp::<(sepika_core::ids::MaterialId, MaterialField, u64)>(active_id))
    {
        if revision != material_id_revision
            || app
                .core
                .model
                .materials
                .get(id.index())
                .is_none_or(|m| m.id != id)
        {
            let cell_id = material_cell_id(id, field, revision);
            let draft = ui
                .data(|d| d.get_temp::<String>(cell_id))
                .unwrap_or_default();
            app.report_error(format!(
                "材料 {} {:?}: 参照不明（保留入力: {draft}）",
                id.0, field
            ));
            ui.memory_mut(|m| m.surrender_focus(cell_id));
            ui.data_mut(|d| {
                d.remove::<(sepika_core::ids::MaterialId, MaterialField, u64)>(active_id)
            });
        }
    }
    let n = app.core.model.materials.len();
    ui.label(format!("材料一覧（{} 件）", n));
    let mut pending_name: Option<(u32, String)> = None;
    let mut pending_category: Option<(u32, MaterialCategory)> = None;
    let mut pending_field: Option<(sepika_core::ids::MaterialId, MaterialField, String)> = None;
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
                (MaterialField::Young, format!("{}", mat.young), true),
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
                    let cell_id = material_cell_id(mat_id, field, material_id_revision);
                    ui.data_mut(|d| {
                        let cells = d.get_temp_mut_or_default::<Vec<egui::Id>>(egui::Id::new(
                            "material_cell_ids",
                        ));
                        if !cells.contains(&cell_id) {
                            cells.push(cell_id);
                        }
                    });
                    let mut buf = ui
                        .data(|d| d.get_temp::<String>(cell_id))
                        .unwrap_or_else(|| current.clone());
                    let resp = ui
                        .add_enabled_ui(editable, |ui| {
                            let width = ui.available_width();
                            ui.add(
                                egui::TextEdit::singleline(&mut buf)
                                    .id(cell_id)
                                    .desired_width(width),
                            )
                        })
                        .inner;
                    if !editable {
                        resp.clone()
                            .on_disabled_hover_text(format!("材料 {} E: 編集不可", mat_id.0));
                    }
                    let policy = if required {
                        EmptyInput::Required
                    } else {
                        EmptyInput::Optional
                    };
                    if let Err(reason) = parse_finite(&buf, policy) {
                        resp.clone()
                            .on_hover_text(format!("材料 {} {:?}: {reason}", mat_id.0, field));
                        ui.painter().rect_filled(
                            resp.rect,
                            0.0,
                            crate::theme::translucent(crate::theme::ERROR_RED, 60),
                        );
                    }
                    if resp.has_focus() {
                        ui.data_mut(|d| {
                            d.insert_temp(active_id, (mat_id, field, material_id_revision))
                        });
                    }
                    let dirty_id = cell_id.with("changed");
                    if resp.changed() {
                        ui.data_mut(|d| d.insert_temp(dirty_id, true));
                    }
                    if resp.lost_focus()
                        && ui.data(|d| d.get_temp::<bool>(dirty_id).unwrap_or(false))
                    {
                        pending_field = Some((mat_id, field, buf.clone()));
                    }
                    if resp.has_focus() || ui.data(|d| d.get_temp::<String>(cell_id).is_some()) {
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

    if let Some((id, name)) = pending_name {
        app.apply_model_edit(Box::new(SetMaterialName {
            id: sepika_core::ids::MaterialId(id),
            name,
        }));
    }
    if let Some((id, category)) = pending_category {
        app.apply_model_edit(Box::new(SetMaterialCategory {
            id: sepika_core::ids::MaterialId(id),
            category,
        }));
    }
    if let Some((id, field, text)) = pending_field {
        ui.data_mut(|d| d.remove::<(sepika_core::ids::MaterialId, MaterialField, u64)>(active_id));
        if apply_material_text(app, id, field, &text).is_ok() {
            ui.data_mut(|d| {
                let cell_id = material_cell_id(id, field, material_id_revision);
                d.remove::<String>(cell_id);
                d.remove::<bool>(cell_id.with("changed"));
            });
        }
    }
    apply_pending_actions(app, pending_focus, pending_delete);
}

/// load_model が UI scoped を破棄した後、材料表に属する temp だけを破棄する。
fn clear_material_inputs(ui: &mut egui::Ui) {
    let registry = egui::Id::new("material_cell_ids");
    let cells = ui.data(|d| d.get_temp::<Vec<egui::Id>>(registry).unwrap_or_default());
    for cell in &cells {
        ui.memory_mut(|m| m.surrender_focus(*cell));
    }
    ui.data_mut(|d| {
        for cell in cells {
            d.remove::<String>(cell);
            d.remove::<bool>(cell.with("changed"));
        }
        d.remove::<Vec<egui::Id>>(registry);
        d.remove::<(sepika_core::ids::MaterialId, MaterialField, u64)>(egui::Id::new(
            "material_active_field",
        ));
        d.remove::<[String; 7]>(egui::Id::new("material_custom_draft"));
        d.remove::<PresetDraft>(egui::Id::new("material_preset_draft"));
    });
}

pub(super) fn material_cell_id(
    id: sepika_core::ids::MaterialId,
    field: MaterialField,
    revision: u64,
) -> egui::Id {
    egui::Id::new(("mat_cell", revision, id.0, field as u8))
}

fn parse_material_draft(draft: &[String; 7]) -> Result<[Option<f64>; 6], String> {
    let mut values = [None; 6];
    for (k, field) in ["E", "ν", "ρ", "Fc", "Fy", "割増"].iter().enumerate() {
        values[k] = parse_finite(
            &draft[k + 1],
            if k < 3 {
                EmptyInput::Required
            } else {
                EmptyInput::Optional
            },
        )
        .map_err(|reason| format!("材料追加 {field}: {reason}"))?;
    }
    Ok(values)
}

fn apply_material_text(
    app: &mut App,
    id: sepika_core::ids::MaterialId,
    field: MaterialField,
    text: &str,
) -> Result<bool, ()> {
    let policy = match field {
        MaterialField::Young | MaterialField::Poisson | MaterialField::Density => {
            EmptyInput::Required
        }
        _ => EmptyInput::Optional,
    };
    match parse_finite(text, policy) {
        Ok(value) => {
            let changed = app.apply_model_edit(Box::new(SetMaterialField { id, field, value }));
            if !changed
                && (app.core.scoped.undo.last_error().is_some()
                    || app.core.scoped.pending_plate_loss_edit.is_some())
            {
                Err(())
            } else {
                Ok(changed)
            }
        }
        Err(reason) => {
            app.report_error(format!("材料 {} {:?}: {reason}", id.0, field));
            Err(())
        }
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

    #[test]
    fn issue546_commit_rechecks_target_and_standard_e_without_consuming_redo() {
        use sepika_core::ids::MaterialId;
        let mut app = App::default();
        assert!(app.apply_model_edit(Box::new(AddMaterial {
            name: "direct".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            fc: None,
            fy: None,
            strength_factor: None,
            concrete_class: ConcreteClass::Normal
        })));
        assert!(
            apply_material_text(&mut app, MaterialId(0), MaterialField::Young, "204000").unwrap()
        );
        app.core.scoped.undo.undo(&mut app.core.model);
        let revision = app.core.scoped.undo.revision();
        assert!(
            apply_material_text(&mut app, MaterialId(99), MaterialField::Young, "203000").is_err()
        );
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("参照不明"));
        app.core.model.materials[0].category = MaterialCategory::Concrete;
        let before = rmp_serde::to_vec_named(&app.core.model).unwrap();
        assert!(
            apply_material_text(&mut app, MaterialId(0), MaterialField::Young, "203000").is_err()
        );
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("編集不可"));
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), before);
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert!(app.core.scoped.undo.can_redo());
        app.core.model.materials[0].category = MaterialCategory::Steel;
        app.core.scoped.undo.redo(&mut app.core.model);
        assert_eq!(app.core.model.materials[0].young, 204000.0);
        app.core.model.materials.clear();
        assert!(apply_material_text(&mut app, MaterialId(0), MaterialField::Density, "1").is_err());
    }

    #[test]
    fn issue546_strength_origin_clear_is_semantic_and_undo_restores_omissions() {
        use sepika_core::ids::MaterialId;
        use sepika_core::model::{
            SourceStory, SourceStoryKind, StbCommonStrength, StbSteelStrength,
        };
        let mut app = App::default();
        app.core.model.stb_strengths.common = Some(StbCommonStrength {
            project_name: "test".into(),
            app_name: "STB".into(),
            app_version: "2".into(),
            strength_concrete: Some("Fc24".into()),
            reinforcement: vec![],
        });
        app.core.model.source_stories.push(SourceStory {
            id: 1,
            guid: None,
            name: "1F".into(),
            height: 0.0,
            kind: SourceStoryKind::General,
            kind_from_native: false,
            id_dependence: None,
            strength_concrete: None,
            node_ids: vec![],
        });
        app.core.model.prepare_stb_strength_materials();
        let original = rmp_serde::to_vec_named(&app.core.model).unwrap();
        let revision = app.core.scoped.undo.revision();
        for text in ["NaN", "abc"] {
            assert!(apply_material_text(&mut app, MaterialId(0), MaterialField::Fc, text).is_err());
            assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), original);
        }
        assert!(
            !apply_material_text(&mut app, MaterialId(0), MaterialField::Fc, " 2.4e1 ").unwrap()
        );
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert!(apply_material_text(&mut app, MaterialId(0), MaterialField::Fc, "").unwrap());
        assert_eq!(app.core.model.materials[0].fc, None);
        assert_eq!(app.core.model.stb_strengths.materials[0].grade, "未指定Fc");
        assert_eq!(
            app.core
                .model
                .stb_strengths
                .common
                .as_ref()
                .unwrap()
                .strength_concrete
                .as_deref(),
            Some("未指定Fc")
        );
        assert_eq!(app.core.model.source_stories[0].strength_concrete, None);
        let cleared = rmp_serde::to_vec_named(&app.core.model).unwrap();
        app.core.scoped.undo.undo(&mut app.core.model);
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), original);
        app.core.scoped.undo.redo(&mut app.core.model);
        assert_eq!(rmp_serde::to_vec_named(&app.core.model).unwrap(), cleared);
        app.core.model.stb_strengths.materials[0].grade = "Fc24".into();
        app.core
            .model
            .stb_strengths
            .common
            .as_mut()
            .unwrap()
            .strength_concrete = Some("Fc24".into());
        assert!(apply_material_text(&mut app, MaterialId(0), MaterialField::Fc, "").unwrap());
        assert_eq!(app.core.model.stb_strengths.materials[0].grade, "未指定Fc");
        assert!(apply_material_text(&mut app, MaterialId(0), MaterialField::Fy, " 235 ").unwrap());
        let resolved = app
            .core
            .model
            .resolve_stb_steel(&StbSteelStrength {
                native_material: Some(MaterialId(0)),
                element: "beam".into(),
                part: "main".into(),
                position: None,
                strength: "SN400".into(),
            })
            .unwrap();
        assert!(resolved.native_override);
        assert_eq!(resolved.value, 235.0);
        app.core.model.materials[0].concrete_class = ConcreteClass::UserDefined;
        assert!(
            apply_material_text(&mut app, MaterialId(0), MaterialField::Young, "20000").unwrap()
        );
    }

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
}
