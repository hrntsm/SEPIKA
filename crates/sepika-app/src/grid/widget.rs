//! グリッド操作の egui グルー（描画・入力処理。gui feature 限定）。
//!
//! ロジックは持ち込まず薄く保つ。
//! スプレッドシート様式のグリッドウィジェット。
//! テーブルごとの差は [`GridAdapter`] に隔離し、本モジュールはテーブル固有の知識を持たない。

use super::{checked_tile_block, parse_tsv, plan_paste, rect_to_tsv, validate_paste_rect};
use super::{CellRef, GridAdapter, GridState, PastePlan, SelRect};
use crate::theme;

/// 失敗フラッシュの表示時間（モーションは控えめ・短く）
const FLASH_SECS: f64 = 1.0;

/// イベントログの保持上限（呼び出し元が take_log で回収するまでのバッファ）
const LOG_CAP: usize = 200;

/// グリッド操作ウィジェット。テーブル 1 つにつき 1 インスタンスを保持し、
/// 毎フレーム [`GridWidget::show`] を呼ぶ。テーブル固有の読み書きは
/// [`GridAdapter`] 経由で行う。
///
/// ステータス表示・イベントログの描画はアプリ側の責務とし、本ウィジェットは
/// ログ行（本文, エラーか）を蓄積して [`GridWidget::take_log`] で引き渡す。
pub struct GridWidget {
    pub grid: GridState,
    /// 行末尾に 1 行削除の 🗑 ボタン列を表示するか（既存テーブルの
    /// 削除ボタンを維持するパイロット用。右クリック行削除と等価な操作）
    pub delete_buttons: bool,
    /// 🗑 ボタンで削除要求された行。描画中の行数変化を避けるため
    /// テーブル描画後に処理する
    pending_row_delete: Option<usize>,
    row_selection: Option<(usize, usize)>,
    rows_deleted: bool,
    edit_buf: String,
    edit_needs_focus: bool,
    /// セル起点のドラッグ選択が進行中か（スクロールバー等のドラッグと区別する。）
    drag_selecting: bool,
    /// ドラッグ選択の起点が行ヘッダか（行単位選択モード）
    drag_rows: bool,
    /// ドラッグ選択の起点が列ヘッダか（列単位選択モード）
    drag_cols: bool,
    /// テーブル領域（スクロールバー含む外形矩形）。この外の押下を
    /// 「表外クリック」として選択解除に使う（セル単位ではなく領域で判定し、
    /// スクロールバー操作で選択が消えないようにする）
    table_rect: egui::Rect,
    /// このフレームでコンテキストメニューが開いているか。メニュー操作の
    /// クリックが「表外クリック＝選択解除」に誤判定されるのを防ぐ
    menu_open: bool,
    /// フィードバックのフラッシュ対象範囲・終了時刻・色（egui の time 基準。）。
    /// 赤 = 全体拒否・編集失敗（何も適用されていない）、
    /// 黄 = サイズ不一致の貼り付け（適用されたが確認を促す）
    flash_rect: Option<SelRect>,
    flash_until: f64,
    flash_color: egui::Color32,
    /// イベントログ（本文, エラーか）。エラー行は赤字で描画すること
    log: Vec<(String, bool)>,
}

impl Default for GridWidget {
    fn default() -> Self {
        Self {
            grid: GridState::new(0, 0),
            delete_buttons: false,
            pending_row_delete: None,
            row_selection: None,
            rows_deleted: false,
            edit_buf: String::new(),
            edit_needs_focus: false,
            drag_selecting: false,
            drag_rows: false,
            drag_cols: false,
            table_rect: egui::Rect::NOTHING,
            menu_open: false,
            flash_rect: None,
            flash_until: 0.0,
            flash_color: theme::ERROR_RED,
            log: Vec::new(),
        }
    }
}

impl GridWidget {
    pub fn new() -> Self {
        Self::default()
    }

    /// 溜まったイベントログ（本文, エラーか）を取り出す。
    /// 呼び出し元がアプリのイベントログへ転記する（エラー行は赤字で。）
    pub fn take_log(&mut self) -> Vec<(String, bool)> {
        std::mem::take(&mut self.log)
    }

    pub fn take_row_selection(&mut self) -> Option<(usize, usize)> {
        self.row_selection.take()
    }

    pub fn take_rows_deleted(&mut self) -> bool {
        std::mem::take(&mut self.rows_deleted)
    }

    fn push_log(&mut self, s: impl Into<String>) {
        self.log.push((s.into(), false));
        if self.log.len() > LOG_CAP {
            self.log.remove(0);
        }
    }

    fn push_err(&mut self, s: impl Into<String>) {
        self.log.push((s.into(), true));
        if self.log.len() > LOG_CAP {
            self.log.remove(0);
        }
    }

    /// フィードバック: 対象範囲を指定色でフラッシュする（赤=拒否、黄=注意。）
    fn start_flash(&mut self, rect: SelRect, now: f64, color: egui::Color32) {
        self.flash_rect = Some(rect);
        self.flash_until = now + FLASH_SECS;
        self.flash_color = color;
    }

    /// セルの表示文字列。新規行プレースホルダは空
    fn cell_display(&self, adapter: &dyn GridAdapter, row: usize, col: usize) -> String {
        if row >= adapter.rows() {
            String::new()
        } else {
            adapter.cell_text(row, col)
        }
    }

    /// ログ用のセル名（「3行目のX」「新規行のY」）
    fn cell_name(&self, adapter: &dyn GridAdapter, headers: &[&str], cell: CellRef) -> String {
        let col = headers.get(cell.col).copied().unwrap_or("?");
        if cell.row >= adapter.rows() {
            format!("新規行の{col}")
        } else {
            format!("{}行目の{col}", cell.row + 1)
        }
    }

    fn begin_edit_with(&mut self, buf: String) {
        self.grid.begin_edit();
        self.edit_buf = buf;
        self.edit_needs_focus = true;
    }

    /// プレースホルダを含む表示行数で GridState を同期する
    fn sync_rows(&mut self, adapter: &dyn GridAdapter) {
        self.grid.rows = adapter.rows() + adapter.can_append_rows() as usize;
        self.grid.cols = adapter.cols();
        self.grid.clamp_selection();
    }

    /// 編集確定。空のまま確定は「変更なし」。
    /// 不正値は変更せず理由をログ＋当該セルを赤フラッシュ。
    /// プレースホルダへの確定は「行追加＋値設定」としてアダプタに渡す
    fn commit_edit(
        &mut self,
        adapter: &mut dyn GridAdapter,
        headers: &[&str],
        cell: CellRef,
        now: f64,
    ) {
        let t = self.edit_buf.trim().to_string();
        let outcome = super::commit_cell_text(adapter, cell, &t);
        let name = self.cell_name(adapter, headers, cell);
        match outcome {
            super::CommitOutcome::NoChange => {}
            super::CommitOutcome::Rejected(reason) => {
                self.push_err(format!("「{t}」: {reason}。{name} は変更しませんでした"));
                self.start_flash(
                    SelRect {
                        r0: cell.row,
                        r1: cell.row,
                        c0: cell.col,
                        c1: cell.col,
                    },
                    now,
                    theme::ERROR_RED,
                );
            }
            super::CommitOutcome::Applied { appended } => {
                if appended {
                    self.push_log(format!("新規行を追加し {name} ← {t}"));
                } else {
                    self.push_log(format!("{name} ← {t}"));
                }
                self.sync_rows(adapter);
            }
        }
    }

    /// 選択範囲を TSV コピー。選択なしはログに理由を出す
    fn do_copy(&mut self, ctx: &egui::Context, adapter: &dyn GridAdapter) {
        if !self.grid.active {
            self.push_log("選択がないためコピーをスキップ（セルをクリックして選択してください）");
            return;
        }
        let rect = self.grid.rect();
        let tsv = rect_to_tsv(rect, |r, c| self.cell_display(adapter, r, c));
        ctx.copy_text(tsv);
        self.push_log(format!(
            "コピー: {}行×{}列を TSV でクリップボードへ",
            rect.r1 - rect.r0 + 1,
            rect.c1 - rect.c0 + 1
        ));
    }

    /// TSV ペースト。
    /// 選択なしは無言で無視する（貼り付け先が定まっていない操作に
    /// フィードバックは出さない）
    fn do_paste(&mut self, adapter: &mut dyn GridAdapter, text: &str, now: f64) {
        if !self.grid.active {
            return;
        }
        let block = parse_tsv(text);
        if block.is_empty() {
            self.push_log("クリップボードが空のため貼り付けをスキップ");
            return;
        }
        let rect = self.grid.rect();
        let anchor = CellRef {
            row: rect.r0,
            col: rect.c0,
        };
        let orig_rows = block.len();
        let orig_cols = block.iter().map(Vec::len).max().unwrap_or(0);
        let (sel_rows, sel_cols) = (rect.r1 - rect.r0 + 1, rect.c1 - rect.c0 + 1);
        let block = match checked_tile_block(&block, sel_rows, sel_cols) {
            Ok(block) => block,
            Err(reason) => {
                self.push_err(format!("貼り付けを全体拒否: {reason}"));
                self.start_flash(rect, now, theme::ERROR_RED);
                return;
            }
        };
        let tiled_cols = block.iter().map(Vec::len).max().unwrap_or(0);
        if let Err(reason) = validate_paste_rect(adapter, anchor, block.len(), tiled_cols) {
            self.push_err(format!("貼り付けを全体拒否: {reason}"));
            self.start_flash(rect, now, theme::ERROR_RED);
            return;
        }
        let tiled = block.len() != orig_rows || tiled_cols != orig_cols;
        let mismatch = !tiled
            && (sel_rows, sel_cols) != (orig_rows, orig_cols)
            && (sel_rows > 1 || sel_cols > 1);
        if tiled {
            self.push_log(format!(
                "コピー元 {orig_rows}×{orig_cols} を選択範囲 {sel_rows}×{sel_cols} にタイル展開"
            ));
        } else if mismatch {
            self.push_log(format!(
                "注意: 選択範囲 {sel_rows}×{sel_cols} とコピー元 {orig_rows}×{orig_cols} のサイズが一致しません。起点から {orig_rows}×{orig_cols} で貼り付けます"
            ));
        }
        let plan = plan_paste(&block, anchor, adapter.rows(), adapter.cols(), |r, c, t| {
            adapter.validate_cell(r, c, t)
        });
        match plan {
            Err(errors) => {
                self.push_err(format!(
                    "貼り付けを全体拒否（all-or-nothing）: 不正 {} 件",
                    errors.len()
                ));
                for e in errors.iter().take(10) {
                    self.push_err(format!("  {e}"));
                }
                if errors.len() > 10 {
                    self.push_err(format!("  …ほか {} 件", errors.len() - 10));
                }
                let fr = SelRect {
                    r0: anchor.row,
                    r1: (anchor.row + block.len().saturating_sub(1))
                        .min(self.grid.rows.saturating_sub(1)),
                    c0: anchor.col,
                    c1: (anchor.col + tiled_cols.max(1) - 1).min(self.grid.cols.saturating_sub(1)),
                };
                self.start_flash(fr, now, theme::ERROR_RED);
            }
            Ok(plan) => {
                self.apply_plan(adapter, plan);
                if mismatch {
                    self.start_flash(self.grid.rect(), now, theme::BEST_YELLOW);
                }
            }
        }
    }

    /// 矩形を再検証し、全セルを1つの編集として適用する。
    fn apply_plan(&mut self, adapter: &mut dyn GridAdapter, plan: PastePlan) {
        if let Err(reason) =
            validate_paste_rect(adapter, plan.anchor, plan.block_rows, plan.block_cols)
        {
            self.push_err(format!("貼り付けを全体拒否: {reason}"));
            return;
        }
        let extra_rows = plan.extra_rows;
        let applied = match adapter.apply_block(&plan.set, extra_rows) {
            Ok(true) => plan.set.len(),
            Ok(false) => 0,
            Err(reason) => {
                self.push_err(format!("貼り付けを全体拒否: {reason}"));
                return;
            }
        };
        let mut msg = format!("貼り付け: {applied} セル適用");
        if plan.skipped_empty > 0 {
            msg += &format!("、空セル {} 個は既存値維持", plan.skipped_empty);
        }
        if extra_rows > 0 {
            msg += &format!("、{extra_rows} 行を追加");
        }
        self.push_log(msg);
        self.sync_rows(adapter);
        if applied > 0 && plan.block_rows > 0 && plan.block_cols > 0 {
            self.grid.anchor = plan.anchor;
            self.grid.cursor = CellRef {
                row: plan.anchor.row + plan.block_rows - 1,
                col: plan.anchor.col + plan.block_cols - 1,
            };
            self.grid.clamp_selection();
        }
    }

    /// 選択範囲のクリア（Delete）。クリアの意味はアダプタが決める。
    /// プレースホルダ行は対象にならない
    fn clear_selection(&mut self, adapter: &mut dyn GridAdapter) {
        if !self.grid.active {
            return;
        }
        let rect = self.grid.rect();
        let rows = adapter.rows();
        let mut cells = Vec::new();
        for r in rect.r0..=rect.r1.min(rows.saturating_sub(1)) {
            if r >= rows {
                continue;
            }
            for c in rect.c0..=rect.c1 {
                cells.push((r, c));
            }
        }
        if cells.is_empty() {
            return;
        }
        let n = adapter.clear_cells(&cells);
        if n > 0 {
            self.push_log(format!("選択範囲 {n} セルをクリア"));
        } else {
            self.push_log("このテーブルは選択範囲のクリアに対応していません");
        }
        self.sync_rows(adapter);
    }

    /// 行削除メニューのラベル。対象行数を明示し、
    /// 「複数行選択したのに 1 行しか消えない／逆」の驚きを防ぐ
    fn delete_menu_label(&self, adapter: &dyn GridAdapter) -> String {
        let rect = self.grid.rect();
        let rows = adapter.rows();
        let n = if rows == 0 || rect.r0 >= rows {
            0
        } else {
            rect.r1.min(rows - 1) - rect.r0 + 1
        };
        if n <= 1 {
            "この行を削除".to_string()
        } else {
            format!("選択した {n} 行を削除")
        }
    }

    /// 行削除（右クリックメニュー／Ctrl+Delete。）。対象は選択が跨ぐ実データ行
    /// （プレースホルダは対象外）。参照中の行が 1 つでもあれば全体拒否
    /// （ペーストと同じ all-or-nothing）
    fn delete_selected_rows(&mut self, adapter: &mut dyn GridAdapter, now: f64) {
        if !self.grid.active || !adapter.can_delete_rows() {
            return;
        }
        let rect = self.grid.rect();
        let rows = adapter.rows();
        if rows == 0 || rect.r0 >= rows {
            self.push_log("削除対象の行がありません（新規行は削除できません）");
            return;
        }
        let r0 = rect.r0;
        let r1 = rect.r1.min(rows - 1);
        let blocked: Vec<(usize, String)> = (r0..=r1)
            .filter_map(|r| adapter.validate_row_deletion(r).err().map(|e| (r, e)))
            .collect();
        if !blocked.is_empty() {
            self.push_err(format!(
                "行削除を全体拒否: 削除できない行が {} 行含まれています",
                blocked.len()
            ));
            for (r, reason) in blocked.iter().take(10) {
                self.push_err(format!("  {}行目: {}", r + 1, reason));
            }
            if blocked.len() > 10 {
                self.push_err(format!("  …ほか {} 件", blocked.len() - 10));
            }
            self.start_flash(
                SelRect {
                    r0,
                    r1,
                    c0: 0,
                    c1: self.grid.cols.saturating_sub(1),
                },
                now,
                theme::ERROR_RED,
            );
            return;
        }
        let targets: Vec<usize> = (r0..=r1).collect();
        adapter.delete_rows(&targets);
        self.rows_deleted = true;
        self.push_log(format!(
            "{} 行を削除（{}〜{} 行目）",
            r1 - r0 + 1,
            r0 + 1,
            r1 + 1
        ));
        self.sync_rows(adapter);
        self.grid.select_row(r0, false);
        self.grid.clamp_selection();
    }

    /// 全選択（実データ行のみ。新規行プレースホルダは含めない。）
    fn select_all_real(&mut self, adapter: &dyn GridAdapter) {
        if adapter.rows() == 0 {
            return;
        }
        self.grid.select_all();
        if self.grid.active {
            self.grid.cursor.row = adapter.rows() - 1;
        }
    }

    /// 選択モード時のグローバル入力（編集モード中は一切処理しない。
    /// TextEdit が Ctrl+C/V・矢印・文字入力を消費する）
    fn handle_events(&mut self, ctx: &egui::Context, adapter: &mut dyn GridAdapter) {
        if self.grid.editing.is_some() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::Copy | egui::Event::Cut => self.do_copy(ctx, adapter),
                egui::Event::Paste(t) => {
                    let now = ctx.input(|i| i.time);
                    self.do_paste(adapter, &t, now);
                }
                egui::Event::Text(t) => {
                    let clean: String = t.chars().filter(|ch| !ch.is_control()).collect();
                    if !clean.is_empty() && self.grid.active {
                        self.begin_edit_with(clean);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key {
                    egui::Key::A if modifiers.command => {
                        self.select_all_real(adapter);
                    }
                    egui::Key::Delete if modifiers.command => {
                        let now = ctx.input(|i| i.time);
                        self.delete_selected_rows(adapter, now);
                    }
                    egui::Key::Delete => self.clear_selection(adapter),
                    egui::Key::ArrowUp => self.grid.move_cursor(-1, 0, modifiers.shift),
                    egui::Key::ArrowDown => self.grid.move_cursor(1, 0, modifiers.shift),
                    egui::Key::ArrowLeft => self.grid.move_cursor(0, -1, modifiers.shift),
                    egui::Key::ArrowRight => self.grid.move_cursor(0, 1, modifiers.shift),
                    egui::Key::Tab => {
                        self.grid
                            .move_cursor(0, if modifiers.shift { -1 } else { 1 }, false)
                    }
                    egui::Key::Enter => {
                        self.grid
                            .move_cursor(if modifiers.shift { -1 } else { 1 }, 0, false)
                    }
                    egui::Key::F2 => {
                        if self.grid.active {
                            self.begin_edit_with(self.cell_display(
                                adapter,
                                self.grid.anchor.row,
                                self.grid.anchor.col,
                            ));
                        }
                    }
                    egui::Key::Backspace => {
                        if self.grid.active {
                            self.begin_edit_with(String::new());
                        }
                    }
                    egui::Key::Escape if self.grid.active => {
                        self.grid.deactivate();
                        self.push_log("選択を解除（Esc）");
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// 行ヘッダ（ID 列）の描画。データセルではないためフラットなラベルで描き、
    /// クリック＝行全体選択、ドラッグ/Shift＝行範囲拡張（Excel の行番号ヘッダ）。
    /// プレースホルダ行は「＋」
    fn row_header_cell(&mut self, ui: &mut egui::Ui, adapter: &mut dyn GridAdapter, row: usize) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.interact(
            rect,
            ui.id().with(("grid_row_header", row)),
            egui::Sense::click_and_drag(),
        );
        let is_real = row < adapter.rows();
        if is_real && adapter.can_delete_rows() {
            if resp.secondary_clicked() {
                let sel = self.grid.rect();
                if !self.grid.active || !(sel.r0..=sel.r1).contains(&row) {
                    self.grid.select_row(row, false);
                    self.row_selection = Some((row, row));
                }
            }
            let label = self.delete_menu_label(adapter);
            resp.context_menu(|ui| {
                self.menu_open = true;
                if ui.button(label.clone()).clicked() {
                    let now = ui.input(|i| i.time);
                    self.delete_selected_rows(adapter, now);
                    ui.close();
                }
            });
        }
        let sp = ui.spacing().item_spacing;
        let g = rect.expand2(egui::vec2(sp.x * 0.5, sp.y * 0.5));
        ui.painter()
            .rect_filled(g, egui::CornerRadius::ZERO, theme::GRAY_100);
        let grid_stroke = egui::Stroke::new(1.0_f32, theme::GRAY_200);
        ui.painter()
            .line_segment([g.right_top(), g.right_bottom()], grid_stroke);
        ui.painter()
            .line_segment([g.left_bottom(), g.right_bottom()], grid_stroke);
        ui.painter()
            .line_segment([g.left_top(), g.left_bottom()], grid_stroke);
        let sel = self.grid.rect();
        if self.grid.active && (sel.r0..=sel.r1).contains(&row) {
            ui.painter().rect_filled(
                g,
                egui::CornerRadius::ZERO,
                theme::translucent(theme::BLUE_300, 80),
            );
        }
        let font = egui::TextStyle::Body.resolve(ui.style());
        ui.painter().text(
            rect.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            if is_real {
                adapter.row_label(row)
            } else {
                "＋".to_string()
            },
            font,
            if is_real {
                theme::GRAY_700
            } else {
                theme::GRAY_600
            },
        );
        let (shift, primary_pressed, primary_down, pointer_pos) = ui.input(|i| {
            (
                i.modifiers.shift,
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
            )
        });
        let contains_pointer = pointer_pos.is_some_and(|p| rect.contains(p));
        if primary_pressed && resp.hovered() {
            self.grid.select_row(row, shift);
            self.drag_selecting = true;
            self.drag_rows = true;
            self.drag_cols = false;
            if self.grid.anchor.row.min(self.grid.cursor.row) < adapter.rows() {
                self.row_selection = Some((self.grid.anchor.row, self.grid.cursor.row));
            }
        } else if primary_down && self.drag_selecting && self.drag_rows && contains_pointer {
            self.grid.select_row(row, true);
            if self.grid.anchor.row.min(self.grid.cursor.row) < adapter.rows() {
                self.row_selection = Some((self.grid.anchor.row, self.grid.cursor.row));
            }
        }
    }

    /// 左上コーナー（ID 見出し）の描画。クリック＝全選択（実データ行のみ。）
    fn corner_header_cell(&mut self, ui: &mut egui::Ui, adapter: &dyn GridAdapter) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.interact(
            rect,
            ui.id().with("grid_corner_header"),
            egui::Sense::click(),
        );
        let sp = ui.spacing().item_spacing;
        let g = rect.expand2(egui::vec2(sp.x * 0.5, sp.y * 0.5));
        ui.painter()
            .rect_filled(g, egui::CornerRadius::ZERO, theme::GRAY_100);
        let grid_stroke = egui::Stroke::new(1.0_f32, theme::GRAY_200);
        ui.painter()
            .line_segment([g.right_top(), g.right_bottom()], grid_stroke);
        ui.painter()
            .line_segment([g.left_bottom(), g.right_bottom()], grid_stroke);
        ui.painter()
            .line_segment([g.left_top(), g.right_top()], grid_stroke);
        ui.painter()
            .line_segment([g.left_top(), g.left_bottom()], grid_stroke);
        let font = egui::TextStyle::Body.resolve(ui.style());
        ui.painter().text(
            rect.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "ID",
            font,
            theme::GRAY_900,
        );
        let primary_pressed = ui.input(|i| i.pointer.primary_pressed());
        if primary_pressed && resp.hovered() {
            self.select_all_real(adapter);
        }
    }

    /// 列ヘッダの描画。行ヘッダと対称: 列が選択に含まれる間は薄いハイライト、
    /// クリック＝列全体選択、ドラッグ/Shift＝列範囲拡張
    fn col_header_cell(&mut self, ui: &mut egui::Ui, headers: &[&str], col: usize) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.interact(
            rect,
            ui.id().with(("grid_col_header", col)),
            egui::Sense::click_and_drag(),
        );
        let sp = ui.spacing().item_spacing;
        let g = rect.expand2(egui::vec2(sp.x * 0.5, sp.y * 0.5));
        ui.painter()
            .rect_filled(g, egui::CornerRadius::ZERO, theme::GRAY_100);
        let grid_stroke = egui::Stroke::new(1.0_f32, theme::GRAY_200);
        ui.painter()
            .line_segment([g.right_top(), g.right_bottom()], grid_stroke);
        ui.painter()
            .line_segment([g.left_bottom(), g.right_bottom()], grid_stroke);
        ui.painter()
            .line_segment([g.left_top(), g.right_top()], grid_stroke);
        let sel = self.grid.rect();
        if self.grid.active && (sel.c0..=sel.c1).contains(&col) {
            ui.painter().rect_filled(
                g,
                egui::CornerRadius::ZERO,
                theme::translucent(theme::BLUE_300, 80),
            );
        }
        let font = egui::TextStyle::Body.resolve(ui.style());
        ui.painter().text(
            rect.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            headers.get(col).copied().unwrap_or(""),
            font,
            theme::GRAY_900,
        );
        let (shift, primary_pressed, primary_down, pointer_pos) = ui.input(|i| {
            (
                i.modifiers.shift,
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
            )
        });
        let contains_pointer = pointer_pos.is_some_and(|p| rect.contains(p));
        if primary_pressed && resp.hovered() {
            self.grid.select_col(col, shift);
            self.drag_selecting = true;
            self.drag_cols = true;
            self.drag_rows = false;
        } else if primary_down && self.drag_selecting && self.drag_cols && contains_pointer {
            self.grid.select_col(col, true);
        }
    }

    /// 選択モードのデータセル描画（スプレッドシート風＋クリック/ドラッグ判定）。
    /// 罫線は各セルが右辺・下辺のみを描き、隣接セルと 1 本の線を共有する。
    /// 選択は Excel 式: 範囲の外周を青枠で囲い、中を薄く塗り、
    /// アクティブセルだけ白抜き
    fn select_mode_cell(
        &mut self,
        ui: &mut egui::Ui,
        adapter: &mut dyn GridAdapter,
        cell: CellRef,
    ) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.interact(
            rect,
            ui.id().with(("grid_cell", cell.row, cell.col)),
            egui::Sense::click_and_drag(),
        );
        let sp = ui.spacing().item_spacing;
        let g = rect.expand2(egui::vec2(sp.x * 0.5, sp.y * 0.5));
        let grid_stroke = egui::Stroke::new(1.0_f32, theme::GRAY_200);
        ui.painter()
            .line_segment([g.right_top(), g.right_bottom()], grid_stroke);
        ui.painter()
            .line_segment([g.left_bottom(), g.right_bottom()], grid_stroke);
        let is_real = cell.row < adapter.rows();
        if !is_real {
            ui.painter()
                .rect_filled(g, egui::CornerRadius::ZERO, theme::GRAY_100);
        }
        let sel = self.grid.rect();
        let selected = self.grid.active && sel.contains(cell.row, cell.col);
        let is_anchor = self.grid.active && self.grid.anchor == cell;
        if selected && !is_anchor {
            ui.painter()
                .rect_filled(g, egui::CornerRadius::ZERO, theme::SELECTION_BG);
        }
        if selected {
            let b = theme::FOCUS_STROKE;
            if cell.row == sel.r0 {
                ui.painter().line_segment([g.left_top(), g.right_top()], b);
            }
            if cell.row == sel.r1 {
                ui.painter()
                    .line_segment([g.left_bottom(), g.right_bottom()], b);
            }
            if cell.col == sel.c0 {
                ui.painter()
                    .line_segment([g.left_top(), g.left_bottom()], b);
            }
            if cell.col == sel.c1 {
                ui.painter()
                    .line_segment([g.right_top(), g.right_bottom()], b);
            }
        }
        if let Some(fr) = self.flash_rect {
            let now = ui.input(|i| i.time);
            if now < self.flash_until && fr.contains(cell.row, cell.col) {
                let t = ((self.flash_until - now) / FLASH_SECS).clamp(0.0, 1.0) as f32;
                let alpha = (110.0 * t) as u8;
                ui.painter().rect_filled(
                    g,
                    egui::CornerRadius::ZERO,
                    theme::translucent(self.flash_color, alpha),
                );
            }
        }
        let text = self.cell_display(adapter, cell.row, cell.col);
        let numeric = text.parse::<f64>().is_ok();
        let style = if numeric {
            egui::TextStyle::Monospace
        } else {
            egui::TextStyle::Body
        };
        let font = style.resolve(ui.style());
        let (position, alignment) = if numeric {
            (
                rect.right_center() - egui::vec2(4.0, 0.0),
                egui::Align2::RIGHT_CENTER,
            )
        } else {
            (
                rect.left_center() + egui::vec2(4.0, 0.0),
                egui::Align2::LEFT_CENTER,
            )
        };
        ui.painter()
            .text(position, alignment, &text, font, theme::GRAY_700);
        let resp = resp.on_hover_text(&text);
        let (shift, primary_pressed, primary_down, pointer_pos) = ui.input(|i| {
            (
                i.modifiers.shift,
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
            )
        });
        let contains_pointer = pointer_pos.is_some_and(|p| rect.contains(p));
        if resp.double_clicked() {
            self.grid.click(cell, false);
            self.begin_edit_with(self.cell_display(adapter, cell.row, cell.col));
        } else if primary_pressed && resp.hovered() {
            self.grid.click(cell, shift);
            self.drag_selecting = true;
            self.drag_rows = false;
            self.drag_cols = false;
        } else if primary_down
            && self.drag_selecting
            && !self.drag_rows
            && !self.drag_cols
            && contains_pointer
        {
            self.grid.drag_to(cell);
        }
        if is_real && adapter.can_delete_rows() {
            if resp.secondary_clicked() {
                let sel = self.grid.rect();
                if !self.grid.active || !sel.contains(cell.row, cell.col) {
                    self.grid.click(cell, false);
                }
            }
            let label = self.delete_menu_label(adapter);
            resp.context_menu(|ui| {
                self.menu_open = true;
                if ui.button(label.clone()).clicked() {
                    let now = ui.input(|i| i.time);
                    self.delete_selected_rows(adapter, now);
                    ui.close();
                }
            });
        }
    }

    /// 1 行削除の 🗑 ボタンセル（`delete_buttons` 有効時のみ。 の
    /// メニュー削除と同じ検証を通す）。削除は行数変化を避けるため
    /// テーブル描画後に処理する
    fn delete_button_cell(&mut self, ui: &mut egui::Ui, adapter: &dyn GridAdapter, row: usize) {
        if row >= adapter.rows() {
            return;
        }
        let deletable = adapter.validate_row_deletion(row);
        let resp = ui.add_enabled(deletable.is_ok(), egui::Button::new("🗑").small());
        match deletable {
            Err(reason) => {
                resp.on_hover_text(reason);
            }
            Ok(()) => {
                if resp.on_hover_text("この行を削除").clicked() {
                    self.pending_row_delete = Some(row);
                }
            }
        }
    }

    /// 編集モードのセル描画（TextEdit＋確定/キャンセル判定）。
    /// 数値は等幅・右寄せ、その他は本文・左寄せで表示する。
    fn edit_mode_cell(
        &mut self,
        ui: &mut egui::Ui,
        adapter: &mut dyn GridAdapter,
        headers: &[&str],
        cell: CellRef,
    ) {
        let rect = ui.available_rect_before_wrap();
        let numeric = self
            .cell_display(adapter, cell.row, cell.col)
            .parse::<f64>()
            .is_ok();
        let resp = ui.put(
            rect,
            egui::TextEdit::singleline(&mut self.edit_buf)
                .font(if numeric {
                    egui::TextStyle::Monospace
                } else {
                    egui::TextStyle::Body
                })
                .horizontal_align(if numeric {
                    egui::Align::Max
                } else {
                    egui::Align::Min
                })
                .vertical_align(egui::Align::Center)
                .margin(egui::Margin::symmetric(4, 0))
                .clip_text(false),
        );
        theme::focus_outline(ui, &resp);
        if self.edit_needs_focus {
            resp.request_focus();
            self.edit_needs_focus = false;
        }
        let (enter, tab, esc) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Tab),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if resp.lost_focus() {
            self.grid.end_edit();
            if esc {
                self.push_log("編集をキャンセル（Esc）");
            } else {
                let now = ui.input(|i| i.time);
                self.commit_edit(adapter, headers, cell, now);
                if enter {
                    self.grid.move_cursor(1, 0, false);
                } else if tab {
                    self.grid.move_cursor(0, 1, false);
                }
            }
            ui.ctx().memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
        }
    }

    /// グリッドを描画し、入力を処理する。毎フレーム呼ぶこと。
    /// `headers` はデータ列の見出し（`adapter.cols()` と同数）。
    pub fn show(&mut self, ui: &mut egui::Ui, adapter: &mut dyn GridAdapter, headers: &[&str]) {
        let ctx = ui.ctx().clone();
        self.sync_rows(adapter);
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.drag_selecting = false;
        }
        self.menu_open = false;
        if self.flash_rect.is_some() {
            if ctx.input(|i| i.time) < self.flash_until {
                ctx.request_repaint();
            } else {
                self.flash_rect = None;
            }
        }
        self.handle_events(&ctx, adapter);

        use egui_extras::{Column, TableBuilder};
        let n = self.grid.rows;
        let row_h = theme::table_row_height(ui);
        let delete_buttons = self.delete_buttons && adapter.can_delete_rows();
        let table_scope = ui.scope(|ui| {
            let mut builder = TableBuilder::new(ui)
                .striped(false)
                .column(Column::auto().at_least(40.0))
                .columns(Column::initial(90.0), adapter.cols());
            if delete_buttons {
                builder = builder.column(Column::auto());
            }
            builder
                .header(row_h, |mut h| {
                    h.col(|ui| {
                        self.corner_header_cell(ui, adapter);
                    });
                    for c in 0..adapter.cols() {
                        h.col(|ui| {
                            self.col_header_cell(ui, headers, c);
                        });
                    }
                    if delete_buttons {
                        h.col(|_ui| {});
                    }
                })
                .body(|body| {
                    body.rows(row_h, n, |mut row| {
                        let r = row.index();
                        row.col(|ui| {
                            self.row_header_cell(ui, adapter, r);
                        });
                        for c in 0..adapter.cols() {
                            row.col(|ui| {
                                let cell = CellRef { row: r, col: c };
                                if self.grid.editing == Some(cell) {
                                    self.edit_mode_cell(ui, adapter, headers, cell);
                                } else {
                                    self.select_mode_cell(ui, adapter, cell);
                                }
                            });
                        }
                        if delete_buttons {
                            row.col(|ui| {
                                self.delete_button_cell(ui, adapter, r);
                            });
                        }
                    });
                });
        });
        self.table_rect = table_scope.response.rect;

        if let Some(r) = self.pending_row_delete.take() {
            if adapter.validate_row_deletion(r).is_ok() {
                adapter.delete_rows(&[r]);
                self.rows_deleted = true;
                self.push_log(format!("{} 行目を削除", r + 1));
                self.sync_rows(adapter);
                self.grid.select_row(r, false);
                self.grid.clamp_selection();
            }
        }

        let (pressed, pos) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.interact_pos()));
        if pressed
            && pos.is_some_and(|p| !self.table_rect.contains(p))
            && self.grid.editing.is_none()
            && !self.menu_open
            && self.grid.active
        {
            self.grid.deactivate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::node_grid::NodeGridAdapter;
    use crate::grid::MAX_PASTE_CELLS;
    use sepika_core::model::Model;
    use sepika_edit::UndoStack;

    struct Restricted<'a> {
        node: NodeGridAdapter<'a>,
        append: bool,
        readonly: Option<CellRef>,
    }

    impl GridAdapter for Restricted<'_> {
        fn rows(&self) -> usize {
            self.node.rows()
        }
        fn cols(&self) -> usize {
            self.node.cols()
        }
        fn cell_text(&self, r: usize, c: usize) -> String {
            self.node.cell_text(r, c)
        }
        fn validate_target(&self, r: usize, c: usize) -> Result<(), String> {
            if self.readonly == Some(CellRef { row: r, col: c }) {
                return Err("読取専用".into());
            }
            self.node.validate_target(r, c)
        }
        fn validate_cell(&self, r: usize, c: usize, t: &str) -> Result<(), String> {
            self.node.validate_cell(r, c, t)
        }
        fn apply_block(
            &mut self,
            cells: &[(usize, usize, String)],
            append: usize,
        ) -> Result<bool, String> {
            self.node.apply_block(cells, append)
        }
        fn clear_cells(&mut self, cells: &[(usize, usize)]) -> usize {
            self.node.clear_cells(cells)
        }
        fn can_append_rows(&self) -> bool {
            self.append
        }
        fn can_delete_rows(&self) -> bool {
            false
        }
        fn validate_row_deletion(&self, _r: usize) -> Result<(), String> {
            Err("削除不可".into())
        }
        fn delete_rows(&mut self, _rows: &[usize]) {
            panic!("削除不可")
        }
    }

    #[test]
    fn rejected_paste_rectangles_and_stale_plans_do_not_edit_or_touch_history() {
        let mut model = Model::default();
        let mut undo = UndoStack::new();
        let mut node = NodeGridAdapter {
            model: &mut model,
            undo: &mut undo,
            edited: false,
        };
        node.apply_block(&[(0, 0, "1".into())], 1).unwrap();
        node.apply_block(&[(1, 0, "2".into())], 1).unwrap();
        node.undo.undo(node.model);
        node.edited = false;
        let original = rmp_serde::to_vec_named(node.model).unwrap();
        let revision = node.undo.revision();
        let mut adapter = Restricted {
            node,
            append: false,
            readonly: None,
        };
        let mut widget = GridWidget::new();
        widget.grid = GridState::new(2, 3);
        widget.grid.click(CellRef { row: 0, col: 0 }, false);
        for text in ["4\n ", "4\n\t", "4\t\t\t", "4\tNaN", "4\n \n5"] {
            widget.do_paste(&mut adapter, text, 0.0);
            assert_eq!(
                rmp_serde::to_vec_named(adapter.node.model).unwrap(),
                original
            );
            assert_eq!(adapter.node.undo.revision(), revision);
            assert!(!adapter.node.edited);
        }
        adapter.append = true;
        adapter.readonly = Some(CellRef { row: 0, col: 1 });
        widget.do_paste(&mut adapter, "4\t ", 0.0);
        let plan = plan_paste(
            &parse_tsv("4\t "),
            CellRef { row: 0, col: 0 },
            1,
            3,
            |_, _, _| Ok(()),
        )
        .unwrap();
        widget.apply_plan(&mut adapter, plan);
        adapter.readonly = None;
        let plan = plan_paste(
            &parse_tsv("4\tNaN"),
            CellRef { row: 0, col: 0 },
            1,
            3,
            |_, _, _| Ok(()),
        )
        .unwrap();
        widget.apply_plan(&mut adapter, plan);
        let plan = plan_paste(
            &parse_tsv("4\n "),
            CellRef { row: 0, col: 0 },
            1,
            3,
            |_, _, _| Ok(()),
        )
        .unwrap();
        adapter.append = false;
        widget.apply_plan(&mut adapter, plan);
        widget.grid = GridState::new(MAX_PASTE_CELLS + 1, 3);
        widget.grid.click(CellRef { row: 0, col: 0 }, false);
        widget.grid.click(
            CellRef {
                row: MAX_PASTE_CELLS,
                col: 0,
            },
            true,
        );
        widget.do_paste(&mut adapter, "4", 0.0);
        assert_eq!(
            rmp_serde::to_vec_named(adapter.node.model).unwrap(),
            original
        );
        assert_eq!(adapter.node.undo.revision(), revision);
        assert!(!adapter.node.edited);
        assert!(adapter.node.undo.can_undo());
        assert!(adapter.node.undo.can_redo());
        assert!(widget.take_log().iter().filter(|(_, err)| *err).count() >= 10);
    }

    fn header_pos(row: usize) -> egui::Pos2 {
        egui::pos2(40.0, 40.0 + row as f32 * 30.0)
    }

    fn header_frame(
        ctx: &egui::Context,
        widget: &mut GridWidget,
        model: &mut Model,
        events: Vec<egui::Event>,
        shift: bool,
    ) -> Option<(usize, usize)> {
        let mut undo = UndoStack::new();
        let mut adapter = NodeGridAdapter {
            model,
            undo: &mut undo,
            edited: false,
        };
        let _ = ctx.run_ui(
            egui::RawInput {
                events,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
                ..Default::default()
            },
            |ui| {
                widget.sync_rows(&adapter);
                if !ui.input(|i| i.pointer.primary_down()) {
                    widget.drag_selecting = false;
                }
                for row in 0..widget.grid.rows {
                    let rect =
                        egui::Rect::from_center_size(header_pos(row), egui::vec2(40.0, 24.0));
                    let mut child =
                        ui.new_child(egui::UiBuilder::new().id_salt(row).max_rect(rect));
                    widget.row_header_cell(&mut child, &mut adapter, row);
                }
            },
        );
        widget.take_row_selection()
    }

    fn button(row: usize, pressed: bool, shift: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: header_pos(row),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
        }
    }

    #[test]
    fn row_header_shift_click_to_placeholder_notifies_range() {
        let ctx = egui::Context::default();
        let mut widget = GridWidget::new();
        let mut model = crate::sample::portal_frame();
        let end = model.nodes.len();
        header_frame(&ctx, &mut widget, &mut model, vec![], false);
        assert_eq!(
            header_frame(
                &ctx,
                &mut widget,
                &mut model,
                vec![button(0, true, false)],
                false
            ),
            Some((0, 0))
        );
        header_frame(
            &ctx,
            &mut widget,
            &mut model,
            vec![button(0, false, false)],
            false,
        );
        header_frame(
            &ctx,
            &mut widget,
            &mut model,
            vec![egui::Event::PointerMoved(header_pos(end))],
            true,
        );
        assert_eq!(
            header_frame(
                &ctx,
                &mut widget,
                &mut model,
                vec![button(end, true, true)],
                true
            ),
            Some((0, end))
        );
        assert_eq!((widget.grid.rect().r0, widget.grid.rect().r1), (0, end));
    }

    #[test]
    fn row_header_drag_to_placeholder_notifies_range() {
        let ctx = egui::Context::default();
        let mut widget = GridWidget::new();
        let mut model = crate::sample::portal_frame();
        let end = model.nodes.len();
        header_frame(&ctx, &mut widget, &mut model, vec![], false);
        assert_eq!(
            header_frame(
                &ctx,
                &mut widget,
                &mut model,
                vec![button(0, true, false)],
                false
            ),
            Some((0, 0))
        );
        assert_eq!(
            header_frame(
                &ctx,
                &mut widget,
                &mut model,
                vec![egui::Event::PointerMoved(header_pos(end))],
                false
            ),
            Some((0, end))
        );
        assert_eq!((widget.grid.rect().r0, widget.grid.rect().r1), (0, end));
    }

    #[test]
    fn row_header_placeholder_alone_is_independent_but_can_anchor_real_range() {
        for empty in [false, true] {
            let ctx = egui::Context::default();
            let mut widget = GridWidget::new();
            let mut model = if empty {
                Model::default()
            } else {
                crate::sample::portal_frame()
            };
            let end = model.nodes.len();
            header_frame(&ctx, &mut widget, &mut model, vec![], false);
            assert_eq!(
                header_frame(
                    &ctx,
                    &mut widget,
                    &mut model,
                    vec![button(end, true, false)],
                    false
                ),
                None
            );
            assert_eq!(widget.grid.anchor.row, end);
            header_frame(
                &ctx,
                &mut widget,
                &mut model,
                vec![button(end, false, false)],
                false,
            );
            if !empty {
                header_frame(
                    &ctx,
                    &mut widget,
                    &mut model,
                    vec![egui::Event::PointerMoved(header_pos(0))],
                    true,
                );
                assert_eq!(
                    header_frame(
                        &ctx,
                        &mut widget,
                        &mut model,
                        vec![button(0, true, true)],
                        true
                    ),
                    Some((end, 0))
                );
            }
        }
    }
}
