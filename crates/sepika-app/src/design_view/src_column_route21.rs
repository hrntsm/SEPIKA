use crate::app::src_column_route21::{
    is_src_column, Route21Axis, Route21Input, Route21Rc, Route21Source,
};
use crate::app::App;
use sepika_core::ids::ElemId;

#[derive(Clone, Default)]
struct AxisDraft {
    enabled: bool,
    positive: Option<bool>,
    steel: [String; 2],
    method: Option<Method>,
    rc: [String; 2],
    head: Option<Head>,
    beams: String,
    shear: String,
    evidence: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Bending,
    PrimaryEarthquake,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Head {
    BothEndsYield,
    Beams,
    TopStoryBeams,
}

fn value(text: &str, label: &str, min_inclusive: bool) -> Result<f64, String> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && if min_inclusive { *v >= 0.0 } else { *v > 0.0 })
        .ok_or_else(|| {
            format!(
                "{label}に有限な{}を入力してください",
                if min_inclusive {
                    "非負値"
                } else {
                    "正の値"
                }
            )
        })
}

fn pair(values: &[String; 2], label: &str) -> Result<[f64; 2], String> {
    Ok([
        value(&values[0], &format!("柱頭の{label}"), true)?,
        value(&values[1], &format!("柱脚の{label}"), true)?,
    ])
}

impl AxisDraft {
    fn input(&self) -> Result<Option<Route21Axis>, String> {
        if !self.enabled {
            return Ok(None);
        }
        let positive = self.positive.ok_or("局所加力の向きを選択してください")?;
        let steel_n_mm = pair(&self.steel, "鉄骨短期許容曲げ耐力")?;
        let evidence = self.evidence.trim();
        if evidence.is_empty() {
            return Err("値と方式の確認根拠を入力してください".into());
        }
        let rc = match self.method.ok_or("RC部の採用方式を選んでください")? {
            Method::Bending => Route21Rc::Bending {
                ultimate_n_mm: pair(&self.rc, "RC終局曲げ耐力")?,
                connected_beams: match self.head.ok_or("曲げ方式の柱頭条件を選んでください")?
                {
                    Head::BothEndsYield => None,
                    head => Some((
                        value(&self.beams, "接続梁の曲げ耐力合計", true)?,
                        head == Head::TopStoryBeams,
                    )),
                },
            },
            Method::PrimaryEarthquake => Route21Rc::PrimaryEarthquake {
                shear_n: value(&self.shear, "一次設計用地震力だけによる柱せん断力Q", true)?,
            },
        };
        Ok(Some(Route21Axis {
            positive,
            steel_n_mm,
            rc,
            evidence: evidence.into(),
        }))
    }
}

impl From<&Route21Axis> for AxisDraft {
    fn from(axis: &Route21Axis) -> Self {
        let mut draft = Self {
            enabled: true,
            positive: Some(axis.positive),
            steel: axis.steel_n_mm.map(|v| v.to_string()),
            evidence: axis.evidence.clone(),
            ..Default::default()
        };
        match axis.rc {
            Route21Rc::Bending {
                ultimate_n_mm,
                connected_beams,
            } => {
                draft.method = Some(Method::Bending);
                draft.rc = ultimate_n_mm.map(|v| v.to_string());
                draft.head = Some(match connected_beams {
                    None => Head::BothEndsYield,
                    Some((sum, top_story)) => {
                        draft.beams = sum.to_string();
                        if top_story {
                            Head::TopStoryBeams
                        } else {
                            Head::Beams
                        }
                    }
                });
            }
            Route21Rc::PrimaryEarthquake { shear_n } => {
                draft.method = Some(Method::PrimaryEarthquake);
                draft.shear = shear_n.to_string();
            }
        }
        draft
    }
}

#[derive(Default)]
pub(crate) struct Route21Draft {
    pub(crate) elem: Option<ElemId>,
    binding: Option<(Route21Source, ElemId)>,
    clear_height: String,
    axes: [AxisDraft; 2],
    error: Option<String>,
}

impl Route21Draft {
    fn input(&self) -> Result<Route21Input, String> {
        let axes = [
            self.axes[0].input().map_err(|e| format!("Qy: {e}"))?,
            self.axes[1].input().map_err(|e| format!("Qz: {e}"))?,
        ];
        if axes.iter().all(Option::is_none) {
            return Err("確認する検討方向を選んでください".into());
        }
        Ok(Route21Input {
            clear_height_mm: value(&self.clear_height, "柱内法高さ", false)?,
            axes,
        })
    }
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
    });
}

fn axis_ui(ui: &mut egui::Ui, label: &str, draft: &mut AxisDraft) {
    ui.push_id(label, |ui| {
        ui.checkbox(&mut draft.enabled, label);
        if !draft.enabled {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("局所加力の向き");
            ui.radio_value(&mut draft.positive, Some(true), "正");
            ui.radio_value(&mut draft.positive, Some(false), "負");
        });
        field(ui, "柱頭の鉄骨短期許容曲げ耐力 [N·mm]", &mut draft.steel[0]);
        field(ui, "柱脚の鉄骨短期許容曲げ耐力 [N·mm]", &mut draft.steel[1]);
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut draft.method, Some(Method::Bending), "曲げ方式（Qd1）");
            ui.radio_value(
                &mut draft.method,
                Some(Method::PrimaryEarthquake),
                "代替式（Qd2）",
            );
        });
        match draft.method {
            Some(Method::Bending) => {
                field(
                    ui,
                    "柱頭のRC終局曲げ耐力（柱頭制限前） [N·mm]",
                    &mut draft.rc[0],
                );
                field(ui, "柱脚のRC終局曲げ耐力 [N·mm]", &mut draft.rc[1]);
                ui.horizontal_wrapped(|ui| {
                    ui.label("柱頭条件");
                    ui.radio_value(&mut draft.head, Some(Head::BothEndsYield), "両端曲げ降伏");
                    ui.radio_value(&mut draft.head, Some(Head::Beams), "接続梁（一般階）");
                    ui.radio_value(
                        &mut draft.head,
                        Some(Head::TopStoryBeams),
                        "接続梁（最上階）",
                    );
                });
                if matches!(draft.head, Some(Head::Beams | Head::TopStoryBeams)) {
                    field(
                        ui,
                        "左右接続梁の鉄骨短期許容・RC終局曲げ耐力の合計 [N·mm]",
                        &mut draft.beams,
                    );
                    ui.label("接続梁のRC部は短期許容曲げ耐力で代用できます。");
                }
            }
            Some(Method::PrimaryEarthquake) => {
                field(
                    ui,
                    "一次設計用地震力だけによる柱せん断力Q [N]",
                    &mut draft.shear,
                );
            }
            None => {}
        }
        ui.label("値・内法高さ・加力方向と方式の確認根拠（計算書の頁・解析ケース等）");
        ui.text_edit_singleline(&mut draft.evidence);
    });
}

pub(super) fn section(ui: &mut egui::Ui, app: &mut App) {
    if !app.study_is_main() {
        return;
    }
    let model = &app.core.model;
    let columns: Vec<_> = model
        .elements
        .iter()
        .filter(|e| is_src_column(model, e))
        .map(|e| {
            (
                e.id,
                format!(
                    "#{} {}",
                    e.id.0,
                    model.element_section(e).map_or("", |sec| sec.name.as_str())
                ),
            )
        })
        .collect();
    if columns.is_empty() {
        return;
    }
    ui.collapsing("SRC柱のルート2-1 地震時せん断", |ui| {
        let source = match app.selected_route21_source() {
            Ok(source) => source,
            Err(reason) => {
                ui.colored_label(crate::theme::WARN_TEXT, reason);
                if !app.core.scoped.route21_registrations.is_empty() {
                    ui.label("対応する解析結果を確認できない間は、登録済みの値と根拠を使いません。");
                }
                return;
            }
        };
        ui.label(format!("短期地震荷重組合せ「{}」", source.combo));
        let draft = &mut app.ui.scoped.src_column_route21;
        egui::ComboBox::from_id_salt("src_route21_column")
            .selected_text(columns.iter().find(|(elem, _)| Some(*elem) == draft.elem)
                .map_or("対象SRC柱を選択", |(_, label)| label.as_str()))
            .show_ui(ui, |ui| {
                for (elem, label) in &columns {
                    ui.selectable_value(&mut draft.elem, Some(*elem), label);
                }
            });
        let Some(elem) = draft.elem.filter(|id| columns.iter().any(|(elem, _)| elem == id)) else {
            return;
        };
        let registered = app.core.scoped.route21_registrations.iter()
            .find(|r| r.elem == elem && r.source.combo == source.combo);
        let active_input = registered.filter(|r| r.source == source).map(|r| &r.input);
        if draft.binding.as_ref() != Some(&(source.clone(), elem)) {
            *draft = Route21Draft {
                elem: Some(elem),
                binding: Some((source.clone(), elem)),
                clear_height: active_input.map_or_else(String::new, |i| i.clear_height_mm.to_string()),
                axes: active_input.map_or_else(Default::default, |i| i.axes.each_ref().map(|a| a.as_ref().map_or_else(AxisDraft::default, AxisDraft::from))),
                error: None,
            };
        }
        match (registered, active_input) {
            (_, Some(_)) => { ui.label("現行の解析・設計条件に登録済み。算定値は下の検定表で確認してください。"); }
            (Some(old), None) => {
                ui.colored_label(crate::theme::WARN_TEXT, "未算定: 登録後に解析実行または設計条件が変わりました。使う値と根拠を確認し直してください。");
                ui.collapsing("古い登録の根拠（現行検定には使用しません）", |ui| {
                    for (label, axis) in ["Qy", "Qz"].into_iter().zip(&old.input.axes) {
                        if let Some(axis) = axis {
                            ui.label(format!("{label}: {}", axis.evidence));
                        }
                    }
                });
            }
            (None, None) => { ui.label("未算定: この柱・地震荷重組合せの確認済み入力がありません。"); }
        }
        ui.label("以下はルート2-1の適用と値を確認済みの場合だけ登録します。方向・耐力・内法高さは自動判定しません。");
        ui.label("入力欄の変更は「確認済み入力を登録して検定」を押すまで検定に反映されません。");
        field(ui, "柱内法高さ [mm]", &mut draft.clear_height);
        axis_ui(ui, "Qy（強軸曲げ）", &mut draft.axes[0]);
        axis_ui(ui, "Qz（弱軸曲げ）", &mut draft.axes[1]);
        let can_edit = app.core.scoped.job.is_none();
        if ui.add_enabled(can_edit, egui::Button::new("確認済み入力を登録して検定")).clicked() {
            let result = draft.input().and_then(|input| app.register_route21(source.clone(), elem, input));
            app.ui.scoped.src_column_route21.error = result.err();
            if app.ui.scoped.src_column_route21.error.is_none() {
                app.run_design_check();
            }
        } else if ui.add_enabled(can_edit && registered.is_some(), egui::Button::new("登録を解除して再検定")).clicked() {
            app.remove_route21(&source.combo, elem);
            app.ui.scoped.src_column_route21.binding = None;
            app.run_design_check();
        }
        if let Some(error) = &app.ui.scoped.src_column_route21.error {
            ui.colored_label(crate::theme::WARN_TEXT, error);
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn src_route21_confirmation_requires_explicit_direction_method_and_evidence() {
        let mut draft = AxisDraft {
            enabled: true,
            steel: ["1.2e8".into(), "3.0e8".into()],
            method: Some(Method::PrimaryEarthquake),
            shear: "400000".into(),
            ..Default::default()
        };
        assert!(draft.input().unwrap_err().contains("加力"));
        draft.positive = Some(false);
        draft.evidence = "  ".into();
        assert!(draft.input().unwrap_err().contains("根拠"));
        draft.evidence = "外部計算書 p.5".into();
        draft.method = None;
        assert!(draft.input().unwrap_err().contains("方式"));
        draft.method = Some(Method::PrimaryEarthquake);
        let axis = draft.input().unwrap().unwrap();
        assert!(!axis.positive);
        assert_eq!(axis.steel_n_mm, [120_000_000.0, 300_000_000.0]);
        assert!(matches!(
            axis.rc,
            Route21Rc::PrimaryEarthquake { shear_n: 400_000.0 }
        ));
        assert_eq!(axis.evidence, "外部計算書 p.5");
    }
}
