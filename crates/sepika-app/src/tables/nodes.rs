use crate::app::node_grid::NodeGridAdapter;
use crate::app::{App, LogLevel};
use sepika_core::dof::Dof6Mask;
use sepika_core::ids::NodeId;
use sepika_core::model::{IsolatorKind, IsolatorProps};
use sepika_core::units::to_display::{force_kn, stiffness_kn_per_mm};
use sepika_core::units::to_internal;
use sepika_edit::{
    AddNode, PlaceSupportIsolator, RemoveSupportIsolator, SetNodeRestraint, SetNodeSupportSpring,
};

/// 免震支承の配置フォーム（境界条件パネル）のドラフト状態。
/// `PlaceSupportIsolator` へ渡す諸元をフォーム上で保持する（節点非依存。
/// どの節点を選んでいても同じ入力中の諸元を使い回す「作成フォーム」）。
#[derive(Clone, Debug, Default)]
pub struct IsolatorSupportDraft {
    pub props: IsolatorProps,
}

/// 免震支承種別の日本語表示名（各免震部材指針の呼称）。
pub fn isolator_kind_label(kind: IsolatorKind) -> &'static str {
    match kind {
        IsolatorKind::LaminatedRubber => "天然ゴム系積層ゴム",
        IsolatorKind::LeadRubber => "鉛プラグ入り積層ゴム(LRB)",
        IsolatorKind::HighDampingRubber => "高減衰ゴム系積層ゴム(HDR)",
        IsolatorKind::ElasticSliding => "弾性すべり支承",
    }
}

/// 免震支承種別セレクタ（4種別をボタン列で選択）。
pub fn isolator_kind_selector(ui: &mut egui::Ui, kind: &mut IsolatorKind) {
    ui.horizontal_wrapped(|ui| {
        ui.label("支承種別:");
        for k in [
            IsolatorKind::LaminatedRubber,
            IsolatorKind::LeadRubber,
            IsolatorKind::HighDampingRubber,
            IsolatorKind::ElasticSliding,
        ] {
            if ui
                .selectable_label(*kind == k, isolator_kind_label(k))
                .clicked()
            {
                *kind = k;
            }
        }
    });
}

/// `IsolatorProps` の諸元入力。種別に応じて関係するフィールドのみ表示する
/// （すべり支承: K1・μ・N長期軸力・Kv／積層ゴム系: K1・K2・Qd・Kv・本数・
/// ゴム総厚＋任意の歪依存係数）。`id_source` は CollapsingHeader の id 衝突回避用
/// （同じ関数が複数箇所〔境界条件パネル・部材タブの免震支承追加フォーム〕から
/// 呼ばれるため）。
///
/// 入力表示は K1/K2/Kv=kN/mm・Qd=kN（免震一覧 `tables::members::isolators_table`
/// と統一）。`IsolatorProps` 自体は N/mm・N 単位で保持するため、
/// `to_display` / `to_internal` で換算する。
pub fn isolator_props_fields(ui: &mut egui::Ui, id_source: &str, props: &mut IsolatorProps) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Kv 鉛直剛性[kN/mm]:");
        let mut kv_kn = stiffness_kn_per_mm(props.kv);
        if ui
            .add(
                egui::DragValue::new(&mut kv_kn)
                    .speed(1.0)
                    .range(0.0..=1.0e9),
            )
            .changed()
        {
            props.kv = to_internal::stiffness_kn_per_mm(kv_kn);
        }
    });
    match props.kind {
        IsolatorKind::ElasticSliding => {
            ui.horizontal_wrapped(|ui| {
                ui.label("K1 すべり前剛性[kN/mm]:");
                let mut k1_kn = stiffness_kn_per_mm(props.k1);
                if ui
                    .add(
                        egui::DragValue::new(&mut k1_kn)
                            .speed(1.0)
                            .range(0.0..=1.0e6),
                    )
                    .changed()
                {
                    props.k1 = to_internal::stiffness_kn_per_mm(k1_kn);
                }
                ui.label("μ 摩擦係数:");
                ui.add(
                    egui::DragValue::new(&mut props.mu)
                        .speed(0.01)
                        .range(0.0..=2.0),
                );
                ui.label("N 長期軸力[kN]（圧縮正、摩擦力算定用）:");
                let mut n_kn = force_kn(props.n_long);
                if ui
                    .add(
                        egui::DragValue::new(&mut n_kn)
                            .speed(1.0)
                            .range(0.0..=1.0e7),
                    )
                    .changed()
                {
                    props.n_long = to_internal::force_kn(n_kn);
                }
            });
        }
        IsolatorKind::LaminatedRubber
        | IsolatorKind::LeadRubber
        | IsolatorKind::HighDampingRubber => {
            ui.horizontal_wrapped(|ui| {
                ui.label("K1 初期(弾性)剛性[kN/mm]:");
                let mut k1_kn = stiffness_kn_per_mm(props.k1);
                if ui
                    .add(
                        egui::DragValue::new(&mut k1_kn)
                            .speed(1.0)
                            .range(0.0..=1.0e6),
                    )
                    .changed()
                {
                    props.k1 = to_internal::stiffness_kn_per_mm(k1_kn);
                }
                ui.label("K2 二次剛性[kN/mm]:");
                let mut k2_kn = stiffness_kn_per_mm(props.k2);
                if ui
                    .add(
                        egui::DragValue::new(&mut k2_kn)
                            .speed(0.1)
                            .range(0.0..=1.0e6),
                    )
                    .changed()
                {
                    props.k2 = to_internal::stiffness_kn_per_mm(k2_kn);
                }
                ui.label("Qd 特性耐力[kN]:");
                let mut qd_kn = force_kn(props.qd);
                if ui
                    .add(
                        egui::DragValue::new(&mut qd_kn)
                            .speed(1.0)
                            .range(0.0..=1.0e6),
                    )
                    .changed()
                {
                    props.qd = to_internal::force_kn(qd_kn);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("マルチシアスプリング本数 n:");
                let mut n = props.n_springs;
                if ui.add(egui::DragValue::new(&mut n).range(1..=64)).changed() {
                    props.n_springs = n;
                }
                ui.label("ゴム総厚 H[mm]（歪依存判定用。0で歪依存を無効化）:");
                ui.add(
                    egui::DragValue::new(&mut props.total_rubber_thickness)
                        .speed(1.0)
                        .range(0.0..=10000.0),
                );
            });
            if props.total_rubber_thickness > 0.0 {
                egui::CollapsingHeader::new("歪依存係数（任意・詳細）")
                    .default_open(false)
                    .id_salt((id_source, "isolator_strain_dep"))
                    .show(ui, |ui| {
                        ui.label(
                            "CKd(γ)=c0+c1・γ+c2・γ²（二次剛性K2の歪依存）／\
                             CQd(γ)=c0+c1・γ+c2・γ²（特性耐力Qdの歪依存）。\
                             γ=δ/H（各免震部材の製品技術資料）。既定[1,0,0]は歪依存なし。",
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.label("CKd c0,c1,c2:");
                            for v in &mut props.ckd_gamma {
                                ui.add(egui::DragValue::new(v).speed(0.01));
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("CQd c0,c1,c2:");
                            for v in &mut props.cqd_gamma {
                                ui.add(egui::DragValue::new(v).speed(0.01));
                            }
                        });
                    });
            }
        }
    }
}

pub fn nodes_table(ui: &mut egui::Ui, app: &mut App) {
    ui.group(|ui| {
        ui.strong("節点を追加");
        ui.horizontal_wrapped(|ui| {
            for (label, k) in [("X", 0), ("Y", 1), ("Z", 2)] {
                ui.label(label);
                let slot = &mut app.ui.scoped.node_draft[k];
                let resp = ui.add(
                    egui::TextEdit::singleline(slot)
                        .desired_width(70.0)
                        .clip_text(false),
                );
                if slot.trim().parse::<f64>().is_err() {
                    ui.painter().rect_filled(
                        resp.rect,
                        0.0,
                        crate::theme::translucent(crate::theme::ERROR_RED, 60),
                    );
                }
            }
            if ui.button("+ 追加").clicked() {
                let mut coord = [0.0; 3];
                for (k, slot) in app.ui.scoped.node_draft.iter().enumerate() {
                    coord[k] = slot.trim().parse::<f64>().unwrap_or(0.0);
                }
                const COORD_TOL: f64 = 1e-9;
                let dup = app.core.model.nodes.iter().any(|n| {
                    (n.coord[0] - coord[0]).abs() < COORD_TOL
                        && (n.coord[1] - coord[1]).abs() < COORD_TOL
                        && (n.coord[2] - coord[2]).abs() < COORD_TOL
                });
                if dup {
                    app.ui.scoped.pending_duplicate_node_coord = Some(coord);
                } else {
                    app.core.scoped.undo.run(
                        &mut app.core.model,
                        Box::new(AddNode {
                            coord,
                            restraint: Dof6Mask::FREE,
                        }),
                    );
                    app.sync_node_edit();
                    app.core.scoped.staleness.mark_edited();
                }
            }
        });
    });
    ui.separator();

    let edited = {
        let mut adapter = NodeGridAdapter {
            model: &mut app.core.model,
            undo: &mut app.core.scoped.undo,
            edited: false,
        };
        app.ui.scoped.node_grid.delete_buttons = true;
        app.ui
            .scoped
            .node_grid
            .show(ui, &mut adapter, &["X", "Y", "Z"]);
        adapter.edited
    };
    for (msg, is_err) in app.ui.scoped.node_grid.take_log() {
        app.core.log.push(
            if is_err {
                LogLevel::Error
            } else {
                LogLevel::Info
            },
            msg,
        );
    }
    if edited {
        app.clear_generated_member_selection();
        app.core.scoped.staleness.mark_edited();
        app.sync_node_edit();
    }
    let row_selection = app.ui.scoped.node_grid.take_row_selection();
    if app.ui.scoped.node_grid.take_rows_deleted() {
        clear_deleted_node_selection(app);
    } else if let Some((anchor, cursor)) = row_selection {
        select_node_rows(app, anchor, cursor);
    }

    if app.ui.scoped.pending_duplicate_node_coord.is_some() {
        let mut do_add = false;
        let mut do_cancel = false;
        let mut open = true;
        egui::Window::new("節点座標の重複")
            .title_bar(true)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                if let Some(coord) = app.ui.scoped.pending_duplicate_node_coord {
                    ui.label(format!(
                        "({:.3}, {:.3}, {:.3}) と同じ座標の節点がすでに存在します。",
                        coord[0], coord[1], coord[2]
                    ));
                }
                ui.label("本当にこの節点を追加しますか？");
                ui.horizontal(|ui| {
                    if ui.button("追加する").clicked() {
                        do_add = true;
                    }
                    if ui.button("キャンセル").clicked() {
                        do_cancel = true;
                    }
                });
            });
        if !open || do_cancel {
            app.ui.scoped.pending_duplicate_node_coord = None;
        }
        if do_add {
            if let Some(coord) = app.ui.scoped.pending_duplicate_node_coord.take() {
                app.core.scoped.undo.run(
                    &mut app.core.model,
                    Box::new(AddNode {
                        coord,
                        restraint: Dof6Mask::FREE,
                    }),
                );
                app.sync_node_edit();
                app.core.scoped.staleness.mark_edited();
            }
        }
    }
}

fn row_geometry_selection(
    model: &sepika_core::model::Model,
    anchor: usize,
    cursor: usize,
) -> (Vec<NodeId>, Option<NodeId>) {
    let ids = (anchor.min(cursor)..=anchor.max(cursor))
        .filter_map(|r| model.nodes.get(r).map(|n| n.id))
        .collect();
    let active = model.nodes.get(anchor).map(|n| n.id);
    (ids, active)
}

fn select_node_rows(app: &mut App, anchor: usize, cursor: usize) {
    let (ids, active) = row_geometry_selection(&app.core.model, anchor, cursor);
    app.ui.scoped.boundary_node = None;
    app.select_nodes(ids, active);
}

fn boundary_edit_node(
    model: &sepika_core::model::Model,
    boundary_node: Option<NodeId>,
    active_node: Option<NodeId>,
) -> Option<NodeId> {
    active_node
        .filter(|id| model.node(*id).is_some())
        .or_else(|| boundary_node.filter(|id| model.node(*id).is_some()))
        .or_else(|| model.nodes.first().map(|node| node.id))
}

fn clear_deleted_node_selection(app: &mut App) {
    app.clear_geometry_selection();
    app.ui.scoped.boundary_node = None;
}

/// 境界条件（拘束）タブ：節点一覧・追加フォームとは別の独立したサブタブ。
/// 節点を選んでから 自由／ピン／固定 やチェックボックスで拘束成分を設定する。
pub fn boundary_condition_panel(ui: &mut egui::Ui, app: &mut App) {
    if app.core.model.nodes.is_empty() {
        ui.label("節点がありません（先に「節点」タブで節点を追加してください）");
        return;
    }

    let node_ids: Vec<NodeId> = app.core.model.nodes.iter().map(|n| n.id).collect();
    let mut selected = boundary_edit_node(
        &app.core.model,
        app.ui.scoped.boundary_node,
        app.ui.scoped.selection.active_node(),
    )
    .unwrap_or(node_ids[0]);

    let node_label = |id: NodeId| -> String {
        let has_spring = app
            .core
            .model
            .node(id)
            .is_some_and(|n| n.support_spring.is_some());
        if has_spring {
            format!("N{} 🌀ばね", id.0)
        } else {
            format!("N{}", id.0)
        }
    };

    let mut explicit_node = None;
    ui.horizontal(|ui| {
        ui.label("対象節点:");
        egui::ComboBox::from_id_salt("bc_node_select")
            .selected_text(node_label(selected))
            .show_ui(ui, |ui| {
                for id in &node_ids {
                    if ui
                        .selectable_label(selected == *id, node_label(*id))
                        .clicked()
                    {
                        explicit_node = Some(*id);
                    }
                }
            });
    });
    if let Some(id) = explicit_node {
        app.ui.scoped.boundary_node = Some(id);
        app.select_node(id);
        selected = id;
    }
    ui.separator();

    let Some(node) = app.core.model.node(selected) else {
        return;
    };
    let r = node.restraint;
    let mut pending_restraint: Option<Dof6Mask> = None;

    ui.horizontal(|ui| {
        if ui.small_button("自由").clicked() {
            pending_restraint = Some(Dof6Mask::FREE);
        }
        if ui.small_button("ピン").clicked() {
            pending_restraint = Some(Dof6Mask::PINNED);
        }
        if ui.small_button("固定").clicked() {
            pending_restraint = Some(Dof6Mask::FIXED);
        }
    });
    ui.horizontal_wrapped(|ui| {
        use sepika_core::dof::Dof;
        for (d, lbl) in [
            (Dof::Ux, "X"),
            (Dof::Uy, "Y"),
            (Dof::Uz, "Z"),
            (Dof::Rx, "RX"),
            (Dof::Ry, "RY"),
            (Dof::Rz, "RZ"),
        ] {
            let mut on = r.is_fixed(d);
            if ui.checkbox(&mut on, lbl).changed() {
                let mut new_mask = r;
                new_mask.set(d, on);
                pending_restraint = Some(new_mask);
            }
        }
    });

    if let Some(mask) = pending_restraint {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(SetNodeRestraint {
                node: selected,
                restraint: mask,
            }),
        );
        app.core.scoped.staleness.mark_edited();
    }

    ui.separator();
    support_spring_section(ui, app, selected);
    ui.separator();
    isolator_support_section(ui, app, selected);
}

/// 「ばね支持」節：対象節点の支点ばね（全体座標系6成分）を編集する。
/// 拘束（`restraint`）で固定済みの成分は入力を無効化し「(固定)」と表示する
/// （`Node::support_spring` の仕様：固定成分のばね値は解析側で無視されるため）。
fn support_spring_section(ui: &mut egui::Ui, app: &mut App, node_id: NodeId) {
    egui::CollapsingHeader::new("ばね支持")
        .default_open(false)
        .id_salt("bc_spring_section")
        .show(ui, |ui| {
            let Some(node) = app.core.model.node(node_id) else {
                return;
            };
            let restraint = node.restraint;
            let mut enabled = node.support_spring.is_some();
            let mut spring = node.support_spring.unwrap_or([0.0; 6]);

            if ui.checkbox(&mut enabled, "ばね支持を有効化").changed() {
                let new_spring = if enabled { Some(spring) } else { None };
                app.core.scoped.undo.run(
                    &mut app.core.model,
                    Box::new(SetNodeSupportSpring {
                        node: node_id,
                        spring: new_spring,
                    }),
                );
                app.core.scoped.staleness.mark_edited();
                return;
            }
            if !enabled {
                ui.colored_label(crate::theme::GRAY_600, "無効（自由 or 固定のみ）");
                return;
            }

            use sepika_core::dof::Dof;
            let mut commit = false;
            ui.horizontal_wrapped(|ui| {
                for (i, (d, label)) in [
                    (Dof::Ux, "Kx[N/mm]"),
                    (Dof::Uy, "Ky[N/mm]"),
                    (Dof::Uz, "Kz[N/mm]"),
                    (Dof::Rx, "KRx[N·mm/rad]"),
                    (Dof::Ry, "KRy[N·mm/rad]"),
                    (Dof::Rz, "KRz[N·mm/rad]"),
                ]
                .into_iter()
                .enumerate()
                {
                    let fixed = restraint.is_fixed(d);
                    ui.label(label);
                    let resp = ui.add_enabled(
                        !fixed,
                        egui::DragValue::new(&mut spring[i])
                            .speed(10.0)
                            .range(0.0..=1.0e12),
                    );
                    if fixed {
                        ui.colored_label(crate::theme::GRAY_600, "(固定)");
                    }
                    if resp.drag_stopped() || resp.lost_focus() {
                        commit = true;
                    }
                }
            });
            if commit {
                app.core.scoped.undo.run(
                    &mut app.core.model,
                    Box::new(SetNodeSupportSpring {
                        node: node_id,
                        spring: Some(spring),
                    }),
                );
                app.core.scoped.staleness.mark_edited();
            }
        });
}

/// 「免震支承の配置」節：対象節点に零長 Isolator 要素＋接地節点を設置する
/// （`PlaceSupportIsolator`）。既に配置済み（対象節点に接続する零長 Isolator
/// 要素がある）場合は諸元の要約のみ表示する（多重設置を避けるため入力フォームは
/// 出さない。取り消しは undo で行う）。
fn isolator_support_section(ui: &mut egui::Ui, app: &mut App, node_id: NodeId) {
    egui::CollapsingHeader::new("免震支承の配置")
        .default_open(false)
        .id_salt("bc_isolator_section")
        .show(ui, |ui| {
            let existing_elem = find_support_isolator(&app.core.model, node_id);

            if let Some(elem_id) = existing_elem {
                let props = app
                    .core.model
                    .isolator_attrs
                    .iter()
                    .find(|a| a.elem == elem_id)
                    .map(|a| a.props);
                match props {
                    Some(p) => {
                        ui.colored_label(
                            crate::theme::GOOD_GREEN,
                            format!(
                                "配置済み（要素#{}）: {} K1={:.0}kN/mm K2={:.0}kN/mm \
                                 Qd={:.1}kN Kv={:.0}kN/mm μ={:.3}",
                                elem_id.0,
                                isolator_kind_label(p.kind),
                                stiffness_kn_per_mm(p.k1),
                                stiffness_kn_per_mm(p.k2),
                                force_kn(p.qd),
                                stiffness_kn_per_mm(p.kv),
                                p.mu
                            ),
                        );
                        ui.label(
                            "諸元の変更は「部材」タブの免震支承一覧から行ってください。",
                        );
                        if ui
                            .button("撤去")
                            .on_hover_text(
                                "接地節点・免震支承要素を削除し、対象節点を直接支点（拘束固定）へ戻します（undo可）",
                            )
                            .clicked()
                        {
                            remove_support_isolator(app, node_id);
                        }
                    }
                    None => {
                        ui.colored_label(crate::theme::ERROR_RED, "免震支承の諸元が見つかりません");
                    }
                }
                return;
            }

            ui.label(
                "この節点を免震支承で支持します（同一座標に接地節点を新規作成し、\
                 零長の免震支承要素を設置。対象節点の拘束は自動的に解放されます）。",
            );
            isolator_kind_selector(ui, &mut app.ui.scoped.isolator_support_draft.props.kind);
            isolator_props_fields(
                ui,
                "bc_isolator_support",
                &mut app.ui.scoped.isolator_support_draft.props,
            );
            if ui
                .button("この支点に免震支承を配置")
                .on_hover_text(
                    "接地節点＋零長の免震支承要素を追加し、対象節点の拘束を解放します（undo可）",
                )
                .clicked()
                && app.apply_model_edit(Box::new(PlaceSupportIsolator {
                    node: node_id,
                    props: app.ui.scoped.isolator_support_draft.props,
                }))
            {
                app.clear_generated_member_selection();
            }
        });
}

fn remove_support_isolator(app: &mut App, node: NodeId) -> bool {
    if !app.apply_model_edit(Box::new(RemoveSupportIsolator { node })) {
        return false;
    }
    clear_deleted_node_selection(app);
    true
}

/// 対象節点 `node_id` に設置済みの支点免震支承（零長 Isolator 要素）を探す。
/// `PlaceSupportIsolator` が生成する要素の形（対象節点と同一座標の接地節点との
/// 2節点、零長、接地節点は `restraint=FIXED` かつ孤立）を満たす `Isolator` 要素が
/// あればその `ElemId` を返す（純関数。`Model::support_isolator_ends` に委譲）。
///
/// `node_id` が接地節点（FIXED側）自身の場合は `None` を返す（接地節点を選んでも
/// 「配置済み」とは表示しない。上部節点＝対象節点側を選んだ場合のみヒットする）。
pub fn find_support_isolator(
    model: &sepika_core::model::Model,
    node_id: NodeId,
) -> Option<sepika_core::ids::ElemId> {
    model
        .elements
        .iter()
        .find(|e| {
            model
                .support_isolator_ends(e.id)
                .is_some_and(|(upper, _ground)| upper == node_id)
        })
        .map(|e| e.id)
}
#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::model::Model;
    use sepika_edit::UndoStack;

    #[test]
    fn rejected_point_slab_support_removal_preserves_model_history_selection_and_validity() {
        use sepika_core::model::{RegionAnchor, SlabPlate};
        let mut app = App::default();
        app.core.model = crate::sample::portal_frame();
        app.run_preparation();
        assert!(app.core.scoped.preparation.is_some());
        app.core.model = Model {
            nodes: vec![app.core.model.nodes[0].clone()],
            ..Default::default()
        };
        assert!(app.apply_model_edit(Box::new(PlaceSupportIsolator {
            node: NodeId(0),
            props: IsolatorProps::default(),
        })));
        assert!(app.apply_model_edit(Box::new(sepika_edit::AddAttachedSlab {
            anchor: RegionAnchor::Point(NodeId(0)),
            extent: [1000.0, 2000.0],
            plate: SlabPlate::default(),
        })));
        assert_eq!(app.core.model.validate_attached_slabs(), Ok(()));
        assert!(app.apply_model_edit(Box::new(AddNode {
            coord: [8000.0, 0.0, 0.0],
            restraint: Dof6Mask::FREE,
        })));
        app.core.scoped.undo.undo(&mut app.core.model);
        app.select_node(NodeId(0));
        app.ui.scoped.boundary_node = Some(NodeId(0));
        app.core.scoped.results = Some(crate::app::ResultsBundle {
            panel_moments: vec![(NodeId(0), [1200.0, 3400.0])],
            ..Default::default()
        });
        app.core.scoped.staleness.results_stale = false;
        app.core.scoped.staleness.design_stale = false;
        app.core.scoped.staleness.preparation_stale = false;
        app.core.scoped.staleness.diagnostics_stale = false;
        app.core.scoped.staleness.unsaved_changes = false;
        let model = format!("{:?}", app.core.model);
        let selection = format!("{:?}", app.ui.scoped.selection);
        let revision = app.core.scoped.undo.revision();
        let preparation = format!("{:?}", app.core.scoped.preparation);
        let results = format!("{:?}", app.core.scoped.results);
        let undo_label = app.core.scoped.undo.undo_label().map(str::to_owned);
        let redo_label = app.core.scoped.undo.redo_label().map(str::to_owned);
        assert!(app.core.scoped.undo.can_undo());
        assert!(app.core.scoped.undo.can_redo());

        assert!(!remove_support_isolator(&mut app, NodeId(0)));

        assert_eq!(format!("{:?}", app.core.model), model);
        assert_eq!(format!("{:?}", app.ui.scoped.selection), selection);
        assert_eq!(app.ui.scoped.boundary_node, Some(NodeId(0)));
        assert_eq!(app.core.scoped.undo.revision(), revision);
        assert_eq!(format!("{:?}", app.core.scoped.preparation), preparation);
        assert_eq!(format!("{:?}", app.core.scoped.results), results);
        assert_eq!(app.core.scoped.undo.undo_label(), undo_label.as_deref());
        assert_eq!(app.core.scoped.undo.redo_label(), redo_label.as_deref());
        assert!(app.core.scoped.undo.can_undo());
        assert!(app.core.scoped.undo.can_redo());
        assert!(!app.core.scoped.staleness.results_stale);
        assert!(!app.core.scoped.staleness.design_stale);
        assert!(!app.core.scoped.staleness.preparation_stale);
        assert!(!app.core.scoped.staleness.diagnostics_stale);
        assert!(!app.core.scoped.staleness.unsaved_changes);
        assert!(app
            .core
            .scoped
            .undo
            .last_error()
            .unwrap()
            .contains("Slab 0: 集中荷重の支持先が欠落"));
    }

    #[test]
    fn grid_cell_and_boundary_default_render_do_not_override_geometry_selection() {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            boundary_condition_panel(ui, &mut app);
        });
        assert_eq!(app.ui.scoped.selection, crate::app::GeometrySelection::None);
        assert_eq!(app.ui.scoped.boundary_node, None);

        let selected = app.core.model.elements[0].id;
        app.select_member(selected);
        app.ui.scoped.node_grid.grid =
            crate::grid::GridState::new(app.core.model.nodes.len() + 1, 3);
        app.ui
            .scoped
            .node_grid
            .grid
            .click(crate::grid::CellRef { row: 1, col: 0 }, false);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            nodes_table(ui, &mut app);
        });
        assert_eq!(app.ui.scoped.selection.members(), &[selected]);
        assert_eq!(app.ui.scoped.selection.active_member(), Some(selected));
        assert_eq!(app.ui.scoped.node_grid.take_row_selection(), None);
        for key in [egui::Key::A, egui::Key::F2] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            command: true,
                            ctrl: true,
                            ..Default::default()
                        },
                    }],
                    ..Default::default()
                },
                |ui| nodes_table(ui, &mut app),
            );
            assert_eq!(app.ui.scoped.selection.members(), &[selected]);
            assert_eq!(app.ui.scoped.selection.active_member(), Some(selected));
            assert_eq!(app.ui.scoped.node_grid.take_row_selection(), None);
        }
        assert!(app.ui.scoped.node_grid.grid.editing.is_some());
    }

    #[test]
    fn row_header_range_uses_real_node_ids_and_anchor() {
        let mut model = Model::default();
        for id in 0..3 {
            model.nodes.push(sepika_core::model::Node {
                id: NodeId(id),
                coord: [0.0; 3],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        assert_eq!(
            row_geometry_selection(&model, 2, 0),
            (vec![NodeId(0), NodeId(1), NodeId(2)], Some(NodeId(2)))
        );
        assert_eq!(
            row_geometry_selection(&model, 2, 5),
            (vec![NodeId(2)], Some(NodeId(2)))
        );
        assert_eq!(row_geometry_selection(&model, 3, 4), (vec![], None));
        model.nodes[1].id = NodeId(7);
        assert_eq!(
            row_geometry_selection(&model, 0, 3),
            (vec![NodeId(0), NodeId(7), NodeId(2)], Some(NodeId(0)))
        );
        assert_eq!(
            row_geometry_selection(&model, 3, 0),
            (vec![NodeId(0), NodeId(7), NodeId(2)], None)
        );
        for (anchor, cursor) in [(0, 3), (3, 0), (3, 3)] {
            let mut app = App::default();
            app.core.model = model.clone();
            select_node_rows(&mut app, anchor, cursor);
            assert!(app.ui.scoped.selection.active_node().is_none_or(|id| app
                .ui
                .scoped
                .selection
                .nodes()
                .contains(&id)));
        }
        assert_eq!(
            row_geometry_selection(&Model::default(), 0, 0),
            (vec![], None)
        );
    }

    #[test]
    fn boundary_edit_tracks_external_node_without_selecting_default_or_cell() {
        let mut model = Model::default();
        for id in 0..2 {
            model.nodes.push(sepika_core::model::Node {
                id: NodeId(id),
                coord: [0.0; 3],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        assert_eq!(
            boundary_edit_node(&model, Some(NodeId(9)), Some(NodeId(1))),
            Some(NodeId(1))
        );
        let mut app = App::default();
        app.core.model = model.clone();
        assert_eq!(boundary_edit_node(&model, None, None), Some(NodeId(0)));
        assert!(app.ui.scoped.selection.nodes().is_empty());
        app.ui.scoped.boundary_node = Some(NodeId(0));
        app.select_node(NodeId(1));
        assert_eq!(
            boundary_edit_node(
                &app.core.model,
                app.ui.scoped.boundary_node,
                app.ui.scoped.selection.active_node(),
            ),
            Some(NodeId(1))
        );
        assert_eq!(app.ui.scoped.selection.active_node(), Some(NodeId(1)));
        app.clear_geometry_selection();
        assert_eq!(
            boundary_edit_node(&model, Some(NodeId(0)), None),
            Some(NodeId(0))
        );
        model.nodes.remove(1);
        assert_eq!(
            boundary_edit_node(&model, Some(NodeId(1)), Some(NodeId(0))),
            Some(NodeId(0))
        );
    }

    #[test]
    fn row_header_selection_replaces_boundary_edit_target() {
        let mut app = App::default();
        for id in 0..2 {
            app.core.model.nodes.push(sepika_core::model::Node {
                id: NodeId(id),
                coord: [0.0; 3],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        app.ui.scoped.boundary_node = Some(NodeId(0));
        app.select_node(NodeId(0));
        select_node_rows(&mut app, 1, 1);
        assert_eq!(app.ui.scoped.boundary_node, None);
        assert_eq!(
            boundary_edit_node(
                &app.core.model,
                app.ui.scoped.boundary_node,
                app.ui.scoped.selection.active_node(),
            ),
            Some(NodeId(1))
        );
    }

    #[test]
    fn node_deletion_invalidates_local_boundary_target() {
        let mut app = App::default();
        app.ui.scoped.boundary_node = Some(NodeId(1));
        app.select_node(NodeId(1));
        clear_deleted_node_selection(&mut app);
        assert_eq!(app.ui.scoped.boundary_node, None);
        assert_eq!(app.ui.scoped.selection, crate::app::GeometrySelection::None);
    }

    /// `find_support_isolator`: 未設置の節点では `None` を返す。
    #[test]
    fn test_find_support_isolator_none_when_not_placed() {
        let mut model = Model::default();
        model.nodes.push(sepika_core::model::Node {
            id: NodeId(0),
            coord: [0.0, 0.0, 0.0],
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
        assert_eq!(find_support_isolator(&model, NodeId(0)), None);
    }

    /// `PlaceSupportIsolator` 実行後は当該節点に接続する零長 Isolator 要素が
    /// `find_support_isolator` で見つかり、対象節点の拘束は解放（FREE）される。
    #[test]
    fn test_place_support_isolator_then_find_support_isolator() {
        let mut model = Model::default();
        model.nodes.push(sepika_core::model::Node {
            id: NodeId(0),
            coord: [0.0, 0.0, 0.0],
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
        let mut undo = UndoStack::new();
        let props = IsolatorProps {
            kind: IsolatorKind::LeadRubber,
            ..IsolatorProps::default()
        };
        undo.run(
            &mut model,
            Box::new(PlaceSupportIsolator {
                node: NodeId(0),
                props,
            }),
        );

        assert_eq!(model.nodes[0].restraint, Dof6Mask::FREE);
        let found = find_support_isolator(&model, NodeId(0));
        assert!(found.is_some());
        let elem_id = found.unwrap();
        let attr_props = model
            .isolator_attrs
            .iter()
            .find(|a| a.elem == elem_id)
            .map(|a| a.props);
        assert_eq!(attr_props, Some(props));

        // undo で接地節点・要素が消え、拘束も元（FIXED）に戻る。
        undo.undo(&mut model);
        assert_eq!(model.nodes.len(), 1);
        assert_eq!(model.elements.len(), 0);
        assert_eq!(model.nodes[0].restraint, Dof6Mask::FIXED);
    }

    /// `find_support_isolator`: 接地節点（FIXED側）を選んだ場合は「配置済み」と
    /// 誤表示しないよう `None` を返す（対象節点＝上部節点側を選んだ場合のみ
    /// `Some` を返す）。
    #[test]
    fn test_find_support_isolator_none_when_ground_node_selected() {
        let mut model = Model::default();
        model.nodes.push(sepika_core::model::Node {
            id: NodeId(0),
            coord: [0.0, 0.0, 0.0],
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
        let mut undo = UndoStack::new();
        undo.run(
            &mut model,
            Box::new(PlaceSupportIsolator {
                node: NodeId(0),
                props: IsolatorProps::default(),
            }),
        );
        let ground_id = NodeId(1);
        assert_eq!(model.nodes[ground_id.index()].restraint, Dof6Mask::FIXED);
        // 上部節点（対象節点）側では見つかる。
        assert!(find_support_isolator(&model, NodeId(0)).is_some());
        // 接地節点側では見つからない。
        assert_eq!(find_support_isolator(&model, ground_id), None);
    }

    /// 境界条件パネルの「撤去」ボタン相当（`RemoveSupportIsolator`）: 配置→撤去で
    /// 接地節点・要素が消え、対象節点の拘束が FIXED へ戻ること。
    #[test]
    fn test_isolator_support_section_remove_button_command() {
        let mut model = Model::default();
        model.nodes.push(sepika_core::model::Node {
            id: NodeId(0),
            coord: [0.0, 0.0, 0.0],
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
        let before = model.clone();
        let mut undo = UndoStack::new();
        undo.run(
            &mut model,
            Box::new(PlaceSupportIsolator {
                node: NodeId(0),
                props: IsolatorProps::default(),
            }),
        );
        assert!(find_support_isolator(&model, NodeId(0)).is_some());

        undo.run(
            &mut model,
            Box::new(RemoveSupportIsolator { node: NodeId(0) }),
        );
        assert!(find_support_isolator(&model, NodeId(0)).is_none());
        assert!(model.eq_ignoring_dofmap(&before));
        assert_eq!(model.nodes[0].restraint, Dof6Mask::FIXED);
    }

    /// `SetNodeSupportSpring`: 固定されていない自由度にばね値を設定し、undo で解除できる。
    #[test]
    fn test_set_node_support_spring_via_undo() {
        let mut model = Model::default();
        model.nodes.push(sepika_core::model::Node {
            id: NodeId(0),
            coord: [0.0, 0.0, 0.0],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
        let mut undo = UndoStack::new();
        let spring = [1.0e5, 1.0e5, 2.0e5, 1.0e9, 1.0e9, 1.0e9];
        undo.run(
            &mut model,
            Box::new(SetNodeSupportSpring {
                node: NodeId(0),
                spring: Some(spring),
            }),
        );
        assert_eq!(model.nodes[0].support_spring, Some(spring));

        undo.undo(&mut model);
        assert_eq!(model.nodes[0].support_spring, None);
    }
}
