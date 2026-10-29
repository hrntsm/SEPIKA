use crate::app::App;
use sepika_core::model::{FrameSectionUse, SupportMemberId};
use sepika_edit::SetBeamVerticalEccentricity;

pub(super) fn beam_eccentricities(ui: &mut egui::Ui, app: &mut App, secondary: bool) {
    let model = &app.core.model;
    let members: Vec<_> = if secondary {
        model
            .beams()
            .map(|sm| (SupportMemberId::Secondary(sm.id), sm.vertical_eccentricity))
            .collect()
    } else {
        model
            .elements
            .iter()
            .filter(|e| {
                e.kind.is_primary_frame_support()
                    && model
                        .element_section(e)
                        .is_some_and(|s| s.frame_use == Some(FrameSectionUse::Girder))
            })
            .map(|e| (SupportMemberId::Primary(e.id), e.vertical_eccentricity))
            .collect()
    };
    let mut pending = Vec::new();
    egui::CollapsingHeader::new("梁の一律鉛直偏心 [mm]（上向き＋）")
        .id_salt(("beam_eccentricities", secondary)).show(ui, |ui| {
        ui.label("両端節点・支持アンカーを結ぶ材軸から梁全体を上下に移動します。空欄は未指定です。");
        for (member, value) in members {
            ui.push_id(member, |ui| {
                ui.horizontal(|ui| {
                    ui.label(match member { SupportMemberId::Primary(id) => format!("大梁 ID {}", id.0), SupportMemberId::Secondary(id) => format!("小梁 ID {}", id.0) });
                    let id = ui.id().with("eccentricity_text");
                    ui.data_mut(|data| {
                        let cells = data.get_temp_mut_or_default::<Vec<egui::Id>>(egui::Id::new("numeric_input_cells"));
                        if !cells.contains(&id) { cells.push(id); }
                    });
                    let stored = ui.data(|d| d.get_temp::<(Option<f64>, String)>(id));
                    let mut text = stored.filter(|(saved, _)| *saved == value)
                        .map(|(_, text)| text).unwrap_or_else(|| value.map(|v| v.to_string()).unwrap_or_default());
                    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(100.0).hint_text("未指定"));
                    let parsed = if text.trim().is_empty() { Some(None) }
                        else { text.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(Some) };
                    if parsed.is_none() { ui.colored_label(crate::theme::ERROR_RED, "有限な数値を入力してください"); }
                    if response.lost_focus() {
                        if let Some(next) = parsed.filter(|next| *next != value) { pending.push((member, next)); }
                    }
                    if ui.button("0を指定").clicked() { pending.push((member, Some(0.0))); }
                    if ui.button("未指定").clicked() { pending.push((member, None)); }
                    ui.data_mut(|d| d.insert_temp(id, (value, text)));
                });
                match model.beam_placement(member) {
                    Ok(placement) => {
                        let mid = sepika_core::geom::vec3::midpoint(placement.actual_ends[0], placement.actual_ends[1]);
                        match placement.vertical_range([mid[0], mid[1]]) {
                            Ok(Some([bottom, top])) => { ui.label(format!("実材軸Z: {:.1} → {:.1} / 中央上端Z: {top:.1} / 下端Z: {bottom:.1} mm", placement.actual_ends[0][2], placement.actual_ends[1][2])); }
                            Ok(None) => { ui.label("中央の鉛直線は実形状と交差しません"); }
                            Err(reason) => { ui.colored_label(crate::theme::ERROR_RED, reason); }
                        }
                    }
                    Err(reason) => { ui.colored_label(crate::theme::ERROR_RED, reason); }
                }
            });
        }
    });
    for (member, value) in pending {
        app.apply_model_edit(Box::new(SetBeamVerticalEccentricity { member, value }));
    }
}
