//! 時刻歴モード（[`super::ViewMode::TimeHistory`]）で部材をクリックした際に開く
//! 詳細ウィンドウ。
//!
//! - ブレース・ダンパー・節点ばねの履歴ループは軸力と軸方向相対変位を表示する。
//!   零長の節点ばねでは、全体座標系での成分別表示に切り替わる。
//!   梁・柱の履歴ループは材端モーメントと材端回転角を表示する。
//!   現在フレームの位置をループ上にマーカー表示し、フレームスライダーと連動する。
//! - 短期の簡易検定: 内力包絡や軸力極値が生じた確定時刻の内力を用いる。
//!   上下非対称組立H形鋼のS柱は全確定時刻の断面単独照査を表示する。
//!
//! ヒンジ詳細ウィンドウ（[`super::hinge`]）と同じ構成方針（`egui::Window` +
//! `egui_plot`）を踏襲するが、対象データ（時刻歴の全フレーム記録）が異なるため
//! 実装は独立させている。
//!
//! 検定の組み立ては `app::actions::run_design_check`（断面・材料に応じた
//! RC/Steel/SRC/CFT の `DesignCheck` 実装への振り分け）と同じ考え方だが、
//! 当該関数は `app` モジュール内の非公開関数（`is_steel` 等）に
//! 依存しビューア側からは呼べないため、必要な部分だけ本ファイルへ複製する。
//! 長期静的結果がある場合は地震時 QD / 柱メカニズムを部分配線する。
//! 長期静的結果に対応するQ0（単純梁せん断）も用いる。
//! 鋼継手欠損の断面性能への反映・一本部材グループ合成は対象外（概算検定）。

use crate::app::App;
use crate::theme;
use sepika_core::geom::vec3::dist as member_len3;
use sepika_core::ids::ElemId;
use sepika_core::model::{ElementData, ElementKind, Model};
use sepika_core::section_shape::SectionShape;
use sepika_core::structure_kind::StructureKind;
use sepika_core::units::to_display::{force_kn, moment_kn_m};
use sepika_design_jp::member_design_check::{concrete_member_torsion_reason, strength_outcomes};
use sepika_design_jp::{
    CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind, QdMethod,
};
use sepika_element::frame::beam::MemberForces;
use sepika_element::transform::LocalFrame;
use sepika_solver::dynamic::timehistory::ThRecording;
use sepika_solver::dynamic::timehistory::{
    uses_signed_bending_peaks, AxialForcePeaks, ForcePeakSnapshot,
};

/// ループ表示の分類（要素種別による）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopKind {
    /// 軸力系（ブレース・ダンパー・節点ばね）: N - δ
    Axial,
    /// 梁・柱（曲げ材）: M - θ
    Flexural,
    /// 上記以外（壁・シェル・パネルゾーン等）: ループ表示非対応
    Unsupported,
}

fn loop_kind_of(kind: &ElementKind) -> LoopKind {
    match kind {
        ElementKind::Brace { .. } | ElementKind::Damper | ElementKind::NodalSpring => {
            LoopKind::Axial
        }
        ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring => LoopKind::Flexural,
        ElementKind::Wall | ElementKind::Shell | ElementKind::PanelZone => LoopKind::Unsupported,
    }
}

/// 要素種別の日本語表示名。
fn kind_label(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::Beam => "梁・柱",
        ElementKind::Fiber => "ファイバー梁",
        ElementKind::MultiSpring => "マルチスプリング梁",
        ElementKind::Wall => "耐震壁",
        ElementKind::PanelZone => "パネルゾーン",
        ElementKind::Brace { .. } => "ブレース",
        ElementKind::NodalSpring => "節点ばね",
        ElementKind::Damper => "ダンパー",
        ElementKind::Shell => "シェル",
    }
}

/// 軸力系要素の軸方向相対変位 δ [mm]（引張を正）。
///
/// 両端節点の変位差（並進成分のみ）を、未変形材軸方向（i→j の単位ベクトル）へ
/// 射影する。部材長が 1e-9 mm 未満の場合は 0 を返す。
pub(super) fn axial_relative_disp(
    p_i: [f64; 3],
    p_j: [f64; 3],
    d_i: [f64; 6],
    d_j: [f64; 6],
) -> f64 {
    let len = member_len3(p_i, p_j);
    if len < 1e-9 {
        return 0.0;
    }
    let ex = [
        (p_j[0] - p_i[0]) / len,
        (p_j[1] - p_i[1]) / len,
        (p_j[2] - p_i[2]) / len,
    ];
    let du = [d_j[0] - d_i[0], d_j[1] - d_i[1], d_j[2] - d_i[2]];
    du[0] * ex[0] + du[1] * ex[1] + du[2] * ex[2]
}

/// 3 次元ベクトルの内積。
use sepika_core::geom::vec3::dot as dot3;

/// 軸力系要素の荷重変形ループで表示する成分。
/// `MemberForces::at` の力成分の並び `[N,Qy,Qz,Mx,My,Mz]` の先頭 3 つに対応する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxialComponent {
    /// 軸方向（局所 x）。`MemberForces` の N。
    Axial,
    /// せん断 y（局所 y）。`MemberForces` の Qy。
    ShearY,
    /// せん断 z（局所 z）。`MemberForces` の Qz。
    ShearZ,
}

impl AxialComponent {
    /// `MemberForces::at` の力配列 `[N,Qy,Qz,Mx,My,Mz]` の添字（0..3）。
    /// 零長の節点ばねでは全体座標系の X・Y・Z 成分に対応する。
    fn axis_index(self) -> usize {
        match self {
            Self::Axial => 0,
            Self::ShearY => 1,
            Self::ShearZ => 2,
        }
    }

    /// プロットの軸ラベル `(横軸=δ, 縦軸=力)`。
    fn axis_labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Axial => ("δ [mm]", "N [kN]"),
            Self::ShearY => ("δy [mm]", "Qy [kN]"),
            Self::ShearZ => ("δz [mm]", "Qz [kN]"),
        }
    }

    /// 成分切替 UI のラベル。
    fn selector_label(self) -> &'static str {
        match self {
            Self::Axial => "軸(N-δ)",
            Self::ShearY => "せん断y(Qy-δy)",
            Self::ShearZ => "せん断z(Qz-δz)",
        }
    }
}

/// 梁・柱の材端回転角（弦からのたわみ角）[rad] を返す `(ry_i, rz_i, ry_j, rz_j)`。
///
/// `sepika-solver` の増分解析（`nonlinear::pushover::member_response`）と同じ
/// 算定式: 局所たわみ v（ey 方向）・w（ez 方向）の弦回転を節点回転の局所成分から
/// 差し引く（x-y 面: θz-(v_j-v_i)/L、x-z 面: θy+(w_j-w_i)/L）。剛体回転（層間変形
/// による部材全体の傾き）を除いた「材端の回転」が M-θ 履歴として意味を持つため。
pub(super) fn beam_end_rotations(
    p_i: [f64; 3],
    p_j: [f64; 3],
    ref_vector: [f64; 3],
    d_i: [f64; 6],
    d_j: [f64; 6],
) -> Option<(f64, f64, f64, f64)> {
    let length = member_len3(p_i, p_j);
    if length < 1e-9 {
        return Some((0.0, 0.0, 0.0, 0.0));
    }
    let frame = LocalFrame::from_nodes(p_i, p_j, ref_vector).ok()?;
    let ey = frame.rot[1];
    let ez = frame.rot[2];
    let u_i = [d_i[0], d_i[1], d_i[2]];
    let u_j = [d_j[0], d_j[1], d_j[2]];
    let r_i = [d_i[3], d_i[4], d_i[5]];
    let r_j = [d_j[3], d_j[4], d_j[5]];
    let chord_v = (dot3(u_j, ey) - dot3(u_i, ey)) / length;
    let chord_w = (dot3(u_j, ez) - dot3(u_i, ez)) / length;
    let ry_i = dot3(r_i, ey) + chord_w;
    let rz_i = dot3(r_i, ez) - chord_v;
    let ry_j = dot3(r_j, ey) + chord_w;
    let rz_j = dot3(r_j, ez) - chord_v;
    Some((ry_i, rz_i, ry_j, rz_j))
}

/// 当該要素の内力が 1 フレームでも記録されているか（要素種別によっては
/// `state_member_forces`/`recover_forces` が `None` を返し続け、記録が存在しない）。
fn elem_has_force_recording(rec: &ThRecording, elem_idx: usize) -> bool {
    rec.member_forces
        .iter()
        .any(|frame| frame.get(elem_idx).is_some_and(|o| o.is_some()))
}

/// 断面検定の対象になる部材種別か（ダンパー・節点ばね要素、
/// 耐震壁・シェル・パネルゾーンは対象外）。
fn design_member_kind(elem: &ElementData, model: &Model) -> Option<MemberKind> {
    match elem.kind {
        ElementKind::Brace { .. } => Some(MemberKind::Brace),
        ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring => {
            Some(MemberKind::of_element(elem, model))
        }
        ElementKind::Wall => None,
        _ => None,
    }
}

/// 時刻歴詳細ウィンドウ（`app.ui.scoped.th_detail_elem` があれば表示）。
pub(crate) fn show_th_detail_window_at(ui: &egui::Ui, app: &mut App, view: &super::ViewportState) {
    let Some(elem_id) = app.ui.scoped.th_detail_elem else {
        return;
    };
    let mut open = true;
    let window = egui::Window::new(format!("時刻歴詳細: 部材 #{}", elem_id.0))
        .id(egui::Id::new("th_detail_window"))
        .resizable(true)
        .collapsible(true)
        .default_size([460.0, 640.0])
        .open(&mut open)
        .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                draw_th_detail_content_at(ui, app, elem_id, view.th_frame, view);
            });
        });
    if let Some(window) = window {
        crate::theme::window_outline(&window.response);
    }
    if !open {
        app.ui.scoped.th_detail_elem = None;
    }
}

/// `app.ui.view.th_detail_axis_z` は一時的にローカル変数へ取り出し、`ThRecording` の
/// 借用（`app.core.scoped.results` 由来。フレーム数×要素数の内力を持つため複製すると重い）と
/// `app: &mut App` の同時使用を避け、最後にまとめて書き戻す
/// （§実装内容2 のループ本体は `app.core.model`／`app.core.scoped.results` の共有参照のみで完結する）。
fn draw_th_detail_content_at(
    ui: &mut egui::Ui,
    app: &mut App,
    elem_id: ElemId,
    frame: usize,
    view: &super::ViewportState,
) {
    if app.core.scoped.staleness.results_stale {
        ui.colored_label(
            theme::WARN_TEXT,
            "⚠ モデルが編集されています。解析を再実行してください\
             （プロット・検定は前回解析時点のままのため非表示にしています）。",
        );
        return;
    }
    let Some(th_result) = view.spatial_result(app) else {
        ui.colored_label(theme::GRAY_600, "時刻歴の詳細記録がありません。");
        return;
    };
    let Some(recording) = th_result.recording.as_ref() else {
        ui.colored_label(
            theme::GRAY_600,
            "時刻歴の詳細記録がないため、内力履歴と簡易検定を表示できません。確認するには解析を再実行してください。",
        );
        return;
    };
    let th_nonlinear = th_result.nonlinear;
    let th_applied_long_term = th_result.applied_long_term;
    let display = super::wall_expanded_view_model(&app.core.model);
    let Some(elem_idx) = display.elements.iter().position(|e| e.id == elem_id) else {
        ui.colored_label(theme::GRAY_600, "この部材はモデルから削除されています。");
        return;
    };
    let elem = &display.elements[elem_idx];
    let Some(axis) = super::member_axis_endpoints(elem, display.as_ref()) else {
        ui.colored_label(theme::GRAY_600, "材軸端点を取得できません。");
        return;
    };
    let (p_i, p_j) = (axis.p_i, axis.p_j);
    let (n0, n1) = (axis.n0, axis.n1);

    ui.label(format!("部材 #{}（{}）", elem_id.0, kind_label(&elem.kind)));
    let n_frames = recording.frame_time.len();
    let frame = frame.min(n_frames.saturating_sub(1));
    if let Some(t) = recording.frame_time.get(frame) {
        ui.label(format!(
            "現在フレーム: {frame}/{} (t={:.3}s)",
            n_frames.saturating_sub(1),
            t
        ));
    }
    ui.separator();

    ui.strong("荷重変形関係の履歴ループ");
    let mut axis_z = app.ui.view.th_detail_axis_z;
    let mut axial_component = match app.ui.scoped.th_detail_axial_component {
        Some((id, c)) if id == elem_id => c,
        _ => AxialComponent::Axial,
    };
    match loop_kind_of(&elem.kind) {
        LoopKind::Axial => draw_axial_loop(
            ui,
            recording,
            elem_id,
            elem_idx,
            n0,
            n1,
            p_i,
            p_j,
            frame,
            &mut axial_component,
        ),
        LoopKind::Flexural => draw_flexural_loop(
            ui,
            recording,
            elem_id,
            elem_idx,
            n0,
            n1,
            p_i,
            p_j,
            elem.local_axis.ref_vector,
            frame,
            &mut axis_z,
        ),
        LoopKind::Unsupported if matches!(elem.kind, ElementKind::Wall) => {
            if elem_has_force_recording(recording, elem_idx) {
                draw_flexural_loop(
                    ui,
                    recording,
                    elem_id,
                    elem_idx,
                    n0,
                    n1,
                    p_i,
                    p_j,
                    elem.local_axis.ref_vector,
                    frame,
                    &mut axis_z,
                );
            } else {
                ui.colored_label(theme::GRAY_600, "この耐震壁の内力は記録されていません。");
            }
        }
        LoopKind::Unsupported => {
            ui.colored_label(
                theme::GRAY_600,
                "この要素種別はループ表示に対応していません。",
            );
        }
    }

    ui.add_space(6.0);
    ui.separator();
    draw_peak_check(
        ui,
        app,
        elem,
        elem_idx,
        recording,
        th_nonlinear,
        th_applied_long_term,
        || damage_control_gravity_issue(app, view, &display, th_result),
        || {
            if !view.spatial_case_id(app).is_some_and(|id| {
                app.require_result_input(crate::app::ResultInputKey::TimeHistory(id))
                    .is_ok()
            }) {
                return Err("時刻歴結果の生成入力を確認できないため、初期載荷を照合できません");
            }
            matching_gravity_member_forces(&display, app, |name| {
                app.require_result_input(crate::app::ResultInputKey::Combo(name.to_owned()))
                    .is_ok()
            })
        },
        &axis,
    );
    app.ui.view.th_detail_axis_z = axis_z;
    app.ui.scoped.th_detail_axial_component = Some((elem_id, axial_component));
}

/// 軸力系要素（ブレース・ダンパー・節点ばね）の荷重変形ループ。
/// 零長の節点ばねは節点変位と内力の全体座標成分を表示する。
#[allow(clippy::too_many_arguments)]
fn draw_axial_loop(
    ui: &mut egui::Ui,
    rec: &ThRecording,
    elem_id: ElemId,
    elem_idx: usize,
    n0: usize,
    n1: usize,
    p_i: [f64; 3],
    p_j: [f64; 3],
    cur_frame: usize,
    component: &mut AxialComponent,
) {
    if !elem_has_force_recording(rec, elem_idx) {
        ui.colored_label(theme::GRAY_600, "この部材の内力は記録されていません。");
        return;
    }
    let zero_length = member_len3(p_i, p_j) < 1e-9;
    if zero_length {
        ui.horizontal(|ui| {
            ui.label("成分:");
            for c in [
                AxialComponent::ShearY,
                AxialComponent::ShearZ,
                AxialComponent::Axial,
            ] {
                ui.selectable_value(component, c, c.selector_label());
            }
        });
    }
    let component = if zero_length {
        *component
    } else {
        AxialComponent::Axial
    };

    let point = |f: usize| -> Option<[f64; 2]> {
        axial_point(rec, elem_idx, n0, n1, p_i, p_j, component, f)
    };
    let series: Vec<[f64; 2]> = (0..rec.frame_time.len()).filter_map(point).collect();
    let current = point(cur_frame);
    let (xlabel, ylabel) = component.axis_labels();

    egui_plot::Plot::new(format!("th_axial_{}", elem_id.0))
        .x_axis_label(xlabel)
        .y_axis_label(ylabel)
        .height(220.0)
        .show(ui, |plot_ui| {
            plot_ui.line(
                egui_plot::Line::new("荷重変形関係", egui_plot::PlotPoints::from(series))
                    .color(theme::DATA_BLUE)
                    .width(1.5_f32),
            );
            if let Some(p) = current {
                plot_ui.points(
                    egui_plot::Points::new("現在フレーム", egui_plot::PlotPoints::from(vec![p]))
                        .color(theme::PARETO_RED)
                        .radius(5.0_f32)
                        .shape(egui_plot::MarkerShape::Circle),
                );
            }
        });
}

/// フレーム `f` の変位 [mm] と力 [kN] を返す。内力か変位が欠けていれば `None`。
#[allow(clippy::too_many_arguments)]
fn axial_point(
    rec: &ThRecording,
    elem_idx: usize,
    n0: usize,
    n1: usize,
    p_i: [f64; 3],
    p_j: [f64; 3],
    component: AxialComponent,
    f: usize,
) -> Option<[f64; 2]> {
    let mf = rec.member_forces.get(f)?.get(elem_idx)?.as_ref()?;
    let force = force_kn(mf.at.first()?.1[component.axis_index()]);
    let disp_frame = rec.node_disp.get(f)?;
    let d_i = *disp_frame.get(n0)?;
    let d_j = *disp_frame.get(n1)?;
    let delta = if member_len3(p_i, p_j) < 1e-9 {
        let axis = component.axis_index();
        d_j[axis] - d_i[axis]
    } else {
        axial_relative_disp(p_i, p_j, d_i, d_j)
    };
    Some([delta, force])
}

/// 梁・柱の M-θ ループ（i端・j端、強軸/弱軸切替可能）。
///
/// `axis_z`（表示中の曲げ軸）は呼び出し側（`App::th_detail_axis_z`）が保持する
/// 状態をローカル変数として受け渡す。`ThRecording` の借用（`app.core.scoped.results` 由来）と
/// `app: &mut App` の同時使用を避けるため、`App` そのものは受け取らない。
#[allow(clippy::too_many_arguments)]
fn draw_flexural_loop(
    ui: &mut egui::Ui,
    rec: &ThRecording,
    elem_id: ElemId,
    elem_idx: usize,
    n0: usize,
    n1: usize,
    p_i: [f64; 3],
    p_j: [f64; 3],
    ref_vector: [f64; 3],
    cur_frame: usize,
    axis_z: &mut bool,
) {
    if !elem_has_force_recording(rec, elem_idx) {
        ui.colored_label(theme::GRAY_600, "この部材の内力は記録されていません。");
        return;
    }
    ui.horizontal(|ui| {
        ui.label("成分:");
        ui.selectable_value(axis_z, true, "強軸(Mz-θz)");
        ui.selectable_value(axis_z, false, "弱軸(My-θy)");
    });
    let axis_z = *axis_z;

    let point = |f: usize| -> Option<([f64; 2], [f64; 2])> {
        flexural_points(rec, elem_idx, n0, n1, p_i, p_j, ref_vector, axis_z, f)
    };
    let mut i_pts = Vec::with_capacity(rec.frame_time.len());
    let mut j_pts = Vec::with_capacity(rec.frame_time.len());
    for f in 0..rec.frame_time.len() {
        if let Some((pi, pj)) = point(f) {
            i_pts.push(pi);
            j_pts.push(pj);
        }
    }
    let current = point(cur_frame);
    let axis_label = if axis_z { "z" } else { "y" };

    egui_plot::Plot::new(format!("th_flex_{}", elem_id.0))
        .x_axis_label(format!("θ{axis_label} [rad]"))
        .y_axis_label(format!("M{axis_label} [kN·m]"))
        .legend(egui_plot::Legend::default())
        .height(240.0)
        .show(ui, |plot_ui| {
            plot_ui.line(
                egui_plot::Line::new("i端", egui_plot::PlotPoints::from(i_pts))
                    .color(theme::DATA_BLUE)
                    .width(1.5_f32),
            );
            plot_ui.line(
                egui_plot::Line::new("j端", egui_plot::PlotPoints::from(j_pts))
                    .color(theme::PARETO_RED)
                    .width(1.5_f32),
            );
            if let Some((pi, pj)) = current {
                plot_ui.points(
                    egui_plot::Points::new("i端(現在)", egui_plot::PlotPoints::from(vec![pi]))
                        .color(theme::DATA_BLUE)
                        .radius(5.0_f32)
                        .shape(egui_plot::MarkerShape::Circle),
                );
                plot_ui.points(
                    egui_plot::Points::new("j端(現在)", egui_plot::PlotPoints::from(vec![pj]))
                        .color(theme::PARETO_RED)
                        .radius(5.0_f32)
                        .shape(egui_plot::MarkerShape::Circle),
                );
            }
        });
}

/// フレーム `f` の i端・j端 (θ[rad], M[kN·m]) を求める（強軸/弱軸は `axis_z` で選択）。
#[allow(clippy::too_many_arguments)]
fn flexural_points(
    rec: &ThRecording,
    elem_idx: usize,
    n0: usize,
    n1: usize,
    p_i: [f64; 3],
    p_j: [f64; 3],
    ref_vector: [f64; 3],
    axis_z: bool,
    f: usize,
) -> Option<([f64; 2], [f64; 2])> {
    let mf = rec.member_forces.get(f)?.get(elem_idx)?.as_ref()?;
    let (mi, mj) = end_moments(mf, axis_z)?;
    let disp_frame = rec.node_disp.get(f)?;
    let d_i = *disp_frame.get(n0)?;
    let d_j = *disp_frame.get(n1)?;
    let (ry_i, rz_i, ry_j, rz_j) = beam_end_rotations(p_i, p_j, ref_vector, d_i, d_j)?;
    let (theta_i, theta_j) = if axis_z { (rz_i, rz_j) } else { (ry_i, ry_j) };
    Some(([theta_i, moment_kn_m(mi)], [theta_j, moment_kn_m(mj)]))
}

/// `MemberForces::at` から i端（最小 pos）・j端（最大 pos）の材端モーメント
/// M[N·mm]（強軸Mz/弱軸My、「節点端モーメント」規約）を取り出す（高-1）。
///
/// `MemberForces::at` の値そのものは「断面内力」規約であり、i端（ξ<0.5）側は
/// `sepika_element::frame::beam::forces::member_forces_from_end_forces` が
/// 節点モーメントの符号を反転して格納している
/// （切断法で連続な内力場にするための反転。j端は反転なし）。
/// 一方、`beam_end_rotations` が返す θ（弦からのたわみ角）はヒンジ詳細
/// （`viewer::hinge`）・増分解析の応答抽出（`nonlinear::pushover::member_response`）
/// と同じ「節点端モーメント」規約（反転なし）を前提に算定している。
/// このため i端だけ符号を再反転し、θ と対にできる規約へ揃える
/// （反転しないと i端の M-θ 勾配の符号がヒンジ詳細と逆転する）。
fn end_moments(mf: &MemberForces, axis_z: bool) -> Option<(f64, f64)> {
    let i_at = mf
        .at
        .iter()
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))?;
    let j_at = mf
        .at
        .iter()
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))?;
    let idx = if axis_z { 5 } else { 4 };
    Some((-i_at.1[idx], j_at.1[idx]))
}

const UNVERIFIED_DAMAGE_CONTROL_GRAVITY: &str =
    "RC損傷制御用せん断に必要な長期荷重を含む応答を確認できません";

fn damage_control_gravity_issue(
    app: &App,
    view: &super::ViewportState,
    analyzed_model: &sepika_core::model::Model,
    result: &sepika_solver::dynamic::timehistory::ResponseResult,
) -> Option<&'static str> {
    use sepika_core::model::LoadCaseKind;
    if !result.nonlinear
        || !result.applied_long_term
        || !view.spatial_case_id(app).is_some_and(|id| {
            app.require_result_input(crate::app::ResultInputKey::TimeHistory(id))
                .is_ok()
        })
    {
        return Some(UNVERIFIED_DAMAGE_CONTROL_GRAVITY);
    }
    let mut loaded_kinds = analyzed_model
        .load_cases
        .iter()
        .filter(|case| {
            case.kind.is_long_term() && (!case.nodal.is_empty() || !case.member.is_empty())
        })
        .map(|case| case.kind)
        .peekable();
    if loaded_kinds.peek().is_none() {
        Some("RC損傷制御用せん断に必要な固定・架構用積載荷重の初期載荷がありません")
    } else if loaded_kinds.any(|kind| !matches!(kind, LoadCaseKind::Dead | LoadCaseKind::Live)) {
        Some(
            "初期載荷に固定・架構用積載以外（積雪または種別未指定）の荷重を含むため、RC損傷制御用せん断は未算定です",
        )
    } else if ![LoadCaseKind::Dead, LoadCaseKind::Live]
        .into_iter()
        .all(|kind| {
            analyzed_model
                .load_cases
                .iter()
                .any(|case| case.kind == kind)
        })
    {
        Some("RC損傷制御用せん断には固定・架構用積載の両荷重ケースを明示してください")
    } else {
        None
    }
}

fn matching_gravity_member_forces<'a>(
    analyzed_model: &sepika_core::model::Model,
    app: &'a App,
    is_current: impl Fn(&str) -> bool,
) -> Result<&'a [(ElemId, MemberForces)], &'static str> {
    use sepika_core::load_combo::{effective_combination_terms, is_gravity_combination};
    use sepika_core::model::LoadCaseKind;

    let has_load =
        |case: &sepika_core::model::LoadCase| !case.nodal.is_empty() || !case.member.is_empty();
    let long_term = analyzed_model
        .load_cases
        .iter()
        .filter(|case| case.kind.is_long_term());
    if long_term.clone().any(|case| {
        analyzed_model
            .load_cases
            .iter()
            .filter(|other| other.id == case.id)
            .nth(1)
            .is_some()
    }) {
        return Err(
            "初期載荷に関わる荷重ケースIDが重複しているため、静的組合せと荷重条件を照合できません",
        );
    }
    let initial: Vec<_> = long_term.filter(|case| has_load(case)).collect();
    if initial.is_empty() {
        return Err("長期荷重を初期載荷する設定ですが、初期載荷の荷重項目がありません");
    }
    if initial
        .iter()
        .any(|case| !matches!(case.kind, LoadCaseKind::Dead | LoadCaseKind::Live))
    {
        return Err(
            "初期載荷に積雪または種別未指定の荷重を含むため、常時重力組合せと荷重条件が一致しません",
        );
    }

    let model = &app.core.model;
    let matches_initial = |name: &str| {
        if !is_gravity_combination(name, model) {
            return false;
        }
        let Some(combo) = model.combinations.iter().find(|combo| combo.name == name) else {
            return false;
        };
        effective_combination_terms(combo, &model.load_cases).is_ok_and(|terms| {
            initial.iter().all(|case| terms.contains(&(case.id, 1.0)))
                && terms.iter().all(|&(id, factor)| {
                    factor == 0.0
                        || initial.iter().any(|case| case.id == id && factor == 1.0)
                        || analyzed_model
                            .load_cases
                            .iter()
                            .any(|case| case.id == id && !has_load(case))
                })
        })
    };
    let Some(results) = &app.core.scoped.results else {
        return Err("初期載荷と荷重ケース・係数が一致する保存済みの常時重力組合せがありません");
    };
    let mut reason = "初期載荷と荷重ケース・係数が一致する保存済みの常時重力組合せがありません";
    for (name, result) in results
        .combos
        .iter()
        .filter(|(name, _)| matches_initial(name))
    {
        if is_current(name) {
            return Ok(&result.member_forces);
        }
        reason = "初期載荷と荷重条件が一致する常時重力組合せの生成入力を確認できません";
    }
    Err(reason)
}

#[allow(clippy::too_many_arguments)]
fn draw_peak_check<'a>(
    ui: &mut egui::Ui,
    app: &App,
    elem: &ElementData,
    elem_idx: usize,
    rec: &ThRecording,
    th_nonlinear: bool,
    th_applied_long_term: bool,
    damage_control_gravity_issue: impl FnOnce() -> Option<&'static str>,
    gravity_reference: impl FnOnce() -> Result<&'a [(ElemId, MemberForces)], &'static str>,
    axis: &super::MemberAxisEndpoints,
) {
    let model = &app.core.model;
    let signed_bending = uses_signed_bending_peaks(model, elem);
    ui.strong(if signed_bending {
        "時刻歴の簡易検定（短期）"
    } else {
        "最大応力に対する検定（短期）"
    });
    ui.label(
        egui::RichText::new(
            "簡易検定です。S部材とSRC柱の座屈長さは軸別の直接入力を優先し、空欄では節点間距離の柱1.2倍、梁・ブレース1.0倍とします。CFT柱は両軸の直接入力が必要です。\
             継手・スカラップ欠損による断面性能の低減や一本部材合成は行いません。\
             上下非対称組立H形鋼柱の圧縮時・軸力ゼロ時の強軸曲げは、欠損を指定した位置では応力度が未算定です。\
             有限長では座屈が未算定となり、独立に確定できる不適合は表示します。",
        )
        .size(11.0)
        .color(theme::GRAY_600),
    );

    if signed_bending {
        draw_asymmetric_column_section_history(ui, rec, elem_idx);
    }
    let Some(peak) = rec
        .peak_member_forces
        .get(elem_idx)
        .and_then(|o| o.as_ref())
    else {
        ui.colored_label(theme::GRAY_600, "内力の記録がないため検定対象外です。");
        return;
    };
    let Some(kind) = design_member_kind(elem, &app.core.model) else {
        ui.colored_label(
            theme::GRAY_600,
            "この要素種別は断面検定の対象外です（ダンパー・節点ばね要素等）。",
        );
        return;
    };
    let Some(sec) = elem
        .section
        .and_then(|sid| app.core.model.sections.get(sid.index()))
    else {
        ui.colored_label(theme::GRAY_600, "断面が未設定のため検定対象外です。");
        return;
    };
    let Some(mat) = app.core.model.element_material(elem) else {
        ui.colored_label(
            theme::GRAY_600,
            "断面に材料が割り当てられていないため検定対象外です。",
        );
        return;
    };
    let axial_peaks = rec.axial_force_peaks.get(elem_idx);
    let moment_peaks = rec.strong_axis_moment_peaks.get(&elem_idx);
    if signed_bending {
        ui.label("軸力と正負の強軸曲げの極値が生じた確定時刻の検定");
        if axial_peaks.is_none() || moment_peaks.is_none() {
            ui.colored_label(
                theme::WARN_TEXT,
                "軸力または正負の強軸曲げの極値記録が不足しています。確認するには解析を再実行してください。",
            );
        } else if axial_peaks.is_some_and(|peaks| peaks.snapshots.is_empty())
            && moment_peaks.is_some_and(|peaks| peaks.snapshots.is_empty())
        {
            ui.label("非零の軸力・強軸曲げ極値がないため、照査対象の時刻がありません。");
        }
    } else {
        ui.label(
            egui::RichText::new(
                "内力包絡では、異なる時刻の成分の組合せにより過大・過小評価が生じる場合があります。\
                 引張側・圧縮側は、各位置でそれぞれの軸力の絶対値が最大となった確定時刻の内力を使います。\
                 軸力が極値に達していない時刻でも、より厳しい応力の組合せが生じる場合があります。",
            )
            .size(11.0)
            .color(theme::GRAY_600),
        );
    }
    let structure = sepika_core::structure_kind::structure_kind_of(Some(sec), Some(mat.category));
    if structure == StructureKind::Src {
        ui.label(if kind == MemberKind::Column {
            "SRC柱の短期照査は軸力・一軸曲げと、分担を確認できるせん断を表示します。"
        } else {
            "SRC梁の短期照査は強軸曲げと、分担を確認できる強軸せん断を表示します。"
        });
    }
    let steel_column = structure == StructureKind::S && kind == MemberKind::Column;
    let symmetric_h_column = steel_column && sepika_design_jp::steel::is_symmetric_h_section(sec);
    let torsion_reason =
        |forces: &MemberForcesAt| concrete_member_torsion_reason(structure, forces);
    let attr = model.steel_design_attrs.iter().find(|a| a.elem == elem.id);
    let length = axis.length;
    let face_sum = elem.rigid_zone.face_i_or_zero() + elem.rigid_zone.face_j_or_zero();
    let clear_span = if length - face_sum > 0.0 {
        length - face_sum
    } else {
        length
    };
    let wire_qd = th_nonlinear && th_applied_long_term && !signed_bending;
    let rc_column = structure == StructureKind::Rc
        && matches!(
            sec.shape,
            Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. })
        );
    let rc_beam = structure == StructureKind::Rc
        && matches!(sec.shape, Some(SectionShape::RcBeamRect { .. }));
    let rc_damage_control = (rc_beam || rc_column) && app.core.analysis_cfg.rc_damage_control;
    let wire_qd = wire_qd && !rc_damage_control;
    let gravity_issue = rc_damage_control
        .then(damage_control_gravity_issue)
        .flatten();
    let qd_method = app.core.analysis_cfg.qd_method;
    let wire_qd = wire_qd
        && matches!(
            structure,
            StructureKind::Rc | StructureKind::Src | StructureKind::Cft
        );
    let gravity_reference = wire_qd.then(gravity_reference);
    let gravity_reference_issue = gravity_reference.and_then(Result::err);
    if th_applied_long_term && (gravity_issue.is_some() || gravity_reference_issue.is_some()) {
        ui.label("長期荷重を初期載荷する設定です。");
    } else {
        draw_long_term_note(ui, th_nonlinear, th_applied_long_term);
    }
    if !signed_bending && !rc_damage_control {
        ui.label(
            egui::RichText::new(
                "非線形時刻歴で長期を重ねた解析のときのみ、地震時 QD / 柱メカニズムを配線します\
                 （線形時刻歴や長期未重畳では、包絡ピークと静的長期の合成が危険側になり得るため無効です）。",
            )
            .size(11.0)
            .color(theme::GRAY_600),
        );
    }
    if rc_damage_control {
        ui.label("RCせん断の損傷制御（15.4式）は長期荷重を含む確定ステップに限ります。抽出時刻の確認であり、全時刻の適否は未判定です。");
        if let Some(reason) = gravity_issue {
            ui.colored_label(theme::WARN_TEXT, reason);
        } else if axial_peaks.is_none_or(|peaks| peaks.snapshots.is_empty()) {
            ui.colored_label(theme::WARN_TEXT, "RCせん断の損傷制御は未算定です（せん断を照査できる確定ステップの抽出記録がありません）。");
        }
    }
    let rebar_material = model.element_rebar_material(elem);
    let wrong_main_rebar = rebar_material
        .is_some_and(|material| material.category != sepika_core::model::MaterialCategory::Rebar);
    let shear_rebar_material = model.element_shear_rebar_material(elem);
    let wrong_shear_rebar = shear_rebar_material
        .is_some_and(|material| material.category != sepika_core::model::MaterialCategory::Rebar);
    let long_mf = gravity_reference.and_then(Result::ok);
    let rc_beam_adjacency =
        (wire_qd && rc_beam && kind != MemberKind::Column && qd_method != QdMethod::Qd2)
            .then(|| sepika_core::adjacency::NodeAdjacency::primary_frame(model));
    let rc_beam_qd1 = rc_beam_adjacency
        .as_ref()
        .map(|adjacency| sepika_design_jp::rc::beam_qd1_geometry(model, adjacency, elem));
    let (q_simple, q0_issue) = if rc_beam_qd1.is_some() {
        match sepika_job::design_q0::rc_beam_q0_for_gravity_cases(model, elem) {
            Ok(q0) => (q0, None),
            Err(reason) => (
                None,
                Some(format!("QD1用の部材荷重由来Q0を算定できません（{reason}）")),
            ),
        }
    } else {
        (None, None)
    };
    use sepika_design_jp::design_position::{
        qd1_saved_station_issue, rc_beam_qd1_positions, same_qd1_station,
    };
    let qd1_positions = rc_beam_qd1
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|geometry| rc_beam_qd1_positions(model, elem, *geometry));
    let slab_moments_nmm = rc_beam_adjacency
        .as_ref()
        .filter(|_| matches!(rc_beam_qd1, Some(Ok(_))))
        .and_then(|adjacency| {
            sepika_design_jp::rc::beam_qd1_slab_end_moments(model, adjacency, elem)
        });
    let long_at = long_mf.and_then(|list| list.iter().find(|(id, _)| *id == elem.id));
    let missing_gravity = wire_qd && long_at.is_none();
    let seismic_qd = (long_at.is_some()
        || (wire_qd && (rc_beam || rc_column || structure == StructureKind::Cft)))
        .then(|| sepika_design_jp::SeismicQd {
            long_at: long_at.map_or_else(Vec::new, |(_, mf)| match &qd1_positions {
                Some(positions) => mf
                    .at
                    .iter()
                    .filter(|(pos, _)| {
                        positions.iter().any(|expected| {
                            same_qd1_station(*expected, *pos)
                                && qd1_saved_station_issue(*expected, &mf.at).is_none()
                        })
                    })
                    .copied()
                    .collect(),
                None => mf.at.clone(),
            }),
            n_factor: 1.5,
            n_mechanism: 1.0,
            q_simple,
            clear_length: match &rc_beam_qd1 {
                Some(Ok(geometry)) => geometry.clear_span_mm,
                Some(Err(_)) => f64::NAN,
                None => clear_span,
            },
            method: qd_method,
        });
    let frame_buckling = structure == StructureKind::S
        || (structure == StructureKind::Src && kind == MemberKind::Column);
    let buckling_lengths = match structure {
        StructureKind::Cft => {
            sepika_design_jp::cft::direct_column_buckling_lengths(elem.id, attr).map(Some)
        }
        _ if frame_buckling => Ok(Some(sepika_design_jp::steel::frame_buckling_lengths(
            model, elem, kind, attr,
        ))),
        _ => Ok(None),
    };
    let (lk_y, lk_z) = match buckling_lengths {
        Ok(Some((strong, weak))) => (Some(strong), Some(weak)),
        Ok(None) => attr
            .map(|attr| (attr.lk_y_direct, attr.lk_z_direct))
            .unwrap_or_default(),
        Err(reason) => {
            let cft_qd2 = seismic_qd
                .as_ref()
                .filter(|qd| structure == StructureKind::Cft && qd.method != QdMethod::Qd1);
            draw_peak_precheck(
                ui,
                peak,
                axial_peaks,
                Some(&reason),
                torsion_reason,
                cft_qd2,
                gravity_reference_issue,
            );
            return;
        }
    };

    if frame_buckling {
        if let Some((strong, weak)) = lk_y
            .zip(lk_z)
            .filter(|(y, z)| [*y, *z].into_iter().all(|v| v.is_finite() && v > 0.0))
        {
            ui.label(format!("採用座屈長さ [mm]　強軸 {strong} ／ 弱軸 {weak}"));
        }
    }

    let gravity_reference_issue = gravity_reference_issue.filter(|_| {
        rc_beam
            || rc_column
            || structure == StructureKind::Src
            || (structure == StructureKind::Cft && qd_method != QdMethod::Qd1)
    });
    let column_mechanism_topology =
        (rc_column && kind == MemberKind::Column && wire_qd && qd_method != QdMethod::Qd2)
            .then(|| sepika_design_jp::rc::ColumnMechanismTopology::new(model));

    let mut ctx = DesignCtx {
        term: LoadTerm::Short,
        kind,
        length,
        node_span_mm: (structure == StructureKind::S)
            .then(|| sepika_design_jp::steel::frame_node_span_mm(model, elem)),
        column_lateral_buckling_length: symmetric_h_column
            .then(|| sepika_design_jp::steel::column_lateral_buckling_length(model, elem, attr)),
        lk_y,
        lk_z,
        clear_length: Some(clear_span),
        rc_damage_control: app.core.analysis_cfg.rc_damage_control,
        bond_method: app.core.analysis_cfg.bond_method,
        rebar_material: rebar_material.cloned(),
        shear_rebar_material: shear_rebar_material.cloned(),
        steel_material: app.core.model.element_steel_material(elem).cloned(),
        steel_attr: attr
            .filter(|_| {
                structure == StructureKind::Src
                    || symmetric_h_column
                    || (structure == StructureKind::S && kind == MemberKind::Girder)
                    || (structure == StructureKind::S
                        && kind == MemberKind::Column
                        && matches!(
                            sec.shape,
                            Some(sepika_core::section_shape::SectionShape::SteelBuiltH { .. })
                        ))
            })
            .map(|attr| {
                let mut attr = attr.clone();
                // 簡易検定のCには直接指定を適用しない。
                attr.c_direct = None;
                if kind == MemberKind::Girder && structure == StructureKind::S {
                    attr.joint_flange_loss = 0.0;
                    attr.joint_web_loss = 0.0;
                    attr.scallop_web_loss = 0.0;
                }
                attr
            }),
        beam_has_slab: kind == MemberKind::Girder
            && sepika_design_jp::beam_has_attached_slab(&app.core.model, elem),
        rc_beam_qd1_slab_end_moments_nmm: slab_moments_nmm,
        rc_beam_qd1_at_material_end: qd1_positions.is_some(),
        rc_beam_qd1_slab_unverified: slab_moments_nmm.is_none()
            && matches!(rc_beam_qd1, Some(Ok(_))),
        rc_beam_qd1_issue: q0_issue.or_else(|| rc_beam_qd1.and_then(Result::err)),
        rc_grouped_beam_qd1_sum_my_nmm: None,
        seismic_qd,
        ..Default::default()
    };
    let checker: Box<dyn DesignCheck> = sepika_design_jp::checker_for(structure);

    #[derive(Clone, Copy)]
    enum PeakComponent {
        Axial,
        Moment,
    }
    let mut lateral_detail_shown = false;
    let mut draw_checks =
        |ui: &mut egui::Ui,
         forces: &MemberForces,
         selected: Option<(&ForcePeakSnapshot, PeakComponent)>| {
            let m_at = |target: f64| {
                forces
                    .at
                    .iter()
                    .find(|(p, _)| (p - target).abs() < 1e-6)
                    .map(|(_, f)| f[5])
            };
            ctx.end_moments_z = m_at(0.0).zip(m_at(1.0));
            ctx.mid_moment_z = m_at(0.5);
            let shear_span = |moment: usize, shear: usize| {
                forces
                    .at
                    .iter()
                    .map(|(_, f)| (f[moment].abs(), f[shear].abs()))
                    .max_by(|a, b| {
                        if rc_damage_control {
                            b.0.is_finite()
                                .cmp(&a.0.is_finite())
                                .then_with(|| a.0.total_cmp(&b.0))
                        } else {
                            a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
                        }
                    })
            };
            ctx.shear_span = shear_span(5, 1);
            ctx.shear_span_y = shear_span(4, 2);
            ctx.column_sum_my = column_mechanism_topology.as_ref().and_then(|topology| {
                let end_axial = |at: &[(f64, [f64; 6])]| {
                    [0.0, 1.0].map(|xi| at.iter().find(|(p, _)| *p == xi).map(|(_, f)| f[0]))
                };
                let qd = ctx.seismic_qd.as_ref()?;
                sepika_design_jp::rc::time_history_column_mechanism_sum_my(
                    model,
                    topology,
                    elem,
                    end_axial(&qd.long_at),
                    end_axial(&forces.at),
                )
            });
            if let Some(positions) = &qd1_positions {
                if selected.is_none() {
                    for &pos in positions {
                        if let Some(reason) = qd1_saved_station_issue(pos, &forces.at) {
                            draw_labeled_outcome_row(
                                ui,
                                &format!("ξ={pos:.2}:"),
                                &CheckOutcome::Skipped {
                                    reason: reason.into(),
                                },
                                false,
                            );
                        }
                    }
                }
            }
            for (index, &(pos, f)) in forces.at.iter().enumerate() {
                if selected
                    .is_some_and(|(snapshot, _)| snapshot.positions.binary_search(&index).is_err())
                {
                    continue;
                }
                let mfa = MemberForcesAt::from_local(pos, f);
                if let Some(positions) = &qd1_positions {
                    match positions
                        .iter()
                        .find(|expected| same_qd1_station(**expected, pos))
                    {
                        Some(expected)
                            if qd1_saved_station_issue(*expected, &forces.at).is_none() => {}
                        Some(_) => continue,
                        None => {
                            if let Some(ng) = sepika_design_jp::RcDesign
                                .independent_old_qd1_station_failure(&mfa, sec, mat, &ctx)
                            {
                                let label = selected.map_or_else(
                                    || format!("旧評価位置 ξ={pos:.6}:"),
                                    |(snapshot, _)| {
                                        format!("旧評価位置 t={:.6} s  ξ={pos:.6}:", snapshot.time)
                                    },
                                );
                                draw_labeled_outcome_row(ui, &label, &ng, false);
                            }
                            continue;
                        }
                    }
                }
                let mut draw_outcome = |outcome: &CheckOutcome| {
                    let with_reference_reason = match (outcome, gravity_reference_issue) {
                        (CheckOutcome::Skipped { reason }, Some(reference_reason)) => {
                            Some(CheckOutcome::Skipped {
                                reason: format!("{reason}。{reference_reason}"),
                            })
                        }
                        _ => None,
                    };
                    let outcome = with_reference_reason.as_ref().unwrap_or(outcome);
                    if let Some((snapshot, component)) = selected {
                        let side = match component {
                            PeakComponent::Axial if mfa.n < 0.0 => "圧縮側",
                            PeakComponent::Axial => "引張側",
                            PeakComponent::Moment if mfa.mz < 0.0 => "負曲げ側",
                            PeakComponent::Moment => "正曲げ側",
                        };
                        draw_labeled_outcome_row(
                            ui,
                            &format!("{side} t={:.6} s  ξ={pos:.2}:", snapshot.time),
                            outcome,
                            signed_bending || rc_damage_control,
                        );
                    } else {
                        draw_outcome_row(ui, pos, outcome);
                    }
                    match outcome {
                        CheckOutcome::Checked(result)
                            if symmetric_h_column
                                && !lateral_detail_shown
                                && !result.detail.is_empty() =>
                        {
                            ui.label(&result.detail);
                            lateral_detail_shown = true;
                        }
                        _ => {}
                    }
                };
                if structure == StructureKind::Src {
                    for outcome in sepika_design_jp::SrcDesign.check_time_history_outcomes(
                        &mfa,
                        sec,
                        mat,
                        &ctx,
                        missing_gravity,
                    ) {
                        draw_outcome(&outcome);
                    }
                } else if structure == StructureKind::Rc {
                    let shear_issue = if !rc_damage_control {
                        None
                    } else if let Some(reason) = gravity_issue {
                        Some(reason)
                    } else if selected.is_none() {
                        Some("RC損傷制御用せん断は異なる時刻の内力包絡では算定しません")
                    } else {
                        None
                    };
                    for outcome in sepika_design_jp::RcDesign.check_time_history(
                        &mfa,
                        sec,
                        mat,
                        &ctx,
                        !rc_beam
                            && !rc_column
                            && missing_gravity
                            && (wrong_shear_rebar || wrong_main_rebar),
                        shear_issue,
                    ) {
                        draw_outcome(&outcome);
                    }
                } else if structure == StructureKind::Cft {
                    for outcome in sepika_design_jp::CftDesign.check_outcomes(&mfa, sec, mat, &ctx)
                    {
                        draw_outcome(&outcome);
                    }
                } else if structure == StructureKind::S {
                    let outcomes = if selected.is_none() {
                        sepika_design_jp::SteelDesign
                            .check_time_history_envelope_outcomes(&mfa, sec, mat, &mut ctx)
                    } else {
                        sepika_design_jp::SteelDesign
                            .check_outcomes(&mfa, sec, mat, &ctx)
                            .collect()
                    };
                    for outcome in outcomes {
                        draw_outcome(&outcome);
                    }
                } else {
                    for outcome in
                        strength_outcomes(checker.check(&mfa, sec, mat, &ctx), torsion_reason(&mfa))
                    {
                        draw_outcome(&outcome);
                    }
                }
            }
        };
    if !signed_bending {
        if axial_peaks.is_none() {
            ui.colored_label(
                theme::WARN_TEXT,
                "引張側・圧縮側の記録がありません。確認するには解析を再実行してください。",
            );
        }
        ui.label("内力包絡");
        draw_checks(ui, peak, None);
    }
    if let Some(peaks) = axial_peaks.filter(|peaks| !peaks.snapshots.is_empty()) {
        ui.label("軸力極値が生じた確定ステップ");
        for snapshot in &peaks.snapshots {
            draw_checks(ui, &snapshot.forces, Some((snapshot, PeakComponent::Axial)));
        }
    }
    if let Some(peaks) = moment_peaks
        .filter(|_| signed_bending)
        .filter(|peaks| !peaks.snapshots.is_empty())
    {
        ui.label("正負の強軸曲げ極値が生じた確定ステップ");
        for snapshot in &peaks.snapshots {
            draw_checks(
                ui,
                &snapshot.forces,
                Some((snapshot, PeakComponent::Moment)),
            );
        }
    }
}

fn draw_asymmetric_column_section_history(ui: &mut egui::Ui, rec: &ThRecording, elem_idx: usize) {
    ui.strong("全確定時刻の断面単独照査");
    ui.label("圧縮時または軸力ゼロ時に、同じ評価位置・時刻の軸力と強軸曲げから上下縁を照査します。部材全体の適否は未判定です。");
    let Some(history) = rec.asymmetric_column_sections.get(&elem_idx) else {
        ui.colored_label(
            theme::WARN_TEXT,
            "全確定時刻の断面照査の記録を取得できません。確認するには解析を再実行してください。",
        );
        return;
    };
    if history.committed_count == 0 {
        ui.colored_label(
            theme::WARN_TEXT,
            "照査に必要な確定時刻の記録を取得できません。",
        );
    }
    for reason in &history.unavailable {
        ui.colored_label(theme::WARN_TEXT, format!("未算定：{reason}"));
    }
    for position in &history.positions {
        ui.label(format!("評価位置 ξ={:.6}", position.xi));
        if let Some(peak) = &position.peak {
            let status = if peak.ratio > 1.0 { " (NG)" } else { "" };
            let color = if peak.ratio > 1.0 {
                theme::status_color(peak.ratio)
            } else {
                theme::GRAY_600
            };
            ui.colored_label(
                color,
                format!(
                    "断面の検定比の下限 {:.5}{status}　t={:.6} s　N={:.3} kN　Mz={:.3} kN·m",
                    peak.ratio,
                    peak.time_s,
                    peak.n_n / 1e3,
                    peak.mz_nmm / 1e6,
                ),
            );
        }
        if position.observed_count < history.committed_count {
            ui.colored_label(
                theme::WARN_TEXT,
                "未算定：この位置の断面力を取得できない確定時刻があります。",
            );
        }
        for reason in &position.unavailable {
            ui.colored_label(theme::WARN_TEXT, format!("未算定：{reason}"));
        }
    }
}

fn draw_peak_precheck(
    ui: &mut egui::Ui,
    peak: &MemberForces,
    axial_peaks: Option<&AxialForcePeaks>,
    reason: Option<&str>,
    torsion_reason: impl Fn(&MemberForcesAt) -> Option<&'static str>,
    cft_qd2: Option<&sepika_design_jp::SeismicQd>,
    gravity_reference_issue: Option<&str>,
) {
    let draw = |ui: &mut egui::Ui, label: &str, pos: f64, torsion: Option<&str>| {
        let gravity = cft_qd2.and_then(|qd| sepika_design_jp::cft::qd2_reference_issue(qd, pos));
        let reasons: Vec<_> = reason
            .into_iter()
            .chain(torsion)
            .chain(gravity.as_deref())
            .chain(gravity_reference_issue.filter(|_| gravity.is_some()))
            .collect();
        if !reasons.is_empty() {
            let skipped = CheckOutcome::Skipped {
                reason: reasons.join("。"),
            };
            draw_labeled_outcome_row(ui, label, &skipped, false);
        }
    };
    if cft_qd2.is_some() {
        ui.label("内力包絡");
    }
    for &(pos, f) in &peak.at {
        let torsion = torsion_reason(&MemberForcesAt::from_local(pos, f));
        draw(ui, &format!("ξ={pos:.2}:"), pos, torsion);
    }
    if let Some(peaks) = axial_peaks {
        if cft_qd2.is_some() && !peaks.snapshots.is_empty() {
            ui.label("軸力極値が生じた確定ステップ");
        }
        for snapshot in &peaks.snapshots {
            for &index in &snapshot.positions {
                let Some(&(pos, f)) = snapshot.forces.at.get(index) else {
                    continue;
                };
                let mfa = MemberForcesAt::from_local(pos, f);
                let torsion = torsion_reason(&mfa);
                if cft_qd2.is_some() || torsion.is_some() {
                    let side = if mfa.n < 0.0 {
                        "圧縮側"
                    } else {
                        "引張側"
                    };
                    let label = format!("{side} t={:.6} s  ξ={pos:.2}:", snapshot.time);
                    draw(ui, &label, pos, torsion);
                }
            }
        }
    }
}

/// 長期重ね合わせの有無に関する注記（線形時刻歴は重ね合わせ運用のため対象外、
/// 非線形時刻歴は長期荷重初期載荷の有無で注記を変える）。
///
/// `nonlinear`/`applied_long_term` は `ResponseResult` に記録された**解析時**の
/// フラグを渡す（解析タブの現在の設定値ではない）。解析後に設定を変更しても
/// 注記が実際の解析条件と食い違わないようにするため。
fn draw_long_term_note(ui: &mut egui::Ui, nonlinear: bool, applied_long_term: bool) {
    let note = if !nonlinear {
        "線形時刻歴のため、この応答は地震動による応答成分のみです（長期荷重との重ね合わせは含みません）。"
    } else if applied_long_term {
        "長期荷重を初期状態として含む結果です。"
    } else {
        "長期荷重を含まない（水平力のみの）結果です。"
    };
    ui.label(egui::RichText::new(note).size(11.0).color(theme::GRAY_600));
}

/// 1 検定位置分の検定結果を 1 行で表示する（検定比・判定・式別内訳）。
fn draw_outcome_row(ui: &mut egui::Ui, pos: f64, outcome: &CheckOutcome) {
    draw_labeled_outcome_row(ui, &format!("ξ={pos:.2}:"), outcome, false);
}

fn draw_labeled_outcome_row(
    ui: &mut egui::Ui,
    label: &str,
    outcome: &CheckOutcome,
    extracted_only: bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        match outcome {
            CheckOutcome::Checked(cr) => {
                let evaluation = crate::app::evaluate_check(cr);
                if let Some(ratio) = if evaluation.complete {
                    evaluation.valid_max
                } else {
                    evaluation.known_ng
                } {
                    let (color, text) = if extracted_only && ratio <= 1.0 {
                        (theme::GRAY_600, format!("検定比 {ratio:.3}（抽出時刻）"))
                    } else {
                        let status = if ratio <= 1.0 { "OK" } else { "NG" };
                        (
                            theme::status_color(ratio),
                            format!("検定比 {ratio:.3} ({status})"),
                        )
                    };
                    ui.colored_label(color, text).on_hover_text(&cr.basis);
                }
                if !evaluation.complete {
                    ui.colored_label(theme::GRAY_600, crate::app::INVALID_CHECK_COMPONENTS);
                }
                for c in &cr.components {
                    let valid = c.ratio.is_finite() && c.ratio >= 0.0;
                    ui.colored_label(
                        if valid && (!extracted_only || c.ratio > 1.0) {
                            theme::status_color(c.ratio)
                        } else {
                            theme::GRAY_600
                        },
                        if valid {
                            format!("{} {:.2}", c.kind.label(), c.ratio)
                        } else {
                            format!("{} 未評価", c.kind.label())
                        },
                    )
                    .on_hover_text(&c.detail);
                    if let Some(warning) = c.rc_beam_qd1_slab_warning() {
                        ui.colored_label(theme::WARN_TEXT, warning);
                    }
                }
            }
            CheckOutcome::Skipped { reason } => {
                ui.colored_label(theme::GRAY_600, format!("検定不能: {reason}"));
            }
        }
    });
}
#[cfg(test)]
#[path = "signed_q0_acceptance_tests.rs"]
mod signed_q0_acceptance_tests;

#[cfg(test)]
#[path = "steel_time_history_c_tests.rs"]
mod steel_time_history_c_tests;

#[cfg(test)]
#[path = "../../../sepika-design-jp/tests/fixtures/steel_column_lateral_buckling_model.rs"]
mod steel_column_lateral_buckling_input;

#[cfg(test)]
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_mechanism_model.rs"]
mod rc_column_mechanism_input;

#[cfg(test)]
#[path = "../../../sepika-design-jp/tests/fixtures/rc_column_mechanism_reference.rs"]
mod rc_column_mechanism_reference;

#[cfg(test)]
use crate::app::shear_area_acceptance_tests::h_column_input as combined_column;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_history_detail_reports_eccentric_h_column_buckling_length_and_status() {
        use sepika_solver::dynamic::timehistory::{AxialForcePeaks, ResponseResult};
        use steel_column_lateral_buckling_input::{column, moments};

        for (offset_mm, confirmed, count, revised, length_text, outcome_text) in [
            (
                0.0,
                true,
                Some(0),
                false,
                Some("lb=3000 mm"),
                "検定比 0.973 (OK)",
            ),
            (
                500.0,
                true,
                None,
                false,
                Some("lb=3500 mm"),
                "検定比 1.039 (NG)",
            ),
            (
                500.0,
                false,
                Some(0),
                false,
                Some("lb=3000 mm"),
                "検定比 0.973 (OK)",
            ),
            (
                500.0,
                true,
                Some(1),
                false,
                None,
                "中間横補剛が入力されています",
            ),
            (
                500.0,
                true,
                None,
                true,
                None,
                "実材端の条件が確認時から変わりました",
            ),
        ] {
            let mut model = column(offset_mm, confirmed);
            model.steel_design_attrs[0].lateral_brace_count = count;
            if revised {
                model.elements[0].end_offsets_mm[1][2] = 0.0;
            }
            let id = model.elements[0].id;
            let mut app = App::default();
            assert!(app.load_model(model));
            let times = vec![0.0, 1.0];
            let forces = moments(215e6);
            app.core.scoped.results = Some(crate::app::ResultsBundle {
                time_history: Some(ResponseResult {
                    wall_capacities: Vec::new(),
                    rc_wall_shear_method: Default::default(),
                    time: times.clone(),
                    peak_disp: Vec::new(),
                    story_drift_angle: Vec::new(),
                    history: Default::default(),
                    recording: Some(ThRecording {
                        frame_time: times,
                        node_disp: vec![vec![[0.0; 6]; 2]; 2],
                        member_forces: vec![vec![Some(forces.clone())]; 2],
                        peak_member_forces: vec![Some(forces)],
                        axial_force_peaks: vec![AxialForcePeaks::default()],
                        ..Default::default()
                    }),
                    nonlinear: false,
                    applied_long_term: false,
                    non_converged_steps: 0,
                }),
                ..Default::default()
            });
            let view = super::super::ViewportState::default();
            let output = egui::Context::default().run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 1200.0),
                    )),
                    ..Default::default()
                },
                |ui| draw_th_detail_content_at(ui, &mut app, id, 0, &view),
            );
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect();
            if let Some(length_text) = length_text {
                assert!(
                    labels.iter().any(|text| text.contains(length_text)
                        && text.contains(if confirmed {
                            "両実材端で横移動・ねじれを拘束"
                        } else {
                            "節点間距離の既定値"
                        })),
                    "{labels:?}"
                );
                for pos in ["ξ=0.00:", "ξ=0.50:", "ξ=1.00:"] {
                    assert!(
                        labels.windows(2).any(|row| row == [pos, outcome_text]),
                        "{labels:?}"
                    );
                }
            } else {
                assert!(
                    labels.iter().any(|text| text.contains(outcome_text)),
                    "{labels:?}"
                );
                assert!(
                    !labels.iter().any(|text| text.starts_with("検定比 ")),
                    "{labels:?}"
                );
                assert!(
                    !labels.iter().any(|text| text.contains("採用横座屈長さ")),
                    "{labels:?}"
                );
            }
        }
    }

    fn peak_check_text(app: &App, index: usize, forces: [f64; 6]) -> String {
        let mut peak_member_forces = vec![None; index + 1];
        let distribution = MemberForces {
            at: vec![(0.0, forces)],
        };
        peak_member_forces[index] = Some(distribution.clone());
        let elem = &app.core.model.elements[index];
        let mut axial_force_peaks = Vec::new();
        let mut strong_axis_moment_peaks = std::collections::BTreeMap::new();
        if sepika_solver::dynamic::timehistory::uses_signed_bending_peaks(&app.core.model, elem) {
            use sepika_solver::dynamic::timehistory::{
                AxialForcePeaks, ForcePeakSnapshot, StrongAxisMomentPeaks,
            };
            let snapshot = ForcePeakSnapshot {
                time: 0.0,
                forces: distribution,
                positions: vec![0],
            };
            axial_force_peaks.resize_with(index + 1, AxialForcePeaks::default);
            if forces[0] != 0.0 {
                axial_force_peaks[index].snapshots.push(snapshot.clone());
            }
            let snapshots = if forces[5] == 0.0 {
                vec![]
            } else {
                vec![snapshot]
            };
            strong_axis_moment_peaks.insert(index, StrongAxisMomentPeaks { snapshots });
        }
        let rec = ThRecording {
            peak_member_forces,
            axial_force_peaks,
            strong_axis_moment_peaks,
            ..Default::default()
        };
        peak_recording_text(app, elem, index, &rec, false)
    }

    fn peak_recording_text(
        app: &App,
        elem: &ElementData,
        index: usize,
        rec: &ThRecording,
        with_gravity: bool,
    ) -> String {
        let axis = super::super::member_axis_endpoints(elem, &app.core.model).unwrap();
        egui::Context::default()
            .run_ui(egui::RawInput::default(), |ui| {
                draw_peak_check(
                    ui,
                    app,
                    elem,
                    index,
                    rec,
                    with_gravity,
                    with_gravity,
                    || (!with_gravity).then_some(UNVERIFIED_DAMAGE_CONTROL_GRAVITY),
                    || matching_gravity_member_forces(&app.core.model, app, |_| true),
                    &axis,
                )
            })
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn h_column_checks_tension_and_strong_bending_at_the_same_axial_peak_step() {
        let mut app = App::default();
        assert!(app.load_model(combined_column::cantilever(77_000.0, 215e6, 0.0)));
        let recording = ThRecording {
            peak_member_forces: vec![Some(MemberForces {
                at: vec![(0.5, [77_000.0, 0.0, 0.0, 0.0, 0.0, 250e6])],
            })],
            axial_force_peaks: vec![AxialForcePeaks {
                snapshots: vec![ForcePeakSnapshot {
                    time: 1.0,
                    forces: MemberForces {
                        at: vec![(0.5, [77_000.0, 0.0, 0.0, 0.0, 0.0, 215e6])],
                    },
                    positions: vec![0],
                }],
            }],
            ..Default::default()
        };
        let text = peak_recording_text(&app, &app.core.model.elements[0], 0, &recording, false);
        assert!(
            text.lines()
                .zip(text.lines().skip(1))
                .any(|row| row == ("引張側 t=1.000000 s  ξ=0.50:", "検定比 0.924 (OK)")),
            "{text}"
        );
    }

    fn rc_column_peak_check_app(
        model: sepika_core::model::Model,
        method: sepika_design_jp::QdMethod,
        axial_n: f64,
    ) -> App {
        use super::rc_column_mechanism_input as input;
        use sepika_solver::statics::linear::StaticOnce;

        let mut app = App::default();
        app.core.analysis_cfg.rc_damage_control = false;
        assert!(app.load_model(model));
        app.core.analysis_cfg.qd_method = method;
        app.core.scoped.results = Some(crate::app::ResultsBundle {
            combos: vec![(
                input::GRAVITY.into(),
                StaticOnce {
                    story_shears: None,
                    stiffness_ratio_disp: None,
                    story_rigidity: Vec::new(),
                    disp: Vec::new(),
                    member_forces: vec![(
                        input::COLUMN,
                        MemberForces {
                            at: [0.0, 0.5, 1.0]
                                .map(|xi| (xi, [axial_n, 0.0, 0.0, 0.0, 0.0, 0.0]))
                                .to_vec(),
                        },
                    )],
                    column_end_horizontal_forces: Vec::new(),
                    column_end_axial_forces: Vec::new(),
                    panel_moments: Vec::new(),
                    girder_column_face_forces: Vec::new(),
                    steel_panel_column_face_forces: Vec::new(),
                },
            )],
            ..Default::default()
        });
        app
    }

    fn rc_column_peak_check_text(app: &App, axial_n: f64) -> String {
        let recording = ThRecording {
            peak_member_forces: vec![Some(MemberForces {
                at: [0.0, 0.5, 1.0]
                    .map(|xi| (xi, [axial_n, 1000.0, 1000.0, 0.0, 0.0, 0.0]))
                    .to_vec(),
            })],
            ..Default::default()
        };
        let i = super::rc_column_mechanism_input::COLUMN.index();
        peak_recording_text(app, &app.core.model.elements[i], i, &recording, true)
    }

    fn time_history_gravity_model(snow: bool) -> sepika_core::model::Model {
        use crate::app::rc_column_boundary_tests::input;
        use sepika_core::ids::{LoadCaseId, NodeId};
        use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination, NodalLoad};

        let mut model = input::column_model(false, 50_000.0);
        let dead = model.load_cases[0].id;
        let live = LoadCaseId(1);
        let extra_live = LoadCaseId(2);
        model.load_cases[0].nodal = vec![NodalLoad::manual(
            NodeId(1),
            [0.0, 10_000.0, -50_000.0, 0.0, 0.0, 0.0],
        )];
        model.load_cases.extend([
            LoadCase {
                origin: Default::default(),
                id: live,
                name: "架構用積載".into(),
                kind: LoadCaseKind::Live,
                nodal: Vec::new(),
                member: Vec::new(),
            },
            LoadCase {
                origin: Default::default(),
                id: extra_live,
                name: "架構用積載追加".into(),
                kind: LoadCaseKind::Live,
                nodal: vec![NodalLoad::manual(
                    NodeId(1),
                    [0.0, 20_000.0, 0.0, 0.0, 0.0, 0.0],
                )],
                member: Vec::new(),
            },
        ]);
        model.combinations.extend([
            LoadCombination {
                name: "積載の一部がない常時重力".into(),
                terms: vec![(dead, 1.0), (live, 1.0)],
            },
            LoadCombination {
                name: "初期載荷と一致する常時重力".into(),
                terms: vec![
                    (extra_live, 0.5),
                    (dead, 1.0),
                    (live, 1.0),
                    (extra_live, 0.5),
                ],
            },
        ]);
        if snow {
            model.load_cases.push(LoadCase {
                origin: Default::default(),
                id: LoadCaseId(3),
                name: "積雪".into(),
                kind: LoadCaseKind::Snow,
                nodal: vec![NodalLoad::manual(
                    NodeId(1),
                    [0.0, 5_000.0, -30_000.0, 0.0, 0.0, 0.0],
                )],
                member: Vec::new(),
            });
        }
        model
    }

    #[test]
    fn time_history_shear_rejects_snow_reference_mismatch_but_checks_without_initial_load() {
        let mut app = App::default();
        assert!(app.load_model(time_history_gravity_model(true)));
        app.core.analysis_cfg.rc_damage_control = false;
        app.core.analysis_cfg.qd_method = QdMethod::Qd2;
        app.core.analysis_cfg.th_nonlinear = true;
        app.core.analysis_cfg.th_record_every = 1;
        app.run_combination(1);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );

        for apply in [true, false] {
            app.core.analysis_cfg.th_apply_long_term = apply;
            app.run_time_history(sepika_solver::dynamic::timehistory::GroundMotion {
                dt: 0.02,
                accel_x: vec![0.0, 50.0, -50.0, 0.0],
                accel_y: None,
                accel_theta: None,
            });
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            let text = time_history_detail_labels(&mut app);
            if apply {
                assert!(
                    text.contains("初期載荷に積雪または種別未指定の荷重を含む"),
                    "{text}"
                );
                assert!(text.contains("ξ=0.00:\n検定不能"), "{text}");
                assert!(!text.contains("(OK)"), "{text}");
            } else {
                assert!(text.contains("長期荷重を含まない"), "{text}");
                assert!(text.contains("せん断 ") && text.contains("(OK)"), "{text}");
                assert!(!text.contains("初期載荷に積雪"), "{text}");
            }
        }
    }

    #[test]
    fn time_history_shear_rejects_duplicate_initial_case_id() {
        let mut model = time_history_gravity_model(false);
        model.load_cases[2].id = model.load_cases[0].id;
        model.combinations.truncate(1);
        let mut app = App::default();
        assert!(app.load_model(model));
        app.core.analysis_cfg.rc_damage_control = false;
        app.core.analysis_cfg.qd_method = QdMethod::Qd2;
        app.core.analysis_cfg.th_nonlinear = true;
        app.core.analysis_cfg.th_apply_long_term = true;
        app.core.analysis_cfg.th_record_every = 1;
        app.run_combination(0);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let reference = &app.current_static().unwrap().member_forces[0].1.at[0].1;
        assert!((reference[1].hypot(reference[2]) - 10_000.0).abs() < 1e-3);
        app.run_time_history(sepika_solver::dynamic::timehistory::GroundMotion {
            dt: 0.02,
            accel_x: vec![0.0, 50.0, -50.0, 0.0],
            accel_y: None,
            accel_theta: None,
        });
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let rec = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .time_history
            .as_ref()
            .unwrap()
            .recording
            .as_ref()
            .unwrap();
        assert_eq!(rec.frame_time[0], 0.0);
        let initial = &rec.member_forces[0][0].as_ref().unwrap().at[0].1;
        assert!(
            (initial[1].hypot(initial[2]) - 30_000.0).abs() < 1.0,
            "{initial:?}"
        );

        let text = time_history_detail_labels(&mut app);
        assert!(
            text.contains("初期載荷に関わる荷重ケースIDが重複"),
            "{text}"
        );
        assert!(
            text.contains("ξ=0.00:\n検定不能") && !text.contains("(OK)"),
            "{text}"
        );
    }

    #[test]
    fn time_history_shear_uses_only_matching_current_cases_independent_of_save_order() {
        let mut app = App::default();
        assert!(app.load_model(time_history_gravity_model(false)));
        app.core.analysis_cfg.rc_damage_control = false;
        app.core.analysis_cfg.qd_method = QdMethod::Qd2;
        app.core.analysis_cfg.th_nonlinear = true;
        app.core.analysis_cfg.th_apply_long_term = true;
        app.core.analysis_cfg.th_record_every = 1;
        app.run_combination(0);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.run_combination(1);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.run_time_history(sepika_solver::dynamic::timehistory::GroundMotion {
            dt: 0.02,
            accel_x: vec![0.0, 50.0, -50.0, 0.0],
            accel_y: None,
            accel_theta: None,
        });
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );

        let text = time_history_detail_labels(&mut app);
        assert!(text.contains("せん断 ") && text.contains("(OK)"), "{text}");
        assert!(!text.contains("検定不能"), "{text}");
        app.core.scoped.results.as_mut().unwrap().combos.reverse();
        assert_eq!(time_history_detail_labels(&mut app), text);

        let bundle = app.core.scoped.results.as_mut().unwrap();
        bundle.input_records.retain(|(key, _)| {
            !matches!(key, crate::app::ResultInputKey::Combo(name) if name == "初期載荷と一致する常時重力")
        });
        let text = time_history_detail_labels(&mut app);
        assert!(
            text.contains("常時重力組合せの生成入力を確認できません"),
            "{text}"
        );
        assert!(
            text.contains("検定不能") && !text.contains("(OK)"),
            "{text}"
        );
    }

    #[test]
    fn rc_column_peak_check_reports_invalid_connected_girder_rebar_for_qd1() {
        use super::rc_column_mechanism_input as input;
        use crate::app::rc_column_boundary_tests::input as column;
        use sepika_design_jp::QdMethod;

        let model = input::column_with_girder(column::column_model(false, 0.0));
        let mut app = rc_column_peak_check_app(model, QdMethod::Min, -100_000.0);
        let text = rc_column_peak_check_text(&app, -100_000.0);
        for message in [
            "検定不能",
            "RC柱 ID 0 の Qy 方向",
            "大梁 ID 1",
            "材料区分は「鋼材」",
            "断面タブで主筋に「鉄筋」区分の材料を割り当て",
        ] {
            assert!(text.contains(message), "{text}");
        }
        assert!(!text.contains("検定比 "), "{text}");

        app.core.model.sections[input::GIRDER_SECTION.index()].rebar_material = Some(input::REBAR);
        let text = rc_column_peak_check_text(&app, -100_000.0);
        assert!(
            text.contains("検定比 ") && text.contains("せん断 "),
            "{text}"
        );
        assert!(!text.contains("検定不能"), "{text}");
    }

    #[test]
    fn rc_column_peak_qd1_check_uses_upper_column_continuity() {
        use super::rc_column_mechanism_input as input;
        use super::rc_column_mechanism_reference::{CONTINUOUS_SHEAR_RATIO, TERMINAL_SHEAR_RATIO};
        use crate::app::rc_column_boundary_tests::input as column;
        use sepika_core::ids::NodeId;
        use sepika_core::model::ElementKind;
        use sepika_design_jp::QdMethod;

        let terminal = input::column_with_yielding_head_girders(column::column_model(false, 0.0));
        let mut continuous = terminal.clone();
        input::add_column_above(&mut continuous, NodeId(1), ElementKind::Fiber);
        for (model, expected_ratio) in [
            (terminal, TERMINAL_SHEAR_RATIO),
            (continuous, CONTINUOUS_SHEAR_RATIO),
        ] {
            let app = rc_column_peak_check_app(model, QdMethod::Qd1, -100_000.0);
            let text = rc_column_peak_check_text(&app, -100_000.0);
            assert!(
                text.contains(&format!("せん断 {expected_ratio:.2}")),
                "{text}"
            );
        }
    }

    #[test]
    fn rc_column_peak_qd1_does_not_reduce_demand_for_a_slab_supported_girder() {
        use super::rc_column_mechanism_input as input;
        use super::rc_column_mechanism_reference::{
            COLUMN_ALLOWABLE_SHEAR_N, CONTINUOUS_SLAB_HEAD_QD1_N, TERMINAL_SLAB_HEAD_QD1_N,
        };
        use crate::app::rc_column_boundary_tests::input as column;
        use sepika_design_jp::QdMethod;

        for (continuous, demand_n) in [
            (false, TERMINAL_SLAB_HEAD_QD1_N),
            (true, CONTINUOUS_SLAB_HEAD_QD1_N),
        ] {
            let (mut model, edge) = input::column_with_slab_sensitive_head(
                column::column_model(false, 0.0),
                continuous,
            );
            let qd2 = rc_column_peak_check_app(model.clone(), QdMethod::Qd2, -100_000.0);
            let bare_qd2 = rc_column_peak_check_text(&qd2, -100_000.0);
            input::attach_slab(&mut model, edge);

            let qd2 = rc_column_peak_check_app(model.clone(), QdMethod::Qd2, -100_000.0);
            assert_eq!(bare_qd2, rc_column_peak_check_text(&qd2, -100_000.0));

            let app = rc_column_peak_check_app(model, QdMethod::Qd1, -100_000.0);
            let text = rc_column_peak_check_text(&app, -100_000.0);
            let ratio = demand_n / COLUMN_ALLOWABLE_SHEAR_N;
            assert!(text.contains(&format!("せん断 {ratio:.2}")), "{text}");
        }
    }

    #[test]
    fn rc_column_peak_qd1_keeps_floor_diagnostic_and_shear_failure_with_unchecked_bending() {
        use super::rc_column_mechanism_input as input;
        use crate::app::rc_column_boundary_tests::input as column;
        use sepika_design_jp::QdMethod;

        let (mut model, edge) =
            input::column_with_slab_sensitive_head(column::column_model(false, 0.0), true);
        input::attach_slab(&mut model, edge);
        let app = rc_column_peak_check_app(model, QdMethod::Qd1, -100_000.0);
        let end_forces = [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
        let recording = ThRecording {
            peak_member_forces: vec![Some(MemberForces {
                at: vec![
                    (0.0, end_forces),
                    (0.5, [10_000_000.0, 1000.0, 1000.0, 0.0, 0.0, 1e6]),
                    (1.0, end_forces),
                ],
            })],
            ..Default::default()
        };
        let i = input::COLUMN.index();
        let text = peak_recording_text(&app, &app.core.model.elements[i], i, &recording, true);
        let (_, middle) = text.split_once("ξ=0.50:").expect("中間断面");
        let (middle, _) = middle.split_once("ξ=1.00:").expect("終端断面");
        for label in ["せん断 ", "軸 "] {
            let ratio = middle
                .lines()
                .find_map(|line| line.strip_prefix(label))
                .and_then(|ratio| ratio.parse::<f64>().ok())
                .unwrap_or_else(|| panic!("{label}の不適合が中間断面にない: {middle}"));
            assert!(ratio > 1.0, "{middle}");
        }
        for expected in [
            "許容曲げが非正",
            "Qy方向の柱頭",
            "梁端降伏曲げを確認できない",
        ] {
            assert!(middle.contains(expected), "{middle}");
        }
    }

    #[test]
    fn rc_column_peak_qd1_uses_the_lower_confirmed_single_end_girder_mechanism() {
        use super::rc_column_mechanism_input as input;
        use super::rc_column_mechanism_reference::{
            COLUMN_ALLOWABLE_SHEAR_N, REVERSIBLE_ONE_BAR_FOOT_QD1_N,
            REVERSIBLE_THREE_BAR_FOOT_QD1_N, TWO_HEAD_GIRDERS_HALF_QD1_N,
        };
        use crate::app::rc_column_boundary_tests::input as column;
        use sepika_design_jp::QdMethod;

        for (head_bars, foot_bars, free_beam_end, expected_n) in [
            (2, 1, false, REVERSIBLE_ONE_BAR_FOOT_QD1_N),
            (2, 1, true, TWO_HEAD_GIRDERS_HALF_QD1_N),
            (4, 3, false, REVERSIBLE_THREE_BAR_FOOT_QD1_N),
        ] {
            let mut model = input::column_with_reversible_girders(
                column::column_model(false, 0.0),
                head_bars,
                foot_bars,
            );
            if free_beam_end {
                model.elements.pop();
                model.nodes.pop();
            }
            let app = rc_column_peak_check_app(model, QdMethod::Qd1, -100_000.0);
            let text = rc_column_peak_check_text(&app, -100_000.0);
            let ratio = expected_n / COLUMN_ALLOWABLE_SHEAR_N;
            assert!(text.contains(&format!("せん断 {ratio:.2}")), "{text}");
            assert!(text.contains(&format!("検定比 {ratio:.3} (OK)")), "{text}");
        }
    }

    #[test]
    fn rc_column_peak_qd1_excludes_foot_girder_yield_at_unexpanded_wall_plate() {
        use super::rc_column_mechanism_input as input;
        use super::rc_column_mechanism_reference::{
            COLUMN_ALLOWABLE_SHEAR_N, TWO_HEAD_GIRDERS_HALF_QD1_N,
        };
        use crate::app::rc_column_boundary_tests::input as column;
        use sepika_design_jp::QdMethod;

        let mut model =
            input::column_with_reversible_girders(column::column_model(false, 0.0), 2, 1);
        input::add_wall_below_reversible_column(&mut model);
        let app = rc_column_peak_check_app(model, QdMethod::Qd1, -100_000.0);
        assert!(app
            .core
            .model
            .elements
            .iter()
            .all(|elem| elem.kind != sepika_core::model::ElementKind::Wall));
        let text = rc_column_peak_check_text(&app, -100_000.0);
        let ratio = TWO_HEAD_GIRDERS_HALF_QD1_N / COLUMN_ALLOWABLE_SHEAR_N;
        assert!(text.contains(&format!("検定比 {ratio:.3} (OK)")), "{text}");
    }

    #[test]
    fn rc_column_peak_qd1_check_counts_nonlinear_girders_and_falls_back_to_column_ends() {
        use super::rc_column_mechanism_input as input;
        use super::rc_column_mechanism_reference::{
            BOTH_COLUMN_ENDS_QD1_N, COLUMN_ALLOWABLE_SHEAR_N, CONTINUOUS_TWO_GIRDER_QD1_N,
            ONE_GIRDER_QD1_N, TWO_GIRDER_QD1_N,
        };
        use crate::app::rc_column_boundary_tests::input as column;
        use sepika_core::ids::NodeId;
        use sepika_core::model::{ElementKind, EndCondition};
        use sepika_design_jp::QdMethod;
        use sepika_element::frame::beam::{apply_auto_rigid_zones, RigidZoneRule};

        let mut model = input::column_with_opposing_head_girders(
            column::column_model(false, 0.0),
            ElementKind::MultiSpring,
        );
        model.elements[input::GIRDER.index()].kind = ElementKind::Fiber;
        let mut continuous = model.clone();
        input::add_column_above(&mut continuous, NodeId(1), ElementKind::Beam);
        let mut incomplete = model.clone();
        input::remove_opposite_girder_yield_strength(&mut incomplete);

        let mut far_pin = model.clone();
        far_pin.elements[input::GIRDER.index()].end_cond[1] = EndCondition::Pinned;
        let mut head_pin = model.clone();
        head_pin.elements[input::GIRDER.index()].end_cond[0] = EndCondition::Pinned;
        head_pin.stress_cfg.consider_rigid_zones = false;
        let mut head_pin_with_rigid_zone = head_pin.clone();
        head_pin_with_rigid_zone.stress_cfg.consider_rigid_zones = true;
        head_pin_with_rigid_zone.elements[input::GIRDER.index()]
            .rigid_zone
            .length_i = 100.0;
        head_pin_with_rigid_zone.elements[input::GIRDER.index()]
            .rigid_zone
            .source_i = sepika_core::model::ZoneSource::Manual;

        for (mut model, qd1_n, status) in [
            (model, TWO_GIRDER_QD1_N, "NG"),
            (far_pin, TWO_GIRDER_QD1_N, "NG"),
            (head_pin, ONE_GIRDER_QD1_N, "OK"),
            (head_pin_with_rigid_zone, BOTH_COLUMN_ENDS_QD1_N, "NG"),
            (continuous, CONTINUOUS_TWO_GIRDER_QD1_N, "OK"),
            (incomplete, BOTH_COLUMN_ENDS_QD1_N, "NG"),
        ] {
            apply_auto_rigid_zones(&mut model, &RigidZoneRule::default());
            let app = rc_column_peak_check_app(model, QdMethod::Qd1, -200_000.0);
            let text = rc_column_peak_check_text(&app, -200_000.0);
            let ratio = qd1_n / COLUMN_ALLOWABLE_SHEAR_N;
            assert!(text.contains(&format!("せん断 {ratio:.2}")), "{text}");
            assert!(
                text.contains(&format!("検定比 {ratio:.3} ({status})")),
                "{text}"
            );
        }
    }

    #[test]
    fn concrete_peak_checks_report_torsion_without_ok_and_preserve_ng() {
        use crate::app::{
            cft_column_buckling_tests::input as cft, rc_column_boundary_tests::input as rc,
        };

        let mut rc_app = App::default();
        rc_app.core.analysis_cfg.rc_damage_control = false;
        assert!(rc_app.load_model(rc::column_model(false, 0.0)));
        let src_app =
            crate::app::src_ql_acceptance_tests::src_column_app(sepika_design_jp::QdMethod::Qd1);
        let mut cft_app = App::default();
        let mut cft_model = cft::cantilever(true, 0.0);
        cft_model.steel_design_attrs =
            vec![cft::direct_lengths(ElemId(0), Some(6000.0), Some(6000.0))];
        assert!(cft_app.load_model(cft_model));

        for (kind, app) in [("RC", rc_app), ("SRC", src_app), ("CFT", cft_app)] {
            let elem = &app.core.model.elements[0];
            for (envelope_mx, snapshot_mx, envelope_reason, snapshot_reason) in [
                (0.0, -0.0, None, None),
                (1.0e6, -0.0, Some("同時に作用"), None),
                (
                    f64::from_bits(1),
                    -f64::from_bits(1),
                    Some("同時に作用"),
                    Some("同時に作用"),
                ),
                (f64::NAN, f64::INFINITY, Some("非有限"), Some("非有限")),
                (f64::NEG_INFINITY, f64::NAN, Some("非有限"), Some("非有限")),
            ] {
                for n in [-100_000.0, -30.0e6] {
                    let forces = |mx| MemberForces {
                        at: vec![(0.0, [n, 0.0, 0.0, mx, 0.0, 0.0])],
                    };
                    let recording = ThRecording {
                        peak_member_forces: vec![Some(forces(envelope_mx))],
                        axial_force_peaks: vec![AxialForcePeaks {
                            snapshots: vec![ForcePeakSnapshot {
                                time: 1.0,
                                forces: forces(snapshot_mx),
                                positions: vec![0],
                            }],
                        }],
                        ..Default::default()
                    };
                    let text = peak_recording_text(&app, elem, 0, &recording, false);
                    let (envelope, snapshot) = text
                        .split_once("軸力極値が生じた確定ステップ\n")
                        .unwrap_or_else(|| panic!("{kind}: {text}"));
                    assert!(
                        snapshot.contains("圧縮側 t=1.000000 s  ξ=0.00:"),
                        "{kind}: {text}"
                    );
                    let baseline = peak_check_text(&app, 0, [n, 0.0, 0.0, 0.0, 0.0, 0.0]);
                    let ratio = baseline
                        .lines()
                        .find(|line| line.starts_with("検定比 "))
                        .unwrap_or_else(|| panic!("{kind}: {baseline}"));
                    let ng = n == -30.0e6;
                    assert!(
                        ratio.ends_with(if ng { "(NG)" } else { "(OK)" }),
                        "{kind}: {baseline}"
                    );
                    for (rows, reason) in [(envelope, envelope_reason), (snapshot, snapshot_reason)]
                    {
                        if let Some(reason) = reason {
                            assert!(
                                rows.lines().any(|line| line.starts_with("検定不能: ")
                                    && line.contains("ねじりモーメントMx")
                                    && line.contains(reason)),
                                "{kind}: {rows}"
                            );
                            assert!(!rows.contains("(OK)"), "{kind}: {rows}");
                            assert_eq!(rows.contains(ratio), ng, "{kind}: {rows}");
                        } else {
                            assert!(rows.contains(ratio), "{kind}: {rows}");
                            assert!(!rows.contains("検定不能:"), "{kind}: {rows}");
                        }
                    }
                }
            }
            let pure = peak_check_text(&app, 0, [0.0, 0.0, 0.0, 1.0e6, 0.0, 0.0]);
            assert!(pure.contains("検定不能: 純ねじり（Mx）"), "{kind}: {pure}");
            assert!(!pure.contains("検定比"), "{kind}: {pure}");
        }
    }

    #[test]
    fn usd_main_bar_reason_does_not_hide_independent_time_history_shear_ng() {
        use crate::app::rc_column_boundary_tests::input;
        use sepika_core::ids::MaterialId;

        for (grade, family, fy) in [
            ("USD590", "USD590", 590.0),
            ("USD590A", "USD590", 590.0),
            ("USD590B", "USD590", 590.0),
            ("USD685", "USD685", 685.0),
            ("USD685A", "USD685", 685.0),
            ("USD685B", "USD685", 685.0),
        ] {
            let mut model = input::column_model(false, 0.0);
            let mut main = model.materials[1].clone();
            main.id = MaterialId(2);
            main.name = grade.into();
            main.fy = Some(fy);
            model.sections[0].rebar_material = Some(main.id);
            model.materials.push(main);

            let mut app = App::default();
            app.core.analysis_cfg.rc_damage_control = false;
            assert!(app.load_model(model));
            let text = peak_check_text(&app, 0, [-100_000.0, 10_000_000.0, 0.0, 0.0, 0.0, 1e6]);
            assert!(
                text.lines().any(|row| row.starts_with("検定不能:")
                    && row.contains("許容軸力境界のRC柱")
                    && row.contains(&format!("{family}系主筋の認定に基づく許容応力度"))),
                "{grade}: {text}"
            );
            assert!(
                text.contains("(NG)") && text.contains("せん断"),
                "{grade}: {text}"
            );
            assert!(
                !text.contains("(OK)") && !text.contains("軸+曲げ "),
                "{grade}: {text}"
            );
        }
    }

    #[test]
    fn steel_peak_check_shows_the_reason_for_unchecked_torsion() {
        use crate::app::steel_column_k_tests::input;
        let mut app = App::default();
        assert!(app.load_model(input::portal(1.0)));

        let text = peak_check_text(&app, 0, [0.0, 0.0, 0.0, -100e6, 0.0, 0.0]);
        assert!(text.contains("最大応力に対する検定（短期）"), "{text}");
        assert!(text.contains("地震時 QD / 柱メカニズム"), "{text}");
        assert!(!text.contains("全確定時刻の断面単独照査"), "{text}");
        assert!(text.contains("検定不能: ねじりモーメントMx"), "{text}");
    }

    #[test]
    fn asymmetric_built_h_peak_check_respects_section_loss_without_using_direct_c() {
        let mut app = App::default();
        let mut model = crate::app::steel_column_k_tests::asymmetric_built_h_cantilever();
        model.steel_design_attrs[0].lk_y_direct = Some(3600.0);
        model.steel_design_attrs[0].lk_z_direct = Some(3600.0);
        assert!(app.load_model(model));

        let forces = [
            [0.0, 0.0, 0.0, 0.0, 0.0, 60e6],
            [0.0, 0.0, 0.0, 0.0, 0.0, 160e6],
            [-77_000.0, 0.0, 0.0, 0.0, 0.0, 160e6],
        ];
        let texts = forces.map(|force| peak_check_text(&app, 0, force));
        let [unchecked, bending_ng, compression_ng] = &texts;
        assert!(
            unchecked.contains("採用座屈長さ [mm]")
                && unchecked.contains("検定不能: 上下非対称断面の横座屈・曲げねじり座屈")
                && !unchecked.contains("検定比"),
            "{unchecked}"
        );
        assert!(bending_ng.contains("検定比 1.073 (NG)"), "{bending_ng}");
        assert!(
            compression_ng.contains("検定比 1.117 (NG)"),
            "{compression_ng}"
        );

        app.core.model.steel_design_attrs[0].c_direct = Some(2.3);
        assert_eq!(forces.map(|force| peak_check_text(&app, 0, force)), texts);

        app.core.model.steel_design_attrs[0].joint_flange_loss = 5.0;
        for force in forces {
            let text = peak_check_text(&app, 0, force);
            assert!(
                text.contains("検定不能: 上下非対称組立H形鋼柱は未算定です。断面欠損"),
                "{text}"
            );
            assert!(!text.contains("検定比"), "{text}");
        }
    }

    #[test]
    fn asymmetric_built_h_history_shows_reverse_bending_failure_after_save_and_reopen() {
        let mut app = App::default();
        let mut model = crate::app::steel_column_k_tests::asymmetric_built_h_cantilever();
        model.steel_design_attrs[0].lk_y_direct = Some(500.0);
        model.steel_design_attrs[0].lk_z_direct = Some(500.0);
        assert!(app.load_model(model));
        app.generate_stories_action();
        app.core.analysis_cfg.th_record_every = 2;
        app.run_time_history(sepika_solver::dynamic::timehistory::GroundMotion {
            dt: 1.0,
            accel_x: vec![0.0, 100.0, -100.0],
            accel_y: None,
            accel_theta: None,
        });
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );

        let case_id = app.core.scoped.view_vibration_case.unwrap();
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let mut th = bundle.time_history_for(case_id).unwrap().clone();
        assert_eq!(th.time, vec![0.0, 1.0, 2.0]);
        th.recording = Some(asymmetric_history(
            &[(-78.0, 0.0), (-77.0, 150.0), (-77.0, -155.0)],
            1,
            2,
        ));
        bundle.upsert_time_history(case_id, th.clone());
        app.set_spatial_time_history_view(case_id, &th);
        let path =
            std::env::temp_dir().join(format!("steel-history-{}.ovika", uuid::Uuid::new_v4()));
        app.save_project_to(path.clone());
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut restored = App::default();
        restored.open_project_from(path.clone());
        std::fs::remove_file(path).unwrap();

        let text = time_history_detail_labels(&mut restored);
        assert!(text.contains("時刻歴の簡易検定（短期）"), "{text}");
        assert!(!text.contains("最大応力に対する検定"), "{text}");
        assert!(!text.contains("地震時 QD / 柱メカニズム"), "{text}");
        assert!(
            text.contains("正曲げ側 t=1.000000 s  ξ=0.50:\n検定比 1.049 (NG)"),
            "{text}"
        );
        assert!(
            text.contains("圧縮側 t=0.000000 s  ξ=0.50:\n検定比 0.045（抽出時刻）"),
            "{text}"
        );
        assert!(
            text.contains(
                "負曲げ側 t=2.000000 s  ξ=0.50:\n検定不能: 上下非対称断面の横座屈・曲げねじり座屈"
            ),
            "{text}"
        );
        assert!(text.contains("地震動による応答成分のみ"), "{text}");
        assert!(text.contains("部材全体の適否は未判定"), "{text}");
        assert!(!text.contains("内力包絡\n"), "{text}");
        assert!(!text.contains("(OK)"), "{text}");

        let bundle = restored.core.scoped.results.as_mut().unwrap();
        let mut th = bundle.time_history_for(case_id).unwrap().clone();
        th.nonlinear = true;
        th.applied_long_term = true;
        bundle.upsert_time_history(case_id, th);
        let text = time_history_detail_labels(&mut restored);
        assert!(!text.contains("地震時 QD / 柱メカニズム"), "{text}");
        assert!(
            text.contains("長期荷重を初期状態として含む結果です"),
            "{text}"
        );
        assert!(
            text.contains("正曲げ側 t=1.000000 s  ξ=0.50:\n検定比 1.049 (NG)"),
            "{text}"
        );
    }

    #[test]
    fn asymmetric_built_h_confirmed_section_history_is_displayed_after_save_and_reopen() {
        for nonlinear in [false, true] {
            let mut app = App::default();
            let model = crate::app::steel_column_k_tests::asymmetric_built_h_cantilever();
            assert!(app.load_model(model));
            app.generate_stories_action();
            app.core.analysis_cfg.th_record_every = 3;
            app.core.analysis_cfg.th_nonlinear = nonlinear;
            app.core.analysis_cfg.th_apply_long_term = false;
            app.run_time_history(sepika_solver::dynamic::timehistory::GroundMotion {
                dt: 0.02,
                accel_x: vec![0.0, 100.0, -100.0, 0.0],
                accel_y: None,
                accel_theta: None,
            });
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            let text = time_history_detail_labels(&mut app);
            assert!(text.contains("全確定時刻の断面単独照査"), "{text}");
            assert!(text.contains("断面の検定比の下限"), "{text}");
            assert!(text.contains("曲げねじり座屈"), "{text}");
            assert!(
                !text.contains("全確定時刻の断面照査の記録を取得できません"),
                "{text}"
            );
            assert!(!text.contains("(OK)"), "{text}");

            let path = std::env::temp_dir().join(format!(
                "steel-section-history-{}.ovika",
                uuid::Uuid::new_v4()
            ));
            app.save_project_to(path.clone());
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );
            let mut restored = App::default();
            restored.open_project_from(path.clone());
            std::fs::remove_file(path).unwrap();
            assert_eq!(time_history_detail_labels(&mut restored), text);
        }
    }

    #[test]
    fn asymmetric_built_h_legacy_history_discloses_missing_section_checks_and_peaks() {
        let mut app = App::default();
        let mut model = crate::app::steel_column_k_tests::asymmetric_built_h_cantilever();
        model.steel_design_attrs[0].lk_y_direct = Some(500.0);
        model.steel_design_attrs[0].lk_z_direct = Some(500.0);
        assert!(app.load_model(model));
        // B の上縁圧縮比は 1.065。N・Mz の個別極値となる A・C・D だけでは確定できない。
        let recording = asymmetric_history(
            &[
                (-800.0, 0.0),
                (-400.0, 125.0),
                (-1.0, 130.0),
                (-1.0, -145.0),
            ],
            2,
            3,
        );
        app.core.scoped.results = Some(crate::app::ResultsBundle {
            time_history: Some(sepika_solver::dynamic::timehistory::ResponseResult {
                wall_capacities: Vec::new(),
                rc_wall_shear_method: Default::default(),
                time: vec![0.0, 1.0, 2.0, 3.0],
                peak_disp: Vec::new(),
                story_drift_angle: Vec::new(),
                history: Default::default(),
                recording: Some(recording),
                nonlinear: true,
                applied_long_term: false,
                non_converged_steps: 0,
            }),
            ..Default::default()
        });
        let text = time_history_detail_labels(&mut app);
        assert!(text.contains("長期荷重を含まない"), "{text}");
        assert!(
            text.contains("全確定時刻の断面照査の記録を取得できません"),
            "{text}"
        );
        assert!(text.contains("部材全体の適否は未判定"), "{text}");
        assert!(text.contains("正曲げ側 t=2.000000 s"), "{text}");
        assert!(!text.contains("t=1.000000 s"), "{text}");
        assert!(!text.contains("(OK)") && !text.contains("(NG)"), "{text}");

        let rec = app
            .core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .time_history
            .as_mut()
            .unwrap()
            .recording
            .as_mut()
            .unwrap();
        rec.strong_axis_moment_peaks.clear();
        let text = time_history_detail_labels(&mut app);
        assert!(text.contains("極値記録が不足しています"), "{text}");
        assert!(text.contains("解析を再実行してください"), "{text}");
        assert!(text.contains("圧縮側 t=0.000000 s"), "{text}");
        assert!(!text.contains("(OK)") && !text.contains("(NG)"), "{text}");
    }

    fn asymmetric_history(
        history: &[(f64, f64)],
        positive_step: usize,
        negative_step: usize,
    ) -> ThRecording {
        use sepika_solver::dynamic::timehistory::{
            AxialForcePeaks, ForcePeakSnapshot, StrongAxisMomentPeaks,
        };
        let forces = |n_kn, mz_knm| MemberForces {
            at: vec![(0.5, [n_kn * 1e3, 0.0, 0.0, 0.0, 0.0, mz_knm * 1e6])],
        };
        let snapshot = |step: usize| {
            let (n, mz) = history[step];
            ForcePeakSnapshot {
                time: step as f64,
                forces: forces(n, mz),
                positions: vec![0],
            }
        };
        let first = snapshot(0);
        let last = snapshot(negative_step);
        ThRecording {
            record_every: negative_step,
            frame_time: vec![first.time, last.time],
            member_forces: vec![
                vec![Some(first.forces.clone())],
                vec![Some(last.forces.clone())],
            ],
            peak_member_forces: vec![Some(forces(history[0].0, history[negative_step].1))],
            axial_force_peaks: vec![AxialForcePeaks {
                snapshots: vec![first],
            }],
            strong_axis_moment_peaks: [(
                0,
                StrongAxisMomentPeaks {
                    snapshots: vec![snapshot(positive_step), last],
                },
            )]
            .into(),
            ..Default::default()
        }
    }

    fn time_history_detail_labels(app: &mut App) -> String {
        let mut view = super::super::ViewportState::default();
        view.capture_result_sources(app);
        egui::Context::default()
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 1200.0),
                    )),
                    ..Default::default()
                },
                |ui| draw_th_detail_content_at(ui, app, ElemId(0), 0, &view),
            )
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn steel_peak_check_fills_missing_column_buckling_lengths_per_axis() {
        use crate::app::steel_column_k_tests::input;
        let mut app = App::default();
        assert!(app.load_model(input::portal(1.0)));

        for (strong, weak, lengths, ratio) in [
            (None, None, "強軸 3600 ／ 弱軸 3600", "検定比 0.956 (OK)"),
            (
                None,
                Some(3000.0),
                "強軸 3600 ／ 弱軸 3000",
                "検定比 0.956 (OK)",
            ),
            (
                Some(6983.630277482522),
                None,
                "強軸 6983.630277482522 ／ 弱軸 3600",
                "検定比 1.036 (NG)",
            ),
        ] {
            let attr = &mut app.core.model.steel_design_attrs[0];
            attr.lk_y_direct = strong;
            attr.lk_z_direct = weak;
            let text = peak_check_text(&app, 0, [-3_322_800.0, 0.0, 0.0, 0.0, 0.0, 10_000_000.0]);
            assert!(text.contains(lengths) && text.contains(ratio), "{text}");
        }
        app.core.model.steel_design_attrs[0].lk_y_direct = Some(6983.630277482522);
        app.core.model.steel_design_attrs[0].lk_z_direct = Some(3000.0);
        let text = peak_check_text(&app, 0, [-3_322_800.0, 0.0, 0.0, 0.0, 0.0, 10_000_000.0]);
        assert!(
            text.contains("強軸 6983.630277482522 ／ 弱軸 3000"),
            "{text}"
        );
        assert!(text.contains("検定比 1.036 (NG)"), "{text}");
        for direct in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            app.core.model.steel_design_attrs[0].lk_z_direct = Some(direct);
            let text = peak_check_text(&app, 0, [-3_322_800.0, 0.0, 0.0, 0.0, 0.0, 10_000_000.0]);
            assert!(text.contains("座屈長さ"), "{text}");
            assert!(!text.contains("検定比"), "{text}");
        }
    }

    #[test]
    fn time_history_detail_shows_compression_ng_even_when_envelope_and_tension_pass() {
        use crate::app::steel_column_k_tests::input;
        use sepika_solver::dynamic::timehistory::{
            AxialForcePeakSnapshot, AxialForcePeaks, ResponseResult,
        };

        let mut app = App::default();
        let mut model = input::portal(1.0);
        model.steel_design_attrs[0].lk_y_direct = Some(6_983.630_277);
        model.steel_design_attrs[0].lk_z_direct = Some(3000.0);
        assert!(app.load_model(model));
        let forces = |n, mz| MemberForces {
            at: vec![(0.0, [n, 0.0, 0.0, 0.0, 0.0, mz])],
        };
        let peak = forces(3_400_000.0, 10_000_000.0);
        let compression = forces(-3_322_800.0, 10_000_000.0);
        let tension = forces(3_400_000.0, 0.0);
        let no_force = forces(0.0, 0.0);

        for (every, frames, old_result) in [
            (1, vec![0, 1, 2, 3], false),
            (3, vec![0, 3], false),
            (3, vec![0, 3], true),
        ] {
            let history = [&no_force, &compression, &tension, &no_force];
            let recording = ThRecording {
                record_every: every,
                frame_time: frames.iter().map(|&i| i as f64).collect(),
                node_disp: vec![vec![[0.0; 6]; app.core.model.nodes.len()]; frames.len()],
                member_forces: frames
                    .iter()
                    .map(|&i| vec![Some(history[i].clone())])
                    .collect(),
                peak_member_forces: vec![Some(peak.clone())],
                axial_force_peaks: if old_result {
                    Vec::new()
                } else {
                    vec![AxialForcePeaks {
                        snapshots: vec![
                            AxialForcePeakSnapshot {
                                time: 1.0,
                                forces: compression.clone(),
                                positions: vec![0],
                            },
                            AxialForcePeakSnapshot {
                                time: 2.0,
                                forces: tension.clone(),
                                positions: vec![0],
                            },
                        ],
                    }]
                },
                ..Default::default()
            };
            app.core.scoped.results = Some(crate::app::ResultsBundle {
                time_history: Some(ResponseResult {
                    wall_capacities: Vec::new(),
                    rc_wall_shear_method: Default::default(),
                    time: vec![0.0, 1.0, 2.0, 3.0],
                    peak_disp: Vec::new(),
                    story_drift_angle: Vec::new(),
                    history: Default::default(),
                    recording: Some(recording),
                    nonlinear: false,
                    applied_long_term: false,
                    non_converged_steps: 0,
                }),
                ..Default::default()
            });
            let view = super::super::ViewportState::default();
            let output = egui::Context::default().run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 1200.0),
                    )),
                    ..Default::default()
                },
                |ui| draw_th_detail_content_at(ui, &mut app, ElemId(0), 0, &view),
            );
            let text: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(text.contains(&"検定比 0.949 (OK)"), "{text:?}");
            if old_result {
                assert!(
                    text.iter().any(|label| label.contains("解析を再実行")),
                    "{text:?}"
                );
                assert!(!text.contains(&"検定比 1.036 (NG)"), "{text:?}");
            } else {
                assert!(
                    text.windows(2)
                        .any(|labels| labels
                            == ["圧縮側 t=1.000000 s  ξ=0.00:", "検定比 1.036 (NG)"]),
                    "{text:?}"
                );
                assert!(
                    text.windows(2)
                        .any(|labels| labels
                            == ["引張側 t=2.000000 s  ξ=0.00:", "検定比 0.927 (OK)"]),
                    "{text:?}"
                );
            }
        }
    }

    #[test]
    fn steel_peak_check_girder_and_ordinary_brace_use_node_span_for_unset_axis() {
        use crate::app::steel_column_k_tests::input;
        use sepika_core::model::FrameSectionUse;

        let mut model = input::portal(1.0);
        model.elements[2].end_offsets_mm = [[100.0, 200.0, 0.0], [600.0, 200.0, 0.0]];
        let mut attr = model.steel_design_attrs[0].clone();
        attr.elem = ElemId(2);
        attr.lk_y_direct = Some(3000.0);
        model.steel_design_attrs.push(attr);
        let mut app = App::default();
        assert!(app.load_model(model));

        for (kind, usage) in [
            (ElementKind::Beam, FrameSectionUse::Girder),
            (
                ElementKind::Brace {
                    tension_only: false,
                },
                FrameSectionUse::Brace,
            ),
        ] {
            app.core.model.elements[2].kind = kind;
            app.core.model.sections[1].frame_use = Some(usage);
            for (weak, expected) in [
                (None, "強軸 3000 ／ 弱軸 3000"),
                (Some(1000.0), "強軸 3000 ／ 弱軸 1000"),
            ] {
                app.core.model.steel_design_attrs[2].lk_z_direct = weak;
                let text = peak_check_text(&app, 2, [-100_000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
                assert!(text.contains(expected), "{text}");
                assert!(text.contains("検定比"), "{text}");
            }
        }
    }

    #[test]
    fn steel_beam_time_history_without_a_verified_moment_diagram_keeps_independent_ng() {
        use crate::app::steel_beam_span_tests::input;
        let mut app = App::default();
        assert!(app.load_model(input::cantilever(0.0, 0.0, None)));
        let recording = ThRecording {
            peak_member_forces: vec![Some(MemberForces {
                at: vec![
                    (0.0, [0.0, 0.0, 0.0, 0.0, 0.0, 400e6]),
                    (1.0, [1_400_000.0, 0.0, 0.0, 0.0, 0.0, -130e6]),
                ],
            })],
            ..Default::default()
        };
        let text = peak_recording_text(&app, &app.core.model.elements[0], 0, &recording, false);
        assert!(
            text.contains("検定不能: 単一補剛区間の強軸曲げ分布を確認できず"),
            "{text}"
        );
        assert!(text.contains("検定比 1.482 (NG)"), "{text}");
        assert_eq!(text.matches("(NG)").count(), 1, "{text}");
        assert!(!text.contains("(OK)"), "{text}");
    }

    #[test]
    fn cft_peak_check_renders_missing_axis_or_uses_the_direct_lengths() {
        use crate::app::cft_column_buckling_tests::input;
        let mut app = App::default();
        assert!(app.load_model(input::cantilever(false, 7_450_000.0)));
        let rec = ThRecording {
            peak_member_forces: vec![Some(MemberForces {
                at: vec![(0.0, [-7_450_000.0, 0.0, 0.0, 0.0, 0.0, 0.0])],
            })],
            ..Default::default()
        };
        for (weak, expected) in [
            (None, "弱軸の座屈長さを有限な正値で直接指定"),
            (Some(6000.0), "検定比 1.185 (NG)"),
        ] {
            app.core.model.steel_design_attrs =
                vec![input::direct_lengths(ElemId(0), Some(1000.0), weak)];
            let elem = &app.core.model.elements[0];
            let axis = super::super::member_axis_endpoints(elem, &app.core.model).unwrap();
            let output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
                draw_peak_check(
                    ui,
                    &app,
                    elem,
                    0,
                    &rec,
                    false,
                    false,
                    || Some(UNVERIFIED_DAMAGE_CONTROL_GRAVITY),
                    || unreachable!(),
                    &axis,
                )
            });
            let text = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains(expected), "{text}");
            if weak.is_none() {
                assert!(!text.contains("検定比"), "{text}");
            }
        }
    }

    #[test]
    fn cft_missing_buckling_length_and_torsion_reasons_are_both_shown() {
        use crate::app::cft_column_buckling_tests::input;
        let mut app = App::default();
        assert!(app.load_model(input::cantilever(false, 0.0)));
        app.core.model.steel_design_attrs =
            vec![input::direct_lengths(ElemId(0), Some(1000.0), None)];
        let forces = MemberForces {
            at: vec![(0.0, [-100_000.0, 0.0, 0.0, 1.0e6, 0.0, 0.0])],
        };
        let recording = ThRecording {
            peak_member_forces: vec![Some(forces.clone())],
            axial_force_peaks: vec![AxialForcePeaks {
                snapshots: vec![ForcePeakSnapshot {
                    time: 1.0,
                    forces,
                    positions: vec![0],
                }],
            }],
            ..Default::default()
        };
        let text = peak_recording_text(&app, &app.core.model.elements[0], 0, &recording, false);
        for label in ["ξ=0.00:", "圧縮側 t=1.000000 s  ξ=0.00:"] {
            assert!(
                text.lines()
                    .zip(text.lines().skip(1))
                    .any(|(line, reason)| line == label
                        && reason.contains("弱軸の座屈長さ")
                        && reason.contains("ねじりモーメントMx")),
                "{text}"
            );
        }
        assert!(!text.contains("検定比"), "{text}");
    }

    #[test]
    fn incomplete_checks_th_detail_renders_unrated_reasons_and_known_ng() {
        for (ratios, expected) in [
            (vec![], "未評価:"),
            (vec![f64::NAN], "未評価:"),
            (vec![f64::INFINITY], "未評価:"),
            (vec![f64::NEG_INFINITY], "未評価:"),
            (vec![-0.5], "未評価:"),
            (vec![0.4, f64::NAN], "未評価:"),
            (vec![1.25, f64::NAN], "検定比 1.250 (NG)"),
            (vec![0.0], "検定比 0.000 (OK)"),
            (vec![0.5], "検定比 0.500 (OK)"),
            (vec![1.0], "検定比 1.000 (OK)"),
            (vec![1.25], "検定比 1.250 (NG)"),
        ] {
            let outcome = CheckOutcome::Checked(sepika_design_jp::CheckResult {
                basis: String::new(),
                detail: String::new(),
                components: ratios
                    .iter()
                    .map(|ratio| sepika_design_jp::CheckComponent {
                        kind: sepika_design_jp::CheckKind::AxialBending,
                        ratio: *ratio,
                        detail: String::new(),
                    })
                    .collect(),
            });
            let output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
                draw_outcome_row(ui, 0.5, &outcome)
            });
            let text = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains(expected), "{ratios:?}: {text}");
            if ratios.is_empty() || ratios.iter().any(|r| !r.is_finite() || *r < 0.0) {
                assert!(
                    text.contains(crate::app::INVALID_CHECK_COMPONENTS),
                    "{text}"
                );
                assert!(!text.contains("(OK)"), "{text}");
            } else {
                assert!(
                    !text.contains(crate::app::INVALID_CHECK_COMPONENTS),
                    "{text}"
                );
            }
        }
    }

    /// X 軸方向部材が両端とも X 方向へ同じだけ動けば相対変位ゼロ（剛体並進）。
    #[test]
    fn axial_relative_disp_rigid_translation_is_zero() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        let d = [5.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert!((axial_relative_disp(p_i, p_j, d, d)).abs() < 1e-9);
    }

    /// j 端だけ材軸方向(X)へ 2mm 動けば δ=+2mm（引張伸び）。
    #[test]
    fn axial_relative_disp_elongation_positive() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        let d_i = [0.0; 6];
        let d_j = [2.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let delta = axial_relative_disp(p_i, p_j, d_i, d_j);
        assert!((delta - 2.0).abs() < 1e-9);
    }

    /// 材軸に直交する変位は軸方向相対変位に寄与しない。
    #[test]
    fn axial_relative_disp_transverse_disp_ignored() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        let d_i = [0.0; 6];
        let d_j = [0.0, 3.0, 0.0, 0.0, 0.0, 0.0];
        assert!(axial_relative_disp(p_i, p_j, d_i, d_j).abs() < 1e-9);
    }

    /// ゼロ長部材は 0 を返す（材軸が定まらない防御的ケース）。
    #[test]
    fn axial_relative_disp_zero_length_returns_zero() {
        let p = [1.0, 2.0, 3.0];
        let d = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(axial_relative_disp(p, p, [0.0; 6], d), 0.0);
    }

    // ref_vector=[0,1,0]のX軸材ではey=全体Y・ez=全体Zとなり、回転成分の手計算をそのまま比較できる。

    /// 剛体並進（弦回転も端部回転もゼロ）なら材端回転もゼロ。
    #[test]
    fn beam_end_rotations_rigid_translation_is_zero() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        let d = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let (ry_i, rz_i, ry_j, rz_j) = beam_end_rotations(p_i, p_j, [0.0, 1.0, 0.0], d, d).unwrap();
        assert!(ry_i.abs() < 1e-9);
        assert!(rz_i.abs() < 1e-9);
        assert!(ry_j.abs() < 1e-9);
        assert!(rz_j.abs() < 1e-9);
    }

    /// 弦回転どおりに材端が回転していれば（弦に沿った素直な傾き、曲げなし）、
    /// 弦成分が打ち消し合い材端回転（たわみ角）はゼロになる。
    #[test]
    fn beam_end_rotations_matching_chord_cancels() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        // j 端を Y 方向へ 10mm 変位（弦回転 = 10/1000 = 0.01 rad、Z軸=ez まわり）。
        // 両端の節点回転（Z軸まわり）も同じ弦回転角にしておくと、曲率ゼロの
        // 剛体的な傾きになり、たわみ角は両端とも 0 になるはず。
        let chord = 10.0 / 1000.0;
        let d_i = [0.0, 0.0, 0.0, 0.0, 0.0, chord];
        let d_j = [0.0, 10.0, 0.0, 0.0, 0.0, chord];
        let (_, rz_i, _, rz_j) = beam_end_rotations(p_i, p_j, [0.0, 1.0, 0.0], d_i, d_j).unwrap();
        assert!(rz_i.abs() < 1e-9, "rz_i={rz_i}");
        assert!(rz_j.abs() < 1e-9, "rz_j={rz_j}");
    }

    /// 弦は動かず材端だけ回転していれば、その回転がそのままたわみ角になる。
    #[test]
    fn beam_end_rotations_pure_end_rotation() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        let d_i = [0.0; 6];
        let d_j = [0.0, 0.0, 0.0, 0.0, 0.0, 0.02];
        let (_, rz_i, _, rz_j) = beam_end_rotations(p_i, p_j, [0.0, 1.0, 0.0], d_i, d_j).unwrap();
        assert!(rz_i.abs() < 1e-9);
        assert!((rz_j - 0.02).abs() < 1e-9);
    }

    #[test]
    fn loop_kind_of_classifies_axial_and_flexural() {
        assert!(matches!(
            loop_kind_of(&ElementKind::Brace {
                tension_only: false
            }),
            LoopKind::Axial
        ));
        assert!(matches!(
            loop_kind_of(&ElementKind::Damper),
            LoopKind::Axial
        ));
        assert!(matches!(
            loop_kind_of(&ElementKind::NodalSpring),
            LoopKind::Axial
        ));
        assert!(matches!(
            loop_kind_of(&ElementKind::Beam),
            LoopKind::Flexural
        ));
        assert!(matches!(
            loop_kind_of(&ElementKind::Wall),
            LoopKind::Unsupported
        ));
    }

    #[test]
    fn zero_length_spring_loop_uses_global_translation_and_force_components() {
        let recording = ThRecording {
            frame_time: vec![0.0],
            node_disp: vec![vec![
                [1.0, 2.0, 3.0, 0.0, 0.0, 0.0],
                [3.0, 6.0, 0.0, 0.0, 0.0, 0.0],
            ]],
            member_forces: vec![vec![Some(MemberForces {
                at: vec![(0.0, [1000.0, 2000.0, -3000.0, 0.0, 0.0, 0.0])],
            })]],
            ..Default::default()
        };
        for (component, expected) in [
            (AxialComponent::Axial, [2.0, 1.0]),
            (AxialComponent::ShearY, [4.0, 2.0]),
            (AxialComponent::ShearZ, [-3.0, -3.0]),
        ] {
            assert_eq!(
                axial_point(&recording, 0, 0, 1, [0.0; 3], [0.0; 3], component, 0),
                Some(expected)
            );
        }
    }

    /// テスト用の弾性梁要素（X 軸材、ref_vector=[0,1,0]。`beam_end_rotations` の
    /// テストと同じ設定で ey=グローバルY・ez=グローバルZ とし、期待値を
    /// 手計算しやすくする）。`sepika_element::frame::beam::tests::make_test_beam`
    /// と同じ諸元。
    fn cantilever_test_beam() -> sepika_element::frame::beam::BeamElement {
        let mut model = crate::sample::portal_frame();
        model.nodes[2].coord = [0.0, 0.0, 0.0];
        model.nodes[3].coord = [1000.0, 0.0, 0.0];
        model.elements[2].local_axis.ref_vector = [0.0, 1.0, 0.0];
        sepika_element::frame::beam::BeamElement::new(&model.elements[2], &model)
    }

    /// 高-1: `flexural_points` の i端 M-θ 勾配が正になること（節点端モーメント
    /// 規約への統一を検証）。
    ///
    /// i端固定・j端自由の弾性片持ち梁（強軸 Mz 面）相当の変位を与える。荷重
    /// パラメータ P に対する理論たわみ・たわみ角 `v_j=PL³/3EI`・`θ_j=PL²/2EI`
    /// を節点変位として与え（`d_i=0`）、`BeamElement::recover_forces`（FE の
    /// 弾性解＝静定片持ち梁の理論解と一致）で内力を復元する。
    ///
    /// P を増加させた複数フレームで i端の (θ_i, M_i) を `flexural_points` から
    /// 取り出し、隣接フレーム間の勾配 ΔM/Δθ が正であることを確認する
    /// （修正前は `end_moments` が i端で符号反転しておらず、断面内力規約のまま
    /// θ と符号が逆転し負勾配になっていた）。
    #[test]
    fn flexural_points_i_end_slope_is_positive_for_elastic_cantilever() {
        let beam = cantilever_test_beam();
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        let ref_vector = [0.0, 1.0, 0.0];
        let l = 1000.0_f64;
        let ei = beam.e * beam.iz;

        let loads = [1.0_f64, 2.0, 3.0];
        let mut frame_time = Vec::new();
        let mut node_disp = Vec::new();
        let mut member_forces = Vec::new();
        for (f, &p) in loads.iter().enumerate() {
            let v_j = p * l.powi(3) / (3.0 * ei);
            let theta_j = p * l.powi(2) / (2.0 * ei);
            let d_i = [0.0; 6];
            let d_j = [0.0, v_j, 0.0, 0.0, 0.0, theta_j];
            let u_elem: [f64; 12] = [
                d_i[0], d_i[1], d_i[2], d_i[3], d_i[4], d_i[5], d_j[0], d_j[1], d_j[2], d_j[3],
                d_j[4], d_j[5],
            ];
            let mf = beam.recover_forces(&u_elem);
            frame_time.push(f as f64);
            node_disp.push(vec![d_i, d_j]);
            member_forces.push(vec![Some(mf)]);
        }
        let rec = ThRecording {
            frame_time,
            node_disp,
            member_forces,
            ..Default::default()
        };

        let mut i_pts = Vec::new();
        for f in 0..loads.len() {
            let (pi, _pj) = flexural_points(&rec, 0, 0, 1, p_i, p_j, ref_vector, true, f)
                .expect("各フレームに内力・変位の記録があるはず");
            i_pts.push(pi);
        }

        for w in i_pts.windows(2) {
            let d_theta = w[1][0] - w[0][0];
            let d_m = w[1][1] - w[0][1];
            assert!(
                d_theta.abs() > 1e-12,
                "theta_i が変化していません: {i_pts:?}"
            );
            let slope = d_m / d_theta;
            assert!(
                slope > 0.0,
                "i端の M-θ 勾配が正ではありません: slope={slope} i_pts={i_pts:?}"
            );
        }
    }

    #[test]
    fn 不正な参照方向の梁端回転は生成しない() {
        assert!(beam_end_rotations(
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [f64::NAN, 0.0, 0.0],
            [0.0; 6],
            [0.0; 6],
        )
        .is_none());
    }
}
