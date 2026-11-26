//! S 造部材の断面検定用属性（`Model.steel_design_attrs` = `SteelDesignAttr`:
//! 継手フランジ／ウェブ欠損率・スカラップ欠損率、横座屈長さ lb の直接入力、
//! 等間隔横補剛本数、座屈長さ lk_y/lk_z の直接入力、横座屈修正係数 C の
//! 直接入力）の編集 UI。
//! 対象は `ElementKind::Beam` の部材と、`Fiber` の SRC柱・CFT柱・一般H形鋼柱。
//! 編集は `sepika_edit::{SetSteelDesignAttr, RemoveSteelDesignAttr}` 経由
//! （undo 対応）。空（すべて既定値）を適用した場合は削除として扱う
//! （[`SteelDesignAttr::is_empty`]）。
//!
//! `member_details.rs` のドラフト同期パターン（`elem`/`synced_for`、選択時に
//! model 値でバッファ初期化、「適用」で Set／空なら Remove）を踏襲する。

use crate::app::App;
use sepika_core::ids::ElemId;
use sepika_core::model::{
    ColumnEndSupports, ElementData, ElementKind, FrameSectionUse, Model, SteelDesignAttr,
};
use sepika_edit::{RemoveSteelDesignAttr, SetSteelDesignAttr};

/// S造検定属性フォームのドラフト状態（GUI 専用）。
/// 対象部材を選択すると `synced_for` の部材の現在値でバッファを初期化し、
/// 「適用」で `SetSteelDesignAttr`（空なら `RemoveSteelDesignAttr`）を発行する。
#[derive(Clone, Debug, Default)]
pub struct SteelAttrDraft {
    /// 編集対象の部材要素。
    pub elem: Option<ElemId>,
    /// バッファを初期化した対象（`elem` と異なれば model 値で再同期する）。
    pub synced_for: Option<ElemId>,
    /// 継手フランジ欠損率 βf [%] の入力バッファ（空欄=0）。
    pub joint_flange_loss: String,
    /// 継手ウェブ欠損率 βw [%] の入力バッファ（空欄=0）。
    pub joint_web_loss: String,
    /// スカラップ欠損率 αw [%] の入力バッファ（空欄=0）。
    pub scallop_web_loss: String,
    /// 横座屈長さ lb 直接入力・始端 `[mm]` の入力バッファ（空欄=自動）。
    pub lb_start: String,
    /// 横座屈長さ lb 直接入力・中央 `[mm]` の入力バッファ（空欄=自動）。
    pub lb_mid: String,
    /// 横座屈長さ lb 直接入力・終端 `[mm]` の入力バッファ（空欄=自動）。
    pub lb_end: String,
    /// 等間隔横補剛の本数の入力バッファ（空欄=なし）。
    pub lateral_brace_count: String,
    pub column_lateral_buckling_end_supports: Option<ColumnEndSupports>,
    /// 強軸まわり座屈長さ lk_y の直接入力 `[mm]`（空欄は未指定）。
    pub lk_y_direct: String,
    /// 弱軸まわり座屈長さ lk_z の直接入力 `[mm]`（空欄は未指定）。
    pub lk_z_direct: String,
    /// 横座屈修正係数 C の直接入力（空欄=自動算定）。
    pub c_direct: String,
}

impl SteelAttrDraft {
    /// モデルの鋼材設計属性から入力バッファを埋め、同期済みの対象を `attr.elem` にする。
    ///
    /// 表の「編集」ボタンと、対象部材が切り替わったときの再同期の 2 か所から呼ぶ。
    /// バッファは全項目を上書きする（前の対象の値が残ると、編集していない項目まで
    /// 別部材の値で保存されてしまう）。
    ///
    /// 欠損率と座屈長さ lb/lk_y/lk_z は元の値を復元できる桁を保持する。未設定の長さは空欄にする。
    fn sync_from(&mut self, attr: &SteelDesignAttr) {
        self.elem = Some(attr.elem);
        self.synced_for = Some(attr.elem);
        self.joint_flange_loss = attr.joint_flange_loss.to_string();
        self.joint_web_loss = attr.joint_web_loss.to_string();
        self.scallop_web_loss = attr.scallop_web_loss.to_string();
        let (lb_s, lb_m, lb_e) = attr
            .lb_direct
            .map(|(s, m, e)| (s.to_string(), m.to_string(), e.to_string()))
            .unwrap_or_default();
        self.lb_start = lb_s;
        self.lb_mid = lb_m;
        self.lb_end = lb_e;
        self.lateral_brace_count = attr
            .lateral_brace_count
            .map(|n| n.to_string())
            .unwrap_or_default();
        self.column_lateral_buckling_end_supports =
            attr.column_lateral_buckling_end_supports.clone();
        self.lk_y_direct = attr.lk_y_direct.map(|v| v.to_string()).unwrap_or_default();
        self.lk_z_direct = attr.lk_z_direct.map(|v| v.to_string()).unwrap_or_default();
        self.c_direct = attr.c_direct.map(|v| format!("{v:.2}")).unwrap_or_default();
    }
}

fn is_symmetric_h_column(model: &Model, elem: &ElementData) -> bool {
    let section = model.element_section(elem);
    let material = model.element_material(elem).map(|mat| mat.category);
    elem.kind.is_primary_frame_support()
        && section.is_some_and(|sec| {
            sec.frame_use == Some(FrameSectionUse::Column)
                && sepika_design_jp::steel::is_symmetric_h_section(sec)
        })
        && sepika_core::structure_kind::structure_kind_of(section, material)
            == sepika_core::structure_kind::StructureKind::S
}

/// 欠損率入力（βf/βw/αw共通）をパースする。空欄は 0（欠損なし）とする
/// （egui 非依存の純関数）。
fn parse_loss_percent(s: &str, label: &str) -> Result<f64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(0.0);
    }
    s.parse()
        .map_err(|_| format!("不正な{label}「{s}」: 数値ではありません"))
}

/// 横座屈長さ lb 直接入力（始端/中央/終端）をパースする。3 欄すべて空欄なら
/// `None`（自動）、いずれか 1 つでも入力があれば 3 欄とも数値必須とする
/// （egui 非依存の純関数）。
fn parse_lb_direct(start: &str, mid: &str, end: &str) -> Result<Option<(f64, f64, f64)>, String> {
    let (s, m, e) = (start.trim(), mid.trim(), end.trim());
    if s.is_empty() && m.is_empty() && e.is_empty() {
        return Ok(None);
    }
    if s.is_empty() || m.is_empty() || e.is_empty() {
        return Err(
            "横座屈長さ lb の直接入力は始端/中央/終端をすべて入力してください\
             （すべて空欄なら自動）"
                .to_string(),
        );
    }
    let parse_one = |v: &str, label: &str| -> Result<f64, String> {
        v.parse()
            .map_err(|_| format!("不正な{label}側 lb「{v}」: 数値ではありません"))
    };
    Ok(Some((
        parse_one(s, "始端")?,
        parse_one(m, "中央")?,
        parse_one(e, "終端")?,
    )))
}

/// 等間隔横補剛本数をパースする。空欄は `None`（補剛なし）とする
/// （egui 非依存の純関数）。
fn parse_brace_count(s: &str) -> Result<Option<u32>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let n: u32 = s
        .parse()
        .map_err(|_| format!("不正な横補剛本数「{s}」: 0以上の整数を入力してください"))?;
    Ok(Some(n))
}

/// 座屈長さ lk_y/lk_z の直接入力をパースする。空欄は `None`（未指定）、
/// 入力があれば有限な正値を要求する。
fn parse_lk_direct(s: &str, label: &str) -> Result<Option<f64>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let v: f64 = s
        .parse()
        .map_err(|_| format!("不正な{label}「{s}」: 数値ではありません"))?;
    if !v.is_finite() || v <= 0.0 {
        return Err(format!(
            "{label}は有限な正の値で入力してください（空欄=未指定）"
        ));
    }
    Ok(Some(v))
}

/// 横座屈長さ lb 直接入力の表示文字列。
fn lb_direct_desc(v: Option<(f64, f64, f64)>) -> String {
    match v {
        Some((s, m, e)) => format!("{s}/{m}/{e}"),
        None => "自動".to_string(),
    }
}

/// 座屈長さ lk_y/lk_z 直接入力の表示文字列。
fn opt_len_desc(v: Option<f64>) -> String {
    match v {
        Some(x) => x.to_string(),
        None => "未指定".to_string(),
    }
}

/// 横座屈修正係数 C 直接入力の表示文字列（小数第2位まで）。
fn opt_c_desc(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.2}"),
        None => "自動".to_string(),
    }
}

/// 等間隔横補剛本数の表示文字列。
fn opt_count_desc(v: Option<u32>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "なし".to_string(),
    }
}

const STEEL_BUCKLING_GUIDANCE: &str =
    "通常S部材・SRC柱の座屈長さは軸ごとに直接入力を優先します。空欄は節点間距離の柱1.2倍、梁・ブレース1.0倍です。";

pub fn steel_attrs_table(ui: &mut egui::Ui, app: &mut App) {
    ui.label(
        "S造部材の断面検定用属性（継手・スカラップ欠損率、横座屈長さ lb の直接入力、\
         等間隔横補剛本数、座屈長さ lk_y/lk_z の直接入力、横座屈修正係数 C の直接入力）\
         を設定します。\
         C の直接入力は自動算定（材端モーメント比・上限2.3）より優先されます。",
    );
    ui.label(STEEL_BUCKLING_GUIDANCE);
    ui.separator();

    let model = &app.core.model;
    let target_elems: Vec<ElemId> = model
        .elements
        .iter()
        .filter(|e| match e.kind {
            ElementKind::Beam | ElementKind::Brace { .. } => true,
            ElementKind::Fiber => {
                is_symmetric_h_column(model, e) || model.element_section(e).is_some_and(|sec| {
                    sec.frame_use == Some(FrameSectionUse::Column)
                        && (sec.is_cft()
                            || matches!(
                                sec.shape,
                                Some(
                                    sepika_core::section_shape::SectionShape::SrcColumnRect { .. }
                                )
                            ))
                })
            }
            _ => false,
        })
        .map(|e| e.id)
        .collect();

    if target_elems.is_empty() {
        ui.label("対象となる部材(柱・梁・ブレース)がありません。");
        return;
    }

    let mut pending_remove: Option<ElemId> = None;
    let mut pending_edit: Option<SteelDesignAttr> = None;
    if app.core.model.steel_design_attrs.is_empty() {
        ui.label("設定済みのS造検定属性はありません。");
    } else {
        for attr in &app.core.model.steel_design_attrs {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "部材#{}: βf={}% βw={}% αw={}% / lb={} / 横補剛n={} / \
                     lk_y={} lk_z={} / C={}",
                    attr.elem.0,
                    attr.joint_flange_loss,
                    attr.joint_web_loss,
                    attr.scallop_web_loss,
                    lb_direct_desc(attr.lb_direct),
                    opt_count_desc(attr.lateral_brace_count),
                    opt_len_desc(attr.lk_y_direct),
                    opt_len_desc(attr.lk_z_direct),
                    opt_c_desc(attr.c_direct),
                ));
                if attr.column_lateral_buckling_end_supports.is_some() {
                    ui.label("柱の両実材端支持の確認記録あり");
                }
                if ui
                    .button("✏")
                    .on_hover_text("フォームへ読み込んで編集")
                    .clicked()
                {
                    pending_edit = Some(attr.clone());
                }
                if ui
                    .button("🗑")
                    .on_hover_text("このS造検定属性を削除")
                    .clicked()
                {
                    pending_remove = Some(attr.elem);
                }
            });
        }
    }
    if let Some(attr) = pending_edit {
        app.ui.scoped.steel_attr_draft.sync_from(&attr);
    }
    if let Some(elem) = pending_remove {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(RemoveSteelDesignAttr { elem }),
        );
        app.core.scoped.staleness.mark_edited();
    }

    ui.separator();
    ui.strong("S造検定属性を設定");

    ui.horizontal(|ui| {
        ui.label("対象部材:");
        let text = app
            .ui
            .scoped
            .steel_attr_draft
            .elem
            .map(|e| format!("部材#{}", e.0))
            .unwrap_or_else(|| "―".to_string());
        egui::ComboBox::from_id_salt("steel_attr_elem")
            .selected_text(text)
            .show_ui(ui, |ui| {
                for &eid in &target_elems {
                    if ui
                        .selectable_label(
                            app.ui.scoped.steel_attr_draft.elem == Some(eid),
                            format!("部材#{}", eid.0),
                        )
                        .clicked()
                    {
                        app.ui.scoped.steel_attr_draft.elem = Some(eid);
                    }
                }
            });
    });
    if app.ui.scoped.steel_attr_draft.elem != app.ui.scoped.steel_attr_draft.synced_for {
        if let Some(eid) = app.ui.scoped.steel_attr_draft.elem {
            let existing = app
                .core
                .model
                .steel_design_attrs
                .iter()
                .find(|a| a.elem == eid)
                .cloned();
            let attr = existing.unwrap_or(SteelDesignAttr {
                elem: eid,
                joint_flange_loss: 0.0,
                joint_web_loss: 0.0,
                scallop_web_loss: 0.0,
                lb_direct: None,
                lateral_brace_count: None,
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
            });
            app.ui.scoped.steel_attr_draft.sync_from(&attr);
        }
    }

    ui.horizontal(|ui| {
        ui.label("βf 継手フランジ欠損率[%]:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.joint_flange_loss)
                .desired_width(60.0),
        );
        ui.label("βw 継手ウェブ欠損率[%]:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.joint_web_loss)
                .desired_width(60.0),
        );
        ui.label("αw スカラップ欠損率[%]:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.scallop_web_loss)
                .desired_width(60.0),
        );
    });

    ui.horizontal(|ui| {
        ui.label("横座屈長さ lb 直接入力[mm] 始端:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.lb_start)
                .desired_width(70.0),
        );
        ui.label("中央:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.lb_mid)
                .desired_width(70.0),
        );
        ui.label("終端:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.lb_end)
                .desired_width(70.0),
        );
    })
    .response
    .on_hover_text(
        "大梁用。3欄すべて空欄なら等間隔横補剛による区間長か、材端節点間距離を用います。",
    );

    ui.horizontal(|ui| {
        ui.label("等間隔横補剛本数:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.lateral_brace_count)
                .desired_width(50.0),
        )
        .on_hover_text(
            "大梁で lb の直接入力がなく本数が正の場合、実材軸長/(本数+1) を用います。本数が空欄または0の場合は材端節点間距離を用います。柱では本数から支持点を決めません。",
        );
    });

    let model = &app.core.model;
    if let Some(elem) = app
        .ui
        .scoped
        .steel_attr_draft
        .elem
        .and_then(|id| model.element(id))
        .filter(|elem| is_symmetric_h_column(model, elem))
    {
        let current = model.column_end_supports(elem);
        let recorded = &mut app
            .ui
            .scoped
            .steel_attr_draft
            .column_lateral_buckling_end_supports;
        let mut confirmed = current.is_some() && *recorded == current;
        if ui
            .add_enabled(
                current.is_some(),
                egui::Checkbox::new(
                    &mut confirmed,
                    "柱の両実材端で横移動・ねじれを拘束し、中間横補剛なし",
                ),
            )
            .on_hover_text(
                "検定する荷重状態で圧縮側になる各フランジについて、両実材端で有効な拘束が成立することを確認してください。",
            )
            .changed()
        {
            *recorded = current.filter(|_| confirmed);
        }
        if recorded.is_some() && !confirmed {
            ui.label("確認時から実材端の条件が変わりました。現在の条件を再確認してください。");
        }
    }

    ui.horizontal(|ui| {
        ui.label("座屈長さ lk_y(強軸)直接入力[mm]:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.lk_y_direct)
                .id(egui::Id::new("steel_attr_lk_y"))
                .desired_width(80.0),
        );
        ui.label("lk_z(弱軸)直接入力[mm]:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.lk_z_direct)
                .id(egui::Id::new("steel_attr_lk_z"))
                .desired_width(80.0),
        );
    })
    .response
    .on_hover_text(STEEL_BUCKLING_GUIDANCE);
    ui.label("CFT柱は両軸に有限な正の座屈長さが必要です。空欄や0があると未検定です。");
    ui.label("SRC柱の軸力・一軸曲げには、各方向の断面せいの12倍以下で正の座屈長さを指定します。");

    ui.horizontal(|ui| {
        ui.label("C 係数直接入力（空欄=自動）:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.steel_attr_draft.c_direct)
                .id(egui::Id::new("steel_attr_c_direct"))
                .desired_width(60.0),
        );
    })
    .response
    .on_hover_text(
        "直接入力に対応する部材では指定値をそのまま採用し、自動算定の上限2.3は適用しません。上下対称H形鋼柱の通常検定では、この入力を使わず補剛区間全体の曲げ分布から算定します。",
    );

    let parsed_flange = parse_loss_percent(&app.ui.scoped.steel_attr_draft.joint_flange_loss, "βf");
    let parsed_web = parse_loss_percent(&app.ui.scoped.steel_attr_draft.joint_web_loss, "βw");
    let parsed_scallop = parse_loss_percent(&app.ui.scoped.steel_attr_draft.scallop_web_loss, "αw");
    let parsed_lb = parse_lb_direct(
        &app.ui.scoped.steel_attr_draft.lb_start,
        &app.ui.scoped.steel_attr_draft.lb_mid,
        &app.ui.scoped.steel_attr_draft.lb_end,
    );
    let parsed_brace_count = parse_brace_count(&app.ui.scoped.steel_attr_draft.lateral_brace_count);
    let parsed_lk_y = parse_lk_direct(&app.ui.scoped.steel_attr_draft.lk_y_direct, "lk_y");
    let parsed_lk_z = parse_lk_direct(&app.ui.scoped.steel_attr_draft.lk_z_direct, "lk_z");
    let parsed_c = parse_lk_direct(&app.ui.scoped.steel_attr_draft.c_direct, "C係数");

    for (label, r) in [
        ("βf", parsed_flange.as_ref().err()),
        ("βw", parsed_web.as_ref().err()),
        ("αw", parsed_scallop.as_ref().err()),
        ("lb", parsed_lb.as_ref().err()),
        ("横補剛本数", parsed_brace_count.as_ref().err()),
        ("lk_y", parsed_lk_y.as_ref().err()),
        ("lk_z", parsed_lk_z.as_ref().err()),
        ("C係数", parsed_c.as_ref().err()),
    ] {
        if let Some(e) = r {
            ui.colored_label(crate::theme::ERROR_RED, format!("{label}の入力エラー: {e}"));
        }
    }

    let can_apply = app.ui.scoped.steel_attr_draft.elem.is_some()
        && parsed_flange.is_ok()
        && parsed_web.is_ok()
        && parsed_scallop.is_ok()
        && parsed_lb.is_ok()
        && parsed_brace_count.is_ok()
        && parsed_lk_y.is_ok()
        && parsed_lk_z.is_ok()
        && parsed_c.is_ok();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(can_apply, egui::Button::new("✔ 適用"))
            .on_hover_text(
                "選択した部材にS造検定属性を設定します(すべて既定値の場合は削除します。undo可)",
            )
            .clicked()
        {
            if let (
                Some(elem),
                Ok(joint_flange_loss),
                Ok(joint_web_loss),
                Ok(scallop_web_loss),
                Ok(lb_direct),
                Ok(lateral_brace_count),
                Ok(lk_y_direct),
                Ok(lk_z_direct),
                Ok(c_direct),
            ) = (
                app.ui.scoped.steel_attr_draft.elem,
                parsed_flange,
                parsed_web,
                parsed_scallop,
                parsed_lb,
                parsed_brace_count,
                parsed_lk_y,
                parsed_lk_z,
                parsed_c,
            ) {
                let attr = SteelDesignAttr {
                    elem,
                    joint_flange_loss,
                    joint_web_loss,
                    scallop_web_loss,
                    lb_direct,
                    lateral_brace_count,
                    column_lateral_buckling_end_supports: app
                        .ui
                        .scoped
                        .steel_attr_draft
                        .column_lateral_buckling_end_supports
                        .clone(),
                    lk_y_direct,
                    lk_z_direct,
                    c_direct,
                };
                if attr.is_empty() {
                    app.core.scoped.undo.run(
                        &mut app.core.model,
                        Box::new(RemoveSteelDesignAttr { elem }),
                    );
                } else {
                    app.core
                        .scoped
                        .undo
                        .run(&mut app.core.model, Box::new(SetSteelDesignAttr { attr }));
                }
                app.core.scoped.staleness.mark_edited();
            }
        }
        if app.ui.scoped.steel_attr_draft.elem.is_some()
            && ui
                .button("🗑 この部材のS造検定属性を削除")
                .on_hover_text("選択した部材のS造検定属性を削除します(undo可)")
                .clicked()
        {
            if let Some(elem) = app.ui.scoped.steel_attr_draft.elem {
                app.core.scoped.undo.run(
                    &mut app.core.model,
                    Box::new(RemoveSteelDesignAttr { elem }),
                );
                app.core.scoped.staleness.mark_edited();
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;

    fn draw(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| steel_attrs_table(ui, app),
        )
    }

    fn click(ctx: &egui::Context, app: &mut App, pos: egui::Pos2) {
        for pressed in [true, false] {
            draw(
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
        let output = draw(ctx, app, vec![]);
        let pos = output
            .shapes
            .iter()
            .rev()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("表示なし: {label}"));
        click(ctx, app, pos);
    }

    fn type_field(ctx: &egui::Context, app: &mut App, id: &'static str, text: &str) {
        draw(ctx, app, vec![]);
        click(
            ctx,
            app,
            ctx.read_response(egui::Id::new(id)).unwrap().rect.center(),
        );
        draw(
            ctx,
            app,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                },
                egui::Event::Text(text.into()),
            ],
        );
    }

    #[test]
    fn general_h_column_support_confirmation_is_saved_and_invalidates_prior_design() {
        const CONFIRM: &str = "柱の両実材端で横移動・ねじれを拘束し、中間横補剛なし";
        for kind in [ElementKind::Beam, ElementKind::Fiber] {
            let mut model = crate::sample::portal_frame();
            model.elements[0].kind = kind;
            let mut app = App::default();
            assert!(app.load_model(model));
            app.core.scoped.results = Some(crate::app::ResultsBundle::default());
            app.certify_design_fixture();
            assert!(app.current_design_results().is_some());

            let ctx = egui::Context::default();
            click_text(&ctx, &mut app, "―");
            click_text(&ctx, &mut app, "部材#0");
            click_text(&ctx, &mut app, CONFIRM);
            click_text(&ctx, &mut app, "✔ 適用");
            let [attr] = app.core.model.steel_design_attrs.as_slice() else {
                panic!("{:?}", app.core.model.steel_design_attrs);
            };
            assert!(attr.column_lateral_buckling_end_supports.is_some());
            assert!(app.core.scoped.staleness.design_stale);
            assert!(app.current_design_results().is_none());

            let path = std::env::temp_dir()
                .join(format!("sepika-column-lb-{}.ovika", uuid::Uuid::new_v4()));
            app.save_project_to(path.clone());
            assert!(app.core.scoped.last_error.is_none());
            let mut app = App::default();
            app.open_project_from(path.clone());
            std::fs::remove_file(path).unwrap();
            assert!(app.core.scoped.last_error.is_none());
            assert!(app.current_design_results().is_none());
            assert!(app.core.model.steel_design_attrs[0]
                .column_lateral_buckling_end_supports
                .is_some());

            let ctx = egui::Context::default();
            click_text(&ctx, &mut app, "✏");
            click_text(&ctx, &mut app, CONFIRM);
            click_text(&ctx, &mut app, "✔ 適用");
            assert!(app.core.model.steel_design_attrs.is_empty());
        }
    }

    #[test]
    fn changing_column_end_conditions_requires_a_new_checkbox_action() {
        use sepika_core::model::SupportMemberId;
        use sepika_edit::SetMemberEndOffsets;

        const CONFIRM: &str = "柱の両実材端で横移動・ねじれを拘束し、中間横補剛なし";
        let mut model = crate::sample::portal_frame();
        let id = model.elements[0].id;
        model.elements[0].end_offsets_mm[1][2] = 500.0;
        let mut app = App::default();
        assert!(app.load_model(model));
        let ctx = egui::Context::default();
        click_text(&ctx, &mut app, "―");
        click_text(&ctx, &mut app, "部材#0");
        click_text(&ctx, &mut app, CONFIRM);
        click_text(&ctx, &mut app, "✔ 適用");

        let adopted_length = |app: &App| {
            let model = &app.core.model;
            sepika_design_jp::steel::column_lateral_buckling_length(
                model,
                &model.elements[0],
                model.steel_design_attrs.first(),
            )
        };
        let original = adopted_length(&app).unwrap();
        let offset_change = || {
            Box::new(SetMemberEndOffsets {
                member: SupportMemberId::Primary(id),
                value: [[0.0; 3]; 2],
            })
        };
        app.core
            .scoped
            .undo
            .run(&mut app.core.model, offset_change());
        app.core.scoped.undo.undo(&mut app.core.model);
        assert_eq!(adopted_length(&app), Ok(original));
        app.core.scoped.undo.redo(&mut app.core.model);
        assert!(adopted_length(&app)
            .unwrap_err()
            .contains("確認時から変わりました"));
        assert!(draw(&ctx, &mut app, vec![]).shapes.iter().any(|s| {
            matches!(&s.shape, egui::Shape::Text(text)
                if text.galley.text().contains("現在の条件を再確認"))
        }));

        click_text(&ctx, &mut app, "✔ 適用");
        assert!(adopted_length(&app).is_err());
        click_text(&ctx, &mut app, CONFIRM);
        click_text(&ctx, &mut app, "✔ 適用");
        assert_eq!(adopted_length(&app), Ok(original - 500.0));
    }

    #[test]
    fn column_end_support_input_is_unavailable_on_a_girder_or_cft_column() {
        let mut app = App::default();
        let ctx = egui::Context::default();
        assert!(app.load_model(crate::sample::portal_frame()));
        click_text(&ctx, &mut app, "―");
        click_text(&ctx, &mut app, "部材#2");
        let assert_checkbox_absent = |app: &mut App| {
            assert!(!draw(&ctx, app, vec![]).shapes.iter().any(|s| {
                matches!(&s.shape, egui::Shape::Text(text)
                    if text.galley.text().contains("柱の両実材端で横移動・ねじれを拘束"))
            }));
        };
        assert_checkbox_absent(&mut app);
        assert!(
            app.load_model(crate::app::cft_column_buckling_tests::input::cantilever(
                false, 1000.0
            ))
        );
        click_text(&ctx, &mut app, "―");
        click_text(&ctx, &mut app, "部材#0");
        assert_checkbox_absent(&mut app);
    }

    #[test]
    fn cft_column_fiber_edit_and_save_both_axes_at_full_precision() {
        use crate::app::cft_column_buckling_tests::input;

        let kind = ElementKind::Fiber;
        let mut model = input::cantilever(false, 1000.0);
        model.elements[0].kind = kind;
        let mut app = App::default();
        assert!(app.load_model(model));
        let ctx = egui::Context::default();
        click_text(&ctx, &mut app, "―");
        click_text(&ctx, &mut app, "部材#0");
        type_field(&ctx, &mut app, "steel_attr_lk_y", "4799.9");
        type_field(&ctx, &mut app, "steel_attr_lk_z", "4800.1");
        click_text(&ctx, &mut app, "✔ 適用");
        let attr = input::direct_lengths(ElemId(0), Some(4799.9), Some(4800.1));
        let expected = std::slice::from_ref(&attr);
        assert_eq!(app.core.model.steel_design_attrs.as_slice(), expected);

        let path =
            std::env::temp_dir().join(format!("sepika-cft-lk-{}.ovika", uuid::Uuid::new_v4()));
        let reload = |app: &mut App| {
            app.save_project_to(path.clone());
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            let mut reopened = App::default();
            reopened.open_project_from(path.clone());
            assert!(
                reopened.core.scoped.last_error.is_none(),
                "{:?}",
                reopened.core.scoped.last_error
            );
            assert_eq!(reopened.core.model.elements[0].kind, kind);
            reopened
        };
        let mut app = reload(&mut app);
        assert_eq!(app.core.model.steel_design_attrs.as_slice(), expected);
        let ctx = egui::Context::default();
        click_text(&ctx, &mut app, "✏");
        assert_eq!(app.ui.scoped.steel_attr_draft.lk_y_direct, "4799.9");
        assert_eq!(app.ui.scoped.steel_attr_draft.lk_z_direct, "4800.1");
        assert!(draw(&ctx, &mut app, vec![]).shapes.iter().any(|s| {
            matches!(&s.shape, egui::Shape::Text(text)
                if text.galley.text().contains("lk_y=4799.9 lk_z=4800.1"))
        }));

        let revision = app.core.scoped.undo.revision();
        for (field, axis, value) in [
            ("steel_attr_lk_y", "lk_y", "NaN"),
            ("steel_attr_lk_z", "lk_z", "inf"),
        ] {
            type_field(&ctx, &mut app, field, value);
            let reason = format!("{axis}の入力エラー: {axis}は有限な正の値");
            assert!(draw(&ctx, &mut app, vec![]).shapes.iter().any(|s| {
                matches!(&s.shape, egui::Shape::Text(text)
                    if text.galley.text().contains(&reason))
            }));
            click_text(&ctx, &mut app, "✔ 適用");
            assert_eq!(app.core.scoped.undo.revision(), revision);
            assert_eq!(app.core.model.steel_design_attrs.as_slice(), expected);
            click_text(&ctx, &mut app, "✏");
        }

        type_field(&ctx, &mut app, "steel_attr_lk_y", "5000.25");
        click_text(&ctx, &mut app, "✔ 適用");
        let app = reload(&mut app);
        assert_eq!(
            app.core.model.steel_design_attrs,
            [input::direct_lengths(
                ElemId(0),
                Some(5000.25),
                Some(4800.1)
            )]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn steel_table_shows_direct_buckling_length_requirements_for_steel_and_cft() {
        let mut app = App::default();
        app.load_model(crate::app::cft_column_buckling_tests::input::cantilever(
            false, 1000.0,
        ));
        let output = draw(&egui::Context::default(), &mut app, vec![]);
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("空欄は節点間距離の柱1.2倍、梁・ブレース1.0倍"),
            "{text}"
        );
        assert!(
            text.contains("CFT柱は両軸に有限な正の座屈長さが必要"),
            "{text}"
        );
    }

    /// 空欄は 0（欠損なし）として扱われること。
    #[test]
    fn test_parse_loss_percent_empty_is_zero() {
        assert_eq!(parse_loss_percent("", "βf").unwrap(), 0.0);
        assert_eq!(parse_loss_percent("   ", "βf").unwrap(), 0.0);
    }

    /// 数値でない入力はエラーになること。
    #[test]
    fn test_parse_loss_percent_rejects_non_numeric() {
        let err = parse_loss_percent("abc", "βf").unwrap_err();
        assert!(err.contains("βf"), "err={err}");
    }

    /// 3欄すべて空欄なら None（自動）になること。
    #[test]
    fn test_parse_lb_direct_all_empty_is_none() {
        assert_eq!(parse_lb_direct("", "", "").unwrap(), None);
        assert_eq!(parse_lb_direct("  ", " ", "").unwrap(), None);
    }

    /// 3欄すべて入力されていれば Some のタプルになること。
    #[test]
    fn test_parse_lb_direct_all_filled() {
        assert_eq!(
            parse_lb_direct("1000", "2000", "3000").unwrap(),
            Some((1000.0, 2000.0, 3000.0))
        );
    }

    /// 一部だけ入力されている場合はエラーになること。
    #[test]
    fn test_parse_lb_direct_partial_is_error() {
        assert!(parse_lb_direct("1000", "", "").is_err());
        assert!(parse_lb_direct("", "2000", "3000").is_err());
    }

    /// 数値でない入力はエラーになること。
    #[test]
    fn test_parse_lb_direct_rejects_non_numeric() {
        assert!(parse_lb_direct("abc", "2000", "3000").is_err());
    }

    #[test]
    fn changing_only_c_preserves_lateral_buckling_lengths_and_losses() {
        for (lb_direct, lb_label) in [
            (Some((0.4, 0.0, 0.04)), "0.4/0/0.04"),
            (Some((0.0, 0.4, 0.0)), "0/0.4/0"),
            (None, "自動"),
        ] {
            let mut model = crate::sample::portal_frame();
            let attr = SteelDesignAttr {
                elem: model.elements[2].id,
                joint_flange_loss: 0.04,
                joint_web_loss: 0.025,
                scallop_web_loss: 0.005,
                lb_direct,
                lateral_brace_count: None,
                column_lateral_buckling_end_supports: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: Some(1.25),
            };
            model.steel_design_attrs = vec![attr.clone()];
            let mut app = App::default();
            assert!(app.load_model(model));
            let ctx = egui::Context::default();
            click_text(&ctx, &mut app, "✏");
            type_field(&ctx, &mut app, "steel_attr_c_direct", "1.375");
            click_text(&ctx, &mut app, "✔ 適用");
            assert_eq!(
                app.core.model.steel_design_attrs,
                [SteelDesignAttr {
                    c_direct: Some(1.375),
                    ..attr
                }]
            );
            let expected_label = format!("βf=0.04% βw=0.025% αw=0.005% / lb={lb_label} /");
            assert!(
                draw(&ctx, &mut app, vec![]).shapes.iter().any(|s| {
                    matches!(&s.shape, egui::Shape::Text(text)
                    if text.galley.text().contains(&expected_label))
                }),
                "{expected_label}"
            );
        }
    }

    /// 空欄は None（補剛なし）として扱われること。
    #[test]
    fn test_parse_brace_count_empty_is_none() {
        assert_eq!(parse_brace_count("").unwrap(), None);
    }

    /// 正の整数はそのままパースされること。
    #[test]
    fn test_parse_brace_count_valid() {
        assert_eq!(parse_brace_count("3").unwrap(), Some(3));
        assert_eq!(parse_brace_count("0").unwrap(), Some(0));
    }

    /// 整数でない入力はエラーになること。
    #[test]
    fn test_parse_brace_count_rejects_non_integer() {
        assert!(parse_brace_count("abc").is_err());
        assert!(parse_brace_count("-1").is_err());
        assert!(parse_brace_count("1.5").is_err());
    }

    /// 空欄は None（自動算定）として扱われること。
    #[test]
    fn test_parse_lk_direct_empty_is_none() {
        assert_eq!(parse_lk_direct("", "lk_y").unwrap(), None);
    }

    /// 正の値はそのままパースされること。
    #[test]
    fn test_parse_lk_direct_valid() {
        assert_eq!(parse_lk_direct("3500", "lk_y").unwrap(), Some(3500.0));
    }

    /// 0 以下・非有限・数値でない入力はエラーになること。
    #[test]
    fn test_parse_lk_direct_rejects_non_positive_or_invalid() {
        assert!(parse_lk_direct("0", "lk_y").is_err());
        assert!(parse_lk_direct("-100", "lk_y").is_err());
        assert!(parse_lk_direct("abc", "lk_y").is_err());
        assert!(parse_lk_direct("NaN", "lk_y").is_err());
        assert!(parse_lk_direct("inf", "lk_z").is_err());
    }

    /// 表示用整形（lb 直接入力・座屈長さ・横補剛本数）が None/Some で
    /// 期待通りの文字列になること。
    #[test]
    fn test_desc_helpers() {
        assert_eq!(lb_direct_desc(None), "自動");
        assert_eq!(
            lb_direct_desc(Some((1000.0, 2000.0, 3000.0))),
            "1000/2000/3000"
        );
        assert_eq!(opt_len_desc(None), "未指定");
        assert_eq!(opt_len_desc(Some(3500.0)), "3500");
        assert_eq!(opt_count_desc(None), "なし");
        assert_eq!(opt_count_desc(Some(3)), "3");
        assert_eq!(opt_c_desc(None), "自動");
        assert_eq!(opt_c_desc(Some(1.5)), "1.50");
    }
}
