//! 二次部材（小梁・間柱）の配置フォームと一覧。床タブ・壁版タブで共用する。

use crate::app::App;
use crate::table_util::{self, Col};
use sepika_core::ids::*;
use sepika_core::model::{
    ElementKind, EndSupport, Model, SecondaryMember, SecondaryMemberAnchor, SecondaryMemberEnds,
    SecondaryMemberKind, SupportMemberId,
};
use sepika_edit::{
    DeleteSecondaryMember, PlaceSecondaryMember, SecondaryParent, SetSecondaryMemberEndSupport,
};

/// 二次部材の配置フォームのドラフト（GUI 専用）。
///
/// 親領域は小梁なら床領域、間柱なら壁領域。`unassigned` は未割当へ置く。
#[derive(Clone, Debug)]
pub struct SecondaryMemberDraft {
    pub unassigned: bool,
    /// 親領域 ID の生値（小梁は床領域、間柱は壁領域）。未選択は `None`。
    pub parent: Option<u32>,
    pub support_a: Option<SupportMemberId>,
    /// 端 A の材軸位置（0..1 の文字列）。
    pub pos_a: String,
    pub support_b: Option<SupportMemberId>,
    /// 端 B の材軸位置（0..1 の文字列）。
    pub pos_b: String,
    /// 端 B を自由端（片持ち）にするか。
    pub free_end: bool,
    /// 片持ちの自由端ベクトル。小梁は `[dx, dy]`、間柱は `[構面内 s, z]` `[mm]`。
    pub free_vector: [String; 2],
    pub section: Option<SectionId>,
    pub name: String,
}

impl Default for SecondaryMemberDraft {
    fn default() -> Self {
        Self {
            unassigned: false,
            parent: None,
            support_a: None,
            pos_a: "0.5".to_string(),
            support_b: None,
            pos_b: "0.5".to_string(),
            free_end: false,
            free_vector: ["0".to_string(), "0".to_string()],
            section: None,
            name: String::new(),
        }
    }
}

/// 端部支持条件の表示・選択で、実際の `ends` と候補が同じ状態を表すか。
///
/// モデルは片持ちを支持端アンカーと自由端ベクトルで表すため、`[Supported, Free]` と
/// `[Free, Supported]` はどちらも `Cantilever` へ正規化される。両者を同じものとして
/// 一致判定する。支持未解決の `Detached` はいずれの組合せにも一致させない。
fn ends_matches(ends: &SecondaryMemberEnds, candidate: [EndSupport; 2]) -> bool {
    let supported = [
        candidate[0] == EndSupport::Supported,
        candidate[1] == EndSupport::Supported,
    ];
    match ends {
        SecondaryMemberEnds::Supported(_) => supported == [true, true],
        SecondaryMemberEnds::Cantilever { .. } => {
            supported == [true, false] || supported == [false, true]
        }
        SecondaryMemberEnds::Detached(_) => false,
    }
}

fn ends_support_label(ends: &SecondaryMemberEnds) -> &'static str {
    match ends {
        SecondaryMemberEnds::Supported(_) => "支持-支持",
        SecondaryMemberEnds::Cantilever { .. } => "支持-自由（片持ち）",
        SecondaryMemberEnds::Detached(_) => "未解決",
    }
}

fn end_support_label(end_support: &[EndSupport; 2]) -> &'static str {
    match end_support {
        [EndSupport::Supported, EndSupport::Supported] => "支持-支持",
        [EndSupport::Supported, EndSupport::Free] => "支持-自由（片持ち）",
        [EndSupport::Free, EndSupport::Supported] => "自由-支持（片持ち）",
        [EndSupport::Free, EndSupport::Free] => "自由-自由",
    }
}

pub(crate) fn support_label(model: &Model, support: SupportMemberId) -> String {
    match support {
        SupportMemberId::Primary(id) => {
            let kind = model
                .element(id)
                .map(|e| {
                    if e.kind == ElementKind::Beam {
                        "大梁"
                    } else {
                        "部材"
                    }
                })
                .unwrap_or("部材");
            format!("{kind} E{}", id.0)
        }
        SupportMemberId::Secondary(id) => {
            let kind = model
                .secondary_member(id)
                .map(|sm| match sm.kind {
                    SecondaryMemberKind::Beam => "小梁",
                    SecondaryMemberKind::Post => "間柱",
                })
                .unwrap_or("二次部材");
            format!("{kind} SM{}", id.0)
        }
    }
}

/// 配置先に選べる支持部材。主架構の有効な 2 節点支持材と、同じ種別の既存二次部材のうち、
/// 材軸の両端が親領域（作業範囲）の内側または境界上にあるもの。
fn support_candidates(
    model: &Model,
    kind: SecondaryMemberKind,
    parent: SecondaryParent,
) -> Vec<SupportMemberId> {
    let mut out = Vec::new();
    for e in &model.elements {
        if e.kind.is_primary_frame_support() && e.nodes.len() == 2 {
            out.push(SupportMemberId::Primary(e.id));
        }
    }
    match kind {
        SecondaryMemberKind::Beam => {
            out.extend(model.beams().map(|sm| SupportMemberId::Secondary(sm.id)));
        }
        SecondaryMemberKind::Post => {
            out.extend(model.posts().map(|sm| SupportMemberId::Secondary(sm.id)));
        }
    }
    out.retain(|support| support_axis_in_parent(model, *support, parent));
    out
}

/// 支持部材の材軸が親領域の内側または境界上にあるか。材軸を解決できない場合は `false`。
fn support_axis_in_parent(
    model: &Model,
    support: SupportMemberId,
    parent: SecondaryParent,
) -> bool {
    let Some((a, b)) = model.support_member_axis(support) else {
        return false;
    };
    [a, b].iter().all(|p| parent_contains(model, parent, *p))
}

/// 点 `p` [mm] が親領域（床領域・壁領域）の内側または境界上にあるか。
fn parent_contains(model: &Model, parent: SecondaryParent, p: [f64; 3]) -> bool {
    match parent {
        SecondaryParent::Floor(id) => model.floor_region_contains_point_including_boundary(id, p),
        SecondaryParent::Wall(id) => model.wall_region_contains_point_including_boundary(id, p),
        SecondaryParent::Unassigned => true,
    }
}

fn parse_position(text: &str) -> Option<f64> {
    let value = text.trim().parse::<f64>().ok()?;
    (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(value)
}

/// フォームの入力から取付き位置表現を組み立てる。入力不足は `None`。
fn build_ends(draft: &SecondaryMemberDraft) -> Option<SecondaryMemberEnds> {
    let a = SecondaryMemberAnchor {
        support: draft.support_a?,
        position: parse_position(&draft.pos_a)?,
    };
    if draft.free_end {
        let vector = [
            draft.free_vector[0].trim().parse::<f64>().ok()?,
            draft.free_vector[1].trim().parse::<f64>().ok()?,
        ];
        if !vector.iter().all(|v| v.is_finite()) || vector == [0.0, 0.0] {
            return None;
        }
        Some(SecondaryMemberEnds::Cantilever {
            support: a,
            free_end_vector: vector,
        })
    } else {
        let b = SecondaryMemberAnchor {
            support: draft.support_b?,
            position: parse_position(&draft.pos_b)?,
        };
        (a != b).then_some(SecondaryMemberEnds::Supported([a, b]))
    }
}

fn parent_exists(model: &Model, kind: SecondaryMemberKind, raw: u32) -> bool {
    match kind {
        SecondaryMemberKind::Beam => model
            .floor_regions
            .get(raw as usize)
            .is_some_and(|r| r.id == FloorRegionId(raw)),
        SecondaryMemberKind::Post => model
            .wall_regions
            .get(raw as usize)
            .is_some_and(|r| r.id == WallRegionId(raw)),
    }
}

fn parent_options(app: &App, kind: SecondaryMemberKind) -> Vec<(u32, String)> {
    match kind {
        SecondaryMemberKind::Beam => app
            .core
            .model
            .floor_regions
            .iter()
            .map(|r| {
                let label = if r.name.is_empty() {
                    format!("床領域 #{}", r.id.0)
                } else {
                    r.name.clone()
                };
                (r.id.0, label)
            })
            .collect(),
        SecondaryMemberKind::Post => app
            .core
            .model
            .wall_regions
            .iter()
            .map(|r| {
                let label = if r.name.is_empty() {
                    format!("壁領域 #{}", r.id.0)
                } else {
                    r.name.clone()
                };
                (r.id.0, label)
            })
            .collect(),
    }
}

fn support_combo(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    selected: &mut Option<SupportMemberId>,
    candidates: &[SupportMemberId],
    model: &Model,
) {
    let current = selected
        .map(|s| support_label(model, s))
        .unwrap_or_else(|| "―".to_string());
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for candidate in candidates {
                if ui
                    .selectable_label(
                        *selected == Some(*candidate),
                        support_label(model, *candidate),
                    )
                    .clicked()
                {
                    *selected = Some(*candidate);
                }
            }
        });
}

/// 二次部材の配置フォーム。親領域（作業範囲）と両端の支持部材・材軸位置を指定する。
pub(crate) fn secondary_member_placement_form(
    app: &mut App,
    ui: &mut egui::Ui,
    kind: SecondaryMemberKind,
) {
    let title = match kind {
        SecondaryMemberKind::Beam => "小梁を配置",
        SecondaryMemberKind::Post => "間柱を配置",
    };
    ui.strong(title);
    ui.label(
        "親領域（小梁は床領域、間柱は壁領域）と、両端が取り付く支持部材・材軸位置\
         （0〜1）を指定します。端 B を「自由端」にすると片持ちになります。支持が\
         決まらない端は配置できません（片持ちへの読み替えはしません）。",
    );

    let parents = parent_options(app, kind);
    {
        let draft = &mut app.ui.scoped.secondary_draft;
        if !draft.unassigned
            && !draft
                .parent
                .is_some_and(|id| parents.iter().any(|(pid, _)| *pid == id))
        {
            draft.parent = parents.first().map(|(id, _)| *id);
        }
    }
    let parent = {
        let draft = &app.ui.scoped.secondary_draft;
        if draft.unassigned {
            SecondaryParent::Unassigned
        } else {
            match (kind, draft.parent) {
                (SecondaryMemberKind::Beam, Some(id)) => SecondaryParent::Floor(FloorRegionId(id)),
                (SecondaryMemberKind::Post, Some(id)) => SecondaryParent::Wall(WallRegionId(id)),
                _ => SecondaryParent::Unassigned,
            }
        }
    };
    let candidates = support_candidates(&app.core.model, kind, parent);
    {
        let draft = &mut app.ui.scoped.secondary_draft;
        if !draft.support_a.is_some_and(|s| candidates.contains(&s)) {
            draft.support_a = candidates.first().copied();
        }
        if !draft.free_end && !draft.support_b.is_some_and(|s| candidates.contains(&s)) {
            draft.support_b = candidates.get(1).or(candidates.first()).copied();
        }
    }

    let selected_parent_text = {
        let draft = &app.ui.scoped.secondary_draft;
        match (draft.unassigned, draft.parent) {
            (true, _) => "未割当".to_string(),
            (false, Some(id)) => parents
                .iter()
                .find(|(pid, _)| *pid == id)
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| format!("#{id}")),
            (false, None) => "―".to_string(),
        }
    };
    let mut parent_changed: Option<Option<u32>> = None;
    ui.horizontal(|ui| {
        ui.label("親領域:");
        egui::ComboBox::from_id_salt(("secondary_parent", kind as u8))
            .selected_text(selected_parent_text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(app.ui.scoped.secondary_draft.unassigned, "未割当")
                    .clicked()
                {
                    parent_changed = Some(None);
                }
                for (id, label) in &parents {
                    let selected = !app.ui.scoped.secondary_draft.unassigned
                        && app.ui.scoped.secondary_draft.parent == Some(*id);
                    if ui.selectable_label(selected, label).clicked() {
                        parent_changed = Some(Some(*id));
                    }
                }
            });
    });
    if let Some(choice) = parent_changed {
        let draft = &mut app.ui.scoped.secondary_draft;
        draft.unassigned = choice.is_none();
        if choice.is_some() {
            draft.parent = choice;
        }
    }

    ui.horizontal(|ui| {
        ui.label("端 A:");
        support_combo(
            ui,
            ("secondary_support_a", kind as u8),
            &mut app.ui.scoped.secondary_draft.support_a,
            &candidates,
            &app.core.model,
        );
        ui.label("材軸位置:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.secondary_draft.pos_a)
                .desired_width(50.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label("端 B:");
        if app.ui.scoped.secondary_draft.free_end {
            ui.label("自由端");
            ui.label("ベクトル:");
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.secondary_draft.free_vector[0])
                    .desired_width(60.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.secondary_draft.free_vector[1])
                    .desired_width(60.0),
            );
            ui.label("[mm]");
        } else {
            support_combo(
                ui,
                ("secondary_support_b", kind as u8),
                &mut app.ui.scoped.secondary_draft.support_b,
                &candidates,
                &app.core.model,
            );
            ui.label("材軸位置:");
            ui.add(
                egui::TextEdit::singleline(&mut app.ui.scoped.secondary_draft.pos_b)
                    .desired_width(50.0),
            );
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut app.ui.scoped.secondary_draft.free_end,
            "端 B を自由端（片持ち）にする",
        );
        if app.ui.scoped.secondary_draft.free_end {
            let hint = match kind {
                SecondaryMemberKind::Beam => "ベクトル [dx, dy] [mm]",
                SecondaryMemberKind::Post => "ベクトル [構面内 s, z] [mm]",
            };
            ui.label(hint);
        }
    });

    ui.horizontal(|ui| {
        ui.label("断面:");
        let label = app
            .ui
            .scoped
            .secondary_draft
            .section
            .and_then(|sid| app.core.model.sections.get(sid.index()))
            .map(|sec| sec.display_name())
            .unwrap_or_else(|| "―".to_string());
        egui::ComboBox::from_id_salt(("secondary_section", kind as u8))
            .selected_text(label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut app.ui.scoped.secondary_draft.section, None, "―");
                for sec in &app.core.model.sections {
                    ui.selectable_value(
                        &mut app.ui.scoped.secondary_draft.section,
                        Some(sec.id),
                        sec.display_name(),
                    );
                }
            });
        ui.label("名前:");
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.scoped.secondary_draft.name).desired_width(80.0),
        );
    });

    let draft = app.ui.scoped.secondary_draft.clone();
    let can_place = build_ends(&draft).is_some()
        && (draft.unassigned
            || draft
                .parent
                .is_some_and(|id| parent_exists(&app.core.model, kind, id)));
    if ui
        .add_enabled(can_place, egui::Button::new("配置"))
        .on_hover_text("両端の支持部材と材軸位置が解決できるときだけ配置できます")
        .clicked()
    {
        let ends = build_ends(&draft).expect("can_place で確認済み");
        let parent = if draft.unassigned {
            SecondaryParent::Unassigned
        } else {
            match kind {
                SecondaryMemberKind::Beam => {
                    SecondaryParent::Floor(FloorRegionId(draft.parent.expect("確認済み")))
                }
                SecondaryMemberKind::Post => {
                    SecondaryParent::Wall(WallRegionId(draft.parent.expect("確認済み")))
                }
            }
        };
        let applied = app.apply_model_edit(Box::new(PlaceSecondaryMember {
            parent,
            kind,
            ends,
            section: draft.section,
            name: draft.name.clone(),
        }));
        if applied {
            app.clear_generated_member_selection();
            app.core.scoped.staleness.mark_edited();
        } else if app.core.scoped.pending_plate_loss_edit.is_none()
            && app.core.scoped.last_error.is_none()
        {
            app.core.scoped.last_notice = Some(
                "支持端が親領域の内側・境界上にないため配置しませんでした。\
                 親領域の内側で支持部材を選んでください。"
                    .to_string(),
            );
        }
    }
}

/// 二次部材の一覧。所属・ID・断面・支持条件の変更・削除。
pub(crate) fn secondary_member_list(app: &mut App, ui: &mut egui::Ui, kind: SecondaryMemberKind) {
    if kind == SecondaryMemberKind::Beam {
        super::beam_placement::beam_eccentricities(ui, app, true);
    }
    let members: Vec<SecondaryMember> = match kind {
        SecondaryMemberKind::Beam => app
            .core
            .model
            .beams()
            .filter(|sm| !app.core.model.secondary_member_materialized(sm))
            .cloned()
            .collect(),
        SecondaryMemberKind::Post => app
            .core
            .model
            .posts()
            .filter(|sm| !app.core.model.secondary_member_materialized(sm))
            .cloned()
            .collect(),
    };
    if kind == SecondaryMemberKind::Post {
        post_design_inputs(app, ui, &members);
    }
    let mut pending_end_support: Vec<(SecondaryMemberId, [EndSupport; 2])> = Vec::new();
    let mut pending_delete: Vec<SecondaryMemberId> = Vec::new();
    let owner_of = |app: &App, id: SecondaryMemberId| -> Option<String> {
        match kind {
            SecondaryMemberKind::Beam => app
                .core
                .model
                .floor_regions
                .iter()
                .find(|r| r.secondary_beams.iter().any(|j| j.id == id))
                .map(|r| {
                    if r.name.is_empty() {
                        format!("#{}", r.id.0)
                    } else {
                        r.name.clone()
                    }
                }),
            SecondaryMemberKind::Post => app
                .core
                .model
                .wall_regions
                .iter()
                .find(|r| r.posts.iter().any(|p| p.id == id))
                .map(|r| {
                    if r.name.is_empty() {
                        format!("#{}", r.id.0)
                    } else {
                        r.name.clone()
                    }
                }),
        }
    };
    let table_id = match kind {
        SecondaryMemberKind::Beam => "secondary_beams_tbl",
        SecondaryMemberKind::Post => "secondary_posts_tbl",
    };
    table_util::standard_table(
        ui,
        table_id,
        &[
            Col::text("所属"),
            Col::text("ID"),
            Col::text("断面"),
            Col::name("支持条件"),
            Col::text("操作"),
        ],
        members.len(),
        |row| {
            let i = row.index();
            let sm = &members[i];
            row.col(|ui| match owner_of(app, sm.id) {
                Some(name) => table_util::text_cell(ui, &name),
                None => table_util::muted_cell(ui, "未割当", "どの領域にも所属していません"),
            });
            row.col(|ui| {
                table_util::text_cell(ui, &format!("SM{}", sm.id.0));
            });
            row.col(|ui| {
                let label = sm
                    .section
                    .and_then(|sid| app.core.model.sections.get(sid.index()))
                    .map(|sec| sec.display_name())
                    .unwrap_or_else(|| "―".to_string());
                table_util::text_cell(ui, &label);
            });
            row.col(|ui| {
                table_util::cell_combo(
                    ui,
                    (table_id, sm.id.0),
                    ends_support_label(&sm.ends),
                    |ui| {
                        for candidate in [
                            [EndSupport::Supported, EndSupport::Supported],
                            [EndSupport::Supported, EndSupport::Free],
                            [EndSupport::Free, EndSupport::Supported],
                        ] {
                            if ui
                                .selectable_label(
                                    ends_matches(&sm.ends, candidate),
                                    end_support_label(&candidate),
                                )
                                .clicked()
                                && !ends_matches(&sm.ends, candidate)
                            {
                                pending_end_support.push((sm.id, candidate));
                            }
                        }
                    },
                );
            });
            row.col(|ui| {
                if ui.button("削除").clicked() {
                    pending_delete.push(sm.id);
                }
            });
        },
    );
    let mut edited = false;
    for (member, end_support) in pending_end_support {
        edited |= app.apply_model_edit(Box::new(SetSecondaryMemberEndSupport {
            member,
            end_support,
        }));
    }
    for member in pending_delete {
        edited |= app.apply_model_edit(Box::new(DeleteSecondaryMember { member }));
    }
    if edited {
        app.clear_generated_member_selection();
        app.core.scoped.staleness.mark_edited();
    }
}

fn optional_number(ui: &mut egui::Ui, label: &str, value: &mut Option<f64>, initial: f64) {
    ui.horizontal(|ui| {
        let mut enabled = value.is_some();
        if ui.checkbox(&mut enabled, label).changed() {
            *value = enabled.then_some(initial);
        }
        if let Some(v) = value {
            ui.add(egui::DragValue::new(v).speed(1.0));
        }
    });
}

fn post_design_inputs(app: &mut App, ui: &mut egui::Ui, members: &[SecondaryMember]) {
    use sepika_core::model::{
        MemberLoadKind, PostDesignInput, PostLateralLoad, PostLateralSupport,
    };
    ui.label("間柱の独立検定条件（局部横荷重は主架構へ自動転送しません。端部反力を別途主架構へ考慮してください）");
    for sm in members {
        let mut input = app
            .core
            .model
            .post_design_inputs
            .iter()
            .find(|i| i.member == sm.id)
            .cloned()
            .unwrap_or(PostDesignInput {
                member: sm.id,
                ..Default::default()
            });
        let old = input.clone();
        let mut shares = sm.gravity_end_shares;
        let mut save = false;
        egui::CollapsingHeader::new(format!("{} SM{} 検定入力", sm.name, sm.id.0))
            .id_salt(("post_design", sm.id.0))
            .show(ui, |ui| {
                ui.label("横支持は鉛直端部負担率・軸座屈長さ・横補剛とは独立です。");
                ui.horizontal(|ui| {
                    ui.label("横支持");
                    ui.selectable_value(
                        &mut input.lateral_support,
                        PostLateralSupport::Pinned,
                        "両端ピン",
                    );
                    ui.selectable_value(
                        &mut input.lateral_support,
                        PostLateralSupport::FixedStart,
                        "始端固定・終端自由",
                    );
                    ui.selectable_value(
                        &mut input.lateral_support,
                        PostLateralSupport::FixedEnd,
                        "始端自由・終端固定",
                    );
                });
                ui.horizontal(|ui| {
                    let mut enabled = shares.is_some();
                    if ui.checkbox(&mut enabled, "鉛直荷重の端部負担率").changed() {
                        shares = enabled.then_some([1.0, 0.0]);
                    }
                    if let Some(v) = &mut shares {
                        ui.label("始端");
                        ui.add(egui::DragValue::new(&mut v[0]).speed(0.01));
                        ui.label("終端");
                        ui.add(egui::DragValue::new(&mut v[1]).speed(0.01));
                        ui.label("合計1.0");
                    }
                });
                optional_number(
                    ui,
                    "強軸曲げを生じる荷重方向角 [度、全体Xから反時計回り]",
                    &mut input.strong_direction_deg,
                    0.0,
                );
                ui.horizontal(|ui| {
                    let mut enabled = input.buckling_lengths_mm.is_some();
                    if ui.checkbox(&mut enabled, "軸圧縮座屈長さ [mm]").changed() {
                        input.buckling_lengths_mm = enabled.then_some([3000.0, 3000.0]);
                    }
                    if let Some(v) = &mut input.buckling_lengths_mm {
                        ui.label("強軸");
                        ui.add(egui::DragValue::new(&mut v[0]));
                        ui.label("弱軸");
                        ui.add(egui::DragValue::new(&mut v[1]));
                    }
                });
                optional_number(
                    ui,
                    "横補剛間隔 [mm]",
                    &mut input.lateral_buckling_length_mm,
                    3000.0,
                );
                optional_number(
                    ui,
                    "たわみ限界 L/分母（未指定は変位のみ）",
                    &mut input.deflection_limit_denom,
                    250.0,
                );
                ui.label("検定対象組合せと許容応力度の期間を明示してください。");
                for (idx, combo) in app.core.model.combinations.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let mut selected = input.combinations.iter().any(|(i, _)| *i == idx);
                        if ui.checkbox(&mut selected, &combo.name).changed() {
                            input.combinations.retain(|(i, _)| *i != idx);
                            if selected {
                                input.combinations.push((idx, false));
                            }
                        }
                        if let Some((_, short)) =
                            input.combinations.iter_mut().find(|(i, _)| *i == idx)
                        {
                            ui.checkbox(short, "短期（解除=長期）");
                        }
                    });
                }
                ui.label("局部横荷重：入力は N・mm・N/mm。方向は全体XY水平面内の角度です。");
                let span = app
                    .core
                    .model
                    .secondary_member_axis(sm)
                    .map(|(_, _, l)| l)
                    .unwrap_or(3000.0);
                let mut remove = None;
                for (idx, load) in input.lateral_loads.iter_mut().enumerate() {
                    ui.push_id(idx, |ui| {
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("case")
                                .selected_text(
                                    app.core
                                        .model
                                        .load_cases
                                        .iter()
                                        .find(|c| c.id == load.case)
                                        .map(|c| c.name.as_str())
                                        .unwrap_or("ケース未解決"),
                                )
                                .show_ui(ui, |ui| {
                                    for c in &app.core.model.load_cases {
                                        ui.selectable_value(&mut load.case, c.id, &c.name);
                                    }
                                });
                            let mut angle = load.direction[1].atan2(load.direction[0]).to_degrees();
                            ui.label("方向角[度]");
                            if ui.add(egui::DragValue::new(&mut angle)).changed() {
                                load.direction =
                                    [angle.to_radians().cos(), angle.to_radians().sin()];
                            }
                            if ui.button("荷重削除").clicked() {
                                remove = Some(idx);
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut point = matches!(load.load, MemberLoadKind::Point { .. });
                            if ui.checkbox(&mut point, "集中荷重（解除=等分布）").changed()
                            {
                                load.load = if point {
                                    MemberLoadKind::Point {
                                        a: span / 2.0,
                                        p: 0.0,
                                    }
                                } else {
                                    MemberLoadKind::Distributed {
                                        a: 0.0,
                                        b: span,
                                        w1: 0.0,
                                        w2: 0.0,
                                    }
                                };
                            }
                            match &mut load.load {
                                MemberLoadKind::Point { a, p } => {
                                    ui.label("位置[mm]");
                                    ui.add(egui::DragValue::new(a));
                                    ui.label("荷重[N]");
                                    ui.add(egui::DragValue::new(p));
                                }
                                MemberLoadKind::Distributed { a, b, w1, w2 } => {
                                    ui.label("始点[mm]");
                                    ui.add(egui::DragValue::new(a));
                                    ui.label("終点[mm]");
                                    ui.add(egui::DragValue::new(b));
                                    ui.label("強度[N/mm]");
                                    if ui.add(egui::DragValue::new(w1).speed(0.1)).changed() {
                                        *w2 = *w1;
                                    }
                                }
                            }
                        });
                    });
                }
                if let Some(idx) = remove {
                    input.lateral_loads.remove(idx);
                }
                if ui.button("局部横荷重を追加").clicked() {
                    if let Some(c) = app.core.model.load_cases.first() {
                        input.lateral_loads.push(PostLateralLoad {
                            case: c.id,
                            direction: [1.0, 0.0],
                            load: MemberLoadKind::Distributed {
                                a: 0.0,
                                b: span,
                                w1: 0.0,
                                w2: 0.0,
                            },
                        });
                    }
                }
                save = ui.button("間柱検定条件を保存").clicked();
            });
        if input != old || save {
            app.apply_model_edit(Box::new(sepika_edit::SetPostDesignInput {
                member: sm.id,
                input: Some(input),
            }));
        }
        if shares != sm.gravity_end_shares {
            app.apply_model_edit(Box::new(sepika_edit::SetPostGravityEndShares {
                member: sm.id,
                shares,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::model::{
        ElementData, EndCondition, FloorRegion, ForceRegime, LocalAxis, Node, WallRegion,
    };

    fn beam(id: u32, a: u32, b: u32) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }
    }

    fn anchor() -> SecondaryMemberAnchor {
        SecondaryMemberAnchor {
            support: SupportMemberId::Primary(ElemId(0)),
            position: 0.5,
        }
    }

    #[test]
    fn 支持未解決はどの端部候補にも一致しない() {
        let ends = SecondaryMemberEnds::Detached([[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]]);
        for candidate in [
            [EndSupport::Supported, EndSupport::Supported],
            [EndSupport::Supported, EndSupport::Free],
            [EndSupport::Free, EndSupport::Supported],
        ] {
            assert!(!ends_matches(&ends, candidate), "{candidate:?}");
        }
    }

    #[test]
    fn 片持ちは正規化後の組合せの両方に一致する() {
        let ends = SecondaryMemberEnds::Cantilever {
            support: anchor(),
            free_end_vector: [0.0, 1000.0],
        };
        assert!(ends_matches(
            &ends,
            [EndSupport::Supported, EndSupport::Free]
        ));
        assert!(ends_matches(
            &ends,
            [EndSupport::Free, EndSupport::Supported]
        ));
        assert!(!ends_matches(
            &ends,
            [EndSupport::Supported, EndSupport::Supported]
        ));
    }

    #[test]
    fn 両端支持は支持支持にのみ一致する() {
        let ends = SecondaryMemberEnds::Supported([anchor(), anchor()]);
        assert!(ends_matches(
            &ends,
            [EndSupport::Supported, EndSupport::Supported]
        ));
        assert!(!ends_matches(
            &ends,
            [EndSupport::Supported, EndSupport::Free]
        ));
        assert!(!ends_matches(
            &ends,
            [EndSupport::Free, EndSupport::Supported]
        ));
    }

    #[test]
    fn 支持部材候補は親領域内の材軸に絞る() {
        let mut model = Model::default();
        for (i, (x, y)) in [(0.0, 0.0), (4000.0, 0.0), (4000.0, 4000.0), (0.0, 4000.0)]
            .into_iter()
            .enumerate()
        {
            model.nodes.push(Node {
                id: NodeId(i as u32),
                coord: [x, y, 0.0],
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        for (i, (a, b)) in [(0u32, 1u32), (1, 2), (2, 3), (3, 0)]
            .into_iter()
            .enumerate()
        {
            model.elements.push(beam(i as u32, a, b));
        }
        model.nodes.push(Node {
            id: NodeId(4),
            coord: [-4000.0, 0.0, 0.0],
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        });
        model.nodes.push(Node {
            id: NodeId(5),
            coord: [-4000.0, 4000.0, 0.0],
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        });
        model.elements.push(beam(4, 4, 5));
        model.floor_regions.push(FloorRegion::new(
            FloorRegionId(0),
            vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        ));

        let parent = SecondaryParent::Floor(FloorRegionId(0));
        let candidates = support_candidates(&model, SecondaryMemberKind::Beam, parent);
        assert!(candidates.contains(&SupportMemberId::Primary(ElemId(0))));
        assert!(
            !candidates.contains(&SupportMemberId::Primary(ElemId(4))),
            "親領域の外にある大梁は候補にしない"
        );
    }

    fn support_model(kind: ElementKind) -> Model {
        let mut model = Model::default();
        for (i, coord) in [
            [0.0, 0.0, 0.0],
            [4000.0, 0.0, 0.0],
            [4000.0, 4000.0, 0.0],
            [0.0, 4000.0, 0.0],
        ]
        .into_iter()
        .enumerate()
        {
            model.nodes.push(Node {
                id: NodeId(i as u32),
                coord,
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        for (i, (a, b)) in [(0, 1), (1, 2), (2, 3), (3, 0)].into_iter().enumerate() {
            let mut element = beam(i as u32, a, b);
            element.kind = kind;
            model.elements.push(element);
        }
        model.floor_regions.push(FloorRegion::new(
            FloorRegionId(0),
            vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        ));
        model
    }

    #[test]
    fn 支持候補は解析方式によらず有効な主架構だけを含む() {
        for element_kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            for secondary_kind in [SecondaryMemberKind::Beam, SecondaryMemberKind::Post] {
                let mut model = support_model(element_kind);
                let parent = match secondary_kind {
                    SecondaryMemberKind::Beam => SecondaryParent::Floor(FloorRegionId(0)),
                    SecondaryMemberKind::Post => {
                        for node in &mut model.nodes {
                            node.coord = [node.coord[0], 0.0, node.coord[1]];
                        }
                        let boundary = model.floor_regions.remove(0).boundary;
                        model
                            .wall_regions
                            .push(WallRegion::new(WallRegionId(0), boundary));
                        SecondaryParent::Wall(WallRegionId(0))
                    }
                };
                let support = SupportMemberId::Primary(ElemId(0));
                assert!(support_candidates(&model, secondary_kind, parent).contains(&support));
                for nodes in [
                    vec![],
                    vec![NodeId(0)],
                    vec![NodeId(0), NodeId(1), NodeId(2)],
                    vec![NodeId(0), NodeId(0)],
                    vec![NodeId(0), NodeId(99)],
                ] {
                    model.elements[0].nodes = nodes.into_iter().collect();
                    assert!(!support_candidates(&model, secondary_kind, parent).contains(&support));
                }
                let mut outside = model.nodes[0].clone();
                outside.id = NodeId(4);
                outside.coord = [-1000.0, 0.0, 0.0];
                model.nodes.push(outside);
                model.elements[0].nodes = [NodeId(4), NodeId(1)].into_iter().collect();
                assert!(!support_candidates(&model, secondary_kind, parent).contains(&support));
            }
        }
        for kind in [
            ElementKind::Shell,
            ElementKind::Wall,
            ElementKind::PanelZone,
            ElementKind::Brace {
                tension_only: false,
            },
            ElementKind::NodalSpring,
            ElementKind::Isolator,
            ElementKind::Damper,
        ] {
            let model = support_model(kind);
            assert!(
                support_candidates(
                    &model,
                    SecondaryMemberKind::Beam,
                    SecondaryParent::Unassigned
                )
                .is_empty(),
                "{kind:?}"
            );
        }
    }

    fn text_center(shape: &egui::Shape, text: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(value) if value.galley.text() == text => {
                Some(value.visual_bounding_rect().center())
            }
            egui::Shape::Vec(values) => values.iter().find_map(|s| text_center(s, text)),
            _ => None,
        }
    }

    #[test]
    fn フォームの再描画と名前変更で支持を保持し配置とundo_redoに引き継ぐ() {
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            let mut app = App::default();
            assert!(app.load_model(support_model(kind)));
            app.ui.scoped.secondary_draft = SecondaryMemberDraft {
                parent: Some(0),
                support_a: Some(SupportMemberId::Primary(ElemId(1))),
                pos_a: "0.25".into(),
                support_b: Some(SupportMemberId::Primary(ElemId(3))),
                pos_b: "0.75".into(),
                ..Default::default()
            };
            let expected = build_ends(&app.ui.scoped.secondary_draft).unwrap();
            let context = egui::Context::default();
            crate::app::install_japanese_fonts(&context);
            let frame = |app: &mut App, events| {
                context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| secondary_member_placement_form(app, ui, SecondaryMemberKind::Beam),
                )
            };
            frame(&mut app, vec![]);
            assert_eq!(build_ends(&app.ui.scoped.secondary_draft), Some(expected));
            app.ui.scoped.secondary_draft.name = "支持を保持".into();
            let output = frame(&mut app, vec![]);
            assert_eq!(build_ends(&app.ui.scoped.secondary_draft), Some(expected));
            let position = output
                .shapes
                .iter()
                .find_map(|s| text_center(&s.shape, "配置"))
                .unwrap();
            for pressed in [true, false] {
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            assert_eq!(
                app.core.model.floor_regions[0].secondary_beams.len(),
                1,
                "{:?}",
                app.core.scoped.last_error
            );
            assert_eq!(
                app.core.model.floor_regions[0].secondary_beams[0].ends,
                expected
            );
            assert_eq!(
                app.core.model.floor_regions[0].secondary_beams[0].name,
                "支持を保持"
            );
            app.undo_action();
            assert!(app.core.model.floor_regions[0].secondary_beams.is_empty());
            app.redo_action();
            assert_eq!(
                app.core.model.floor_regions[0].secondary_beams[0].ends,
                expected
            );
        }
    }
}
