# 対象・操作検索の確認（#977）

確認日: 2026-10-29。状態は🔶。最新の実装コミットは `d1e90cec`。
現在の操作仕様は[画面構成と工程タブ](../../docs/model_edit/06_画面構成と工程タブ.md#対象操作の検索)を正とする。
計算式・解析モデル・検定式は変更しない。外部AI、比較案、全文検索は対象外である。

## 自動回帰と観測対象

`crates/sepika-app/src/app/search/tests.rs` の次の8件を、全GUIライブラリ回帰に含めて確認した。
eguiの実描画と合成したポインタ・キー・IMEイベント、および参照・モデル・Undoの直接照合によるVerificationである。
Windowsの日本語IME、実画面での判読性、通常の利用者操作を確認した証拠には代用しない。

| 観測する契約 | 回帰テスト（`app::search::tests`） |
| --- | --- |
| 日本語・英数字・同名候補の識別、結果なし、自動選択なし、明示確定だけで解析設定へ移動し解析を開始しない | `search_names_distinguish_member_section_story_axis_and_operation_without_selecting` |
| ボタン・ショートカットの開閉で不正文字と有効数値の節点ドラフト・focus・選択・モデル・Undoを保持し、別表への確定後も元の入力を保持する | `search_button_and_shortcut_preserve_invalid_grid_draft_and_selection_on_cancel` |
| IME変換中・変換確定と同時のEnterでは移動せず、その後の明示Enterで移動する | `ime_commit_enter_does_not_navigate_but_next_explicit_enter_does` |
| 幅460pxで図表示の非対応理由と読み取り専用の検索元を表示し、本案への移動を拒否する | `narrow_search_reports_unavailable_views_and_read_only_source_without_navigation` |
| 共通参照と同じ対象・図へ移動し、検索元入力版の変更後は確定を拒否する | `search_target_reuses_reference_visibility_and_rejects_changed_input` |
| 両領域から既存表へ移動し、既存編集の同名拒否・Undo／Redoを共有する | `both_panes_open_the_existing_table_and_share_validation_and_undo` |
| 非表示状態を示し、詳細への暗黙移動を拒否し、明示した図表示だけで絞込みを解除する | `hidden_member_requires_explicit_reveal_before_detail_navigation` |
| 狭幅で補助領域が閉じている場合と左側を表示している場合の両方で、検索・共通参照から詳細ドックと対象を表示する | `narrow_detail_confirmation_reveals_the_shared_reference_inspector` |

初回の限定回帰では、テスト用モデルの断面件数・部材への割当・IME入力列の設定不備によって失敗した。
これらを修正し、最終差分の全GUIライブラリ回帰を再実行して成功した。初回の失敗を成功として扱わない。

## ローカル検証

| コマンド／確認 | 結果 |
| --- | --- |
| `rustup update stable` | 最新stableのRust 1.99.1を確認 |
| `cargo clippy --version` | clippy 0.1.99（58cf8f9c35 2026-10-13） |
| `cargo test -p sepika-app --features gui --locked --lib --quiet` | 最新main（#976統合済みの `c17c7ce9`）と共通テーマ接続後の `d1e90cec` で1075件成功、失敗0。上記検索8件・既存参照・主領域回帰を含む |
| `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings` | 成功 |
| `cargo build -p sepika-app --features gui --bin sepika --locked` | 成功。専用作業ツリーの `target/debug/sepika.exe` を生成 |
| `cargo fmt --all -- --check`・`git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | 製品文書のリンク・実装参照の検証成功 |
| `mdbook build` | 製品文書HTMLのビルド成功 |

#976の共通テーマに、検索候補の選択印・省略と全文ホバー、案名の省略と全文ホバー、検索入力のフォーカス輪郭を接続した。
上表の全コマンドを `d1e90cec` のコードで再確認した。実機再確認用exeは専用作業ツリーの
`target/debug/sepika.exe`（2026-10-29 08:55:45 JST、51,140,608 bytes）である。

## Windows実機の試行と未確認範囲

親エージェントによる実機試行の報告を記録する。初回は専用exeのウィンドウ取得後の
`activate_window` が `failed to activate captured window` となり、規定の1回の復旧も失敗した。
この初回試行ではアプリへ入力せず、画面内容も未確認だった。

再試行では `d1e90cec` の専用exe（exec session 72103、ウィンドウ43387814）を操作した。
内蔵の門型サンプルだけを使用し、ファイル保存は行っていない。ウィンドウは1282×827であった。

| 実機での操作 | 観測結果 |
| --- | --- |
| 検索ボタンとCtrl+Shift+P | どちらも検索を開いた |
| `H-300`を検索 | 同名の部材#0・#1と断面#0を識別して表示した |
| 候補未選択のEnter | 移動せず検索を継続した |
| Downを2回、Enter | 部材#1の詳細へ移動し、選択は1件。Undo無効・解析未実行を維持した |
| 空の検索欄でAlt+grave、`k`・`a`・`i`を入力、Space、Return | 未確定の「かい」から「解」へ変換し、変換確定後も検索を継続した。候補は未選択だった |
| Alt+graveで入力モードを戻し、Down、Return | 既存画面へ移動し、解析未実行を維持した |

狭幅化のためSEPIKAのウィンドウ境界のドラッグとシステムメニューのSizeを試したが、
1282×827から変化しなかった。Alt+Spaceは常駐ランチャーに捕捉され、直ちにSEPIKAへ戻した。
以後Alt+Spaceを使用していない。OS設定の取得・起動・変更は行っていない。

その後、ユーザーの物理EscapeによってComputer Useが停止された。実機操作はそこで停止した。
狭幅での候補・理由・詳細ドックの判読性、二画面での一連操作、入力中セルのドラフト保持、
通常の取消操作は実機未確認である。これらを自動イベント試験の成功で確認済みとはしない。
Draft PRを維持し、CIが成功しても実機未確認のためmergeを保留する。
最終headのCI・mergeは本記録時点では未実施であり、将来の成功を予記しない。
