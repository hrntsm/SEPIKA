use crate::app::App;
use sepika_core::model::{FrameSectionUse, SecondaryMemberKind, SupportMemberId};
use sepika_edit::SetMemberEndOffsets;

pub(super) fn beam_eccentricities(
    ui: &mut egui::Ui,
    app: &mut App,
    secondary: Option<SecondaryMemberKind>,
) {
    let model = &app.core.model;
    let members: Vec<_> = if let Some(kind) = secondary {
        model
            .beams()
            .chain(model.posts())
            .filter(|sm| sm.kind == kind)
            .map(|sm| (SupportMemberId::Secondary(sm.id), sm.end_offsets_mm))
            .collect()
    } else {
        model
            .elements
            .iter()
            .filter(|e| {
                e.kind.is_primary_frame_support()
                    && model.element_section(e).is_some_and(|s| {
                        matches!(
                            s.frame_use,
                            Some(FrameSectionUse::Girder | FrameSectionUse::Column)
                        )
                    })
            })
            .map(|e| (SupportMemberId::Primary(e.id), e.end_offsets_mm))
            .collect()
    };
    let mut pending = Vec::new();
    egui::CollapsingHeader::new("部材の端別XYZ偏心 [mm]（全体座標）")
        .id_salt(("member_eccentricities", format!("{secondary:?}"))).show(ui, |ui| {
        ui.label(if secondary.is_some() { "小梁・間柱の偏心は表示専用です。支持先の曲げ・ねじりへの偏心効果は計算しません。" } else { "基準節点から実部材端までの移動量を入力します。0は偏心なしです。" });
        for (member, value) in members {
            ui.push_id(member, |ui| {
                ui.label(match member { SupportMemberId::Primary(id) => format!("主架構 ID {}", id.0), SupportMemberId::Secondary(id) => format!("二次部材 ID {}", id.0) });
                let mut next = value;
                let mut changed = false;
                for side in 0..2 {
                    ui.horizontal(|ui| {
                        ui.label(if side == 0 { "i端" } else { "j端" });
                        for (axis, label) in ["X", "Y", "Z"].iter().enumerate() {
                            ui.label(*label);
                            let id = ui.id().with((side, axis));
                            ui.data_mut(|data| {
                                let cells = data.get_temp_mut_or_default::<Vec<egui::Id>>(egui::Id::new("numeric_input_cells"));
                                if !cells.contains(&id) { cells.push(id); }
                            });
                            let stored = ui.data(|d| d.get_temp::<(f64, String)>(id));
                            let mut text = stored.filter(|(saved, _)| *saved == value[side][axis]).map(|(_,text)| text).unwrap_or_else(|| value[side][axis].to_string());
                            let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0));
                            let parsed = text.trim().parse::<f64>().ok().filter(|v| v.is_finite());
                            if parsed.is_none() { ui.colored_label(crate::theme::ERROR_RED, "有限値が必要です"); }
                            if response.lost_focus() { if let Some(v) = parsed.filter(|v| *v != value[side][axis]) { next[side][axis] = v; changed = true; } }
                            ui.data_mut(|d| d.insert_temp(id, (value[side][axis], text)));
                        }
                    });
                }
                if changed { pending.push((member, next)); }
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
        app.apply_model_edit(Box::new(SetMemberEndOffsets { member, value }));
    }
}
