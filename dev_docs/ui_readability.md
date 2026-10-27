# GUIの文字と状態表示

既存ライトテーマの共通表示規約。画面配置・計算値・編集確定契約は対象としない。

| 対象 | 規約 | 実装 |
| --- | --- | --- |
| 文字 | Body/Button 14pt、Small 12pt、数値Monospace 14pt。製品倍率を掛ける前の論理pt | `theme::apply_theme` |
| 表 | 行高は実際のBody行高+6pt。数値列と単位見出しを維持し、節点入力表の数値は等幅・右寄せ | `theme::table_row_height`、`table_util`、`grid/widget.rs` |
| 主・補助領域 | 主領域は白、補助パネルは淡色。見出しと余白で区別し、ナビゲータ・属性を追加枠で囲まない | `central_panel_frame`、`content_panel_frame`、`panel_heading` |
| 名称 | 一行の省略表示と全文ホバー。属性の名称はIDと分ける。単位・検定状態を名称に連結して省略しない | `name_label`、`selectable_name`、`table_util::text_cell` |
| 選択 | 淡青の不透明背景と選択印「▸」。グリッドは選択範囲の外周線とアクティブセルで識別する | `selectable_name`、`SELECTION_BG`、`grid/widget.rs` |
| フォーカス | 標準のeguiフォーカス処理を維持。共通選択・表入力の操作欄へ2ptの輪郭を重ねる | `focus_outline`、`FOCUS_STROKE` |
| 状態文字 | 注意・エラー・良好の文字色は淡色地上で読める濃色。検定表・内訳・属性の検定比は「良好/注意/超過」を併記 | `WARN_TEXT`、`ERROR_TEXT`、`GOOD_TEXT`、`ratio_status` |
| 編集・参照 | 入力は白地の標準操作欄、属性値は参照表示とし、断面複製・割当は編集操作と明記。結果は参照専用の見出し。対象外の無効数値欄には編集不可の理由をホバー表示 | `apply_theme`、`cell_drag_value`、属性・結果パネル |

コンター・ヒンジ・3Dで使う既存データ配色は変更しない。結果表のUI文字色だけを専用色へ置換する。状態表示の変更からモデル・履歴・結果有効性へ書き戻さない。数値編集用のバッファは表示の丸め文字列を新たに保持値へ反映する経路を持たない。

Tab/Shift+Tab/Enter/EscとIMEはegui・既存グリッドの編集処理を通す。描画側でキーボードイベントを追加消費しない。重畳ウィンドウ上の入力は既存のeguiレイヤー判定に従い、背後のビューアはhovered/responseの判定を通す。実機確認と未確認範囲は[#976検証記録](v_and_v/UI可読性_Issue976_確認記録.md)を参照。
