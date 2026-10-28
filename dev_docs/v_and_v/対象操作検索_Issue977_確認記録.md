# 対象・操作検索の確認（#977）

確認日: 2026-10-29。状態は🔶。実装コミットは `e6ce83a5`。
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
| `cargo test -p sepika-app --features gui --locked --lib --quiet` | 1072件成功、失敗0。上記検索8件・既存参照・主領域回帰を含む |
| `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings` | 成功 |
| `cargo build -p sepika-app --features gui --bin sepika --locked` | 成功。専用作業ツリーの `target/debug/sepika.exe` を生成 |
| `cargo fmt --all -- --check`・`git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | 製品文書のリンク・実装参照の検証成功 |
| `mdbook build` | 製品文書HTMLのビルド成功 |

## Windows実機の試行と未確認範囲

親エージェントによる実機試行の報告を記録する。専用作業ツリーの製品exeを通常の可視 `Start-Process` で
起動し、PID 2256、`sky.list_windows` で専用ウィンドウ4917996を取得した。
`get_window` 後の `activate_window` は `failed to activate captured window` となった。
ウィンドウ一覧の再取得とウィンドウの再選択による規定の1回の復旧も同じエラーとなり、試行を停止した。
コンピューター操作によるアプリへの入力は行っておらず、画面内容は未確認である。OSの表示倍率も変更していない。

したがって、日本語IMEの実入力、検索ボタン／ショートカット・候補移動・確定・取消の通常操作、
狭幅での候補・理由・詳細ドックの判読性、二画面での一連操作は実機未確認である。
自動イベント試験の成功や起動プロセスの取得を、これらの完了条件の達成と扱わない。
Draft PRで継続し、CIが成功しても実機未確認を理由にmergeしない。
最終headのCI・mergeは本記録時点では未実施であり、将来の成功を予記しない。

再確認では専用exeのウィンドウを操作できる状態にしたうえで、通常幅と狭幅の双方、日本語IME、
二画面と入力中セルを含む操作を実行し、観測結果を本記録へ追記する。
