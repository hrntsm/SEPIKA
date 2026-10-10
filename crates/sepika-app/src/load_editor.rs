//! 荷重の追加・編集モーダルと、対象を 3D ビューで選ぶピックモード。
//!
//! ナビゲータ（左パネル）の荷重ツリーを右クリックして開く。対象の節点・部材は
//! 数が多く ID を並べても選べないため、3D ビューでのクリック選択を既定とする。
//!
//! モーダルは対象選択の間だけ閉じる。「3D で選択」を押すと入力内容を保持したまま
//! [`LoadEditor::picking`] を立ててモーダルを閉じ、3D クリックで仮選択、Enter で
//! 確定してモーダルへ戻る（Esc は選び直しを取り消して元の対象へ戻す）。
//! ピック待ちの間はアプリ全体を操作できるため、確定時に対象の存在と、
//! 編集の場合は開いた時点の内容との一致を検証する。

use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::{ElementKind, MemberLoad, MemberLoadKind, Model, NodalLoad};

use crate::app::App;

/// 部材荷重の作用方向の選択肢（全体座標）。
const DIR_CHOICES: [(&str, [f64; 3]); 6] = [
    ("鉛直下(-Z)", [0.0, 0.0, -1.0]),
    ("鉛直上(+Z)", [0.0, 0.0, 1.0]),
    ("X+", [1.0, 0.0, 0.0]),
    ("X-", [-1.0, 0.0, 0.0]),
    ("Y+", [0.0, 1.0, 0.0]),
    ("Y-", [0.0, -1.0, 0.0]),
];

/// 部材荷重の作用方向が材軸方向であることを示す選択肢の番号
/// （ブレースはこの選択肢しか選べない。[`brace_axis_dir`] を参照）。
const DIR_ALONG_AXIS: usize = DIR_CHOICES.len();
const DIR_SAVED: usize = DIR_ALONG_AXIS + 1;

/// 荷重の種類。ツリーのどのグループから開いたかで決まり、モーダルの間は変わらない。
#[derive(Clone, Debug, PartialEq)]
pub enum LoadDraft {
    Nodal(NodalDraft),
    Member(MemberDraft),
}

/// 節点荷重の入力内容。成分は文字列で保持し、確定時に解釈する
/// （入力途中の `-` や空文字で値が飛ばないようにする）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodalDraft {
    pub name: String,
    pub node: Option<NodeId>,
    pub values: [String; 6],
}

/// 部材荷重の入力内容。
#[derive(Clone, Debug, PartialEq)]
pub struct MemberDraft {
    pub name: String,
    pub elem: Option<ElemId>,
    /// 0=中間集中、1=等分布、2=台形。
    pub kind: u8,
    /// 全体方向の選択肢、現在の材軸方向、または既存荷重の保存済み方向。
    pub dir: usize,
    pub a: String,
    pub b: String,
    pub w1: String,
    pub w2: String,
    pub p: String,
}

impl Default for MemberDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            elem: None,
            kind: 1,
            dir: 0,
            a: "0".into(),
            b: "0".into(),
            w1: "0".into(),
            w2: "0".into(),
            p: "0".into(),
        }
    }
}

/// モーダルが編集している対象。
#[derive(Clone, Debug, PartialEq)]
pub enum LoadEditTarget {
    /// 新規追加。
    New,
    /// 既存荷重の編集。`index` は荷重ケース内の添字、`snapshot` は開いた時点の内容。
    /// ピック待ちの間に他の操作で添字がずれても取り違えないよう、確定時に照合する。
    ExistingNodal { index: usize, snapshot: NodalLoad },
    /// 既存の部材荷重の編集（[`LoadEditTarget::ExistingNodal`] と同じ規約）。
    ExistingMember { index: usize, snapshot: MemberLoad },
}

/// 荷重の追加・編集モーダルの状態。
#[derive(Clone, Debug, PartialEq)]
pub struct LoadEditor {
    /// 対象の荷重ケース。
    pub lc: LoadCaseId,
    pub target: LoadEditTarget,
    pub draft: LoadDraft,
    /// 3D ピック待ち（true の間モーダルは閉じている）。
    pub picking: bool,
    /// ピック待ちに入る直前の対象。Esc で元へ戻すために保持する。
    pick_backup: Option<PickBackup>,
    /// 直前の確定操作で生じたエラー（対象が消えた・内容が変わった等）。
    pub error: Option<String>,
}

/// ピック待ちに入る前の対象（Esc の復元用）。
#[derive(Clone, Copy, Debug, PartialEq)]
enum PickBackup {
    Node(Option<NodeId>),
    /// 対象部材と作用方向の選択肢番号。
    Member(Option<ElemId>, usize),
}

impl LoadEditor {
    /// 節点荷重を新規追加するモーダルを開く。`focus_node` が指す節点を初期値にする。
    pub fn new_nodal(lc: LoadCaseId, focus_node: Option<NodeId>) -> Self {
        Self {
            lc,
            target: LoadEditTarget::New,
            draft: LoadDraft::Nodal(NodalDraft {
                node: focus_node,
                values: std::array::from_fn(|_| "0".to_string()),
                ..Default::default()
            }),
            picking: false,
            pick_backup: None,
            error: None,
        }
    }

    /// 部材荷重を新規追加するモーダルを開く。`focus_member` が指す部材を初期値にする。
    pub fn new_member(lc: LoadCaseId, focus_member: Option<ElemId>) -> Self {
        Self {
            lc,
            target: LoadEditTarget::New,
            draft: LoadDraft::Member(MemberDraft {
                elem: focus_member,
                ..Default::default()
            }),
            picking: false,
            pick_backup: None,
            error: None,
        }
    }

    /// 既存の節点荷重を編集するモーダルを開く。
    pub fn edit_nodal(lc: LoadCaseId, index: usize, load: &NodalLoad) -> Self {
        Self {
            lc,
            target: LoadEditTarget::ExistingNodal {
                index,
                snapshot: load.clone(),
            },
            draft: LoadDraft::Nodal(NodalDraft {
                name: load.name.clone(),
                node: Some(load.node),
                values: load.values.map(|v| format!("{}", v)),
            }),
            picking: false,
            pick_backup: None,
            error: None,
        }
    }

    /// 既存の部材荷重を編集するモーダルを開く。
    pub fn edit_member(lc: LoadCaseId, index: usize, load: &MemberLoad, _model: &Model) -> Self {
        let (kind, a, b, w1, w2, p) = match load.kind {
            MemberLoadKind::Point { a, p } => (
                0u8,
                format!("{}", a),
                "0".into(),
                "0".into(),
                "0".into(),
                format!("{}", p),
            ),
            MemberLoadKind::Distributed { a, b, w1, w2 } => {
                let uniform =
                    load.extent == sepika_core::model::MemberLoadExtent::FullLengthUniform;
                (
                    if uniform { 1 } else { 2 },
                    format!("{}", a),
                    format!("{}", b),
                    format!("{}", w1),
                    format!("{}", w2),
                    "0".into(),
                )
            }
        };
        Self {
            lc,
            target: LoadEditTarget::ExistingMember {
                index,
                snapshot: load.clone(),
            },
            draft: LoadDraft::Member(MemberDraft {
                name: load.name.clone(),
                elem: Some(load.elem),
                kind,
                dir: DIR_SAVED,
                a,
                b,
                w1,
                w2,
                p,
            }),
            picking: false,
            pick_backup: None,
            error: None,
        }
    }

    /// ピック待ちへ入る（モーダルを閉じる）。現在の対象を復元用に控える。
    pub fn begin_pick(&mut self) {
        self.pick_backup = Some(match &self.draft {
            LoadDraft::Nodal(d) => PickBackup::Node(d.node),
            LoadDraft::Member(d) => PickBackup::Member(d.elem, d.dir),
        });
        self.picking = true;
        self.error = None;
    }

    /// ピックを確定してモーダルへ戻る。
    pub fn confirm_pick(&mut self) {
        self.picking = false;
        self.pick_backup = None;
    }

    /// ピックを取り消し、対象と方向を元へ戻してモーダルへ戻る。
    pub fn cancel_pick(&mut self) {
        match (self.pick_backup.take(), &mut self.draft) {
            (Some(PickBackup::Node(n)), LoadDraft::Nodal(d)) => d.node = n,
            (Some(PickBackup::Member(e, dir)), LoadDraft::Member(d)) => {
                d.elem = e;
                d.dir = dir;
            }
            _ => {}
        }
        self.picking = false;
    }

    /// 3D でピックした節点を仮選択として反映する（節点荷重のときのみ）。
    pub fn set_picked_node(&mut self, node: NodeId) {
        if let LoadDraft::Nodal(d) = &mut self.draft {
            d.node = Some(node);
        }
    }

    /// 3D でピックした部材を仮選択として反映する（部材荷重のときのみ）。
    /// ブレースを選んだ場合、材軸直交方向の入力は意味を持たないため
    /// 方向を材軸方向へ切り替える（[`brace_axis_dir`] を参照）。
    /// `is_brace` はモデルではなく判定結果で受け取る。ビューア側は不変借用を
    /// 先に終わらせてからエディタを可変借用でき、ピック 1 回ごとにモデルを
    /// 複製する必要がなくなる（大規模モデルではクリックのたびに大きな確保が走る）。
    pub fn set_picked_member(&mut self, elem: ElemId, is_brace: bool) {
        if let LoadDraft::Member(d) = &mut self.draft {
            d.elem = Some(elem);
            if d.dir == DIR_SAVED {
                return;
            }
            if is_brace {
                d.dir = DIR_ALONG_AXIS;
            } else if d.dir == DIR_ALONG_AXIS {
                d.dir = 0;
            }
        }
    }

    /// ピックの対象が節点か（false なら部材）。
    pub fn picks_node(&self) -> bool {
        matches!(self.draft, LoadDraft::Nodal(_))
    }
}

/// 指定部材がブレース（トラス要素）か。
pub fn is_brace(model: &Model, elem: ElemId) -> bool {
    model
        .elements
        .iter()
        .any(|e| e.id == elem && matches!(e.kind, ElementKind::Brace { .. }))
}

/// ブレースの材軸方向（i→j の単位ベクトル）。求まらない場合は鉛直下向き。
fn brace_axis_dir(model: &Model, elem: ElemId) -> [f64; 3] {
    let Some(e) = model.element(elem) else {
        return [0.0, 0.0, -1.0];
    };
    if e.nodes.len() < 2 {
        return [0.0, 0.0, -1.0];
    }
    let (Some(i), Some(j)) = (
        model.nodes.get(e.nodes[0].index()),
        model.nodes.get(e.nodes[1].index()),
    ) else {
        return [0.0, 0.0, -1.0];
    };
    let d = [
        j.coord[0] - i.coord[0],
        j.coord[1] - i.coord[1],
        j.coord[2] - i.coord[2],
    ];
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if n < 1e-9 {
        [0.0, 0.0, -1.0]
    } else {
        [d[0] / n, d[1] / n, d[2] / n]
    }
}

/// 入力欄の文字列を数値へ。空欄・解釈できない文字列は 0 とする。
fn parse(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

impl App {
    /// 荷重の 3D ピック待ちか（ビューアのクリック処理・他の作成モードとの排他判定）。
    pub(crate) fn load_pick_active(&self) -> bool {
        self.ui
            .scoped
            .load_editor
            .as_ref()
            .is_some_and(|e| e.picking)
    }

    /// 荷重モーダル・ピックモードの毎フレーム処理。
    /// ビューアより先に呼ぶと、ピック確定のキー入力を 3D クリックと同じフレームで
    /// 拾ってしまうため、中央パネルの描画後に呼ぶ。
    pub(crate) fn load_editor_ui(&mut self, ctx: &egui::Context) {
        if self.ui.scoped.load_editor.is_none() {
            return;
        }
        if self.load_pick_active() {
            self.load_pick_bar(ctx);
            return;
        }
        self.load_editor_modal(ctx);
    }

    /// ピック待ち中の案内バー（画面上端）。モーダルは閉じているため、
    /// いま何を求められているか・どう抜けるかをここだけが示す。
    fn load_pick_bar(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.ui.scoped.load_editor.as_ref() else {
            return;
        };
        let picks_node = editor.picks_node();
        let current = match &editor.draft {
            LoadDraft::Nodal(d) => d.node.map(|n| format!("節点 N{}", n.0)),
            LoadDraft::Member(d) => d.elem.map(|e| format!("部材 #{}", e.0)),
        };
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("load_pick_bar")
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 8.0])
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        crate::theme::WARN_TEXT,
                        if picks_node {
                            "荷重の対象を選択中：3D ビューで節点をクリック"
                        } else {
                            "荷重の対象を選択中：3D ビューで部材をクリック"
                        },
                    );
                    match &current {
                        Some(label) => {
                            ui.label(format!("選択中: {}", label));
                        }
                        None => {
                            ui.colored_label(crate::theme::GRAY_600, "未選択");
                        }
                    }
                    confirm = ui
                        .add_enabled(current.is_some(), egui::Button::new("確定 (Enter)"))
                        .clicked();
                    cancel = ui.button("取消 (Esc)").clicked();
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && current.is_some() {
            confirm = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }

        if let Some(editor) = self.ui.scoped.load_editor.as_mut() {
            if confirm {
                editor.confirm_pick();
            } else if cancel {
                editor.cancel_pick();
            }
        }
    }

    /// 荷重の追加・編集モーダル本体。
    fn load_editor_modal(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.ui.scoped.load_editor.take() else {
            return;
        };
        let mut editor = editor;
        let mut close = false;
        let mut commit = false;
        let mut begin_pick = false;

        let title = match (&editor.target, &editor.draft) {
            (LoadEditTarget::New, LoadDraft::Nodal(_)) => "節点荷重の追加",
            (LoadEditTarget::New, LoadDraft::Member(_)) => "部材荷重の追加",
            (_, LoadDraft::Nodal(_)) => "節点荷重の編集",
            (_, LoadDraft::Member(_)) => "部材荷重の編集",
        };
        let case_label = self
            .core
            .model
            .load_cases
            .iter()
            .find(|lc| lc.id == editor.lc)
            .map(|lc| format!("[{}] {}", lc.id.0, lc.name))
            .unwrap_or_else(|| "（不明な荷重ケース）".to_string());

        let saved_dir = match &editor.target {
            LoadEditTarget::ExistingMember { snapshot, .. } => Some(snapshot.dir),
            _ => None,
        };
        let modal = egui::Modal::new(egui::Id::new("load_editor_modal")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading(title);
            ui.label(format!("荷重ケース: {}", case_label));
            ui.separator();

            match &mut editor.draft {
                LoadDraft::Nodal(d) => {
                    ui.horizontal(|ui| {
                        ui.label("名称:");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.name)
                                .hint_text("未入力可（成分から自動表示）")
                                .desired_width(260.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("対象節点:");
                        match d.node {
                            Some(n) => {
                                let coord = self
                                    .core
                                    .model
                                    .nodes
                                    .get(n.index())
                                    .map(|nd| {
                                        format!(
                                            "N{} ({:.0}, {:.0}, {:.0})",
                                            n.0, nd.coord[0], nd.coord[1], nd.coord[2]
                                        )
                                    })
                                    .unwrap_or_else(|| format!("N{}（存在しません）", n.0));
                                ui.label(coord);
                            }
                            None => {
                                ui.colored_label(crate::theme::WARN_TEXT, "未選択");
                            }
                        }
                        begin_pick |= ui
                            .button("3D で選択")
                            .on_hover_text(
                                "入力内容を保ったままモーダルを閉じ、3D ビューで節点を選びます",
                            )
                            .clicked();
                    });
                    ui.add_space(4.0);
                    ui.label("荷重成分（力 [N]・モーメント [N·mm]）");
                    egui::Grid::new("load_editor_nodal_values")
                        .num_columns(4)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            for (k, label) in
                                ["Fx", "Fy", "Fz", "Mx", "My", "Mz"].into_iter().enumerate()
                            {
                                ui.label(label);
                                ui.add(
                                    egui::TextEdit::singleline(&mut d.values[k])
                                        .desired_width(110.0),
                                );
                                if k % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                }
                LoadDraft::Member(d) => {
                    ui.horizontal(|ui| {
                        ui.label("名称:");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.name)
                                .hint_text("未入力可（種別から自動表示）")
                                .desired_width(260.0),
                        );
                    });
                    let brace = d.elem.is_some_and(|e| is_brace(&self.core.model, e));
                    ui.horizontal(|ui| {
                        ui.label("対象部材:");
                        match d.elem {
                            Some(e) => {
                                let kind = self
                                    .core
                                    .model
                                    .elements
                                    .iter()
                                    .find(|el| el.id == e)
                                    .map(|el| format!("{:?}", el.kind))
                                    .unwrap_or_else(|| "存在しません".to_string());
                                ui.label(format!("#{} ({})", e.0, kind));
                            }
                            None => {
                                ui.colored_label(crate::theme::WARN_TEXT, "未選択");
                            }
                        }
                        begin_pick |= ui
                            .button("3D で選択")
                            .on_hover_text(
                                "入力内容を保ったままモーダルを閉じ、3D ビューで部材を選びます",
                            )
                            .clicked();
                    });
                    if brace {
                        ui.colored_label(
                            crate::theme::GRAY_600,
                            "新規ブレース荷重は材軸方向です。既存荷重の保存済み方向は保持できます。\
                             材軸直交方向の荷重は両端の節点へ静定分配されます。",
                        );
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.label("種別:");
                        ui.selectable_value(&mut d.kind, 0u8, "中間集中");
                        ui.selectable_value(&mut d.kind, 1u8, "全長等分布（全長追従）");
                        ui.selectable_value(&mut d.kind, 2u8, "区間分布（固定距離）");
                    });
                    ui.horizontal(|ui| {
                        ui.label("方向:");
                        if brace && d.dir != DIR_SAVED {
                            d.dir = DIR_ALONG_AXIS;
                        }
                        let saved_label = saved_dir
                            .map(|dir| format!("保存済み方向 ({},{},{})", dir[0], dir[1], dir[2]));
                        let current = if d.dir == DIR_SAVED {
                            saved_label.as_deref().unwrap_or("鉛直下(-Z)")
                        } else if brace {
                            "材軸方向"
                        } else {
                            DIR_CHOICES
                                .get(d.dir)
                                .map(|(label, _)| *label)
                                .unwrap_or("鉛直下(-Z)")
                        };
                        egui::ComboBox::from_id_salt("load_editor_member_dir")
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                if let Some(label) = saved_label.as_deref() {
                                    ui.selectable_value(&mut d.dir, DIR_SAVED, label);
                                }
                                if brace {
                                    ui.selectable_value(
                                        &mut d.dir,
                                        DIR_ALONG_AXIS,
                                        "現在の材軸方向",
                                    );
                                } else {
                                    for (idx, (label, _)) in DIR_CHOICES.iter().enumerate() {
                                        ui.selectable_value(&mut d.dir, idx, *label);
                                    }
                                }
                            });
                    });
                    match d.kind {
                        0 => {
                            ui.horizontal(|ui| {
                                ui.label("a [mm]:");
                                ui.add(egui::TextEdit::singleline(&mut d.a).desired_width(90.0));
                                ui.label("P [N]:");
                                ui.add(egui::TextEdit::singleline(&mut d.p).desired_width(90.0));
                            });
                        }
                        1 => {
                            ui.horizontal(|ui| {
                                ui.label("w [N/mm]:");
                                ui.add(egui::TextEdit::singleline(&mut d.w1).desired_width(90.0));
                                ui.colored_label(crate::theme::GRAY_600, "材長全体に等分布");
                            });
                        }
                        _ => {
                            ui.horizontal(|ui| {
                                ui.label("a [mm]:");
                                ui.add(egui::TextEdit::singleline(&mut d.a).desired_width(90.0));
                                ui.label("b [mm]:");
                                ui.add(egui::TextEdit::singleline(&mut d.b).desired_width(90.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("w1 [N/mm]:");
                                ui.add(egui::TextEdit::singleline(&mut d.w1).desired_width(90.0));
                                ui.label("w2 [N/mm]:");
                                ui.add(egui::TextEdit::singleline(&mut d.w2).desired_width(90.0));
                            });
                        }
                    }
                }
            }

            if let Some(err) = &editor.error {
                ui.add_space(4.0);
                ui.colored_label(crate::theme::ERROR_RED, err);
            }

            ui.separator();
            ui.horizontal(|ui| {
                let has_target = match &editor.draft {
                    LoadDraft::Nodal(d) => d.node.is_some(),
                    LoadDraft::Member(d) => d.elem.is_some(),
                };
                let ok_label = if matches!(editor.target, LoadEditTarget::New) {
                    "追加"
                } else {
                    "更新"
                };
                commit = ui
                    .add_enabled(has_target, egui::Button::new(ok_label))
                    .on_disabled_hover_text("対象の節点／部材を選んでください")
                    .clicked();
                close = ui.button("キャンセル").clicked();
            });
        });
        close |= modal.should_close();

        if begin_pick {
            editor.begin_pick();
            self.ui.scoped.load_editor = Some(editor);
            return;
        }
        if close {
            return;
        }
        if commit {
            match self.commit_load_editor(&editor) {
                Ok(()) => return,
                Err(msg) => editor.error = Some(msg),
            }
        }
        self.ui.scoped.load_editor = Some(editor);
    }

    /// モーダルの入力内容を編集コマンドとして発行する。
    /// 対象が消えている・編集対象の内容が開いた時点と変わっている場合はエラーを返す
    /// （ピック待ちの間にモデルが編集され、添字が別の荷重を指している可能性がある）。
    fn commit_load_editor(&mut self, editor: &LoadEditor) -> Result<(), String> {
        let lc = editor.lc;
        if !self.core.model.load_cases.iter().any(|c| c.id == lc) {
            return Err("荷重ケースが見つかりません".to_string());
        }
        match &editor.draft {
            LoadDraft::Nodal(d) => {
                let Some(node) = d.node else {
                    return Err("対象の節点が選ばれていません".to_string());
                };
                if node.index() >= self.core.model.nodes.len() {
                    return Err(format!("節点 N{} は存在しません", node.0));
                }
                let load = NodalLoad {
                    node,
                    values: std::array::from_fn(|k| parse(&d.values[k])),
                    name: d.name.trim().to_string(),
                    source: sepika_core::model::LoadSource::Manual,
                };
                match &editor.target {
                    LoadEditTarget::New => {
                        self.core.scoped.undo.run(
                            &mut self.core.model,
                            Box::new(sepika_edit::AddNodalLoad { lc, load }),
                        );
                    }
                    LoadEditTarget::ExistingNodal { index, snapshot } => {
                        self.verify_nodal_snapshot(lc, *index, snapshot)?;
                        self.core.scoped.undo.run(
                            &mut self.core.model,
                            Box::new(sepika_edit::SetNodalLoad {
                                lc,
                                index: *index,
                                load,
                            }),
                        );
                    }
                    LoadEditTarget::ExistingMember { .. } => {
                        return Err("編集対象の種類が一致しません".to_string())
                    }
                }
            }
            LoadDraft::Member(d) => {
                let Some(elem) = d.elem else {
                    return Err("対象の部材が選ばれていません".to_string());
                };
                let Some(element) = self.core.model.element(elem) else {
                    return Err(format!("部材 #{} は存在しません", elem.0));
                };
                let length = self.core.model.member_length(element);
                if length <= 1e-9 {
                    return Err(format!("部材 #{} の材長が 0 です", elem.0));
                }
                let preserved_dir = match &editor.target {
                    LoadEditTarget::ExistingMember { snapshot, .. } if d.dir == DIR_SAVED => {
                        Some(snapshot.dir)
                    }
                    _ => None,
                };
                let dir = if let Some(dir) = preserved_dir {
                    dir
                } else if is_brace(&self.core.model, elem) {
                    brace_axis_dir(&self.core.model, elem)
                } else {
                    DIR_CHOICES
                        .get(d.dir)
                        .map(|(_, v)| *v)
                        .unwrap_or(DIR_CHOICES[0].1)
                };
                let kind = match d.kind {
                    0 => MemberLoadKind::Point {
                        a: parse(&d.a),
                        p: parse(&d.p),
                    },
                    1 => MemberLoadKind::Distributed {
                        a: 0.0,
                        b: length,
                        w1: parse(&d.w1),
                        w2: parse(&d.w1),
                    },
                    _ => MemberLoadKind::Distributed {
                        a: parse(&d.a),
                        b: parse(&d.b),
                        w1: parse(&d.w1),
                        w2: parse(&d.w2),
                    },
                };
                if let MemberLoadKind::Distributed { a, b, .. } = kind {
                    if b <= a {
                        return Err("分布区間は b > a となるように入力してください".to_string());
                    }
                }
                let load = MemberLoad {
                    elem,
                    dir,
                    kind,
                    extent: if d.kind == 1 {
                        sepika_core::model::MemberLoadExtent::FullLengthUniform
                    } else {
                        sepika_core::model::MemberLoadExtent::FixedDistance
                    },
                    name: d.name.trim().to_string(),
                    source: sepika_core::model::LoadSource::Manual,
                };
                let applied = match &editor.target {
                    LoadEditTarget::New => self.core.scoped.undo.run(
                        &mut self.core.model,
                        Box::new(sepika_edit::AddMemberLoad { lc, load }),
                    ),
                    LoadEditTarget::ExistingMember { index, snapshot } => {
                        self.verify_member_snapshot(lc, *index, snapshot)?;
                        self.core.scoped.undo.run(
                            &mut self.core.model,
                            Box::new(sepika_edit::SetMemberLoad {
                                lc,
                                index: *index,
                                load,
                            }),
                        )
                    }
                    LoadEditTarget::ExistingNodal { .. } => {
                        return Err("編集対象の種類が一致しません".to_string())
                    }
                };
                if !applied {
                    return Err(self
                        .core
                        .scoped
                        .undo
                        .last_error()
                        .unwrap_or("部材荷重を変更できません")
                        .to_owned());
                }
            }
        }
        self.core.scoped.staleness.mark_edited();
        Ok(())
    }

    /// 編集対象の節点荷重が、モーダルを開いた時点の内容のままか確認する。
    fn verify_nodal_snapshot(
        &self,
        lc: LoadCaseId,
        index: usize,
        snapshot: &NodalLoad,
    ) -> Result<(), String> {
        let case = self
            .core
            .model
            .load_cases
            .iter()
            .find(|c| c.id == lc)
            .ok_or_else(|| "荷重ケースが見つかりません".to_string())?;
        match case.nodal.get(index) {
            Some(cur) if cur == snapshot => Ok(()),
            _ => Err(STALE_TARGET_MESSAGE.to_string()),
        }
    }

    /// 編集対象の部材荷重が、モーダルを開いた時点の内容のままか確認する。
    fn verify_member_snapshot(
        &self,
        lc: LoadCaseId,
        index: usize,
        snapshot: &MemberLoad,
    ) -> Result<(), String> {
        let case = self
            .core
            .model
            .load_cases
            .iter()
            .find(|c| c.id == lc)
            .ok_or_else(|| "荷重ケースが見つかりません".to_string())?;
        match case.member.get(index) {
            Some(cur) if cur == snapshot => Ok(()),
            _ => Err(STALE_TARGET_MESSAGE.to_string()),
        }
    }
}

/// 編集対象が入れ替わっていたときの案内。
const STALE_TARGET_MESSAGE: &str =
    "編集中に対象の荷重が変更・削除されました。閉じてから選び直してください";
#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::model::{ElementKind, LoadCase, LoadCaseKind};

    /// ブレースと梁を 1 本ずつ持つモデル（荷重の対象選択の検証用）。
    /// 要素 0 が梁、要素 1 がブレース。
    fn beam_and_brace_model() -> Model {
        use sepika_core::dof::Dof6Mask;
        use sepika_core::ids::NodeId;
        use sepika_core::model::{ElementData, EndCondition, ForceRegime, LocalAxis, Node};

        let node = |id: u32, x: f64, z: f64| Node {
            id: NodeId(id),
            coord: [x, 0.0, z],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        let elem = |id: u32, kind: ElementKind, a: u32, b: u32| ElementData {
            id: ElemId(id),
            kind,
            nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        };
        Model {
            nodes: vec![
                node(0, 0.0, 0.0),
                node(1, 6000.0, 0.0),
                node(2, 6000.0, 4000.0),
            ],
            elements: vec![
                elem(0, ElementKind::Beam, 0, 1),
                elem(
                    1,
                    ElementKind::Brace {
                        tension_only: false,
                    },
                    0,
                    2,
                ),
            ],
            load_cases: vec![LoadCase {
                id: LoadCaseId(0),
                name: "LC0".into(),
                nodal: Vec::new(),
                member: Vec::new(),
                kind: LoadCaseKind::Other,
            }],
            ..Default::default()
        }
    }

    /// ピック中にブレースを選んでから取り消すと、対象だけでなく作用方向も元へ戻る。
    ///
    /// 方向が戻らないと、対象が梁に戻ったのに方向だけ「材軸方向」を指したままになり、
    /// 確定時に選択肢の範囲外を引く。
    #[test]
    fn cancel_pick_restores_direction_together_with_target() {
        let model = beam_and_brace_model();
        let mut editor = LoadEditor::new_member(LoadCaseId(0), Some(ElemId(0)));
        if let LoadDraft::Member(d) = &mut editor.draft {
            d.dir = 2; // X+
        }

        editor.begin_pick();
        editor.set_picked_member(ElemId(1), is_brace(&model, ElemId(1))); // ブレース
        match &editor.draft {
            LoadDraft::Member(d) => {
                assert_eq!(d.elem, Some(ElemId(1)));
                assert_eq!(d.dir, DIR_ALONG_AXIS, "ブレースは材軸方向へ切り替わる");
            }
            _ => panic!("部材荷重のはず"),
        }

        editor.cancel_pick();
        match &editor.draft {
            LoadDraft::Member(d) => {
                assert_eq!(d.elem, Some(ElemId(0)), "対象が元へ戻る");
                assert_eq!(d.dir, 2, "方向も元へ戻る");
            }
            _ => panic!("部材荷重のはず"),
        }
    }

    /// 対象がブレースでないのに方向が材軸方向を指していても、確定は落ちずに
    /// 対象部材から方向を決め直す（下書きの番号を信じない）。
    #[test]
    fn commit_derives_direction_from_target_not_draft() {
        let mut app = App::default();
        app.load_model(beam_and_brace_model());

        let mut editor = LoadEditor::new_member(LoadCaseId(0), Some(ElemId(0)));
        if let LoadDraft::Member(d) = &mut editor.draft {
            d.dir = DIR_ALONG_AXIS; // 梁なのに材軸方向を指した状態
            d.w1 = "2.0".into();
        }
        app.commit_load_editor(&editor).expect("追加できるはず");

        let load = &app.core.model.load_cases[0].member[0];
        assert_eq!(load.dir, DIR_CHOICES[0].1, "梁は既定の鉛直下向きへ落とす");
    }

    /// ブレースを対象にすると、下書きの方向によらず材軸方向の単位ベクトルになる。
    #[test]
    fn commit_uses_axis_direction_for_brace() {
        let mut app = App::default();
        app.load_model(beam_and_brace_model());

        let mut editor = LoadEditor::new_member(LoadCaseId(0), Some(ElemId(1)));
        if let LoadDraft::Member(d) = &mut editor.draft {
            d.dir = 0; // 鉛直下向きを指した状態
            d.w1 = "1.0".into();
        }
        app.commit_load_editor(&editor).expect("追加できるはず");

        // 節点 0 (0,0,0) → 節点 2 (6000,0,4000) の単位ベクトル
        let load = &app.core.model.load_cases[0].member[0];
        let n = (6000.0_f64.powi(2) + 4000.0_f64.powi(2)).sqrt();
        assert!((load.dir[0] - 6000.0 / n).abs() < 1e-9, "{:?}", load.dir);
        assert!(load.dir[1].abs() < 1e-9, "{:?}", load.dir);
        assert!((load.dir[2] - 4000.0 / n).abs() < 1e-9, "{:?}", load.dir);
    }

    /// 編集中に対象の荷重が入れ替わっていたら、別の荷重を書き換えずにエラーにする。
    #[test]
    fn commit_rejects_stale_edit_target() {
        use sepika_core::model::NodalLoad;

        let mut app = App::default();
        app.load_model(beam_and_brace_model());
        let first = NodalLoad::manual(NodeId(0), [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let second = NodalLoad::manual(NodeId(1), [2.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        for load in [first.clone(), second.clone()] {
            app.core.scoped.undo.run(
                &mut app.core.model,
                Box::new(sepika_edit::AddNodalLoad {
                    lc: LoadCaseId(0),
                    load,
                }),
            );
        }

        // 添字 1 の荷重を編集するモーダルを開いた状態を作る。
        let editor = LoadEditor::edit_nodal(LoadCaseId(0), 1, &second);
        // 開いている間に添字 0 が消え、添字 1 が別の荷重を指すようになる。
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::DeleteNodalLoad {
                lc: LoadCaseId(0),
                index: 0,
            }),
        );

        let err = app.commit_load_editor(&editor).unwrap_err();
        assert_eq!(err, STALE_TARGET_MESSAGE);
        assert_eq!(
            app.core.model.load_cases[0].nodal,
            vec![second],
            "書き換わらない"
        );
    }
    #[test]
    fn full_length_creation_edit_and_coordinate_change_preserve_intent_and_resultant() {
        use sepika_core::model::MemberLoadExtent;
        use sepika_element::frame::member_load::{consistent_load_local, SpanLoadTransfer};
        use sepika_element::transform::LocalFrame;
        let mut app = App::default();
        app.load_model(beam_and_brace_model());
        let mut editor = LoadEditor::new_member(LoadCaseId(0), Some(ElemId(0)));
        if let LoadDraft::Member(d) = &mut editor.draft {
            d.w1 = "10".into();
            d.name = "機器".into();
        }
        app.commit_load_editor(&editor).unwrap();
        for (origin, coord, expected_force_n, expected_centroid_mm) in [
            (
                [0.0, 0.0, 0.0],
                [6000.0, 0.0, 0.0],
                60000.0,
                [3000.0, 0.0, 0.0],
            ),
            (
                [0.0, 0.0, 0.0],
                [8000.0, 0.0, 0.0],
                80000.0,
                [4000.0, 0.0, 0.0],
            ),
            (
                [0.0, 0.0, 0.0],
                [0.0, 8000.0, 0.0],
                80000.0,
                [0.0, 4000.0, 0.0],
            ),
            (
                [6000.0, 0.0, 0.0],
                [12000.0, 0.0, 0.0],
                60000.0,
                [9000.0, 0.0, 0.0],
            ),
        ] {
            assert!(app.core.scoped.undo.run(
                &mut app.core.model,
                Box::new(sepika_edit::CompositeCommand {
                    label: "座標更新".into(),
                    children: vec![
                        Box::new(sepika_edit::SetNodeCoord {
                            node: NodeId(0),
                            coord: origin
                        }),
                        Box::new(sepika_edit::SetNodeCoord {
                            node: NodeId(1),
                            coord
                        }),
                    ],
                })
            ));
            let model = &app.core.model;
            let load = &model.load_cases[0].member[0];
            assert_eq!(load.extent, MemberLoadExtent::FullLengthUniform);
            assert_eq!(load.dir, [0.0, 0.0, -1.0]);
            let length_mm = model.member_length(&model.elements[0]);
            let frame = LocalFrame::from_nodes(model.nodes[0].coord, coord, [0.0, 0.0, 1.0]);
            let q = frame.rotate_to_global(&consistent_load_local(
                std::slice::from_ref(load),
                &frame,
                length_mm,
                SpanLoadTransfer::Consistent,
            ));
            assert!((q[2] + q[8] + expected_force_n).abs() < 1e-7);
            let centroid = std::array::from_fn::<_, 3, _>(|axis| {
                (q[2] * model.nodes[0].coord[axis] + q[8] * coord[axis]) / (q[2] + q[8])
            });
            for axis in 0..3 {
                assert!((centroid[axis] - expected_centroid_mm[axis]).abs() < 1e-7);
            }
            let opened = LoadEditor::edit_member(LoadCaseId(0), 0, load, model);
            assert!(matches!(
                opened.draft,
                LoadDraft::Member(MemberDraft { kind: 1, .. })
            ));
            app.commit_load_editor(&opened).unwrap();
        }
    }

    #[test]
    fn fixed_uniform_full_span_opens_as_fixed_and_keeps_its_interval_on_commit() {
        let mut app = App::default();
        app.load_model(beam_and_brace_model());
        let load = MemberLoad::manual(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 6000.0,
                w1: 10.0,
                w2: 10.0,
            },
        );
        app.core.model.load_cases[0].member.push(load.clone());
        let editor = LoadEditor::edit_member(LoadCaseId(0), 0, &load, &app.core.model);
        assert!(matches!(
            editor.draft,
            LoadDraft::Member(MemberDraft { kind: 2, .. })
        ));
        app.commit_load_editor(&editor).unwrap();
        assert_eq!(app.core.model.load_cases[0].member[0], load);
    }
    #[test]
    fn grid_paste_moves_both_ends_and_rejects_invalid_final_shortening_without_stale_change() {
        use crate::app::node_grid::NodeGridAdapter;
        use crate::grid::GridAdapter;
        let mut app = App::default();
        app.load_model(beam_and_brace_model());
        app.core.model.load_cases[0].member = vec![
            MemberLoad::full_length_uniform(ElemId(0), [0.0, 0.0, -1.0], 6000.0, 10.0),
            MemberLoad::manual(
                ElemId(0),
                [0.0, 0.0, -1.0],
                MemberLoadKind::Distributed {
                    a: 1000.0,
                    b: 3000.0,
                    w1: 10.0,
                    w2: 10.0,
                },
            ),
        ];
        let before = app.core.model.load_cases.clone();
        let mut grid = NodeGridAdapter {
            model: &mut app.core.model,
            undo: &mut app.core.scoped.undo,
            edited: false,
        };
        grid.apply_block(&[(0, 0, "6000".into()), (1, 0, "12000".into())], 0);
        assert!(grid.edited);
        assert_eq!(grid.model.load_cases, before);
        grid.undo.undo(grid.model);
        assert_eq!(grid.model.nodes[0].coord, [0.0, 0.0, 0.0]);
        assert_eq!(grid.model.nodes[1].coord, [6000.0, 0.0, 0.0]);
        grid.undo.redo(grid.model);
        let before = rmp_serde::to_vec_named(grid.model).unwrap();
        grid.edited = false;
        grid.apply_block(&[(0, 0, "6000".into()), (1, 0, "8000".into())], 0);
        assert!(!grid.edited);
        assert_eq!(rmp_serde::to_vec_named(grid.model).unwrap(), before);
        assert!(grid.undo.last_error().unwrap().contains("member[1]"));
    }
    #[test]
    fn rotating_brace_then_reopening_and_committing_preserves_saved_global_direction() {
        let mut app = App::default();
        app.load_model(beam_and_brace_model());
        let editor = LoadEditor::new_member(LoadCaseId(0), Some(ElemId(1)));
        app.commit_load_editor(&editor).unwrap();
        let saved_dir = app.core.model.load_cases[0].member[0].dir;
        assert!(app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetNodeCoord {
                node: NodeId(2),
                coord: [0.0, 6000.0, 0.0]
            })
        ));
        let load = app.core.model.load_cases[0].member[0].clone();
        let mut opened = LoadEditor::edit_member(LoadCaseId(0), 0, &load, &app.core.model);
        app.ui.scoped.load_editor = Some(opened.clone());
        let _ =
            egui::Context::default().run_ui(Default::default(), |ui| app.load_editor_ui(ui.ctx()));
        opened = app.ui.scoped.load_editor.take().unwrap();
        app.commit_load_editor(&opened).unwrap();
        assert_eq!(app.core.model.load_cases[0].member[0].dir, saved_dir);
        if let LoadDraft::Member(draft) = &mut opened.draft {
            draft.dir = DIR_ALONG_AXIS;
        }
        app.commit_load_editor(&opened).unwrap();
        assert_eq!(app.core.model.load_cases[0].member[0].dir, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn existing_arbitrary_and_nearly_axis_directions_survive_edit_and_target_selection() {
        for dir in [[0.2, 0.3, -0.9], [1e-8, 0.0, -1.0]] {
            let mut app = App::default();
            app.load_model(beam_and_brace_model());
            let load = MemberLoad::full_length_uniform(ElemId(0), dir, 6000.0, 10.0);
            app.core.model.load_cases[0].member.push(load.clone());
            let mut editor = LoadEditor::edit_member(LoadCaseId(0), 0, &load, &app.core.model);
            app.commit_load_editor(&editor).unwrap();
            assert_eq!(app.core.model.load_cases[0].member[0].dir, dir);
            editor.set_picked_member(ElemId(1), true);
            app.commit_load_editor(&editor).unwrap();
            assert_eq!(app.core.model.load_cases[0].member[0].dir, dir);
        }
    }
}
