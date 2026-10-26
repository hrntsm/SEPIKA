//! ビューアの主操作と、現在の表示モードに対応する操作 UI。

use crate::app::App;
use crate::theme;

use super::playback::{advance_play_time, frame_at_time};
use super::{check_ratio, lumped};

use super::{
    CheckRatioFilter, CmqComponent, ForceComponent, ForceComponents, ModelingAnalysis, ViewMode,
};

mod primary;

#[cfg(test)]
mod tests;

/// 描画領域の上に置く操作 UI を描き、変更を `app` へ書き戻す。
pub(super) fn view_controls(ui: &mut egui::Ui, app: &mut App) {
    primary::primary_controls(ui, app);
    let mode = app.ui.view.view_mode;
    let mut mode_idx = app.ui.scoped.view_mode_idx;
    let mut force_components = app.ui.view.force_components;
    let mut cmq_component = app.ui.view.cmq_component;
    let mut cmq_axes = app.ui.view.cmq_axes;
    let mut check_ratio_filter = app.ui.view.check_ratio_filter;
    let mut modeling_analysis = app.ui.view.modeling_analysis;
    if mode == ViewMode::Cmq {
        ui.horizontal_wrapped(|ui| {
            ui.label("成分:");
            ui.selectable_value(&mut cmq_component, CmqComponent::C, "C(モーメント)");
            ui.selectable_value(&mut cmq_component, CmqComponent::M, "M(中央)");
            ui.selectable_value(&mut cmq_component, CmqComponent::Q, "Q(せん断)");
            ui.separator();
            ui.label("軸:");
            ui.checkbox(&mut cmq_axes.ey, "強軸(ey)");
            ui.checkbox(&mut cmq_axes.ez, "弱軸(ez)");
            ui.separator();
            let case_label = app
                .cmq_display_load_case()
                .map(|lc| lc.name.clone())
                .unwrap_or_else(|| "(荷重ケースなし)".to_string());
            ui.label(format!("荷重ケース: {case_label}（ナビゲータで切替）"));
        });
    }
    if mode == ViewMode::Modeling {
        ui.horizontal_wrapped(|ui| {
            ui.label("解析種別:");
            ui.selectable_value(
                &mut modeling_analysis,
                ModelingAnalysis::Static,
                "静解析(弾性)",
            );
            ui.selectable_value(
                &mut modeling_analysis,
                ModelingAnalysis::Incremental,
                "増分解析(弾塑性)",
            );
            ui.selectable_value(
                &mut modeling_analysis,
                ModelingAnalysis::TimeHistory,
                "非線形時刻歴",
            );
            ui.separator();
            ui.add_enabled(
                false,
                egui::Label::new(
                    egui::RichText::new("部材の色＝解析上の要素モデル。○=端部ピン／□=半剛")
                        .size(11.0),
                ),
            );
        });
    }
    if mode == ViewMode::Force {
        ui.horizontal_wrapped(|ui| {
            ui.label("成分:");
            for c in ForceComponent::ALL {
                ui.checkbox(
                    force_components.flag_mut(c),
                    egui::RichText::new(c.label()).color(c.color()),
                );
            }
            ui.separator();
            if ui.button("N図").clicked() {
                force_components = ForceComponents::PRESET_N;
            }
            if ui.button("Q図").clicked() {
                force_components = ForceComponents::PRESET_Q;
            }
            if ui.button("M図").clicked() {
                force_components = ForceComponents::PRESET_M;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.toggle_value(&mut app.ui.view.overlay_deform, "変形表示");
            ui.toggle_value(&mut app.ui.view.diagram_contour, "コンター");
            if app.ui.view.diagram_contour {
                let mut colormap = app.ui.view.contour_colormap;
                egui::ComboBox::from_id_salt("contour_colormap")
                    .selected_text(colormap.label())
                    .show_ui(ui, |ui| {
                        for cm in [
                            theme::ColorMap::Viridis,
                            theme::ColorMap::Plasma,
                            theme::ColorMap::Turbo,
                            theme::ColorMap::Jet,
                            theme::ColorMap::BlueWhiteRed,
                        ] {
                            ui.selectable_value(&mut colormap, cm, cm.label());
                        }
                    });
                app.ui.view.contour_colormap = colormap;
            }
            ui.toggle_value(&mut app.ui.view.diagram_values, "値を表示")
                .on_hover_text(
                    "各部材の両端部と中央（ξ=0・0.5・1.0）の値を kN・kN·m で表示します\
                     （その成分の最大値の 1% 未満は表示しません）。",
                );
        });
    }
    if mode == ViewMode::CheckRatio {
        fn checked_components(
            outcome: &sepika_design_jp::CheckOutcome,
        ) -> Option<&[sepika_design_jp::CheckComponent]> {
            match outcome {
                sepika_design_jp::CheckOutcome::Checked(cr) => Some(cr.components.as_slice()),
                sepika_design_jp::CheckOutcome::Skipped { .. } => None,
            }
        }
        let available_kinds = app
            .current_design_results()
            .map(|r| {
                check_ratio::available_check_kinds(
                    r.member_checks
                        .iter()
                        .flat_map(|m| m.positions.iter())
                        .filter_map(|p| checked_components(&p.outcome))
                        .chain(
                            r.joint_checks
                                .iter()
                                .filter_map(|j| checked_components(&j.outcome)),
                        )
                        .chain(
                            r.wall_checks
                                .iter()
                                .filter_map(|w| checked_components(&w.outcome)),
                        ),
                )
            })
            .unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.label("検定式:");
            ui.selectable_value(&mut check_ratio_filter, CheckRatioFilter::Max, "最大");
            for k in &available_kinds {
                ui.selectable_value(
                    &mut check_ratio_filter,
                    CheckRatioFilter::Kind(*k),
                    k.label(),
                );
            }
            ui.separator();
            ui.checkbox(&mut app.ui.view.check_ratio_markers, "位置別マーカー");
            ui.checkbox(&mut app.ui.view.check_ratio_label_all, "全部材に数値ラベル")
                .on_hover_text(
                    "既定では検定比 0.8 以上の部材にのみ数値ラベルを表示し、\
                     それ未満は色の濃淡（グラデーション）で余裕度を示します。",
                );
        });
    }
    if mode == ViewMode::Mode {
        let n_modes = app
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.modal.as_ref())
            .map(|m| m.period.len())
            .unwrap_or(0);
        if n_modes > 0 {
            ui.horizontal(|ui| {
                ui.label("モード:");
                let mut idx = mode_idx.min(n_modes - 1);
                ui.add(egui::Slider::new(&mut idx, 0..=n_modes - 1).text(""));
                mode_idx = idx;
                if let Some(t) = app
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|r| r.modal.as_ref())
                    .and_then(|m| m.period.get(idx))
                {
                    ui.label(format!("T={:.3} s", t));
                }
            });
        }
    }
    if mode == ViewMode::LumpedMode {
        let n_modes = app
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.lumped.as_ref())
            .map(|m| m.modal.period.len())
            .unwrap_or(0);
        if n_modes > 0 {
            ui.horizontal(|ui| {
                ui.label("モード:");
                let mut idx = mode_idx.min(n_modes - 1);
                ui.add(egui::Slider::new(&mut idx, 0..=n_modes - 1).text(""));
                mode_idx = idx;
                if let Some(t) = app
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|r| r.lumped.as_ref())
                    .and_then(|m| m.modal.period.get(idx))
                {
                    ui.label(format!("T={:.3} s", t));
                }
            });
        }
    }
    if lumped::is_lumped_view(mode) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut app.ui.view.lumped_show_frame, "骨組を重ねる");
        });
    }
    if mode == ViewMode::TimeHistory {
        if app.core.scoped.staleness.results_stale {
            ui.colored_label(
                theme::WARN_TEXT,
                "⚠ モデルが編集されています。解析を再実行してください\
                 （変形アニメーション・部材クリックは無効化しています）。",
            );
        } else if let Some(recording) = app
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.time_history.as_ref())
            .and_then(|t| t.recording.as_ref())
        {
            let n_frames = recording.frame_time.len();
            if n_frames > 0 {
                let duration = recording.frame_time.last().copied().unwrap_or(0.0);
                app.ui.scoped.th_frame = app.ui.scoped.th_frame.min(n_frames - 1);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(if app.ui.scoped.th_playing {
                            "⏸"
                        } else {
                            "▶"
                        })
                        .on_hover_text("再生 / 一時停止")
                        .clicked()
                    {
                        app.ui.scoped.th_playing = !app.ui.scoped.th_playing;
                    }
                    ui.label("速度:");
                    for s in [0.25_f32, 0.5, 1.0, 2.0] {
                        ui.selectable_value(&mut app.ui.view.th_speed, s, format!("×{s}"));
                    }
                    ui.separator();
                    let mut frame = app.ui.scoped.th_frame;
                    if ui
                        .add(egui::Slider::new(&mut frame, 0..=n_frames - 1).text(""))
                        .changed()
                    {
                        app.ui.scoped.th_frame = frame;
                        app.ui.scoped.th_play_time = recording.frame_time[frame];
                    }
                    let t = recording.frame_time[app.ui.scoped.th_frame];
                    ui.label(format!("t={:.2}s / {:.2}s", t, duration));
                });
                if app.ui.scoped.th_playing {
                    let dt = ui.input(|i| i.stable_dt);
                    app.ui.scoped.th_play_time = advance_play_time(
                        app.ui.scoped.th_play_time,
                        dt,
                        app.ui.view.th_speed,
                        duration,
                    );
                    app.ui.scoped.th_frame =
                        frame_at_time(&recording.frame_time, app.ui.scoped.th_play_time);
                    ui.ctx().request_repaint();
                }
            } else {
                ui.label("時刻歴の記録フレームがありません。");
            }
        } else {
            ui.label("時刻歴の詳細記録がありません（再解析すると記録されます）。");
        }
    }
    if mode == ViewMode::LumpedTimeHistory {
        if let Some(th) = app
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.lumped.as_ref())
            .and_then(|l| l.response.as_ref())
        {
            let n_frames = th.time.len();
            if n_frames > 0 {
                let duration = th.time.last().copied().unwrap_or(0.0);
                app.ui.scoped.th_frame = app.ui.scoped.th_frame.min(n_frames - 1);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(if app.ui.scoped.th_playing {
                            "⏸"
                        } else {
                            "▶"
                        })
                        .clicked()
                    {
                        app.ui.scoped.th_playing = !app.ui.scoped.th_playing;
                    }
                    ui.label("速度:");
                    for s in [0.25_f32, 0.5, 1.0, 2.0] {
                        ui.selectable_value(&mut app.ui.view.th_speed, s, format!("×{s}"));
                    }
                    let mut frame = app.ui.scoped.th_frame;
                    if ui
                        .add(egui::Slider::new(&mut frame, 0..=n_frames - 1).text(""))
                        .changed()
                    {
                        app.ui.scoped.th_frame = frame;
                        app.ui.scoped.th_play_time = th.time.get(frame).copied().unwrap_or(0.0);
                        app.ui.scoped.th_playing = false;
                    }
                    ui.label(format!("t={:.3} s", app.ui.scoped.th_play_time));
                });
                if app.ui.scoped.th_playing {
                    let dt = ui.input(|i| i.stable_dt);
                    app.ui.scoped.th_play_time = advance_play_time(
                        app.ui.scoped.th_play_time,
                        dt,
                        app.ui.view.th_speed,
                        duration,
                    );
                    app.ui.scoped.th_frame = frame_at_time(&th.time, app.ui.scoped.th_play_time);
                    ui.ctx().request_repaint();
                }
            }
        } else {
            ui.colored_label(theme::GRAY_600, "質点系時刻歴の結果がありません。");
        }
    }
    let show_deform_options = matches!(
        mode,
        ViewMode::Deformed
            | ViewMode::Mode
            | ViewMode::TimeHistory
            | ViewMode::LumpedMode
            | ViewMode::LumpedTimeHistory
    ) || (mode == ViewMode::Force && app.ui.view.overlay_deform);
    if show_deform_options {
        ui.horizontal(|ui| {
            ui.toggle_value(&mut app.ui.view.show_beam_interpolation, "内部たわみ")
                .on_hover_text(
                    "梁を内部たわみ（Hermite 曲線）で描き、床・二次部材も曲線に追従。\
                     OFF で梁を直線（弦）にし全体の変形を見る",
                );
            ui.separator();
            ui.label("変形倍率:");
            ui.add(
                egui::Slider::new(&mut app.ui.view.deform_scale_factor, 0.1..=10.0)
                    .logarithmic(true)
                    .text("×（自動比）"),
            );
            if ui.button("リセット").clicked() {
                app.ui.view.deform_scale_factor = 1.0;
            }
        });
    }

    app.ui.view.view_mode = mode;
    app.ui.scoped.view_mode_idx = mode_idx;
    app.ui.view.force_components = force_components;
    app.ui.view.cmq_component = cmq_component;
    app.ui.view.cmq_axes = cmq_axes;
    app.ui.view.check_ratio_filter = check_ratio_filter;
    app.ui.view.modeling_analysis = modeling_analysis;
}
