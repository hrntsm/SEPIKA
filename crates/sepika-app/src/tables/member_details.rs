//! 部材の付帯情報（`Model.member_detail_attrs` = `MemberDetailAttr`: ハンチ・継手
//! 位置）の編集 UI。対象は `ElementKind::Beam` の部材のみ（本ソフトでは柱も梁も
//! `ElementKind::Beam` で表現するフレーム部材のため、実質すべての一般部材が対象）。
//! 編集は `sepika_edit::{SetMemberDetailAttr, RemoveMemberDetailAttr}` 経由
//! （undo 対応）。空（ハンチなし・継手なし）を適用した場合は削除として扱う
//! （[`MemberDetailAttr::is_empty`]）。
//!
//! ここで設定した付帯情報は剛性・応力解析には影響せず、断面算定の検定位置
//! 追加（`sepika_app::app::design_positions`）と `sepika_element` の評価断面
//! 追加に用いられる。

use crate::app::App;
use sepika_core::ids::ElemId;
use sepika_core::model::{
    ElementKind, Haunch, HaunchQd1Face, HaunchQd1Tension, JointKind, MemberDetailAttr, MemberJoint,
};
use sepika_edit::{RemoveMemberDetailAttr, SetMemberDetailAttr};

/// 部材付帯情報フォームのドラフト状態（GUI 専用）。
/// 対象部材を選択すると `synced_for` の部材の現在値でバッファを初期化し、
/// 「適用」で `SetMemberDetailAttr`（空なら `RemoveMemberDetailAttr`）を発行する。
#[derive(Clone, Debug, Default)]
pub struct MemberDetailDraft {
    /// 編集対象の部材要素。
    pub elem: Option<ElemId>,
    /// バッファを初期化した対象（`elem` と異なれば model 値で再同期する）。
    pub synced_for: Option<ElemId>,
    /// i 端ハンチの有無。
    pub haunch_i_enabled: bool,
    /// i 端ハンチ長 `[mm]` の入力バッファ。
    pub haunch_i_length: String,
    /// i 端ハンチのせい増分 `[mm]` の入力バッファ（空欄可、既定 0）。
    pub haunch_i_depth: String,
    /// i 端ハンチの幅増分 `[mm]` の入力バッファ（空欄可、既定 0）。
    pub haunch_i_width: String,
    /// j 端ハンチの有無。
    pub haunch_j_enabled: bool,
    /// j 端ハンチ長 `[mm]` の入力バッファ。
    pub haunch_j_length: String,
    /// j 端ハンチのせい増分 `[mm]` の入力バッファ（空欄可、既定 0）。
    pub haunch_j_depth: String,
    /// j 端ハンチの幅増分 `[mm]` の入力バッファ（空欄可、既定 0）。
    pub haunch_j_width: String,
    qd1_face_i: HaunchQd1FaceDraft,
    qd1_face_j: HaunchQd1FaceDraft,
    /// 継手一覧の入力バッファ（1行1件または「,」区切り、`距離` または
    /// `距離/種別`。種別省略時は現場継手）。
    pub joints: String,
}

impl MemberDetailDraft {
    fn load(&mut self, attr: &MemberDetailAttr) {
        self.elem = Some(attr.elem);
        self.synced_for = Some(attr.elem);
        self.haunch_i_enabled = attr.haunch_i.is_some();
        (
            self.haunch_i_length,
            self.haunch_i_depth,
            self.haunch_i_width,
        ) = attr
            .haunch_i
            .map(|h| {
                (
                    h.length.to_string(),
                    h.depth_increase.to_string(),
                    h.width_increase.to_string(),
                )
            })
            .unwrap_or_default();
        self.haunch_j_enabled = attr.haunch_j.is_some();
        (
            self.haunch_j_length,
            self.haunch_j_depth,
            self.haunch_j_width,
        ) = attr
            .haunch_j
            .map(|h| {
                (
                    h.length.to_string(),
                    h.depth_increase.to_string(),
                    h.width_increase.to_string(),
                )
            })
            .unwrap_or_default();
        self.joints = format_joints(&attr.joints);
        self.qd1_face_i = HaunchQd1FaceDraft::from_face(attr.haunch_i.and_then(|h| h.qd1_face));
        self.qd1_face_j = HaunchQd1FaceDraft::from_face(attr.haunch_j.and_then(|h| h.qd1_face));
    }
}

#[derive(Clone, Debug, Default)]
struct HaunchQd1FaceDraft {
    confirmed: bool,
    dimensions: [String; 3],
    top: [String; 3],
    bottom: [String; 3],
}

const FACE_DIMENSIONS: [&str; 3] = ["解析材端からフェースまで [mm]", "実幅 [mm]", "実せい [mm]"];
const TENSION_VALUES: [&str; 3] = [
    "有効引張鉄筋面積 [mm²]",
    "実降伏応力度 [N/mm²]",
    "圧縮縁からの有効せい [mm]",
];

impl HaunchQd1FaceDraft {
    fn from_face(face: Option<HaunchQd1Face>) -> Self {
        let Some(face) = face else {
            return Self::default();
        };
        let tension = |t: HaunchQd1Tension| {
            [t.area_mm2, t.yield_strength_n_mm2, t.effective_depth_mm].map(|v| v.to_string())
        };
        Self {
            confirmed: true,
            dimensions: [face.distance_mm, face.width_mm, face.depth_mm].map(|v| v.to_string()),
            top: tension(face.top),
            bottom: tension(face.bottom),
        }
    }

    fn apply(&self, haunch: Option<Haunch>) -> Result<Option<Haunch>, String> {
        let Some(mut haunch) = haunch else {
            return Ok(None);
        };
        if self.confirmed {
            let [distance_mm, width_mm, depth_mm] =
                parse_qd1_values(&self.dimensions, FACE_DIMENSIONS)?;
            let tension = |side, values: &[String; 3]| {
                let [area_mm2, yield_strength_n_mm2, effective_depth_mm] =
                    parse_qd1_values(values, TENSION_VALUES)
                        .map_err(|error| format!("{side}引張: {error}"))?;
                Ok::<_, String>(HaunchQd1Tension {
                    area_mm2,
                    yield_strength_n_mm2,
                    effective_depth_mm,
                })
            };
            let face = HaunchQd1Face {
                distance_mm,
                width_mm,
                depth_mm,
                top: tension("上端", &self.top)?,
                bottom: tension("下端", &self.bottom)?,
            };
            if let Some(reason) = face.input_issue() {
                return Err(reason.into());
            }
            haunch.qd1_face = Some(face);
        }
        Ok(Some(haunch))
    }

    fn draw(&mut self, ui: &mut egui::Ui, end: &str) {
        ui.indent(("haunch-qd1", end), |ui| {
            ui.checkbox(
                &mut self.confirmed,
                format!("{end}の短期QD1用の矩形材端フェース・有効主筋を確認済み"),
            );
            if !self.confirmed {
                return;
            }
            egui::Grid::new(("haunch-qd1-face", end)).show(ui, |ui| {
                draw_qd1_values(ui, FACE_DIMENSIONS, &mut self.dimensions);
                ui.strong("上端引張");
                ui.end_row();
                draw_qd1_values(ui, TENSION_VALUES, &mut self.top);
                ui.strong("下端引張");
                ui.end_row();
                draw_qd1_values(ui, TENSION_VALUES, &mut self.bottom);
            });
        });
    }
}

fn parse_qd1_values(values: &[String; 3], labels: [&str; 3]) -> Result<[f64; 3], String> {
    let mut parsed = [0.0; 3];
    for ((value, label), out) in values.iter().zip(labels).zip(&mut parsed) {
        *out = value
            .trim()
            .parse()
            .map_err(|_| format!("{label}に数値を入力してください"))?;
    }
    Ok(parsed)
}

fn draw_qd1_values(ui: &mut egui::Ui, labels: [&str; 3], values: &mut [String; 3]) {
    for (label, value) in labels.into_iter().zip(values) {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(value).desired_width(90.0));
        ui.end_row();
    }
}

/// 継手種別のラベル（往復変換に使う固定表記）。
fn joint_kind_label(kind: JointKind) -> &'static str {
    match kind {
        JointKind::Site => "現場",
        JointKind::Shop => "工場",
    }
}

/// 継手種別を表す文字列（`現場`/`工場`/`site`/`shop`、大小文字・前後空白を許容）を
/// パースする。空文字列は既定（現場継手）とする。
fn parse_joint_kind(s: &str) -> Result<JointKind, String> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("現場") || s.eq_ignore_ascii_case("site") {
        Ok(JointKind::Site)
    } else if s.eq_ignore_ascii_case("工場") || s.eq_ignore_ascii_case("shop") {
        Ok(JointKind::Shop)
    } else {
        Err(format!(
            "不正な継手種別「{s}」: '現場' または '工場' を指定してください"
        ))
    }
}

/// 継手1件分の入力（`距離` または `距離/種別`）をパースする（egui 非依存の純関数）。
fn parse_single_joint(entry: &str) -> Result<MemberJoint, String> {
    let (dist_str, kind_str) = match entry.split_once('/') {
        Some((d, k)) => (d, Some(k)),
        None => (entry, None),
    };
    let distance: f64 = dist_str
        .trim()
        .parse()
        .map_err(|_| format!("不正な継手位置「{}」: 数値ではありません", dist_str.trim()))?;
    if !distance.is_finite() {
        return Err("継手位置は有限の値で入力してください".to_string());
    }
    if distance <= 0.0 {
        return Err(format!(
            "継手位置「{entry}」: 距離(始端節点芯から)は正の値で入力してください"
        ));
    }
    let kind = parse_joint_kind(kind_str.unwrap_or(""))?;
    Ok(MemberJoint { distance, kind })
}

/// 継手入力バッファ（1行1件または「,」区切り、`距離` または `距離/種別`）を
/// パースする（egui 非依存の純関数。`wall_plates::parse_openings` の流儀）。
pub fn parse_joints(s: &str) -> Result<Vec<MemberJoint>, String> {
    let normalized = s.replace('\n', ",");
    normalized
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(parse_single_joint)
        .collect()
}

/// 継手リストを入力バッファ書式（1行1件、`距離/種別`）へ整形する
/// （`parse_joints` の逆変換）。既存値をフォームへ読み込む際に使用する。
pub fn format_joints(joints: &[MemberJoint]) -> String {
    joints
        .iter()
        .map(|j| format!("{}/{}", j.distance, joint_kind_label(j.kind)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// ハンチ1端分の入力（有効フラグ・長さ・せい増分・幅増分）をパースする
/// （egui 非依存の純関数）。無効なら `Ok(None)`。長さは正の値が必須、
/// 各寸法は有限値を要し、せい増分・幅増分は空欄なら 0 とする。
pub fn parse_haunch(
    enabled: bool,
    length: &str,
    depth_increase: &str,
    width_increase: &str,
) -> Result<Option<Haunch>, String> {
    if !enabled {
        return Ok(None);
    }
    let length: f64 = length
        .trim()
        .parse()
        .map_err(|_| format!("不正なハンチ長「{}」: 数値ではありません", length.trim()))?;
    if !length.is_finite() {
        return Err("ハンチ長は有限の値で入力してください".to_string());
    }
    if length <= 0.0 {
        return Err("ハンチ長は正の値で入力してください".to_string());
    }
    let parse_opt = |s: &str, label: &str| -> Result<f64, String> {
        let s = s.trim();
        if s.is_empty() {
            Ok(0.0)
        } else {
            let value: f64 = s
                .parse()
                .map_err(|_| format!("不正な{label}「{s}」: 数値ではありません"))?;
            if !value.is_finite() {
                return Err(format!("{label}は有限の値で入力してください"));
            }
            Ok(value)
        }
    };
    let depth_increase = parse_opt(depth_increase, "せい増分")?;
    let width_increase = parse_opt(width_increase, "幅増分")?;
    Ok(Some(Haunch {
        length,
        depth_increase,
        width_increase,
        qd1_face: None,
    }))
}

/// ハンチの表示文字列（一覧行用）。`None` は「なし」。
fn haunch_desc(h: &Option<Haunch>) -> String {
    match h {
        Some(h) => format!(
            "長さ{:.0} せい+{:.0} 幅+{:.0}{}",
            h.length,
            h.depth_increase,
            h.width_increase,
            if h.qd1_face.is_some() {
                "（QD1入力あり）"
            } else {
                ""
            }
        ),
        None => "なし".to_string(),
    }
}

fn apply_member_detail(app: &mut App, attr: MemberDetailAttr) -> bool {
    if attr.is_empty() {
        app.apply_model_edit(Box::new(RemoveMemberDetailAttr { elem: attr.elem }))
    } else {
        let command = SetMemberDetailAttr { attr }.command_for(&app.core.model);
        app.apply_model_edit(command)
    }
}

pub fn member_details_table(ui: &mut egui::Ui, app: &mut App) {
    ui.label(
        "部材(柱・梁)の付帯情報(ハンチ・継手位置)を設定します。剛性・応力解析には\
         影響せず、検定位置の追加、RC梁の短期QD1用の材端情報、数量拾いに用います。",
    );
    ui.separator();

    let target_elems: Vec<ElemId> = app
        .core
        .model
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Beam)
        .map(|e| e.id)
        .collect();

    if target_elems.is_empty() {
        ui.label("対象となる部材(柱・梁)がありません。");
        return;
    }

    let mut pending_remove: Option<ElemId> = None;
    let mut pending_edit: Option<MemberDetailAttr> = None;
    if app.core.model.member_detail_attrs.is_empty() {
        ui.label(
            "設定済みの付帯情報はありません(未設定の部材はハンチ・継手なしとして扱われます)。",
        );
    } else {
        for attr in &app.core.model.member_detail_attrs {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "部材#{}: i端ハンチ[{}] / j端ハンチ[{}] / 継手{}箇所",
                    attr.elem.0,
                    haunch_desc(&attr.haunch_i),
                    haunch_desc(&attr.haunch_j),
                    attr.joints.len(),
                ));
                if ui
                    .button("✏")
                    .on_hover_text("フォームへ読み込んで編集")
                    .clicked()
                {
                    pending_edit = Some(attr.clone());
                }
                if ui.button("🗑").on_hover_text("この付帯情報を削除").clicked() {
                    pending_remove = Some(attr.elem);
                }
            });
        }
    }
    if let Some(attr) = pending_edit {
        app.ui.scoped.member_detail_draft.load(&attr);
    }
    if let Some(elem) = pending_remove {
        app.apply_model_edit(Box::new(RemoveMemberDetailAttr { elem }));
    }

    ui.separator();
    ui.strong("付帯情報を設定");

    ui.horizontal(|ui| {
        ui.label("対象部材:");
        let text = app
            .ui
            .scoped
            .member_detail_draft
            .elem
            .map(|e| format!("部材#{}", e.0))
            .unwrap_or_else(|| "―".to_string());
        egui::ComboBox::from_id_salt("member_detail_elem")
            .selected_text(text)
            .show_ui(ui, |ui| {
                for &eid in &target_elems {
                    if ui
                        .selectable_label(
                            app.ui.scoped.member_detail_draft.elem == Some(eid),
                            format!("部材#{}", eid.0),
                        )
                        .clicked()
                    {
                        app.ui.scoped.member_detail_draft.elem = Some(eid);
                    }
                }
            });
    });
    if app.ui.scoped.member_detail_draft.elem != app.ui.scoped.member_detail_draft.synced_for {
        if let Some(eid) = app.ui.scoped.member_detail_draft.elem {
            let existing = app
                .core
                .model
                .member_detail(eid)
                .cloned()
                .unwrap_or(MemberDetailAttr {
                    elem: eid,
                    haunch_i: None,
                    haunch_j: None,
                    joints: Vec::new(),
                });
            app.ui.scoped.member_detail_draft.load(&existing);
        }
    }

    let selected_rc_beam = app
        .ui
        .scoped
        .member_detail_draft
        .elem
        .and_then(|id| app.core.model.element(id))
        .and_then(|elem| app.core.model.element_section(elem))
        .is_some_and(|sec| {
            matches!(
                sec.shape,
                Some(sepika_core::section_shape::SectionShape::RcBeamRect { .. })
            )
        });
    let draft = &app.ui.scoped.member_detail_draft;
    if selected_rc_beam || draft.qd1_face_i.confirmed || draft.qd1_face_j.confirmed {
        ui.label(
            "短期QD1用の入力は、梁軸に直交する矩形の実フェース、軸力の影響を無視できること、\
             梁軸に平行な全有効引張主筋の付着・定着を設計者が図面等で確認した場合に限ります。\
             斜行する有効主筋がある場合、または一つの引張側に異なる降伏点の筋が混在する場合は指定できません。\
             入力はハンチ区間・接合部の適合を示しません。",
        );
    }
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut app.ui.scoped.member_detail_draft.haunch_i_enabled,
            "i端ハンチ",
        );
        ui.add_enabled_ui(app.ui.scoped.member_detail_draft.haunch_i_enabled, |ui| {
            ui.label("長さ[mm]:");
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.member_detail_draft.haunch_i_length)
                    .desired_width(70.0),
            );
            ui.label("せい増分[mm]:");
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.member_detail_draft.haunch_i_depth)
                    .desired_width(70.0),
            );
            ui.label("幅増分[mm]:");
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.member_detail_draft.haunch_i_width)
                    .desired_width(70.0),
            );
        });
    });
    let draft = &mut app.ui.scoped.member_detail_draft;
    if draft.haunch_i_enabled && (selected_rc_beam || draft.qd1_face_i.confirmed) {
        draft.qd1_face_i.draw(ui, "i端");
    }
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut app.ui.scoped.member_detail_draft.haunch_j_enabled,
            "j端ハンチ",
        );
        ui.add_enabled_ui(app.ui.scoped.member_detail_draft.haunch_j_enabled, |ui| {
            ui.label("長さ[mm]:");
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.member_detail_draft.haunch_j_length)
                    .desired_width(70.0),
            );
            ui.label("せい増分[mm]:");
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.member_detail_draft.haunch_j_depth)
                    .desired_width(70.0),
            );
            ui.label("幅増分[mm]:");
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.member_detail_draft.haunch_j_width)
                    .desired_width(70.0),
            );
        });
    });

    let draft = &mut app.ui.scoped.member_detail_draft;
    if draft.haunch_j_enabled && (selected_rc_beam || draft.qd1_face_j.confirmed) {
        draft.qd1_face_j.draw(ui, "j端");
    }
    ui.label("継手一覧(始端節点芯からの距離[mm]。任意で「/現場」「/工場」を付記):");
    ui.add(
        egui::TextEdit::multiline(&mut app.ui.scoped.member_detail_draft.joints)
            .desired_rows(3)
            .desired_width(320.0)
            .hint_text(
                "距離 または 距離/種別\n\
                 複数継手は改行または「,」区切りで入力\n\
                 例: 1000/現場, 3000/工場",
            ),
    )
    .on_hover_text(
        "1行1件または「,」区切りで '距離' もしくは種別付き '距離/現場'・'距離/工場' を\
         入力します。種別省略時は現場継手として扱います。",
    );

    let parsed_haunch_i = parse_haunch(
        app.ui.scoped.member_detail_draft.haunch_i_enabled,
        &app.ui.scoped.member_detail_draft.haunch_i_length,
        &app.ui.scoped.member_detail_draft.haunch_i_depth,
        &app.ui.scoped.member_detail_draft.haunch_i_width,
    )
    .and_then(|haunch| app.ui.scoped.member_detail_draft.qd1_face_i.apply(haunch));
    let parsed_haunch_j = parse_haunch(
        app.ui.scoped.member_detail_draft.haunch_j_enabled,
        &app.ui.scoped.member_detail_draft.haunch_j_length,
        &app.ui.scoped.member_detail_draft.haunch_j_depth,
        &app.ui.scoped.member_detail_draft.haunch_j_width,
    )
    .and_then(|haunch| app.ui.scoped.member_detail_draft.qd1_face_j.apply(haunch));
    let parsed_joints = parse_joints(&app.ui.scoped.member_detail_draft.joints);
    for (label, r) in [
        ("i端ハンチ", &parsed_haunch_i),
        ("j端ハンチ", &parsed_haunch_j),
    ] {
        if let Err(e) = r {
            ui.colored_label(crate::theme::ERROR_RED, format!("{label}の書式エラー: {e}"));
        }
    }
    if let Err(e) = &parsed_joints {
        ui.colored_label(crate::theme::ERROR_RED, format!("継手の書式エラー: {e}"));
    }

    let can_apply = app.ui.scoped.member_detail_draft.elem.is_some()
        && parsed_haunch_i.is_ok()
        && parsed_haunch_j.is_ok()
        && parsed_joints.is_ok();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(can_apply, egui::Button::new("✔ 適用"))
            .on_hover_text(
                "選択した部材にハンチ・継手位置を設定します(すべて空の場合は削除します。undo可)",
            )
            .clicked()
        {
            if let (Some(elem), Ok(haunch_i), Ok(haunch_j), Ok(joints)) = (
                app.ui.scoped.member_detail_draft.elem,
                parsed_haunch_i,
                parsed_haunch_j,
                parsed_joints,
            ) {
                apply_member_detail(
                    app,
                    MemberDetailAttr {
                        elem,
                        haunch_i,
                        haunch_j,
                        joints,
                    },
                );
            }
        }
        if app.ui.scoped.member_detail_draft.elem.is_some()
            && ui
                .button("🗑 この部材の付帯情報を削除")
                .on_hover_text("選択した部材の付帯情報を削除します(undo可)")
                .clicked()
        {
            if let Some(elem) = app.ui.scoped.member_detail_draft.elem {
                app.apply_model_edit(Box::new(RemoveMemberDetailAttr { elem }));
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_fields_are_rejected_with_reasons() {
        for text in ["NaN", "inf", "+inf", "-inf", "Infinity", "1e309", "-1e309"] {
            assert!(parse_joints(text).unwrap_err().contains("継手位置は有限"));
            for (fields, label) in [
                ([text, "0", "0"], "ハンチ長"),
                (["700", text, "0"], "せい増分"),
                (["700", "0", text], "幅増分"),
            ] {
                let error = parse_haunch(true, fields[0], fields[1], fields[2]).unwrap_err();
                assert!(error.contains(&format!("{label}は有限")), "{error}");
            }
        }
    }

    #[test]
    fn finite_decimal_and_signed_increases_keep_existing_contract() {
        assert_eq!(
            parse_haunch(true, "7.0025e2", "-0.125", "+0.0625").unwrap(),
            Some(Haunch {
                length: 700.25,
                depth_increase: -0.125,
                width_increase: 0.0625,
                qd1_face: None,
            })
        );
        assert_eq!(parse_haunch(false, "NaN", "inf", "-inf").unwrap(), None);
        assert_eq!(parse_joints("1.00025e3").unwrap()[0].distance, 1000.25);
    }

    #[test]
    fn haunch_qd1_form_requires_both_rebar_groups_and_preserves_entered_values() {
        let mut draft = HaunchQd1FaceDraft {
            confirmed: true,
            dimensions: ["250.25", "400.5", "800.125"].map(String::from),
            top: ["859.5", "385.25", "750.75"].map(String::from),
            bottom: ["1146.0", "390.5", ""].map(String::from),
        };
        let haunch = parse_haunch(true, "600", "200.125", "50.5").unwrap();
        assert!(draft.apply(haunch).unwrap_err().contains("下端引張"));
        draft.bottom[2] = "NaN".into();
        assert!(draft.apply(haunch).unwrap_err().contains("下端引張"));
        draft.bottom[2] = "730.0625".into();
        let value = draft.apply(haunch).unwrap().unwrap();
        let face = value.qd1_face.unwrap();
        assert_eq!(face.distance_mm, 250.25);
        assert_eq!(face.top.yield_strength_n_mm2, 385.25);
        assert_eq!(face.bottom.effective_depth_mm, 730.0625);
        let mut detail = MemberDetailAttr::new(ElemId(0));
        detail.haunch_i = Some(value);
        let mut loaded = MemberDetailDraft::default();
        loaded.load(&detail);
        assert!(loaded.qd1_face_i.confirmed);
        assert_eq!(loaded.qd1_face_i.apply(haunch).unwrap(), Some(value));
        assert!(!loaded.qd1_face_j.confirmed);
        draft.confirmed = false;
        draft.bottom[2].clear();
        assert_eq!(draft.apply(haunch).unwrap(), haunch);
        assert_eq!(draft.apply(None).unwrap(), None);
    }

    #[test]
    fn haunch_qd1_confirmation_edit_uses_design_identity_without_expiring_analysis() {
        use crate::app::{signed_q0_acceptance_tests::fixture, ResultInputKey};
        use sepika_design_jp::QdMethod;

        let mut model = fixture::model();
        let mut detail = MemberDetailAttr::new(ElemId(0));
        let haunch = parse_haunch(true, "600", "200", "50").unwrap();
        detail.haunch_i = haunch;
        model.member_detail_attrs.push(detail.clone());
        let static_key = ResultInputKey::Combo(model.combinations[0].name.clone());
        let mut app = App::default();
        assert!(app.load_model(model));
        app.core.analysis_cfg.rc_damage_control = false;
        app.core.analysis_cfg.qd_method = QdMethod::Qd1;
        app.run_combination(0);
        assert!(app.require_result_input(static_key.clone()).is_ok());
        assert!(app.current_design_results().is_some());
        let preparation_stale = app.core.scoped.staleness.preparation_stale;

        let mut draft = HaunchQd1FaceDraft {
            confirmed: true,
            dimensions: ["0", "350", "800"].map(String::from),
            top: ["859.5", "385", "750"].map(String::from),
            bottom: ["1146", "385", "730"].map(String::from),
        };
        detail.haunch_i = draft.apply(haunch).unwrap();
        assert!(apply_member_detail(&mut app, detail.clone()));
        assert!(app.require_result_input(static_key.clone()).is_ok());
        assert!(app.current_design_results().is_none());
        assert!(!app.core.scoped.staleness.results_stale);
        assert_eq!(
            app.core.scoped.staleness.preparation_stale,
            preparation_stale
        );

        app.undo_action();
        assert!(app.require_result_input(static_key.clone()).is_ok());
        assert!(app.current_design_results().is_some());
        assert!(!app.core.scoped.staleness.results_stale);
        assert_eq!(
            app.core.scoped.staleness.preparation_stale,
            preparation_stale
        );
        app.redo_action();
        assert!(app.require_result_input(static_key.clone()).is_ok());
        assert!(app.current_design_results().is_none());
        assert!(!app.core.scoped.staleness.results_stale);
        assert_eq!(
            app.core.scoped.staleness.preparation_stale,
            preparation_stale
        );

        detail
            .haunch_i
            .as_mut()
            .unwrap()
            .qd1_face
            .as_mut()
            .unwrap()
            .top
            .area_mm2 = f64::NAN;
        let revision = app.core.scoped.undo.revision();
        assert!(!apply_member_detail(&mut app, detail.clone()));
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert!(app.require_result_input(static_key.clone()).is_ok());
        assert!(!app.core.scoped.staleness.results_stale);

        draft.top[0] = "950".into();
        detail.haunch_i = draft
            .apply(parse_haunch(true, "650", "200", "50").unwrap())
            .unwrap();
        assert!(apply_member_detail(&mut app, detail.clone()));
        assert_eq!(app.core.model.member_detail(ElemId(0)), Some(&detail));
        assert!(app.core.scoped.staleness.results_stale);
        assert!(app.require_result_input(static_key).is_err());
    }

    #[test]
    fn loading_and_applying_other_fields_then_undo_preserves_both_decimal_haunches() {
        let mut model = crate::sample::portal_frame();
        let original = MemberDetailAttr {
            elem: ElemId(0),
            haunch_i: Some(Haunch {
                length: 700.25,
                depth_increase: 200.125,
                width_increase: 50.0625,
                qd1_face: None,
            }),
            haunch_j: Some(Haunch {
                length: 600.375,
                depth_increase: -100.625,
                width_increase: 25.875,
                qd1_face: None,
            }),
            joints: Vec::new(),
        };
        model.member_detail_attrs.push(original.clone());
        let mut draft = MemberDetailDraft::default();
        draft.load(model.member_detail(original.elem).unwrap());
        draft.joints = "1000.25/工場".into();
        let updated = MemberDetailAttr {
            elem: draft.elem.unwrap(),
            haunch_i: parse_haunch(
                draft.haunch_i_enabled,
                &draft.haunch_i_length,
                &draft.haunch_i_depth,
                &draft.haunch_i_width,
            )
            .unwrap(),
            haunch_j: parse_haunch(
                draft.haunch_j_enabled,
                &draft.haunch_j_length,
                &draft.haunch_j_depth,
                &draft.haunch_j_width,
            )
            .unwrap(),
            joints: parse_joints(&draft.joints).unwrap(),
        };
        let mut undo = sepika_edit::UndoStack::new();
        assert!(undo.run(&mut model, Box::new(SetMemberDetailAttr { attr: updated })));
        let actual = model.member_detail(original.elem).unwrap();
        assert_eq!(actual.haunch_i, original.haunch_i);
        assert_eq!(actual.haunch_j, original.haunch_j);
        assert_eq!(actual.joints[0].distance, 1000.25);
        undo.undo(&mut model);
        assert_eq!(model.member_detail(original.elem), Some(&original));
        draft.load(model.member_detail(original.elem).unwrap());
        assert_eq!(draft.haunch_i_length, "700.25");
        assert_eq!(draft.haunch_j_length, "600.375");
    }

    /// 空文字列は「継手なし」を表し、空の Vec になること。
    #[test]
    fn test_parse_joints_empty_is_empty_vec() {
        assert_eq!(parse_joints("").unwrap(), Vec::new());
        assert_eq!(parse_joints("   ").unwrap(), Vec::new());
    }

    /// 種別省略時は現場継手として扱われること。
    #[test]
    fn test_parse_joints_defaults_to_site() {
        let joints = parse_joints("1000").unwrap();
        assert_eq!(
            joints,
            vec![MemberJoint {
                distance: 1000.0,
                kind: JointKind::Site,
            }]
        );
    }

    /// 「,」区切り・種別付きが正しくパースされること。
    #[test]
    fn test_parse_joints_comma_separated_with_kind() {
        let joints = parse_joints("1000/現場, 3000/工場").unwrap();
        assert_eq!(
            joints,
            vec![
                MemberJoint {
                    distance: 1000.0,
                    kind: JointKind::Site,
                },
                MemberJoint {
                    distance: 3000.0,
                    kind: JointKind::Shop,
                },
            ]
        );
    }

    /// 改行区切り・英語表記('site'/'shop'、大小文字を問わない)でもパースできること。
    #[test]
    fn test_parse_joints_newline_separated_english_kind() {
        let joints = parse_joints("1000/SITE\n3000/Shop").unwrap();
        assert_eq!(
            joints,
            vec![
                MemberJoint {
                    distance: 1000.0,
                    kind: JointKind::Site,
                },
                MemberJoint {
                    distance: 3000.0,
                    kind: JointKind::Shop,
                },
            ]
        );
    }

    /// 距離が数値でない・0以下はエラーになること。
    #[test]
    fn test_parse_joints_rejects_invalid_distance() {
        assert!(parse_joints("abc").is_err());
        assert!(parse_joints("0").is_err());
        assert!(parse_joints("-100").is_err());
    }

    /// 不正な種別はエラーになること。
    #[test]
    fn test_parse_joints_rejects_invalid_kind() {
        let err = parse_joints("1000/不明").unwrap_err();
        assert!(err.contains("不明"), "err={err}");
    }

    /// format_joints は parse_joints の逆変換になっていること(往復一致)。
    #[test]
    fn test_format_joints_roundtrip() {
        let joints = vec![
            MemberJoint {
                distance: 1000.0,
                kind: JointKind::Site,
            },
            MemberJoint {
                distance: 3000.0,
                kind: JointKind::Shop,
            },
        ];
        let formatted = format_joints(&joints);
        assert_eq!(parse_joints(&formatted).unwrap(), joints);
    }

    /// 空リストは空文字列へ整形されること。
    #[test]
    fn test_format_joints_empty() {
        assert_eq!(format_joints(&[]), "");
    }

    /// ハンチ無効時は入力値によらず None になること。
    #[test]
    fn test_parse_haunch_disabled_is_none() {
        assert_eq!(parse_haunch(false, "700", "200", "0").unwrap(), None);
    }

    /// せい増分・幅増分が空欄なら 0 として扱われること。
    #[test]
    fn test_parse_haunch_optional_fields_default_zero() {
        let h = parse_haunch(true, "700", "", "").unwrap().unwrap();
        assert_eq!(
            h,
            Haunch {
                length: 700.0,
                depth_increase: 0.0,
                width_increase: 0.0,
                qd1_face: None,
            }
        );
    }

    /// 全フィールド指定時の正常系。
    #[test]
    fn test_parse_haunch_all_fields() {
        let h = parse_haunch(true, "700", "200", "50").unwrap().unwrap();
        assert_eq!(
            h,
            Haunch {
                length: 700.0,
                depth_increase: 200.0,
                width_increase: 50.0,
                qd1_face: None,
            }
        );
    }

    /// 長さが 0 以下・数値でない場合はエラーになること。
    #[test]
    fn test_parse_haunch_rejects_non_positive_or_invalid_length() {
        assert!(parse_haunch(true, "0", "0", "0").is_err());
        assert!(parse_haunch(true, "-10", "0", "0").is_err());
        assert!(parse_haunch(true, "abc", "0", "0").is_err());
    }

    /// せい増分・幅増分が数値でない場合はエラーになること。
    #[test]
    fn test_parse_haunch_rejects_non_numeric_optional_fields() {
        assert!(parse_haunch(true, "700", "abc", "0").is_err());
        assert!(parse_haunch(true, "700", "0", "abc").is_err());
    }
}
