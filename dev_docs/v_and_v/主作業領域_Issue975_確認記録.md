# 図・表・グラフの主作業領域の確認（#975）

確認日: 2026-10-28

対象は本案の画面配置・入力・選択である。計算式・解析モデル・検定式は変更しない。
比較案と異なる案の比較、外部AIの実通信は対象外とし、#979/#980/#982/#985の証拠に代用しない。
現在の操作仕様は[画面構成と工程タブ](../../docs/model_edit/06_画面構成と工程タブ.md)を正とする。

## 自動回帰と観測対象

初期の主領域回帰8件、および`issue975`フィルタのレビュー・入力経路対応10件が成功した。
実際のegui描画・ポインタ／キーイベントを使う試験と、保存形式・参照を直接照合する試験を分け、
変更前後のモデル・結果・Undo revisionと表示内容を確認する。
設定の再読込は保存形式の直列化・復元を通して確認し、製品exeの再起動とは分ける。

| 観測する契約 | 回帰テスト（`app::workspace::tests`） |
| --- | --- |
| 図＋表、3D＋構面、図＋結果画面を左右／上下に配置でき、表示だけでモデル・結果・選択を変えない | `requested_pairs_render_in_both_directions_without_changing_document_or_selection` |
| 構面・表示設定・倍率が独立し、ポインタのある図だけがズームする。明示同期だけで表示条件をコピーする | `drawing_controls_and_pointer_zoom_belong_to_each_view_and_sync_is_explicit` |
| 図ごとの静的結果ケースを保持し、他方のケースを変更しても結果そのものを変更しない | `each_drawing_keeps_its_static_result_when_the_other_result_changes` |
| 工程往復と保存形式からの復元で開閉・幅・分割・表示内容を保持する。ドラフト・カメラを破棄しない | `stage_roundtrip_and_preference_reload_restore_layout_without_discarding_drafts` |
| 表の拡大から以前の二分割へ戻り、入力途中の内容を保持する | `table_expansion_returns_to_its_previous_pair_and_preserves_pending_input` |
| 狭幅で領域を切り替え、リセットへ到達する。両カメラと画面選択を保持する | `narrow_switch_and_reset_keep_both_cameras_and_selection` |
| 節点セルに不正な文字を入力したまま配置をクリックしても、入力を確定・破棄しない | `changing_layout_keeps_invalid_node_cell_input_open_without_committing` |
| 図を隠すと作成モードと始点を解除し、モデルとUndoを変更しない | `switching_away_from_drawing_cancels_hidden_creation_without_editing` |

追加のレビュー・入力経路対応は次の10件で直接確認した。

| 観測する契約 | 回帰テスト |
| --- | --- |
| 保留候補より前の断面を削除して番号が再配置されても別断面を割り当てない。Undo後も古い入力版を再使用せず、再選択後だけ適用できる | `app::workspace::tests::issue975_pending_section_assignment_rejects_reindexed_candidates_and_undo_generations` |
| 保存組合せA/B/CからAを除外してもB/Cの図を維持し、表示中のBも除外した場合はCへ読み替えない | `app::actions::linear_static::workspace_result_tests::issue975_combination_removal_preserves_each_view_and_never_substitutes_the_next_result` |
| 右の通り・応力図と左3Dの固定条件を取り違えず、モデル工程のグラフからも未対応として拒否する | `app::workspace::tests::issue975_reference_conditions_come_from_the_active_drawing_and_graphs_are_rejected_in_every_stage` |
| 立体／質点ケースの選択・取得・再生を各図に保持し、ナビゲータで他方を変更しても結果を混ぜない | `app::workspace::tests::issue975_spatial_and_lumped_cases_remain_independent_through_navigation_and_playback` |
| 節点表の選択を残して別入力へフォーカス移動後に貼付しても、別入力だけが変更され、モデル・Undoを変更しない | `app::workspace::tests::issue975_paste_into_another_focused_input_does_not_edit_the_selected_node_grid` |
| 過去に描画したAが残っていても、保存した立体／質点B参照を開く先の図はBの結果・時刻へ復元され、隣接Aグラフの種別・ケースを変更しない | `app::references::tests::issue975_saved_spatial_and_lumped_reference_rebinds_the_destination_before_restoring_time` |
| 初回の結果工程への移動で配置を復元してから、見える結果領域へ静的・固有値・振動ケースを反映する | `app::nav_results::workspace_navigation_tests::issue975_result_navigation_resolves_the_restored_destination_before_updating_its_view` |
| ヒンジX図の方向・表示stepとマーカーが、隣接する増分グラフのY方向選択へ追従せず、固定条件もX・そのstepを保持する | `app::workspace::tests::issue975_hinge_direction_and_step_do_not_follow_the_adjacent_performance_curve` |
| 固定した左補助の閉状態と右時刻歴パネルを初回復元し、その後の工程往復では固定状態を維持する | `app::workspace::tests::issue975_pinned_panels_restore_from_preferences_before_pinning_across_stages` |
| 参照から領域1の図を開いても、領域2のグラフ種別・静的ケースと共有成果データを変更しない | `app::references::tests::issue975_reference_diagram_preserves_the_adjacent_graph_kind_and_static_case` |

結果画面の組合せ回帰は表示・入力分離の検証であり、実計算済みグラフの数値妥当性や外部AI通知の実通信を確認したものではない。

初回の全GUI回帰では、既存の極小高さの全画面バー試験で表示領域がなくなり、不正なフィット倍率から
グリッド線の過大生成に至って異常終了した。表示領域と倍率の検査・低い画面での配置メニュー化を行い、
`app::panels::activity_bar::tests::right_activity_bar_full_app_layout_routes_wheel_without_editing_inputs`を単独再実行して成功した。
初回異常終了を全回帰成功として扱わない。

## 最終差分のローカル検証

レビュー修正を反映した差分を固定して、2026-10-28に次を確認した。

| コマンド／確認 | 結果 |
| --- | --- |
| `cargo test -p sepika-app --features gui --locked --offline -- --quiet` | unit 1064件と全integration・doc-test成功、失敗0 |
| `cargo test -p sepika-app --locked --offline -- --quiet` | unit 435件と全integration・doc-test成功、失敗0 |
| `cargo clippy -p sepika-app --all-targets --features gui --locked --offline -- -D warnings` | 成功 |
| `cargo clippy -p sepika-app --all-targets --locked --offline -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` | 成功 |
| `cargo run -p xtask --offline -- check-docs` | 成功、summary 124・links 674・impl refs 715 |
| `mdbook build` | 成功 |
| `cargo build -p sepika-app --features gui --bin sepika --locked --offline` | Windows実操作用exe生成成功 |
| 独立レビュー最終確認（Reviewer5） | 修正必須指摘なし |

両test構成とも、通常実行から除外する既存の性能計測用`perf_probe_generate_stories_action`だけが1件ignoreである。
これは正しさ試験ではなく、手動の`--release --ignored`実行を前提とする。
以下の過去版の実機証拠は、この最終差分の製品exeを操作した証拠には読み替えない。

## Windows製品画面で確認した操作

進行担当が当該worktreeで生成したネイティブexeを別パスへコピーして操作した。
最初の確認は21:16生成版であり、以降の追加修正を含む最終版の確認とは区別する。

| 操作 | 実画面での観測 |
| --- | --- |
| サンプル読込→表を主領域へ拡大→節点追加のXへ`01234`を入力→表を下部へ戻す | 未確定の文字列を保持。部材3・節点4・編集世代0のまま |
| 図＋モデル入力表を左右二分割→他方を図へ変更→2F伏図を選ぶ | 一方は全体3Dのまま、他方は2F伏図 |
| 2F伏図上だけでスクロールズーム | 2F伏図だけが拡大し、全体3Dの投影倍率は不変 |
| モデルの二分割→解析の単独→モデルへ戻る | 左右分割、2F伏図、図ごとのズーム、未確定`01234`を保持 |
| 上下二分割へ切替 | 上下の二領域を表示 |
| 補助領域の開閉 | 不要な自動ズームなし。表示操作中の部材数・節点数・編集世代は不変 |

21:31生成版では、さらに次を確認した。この版はレビュー4点の修正前であり、最終版の証拠とは分ける。

| 操作 | 実画面での観測 |
| --- | --- |
| 再起動 | 上下二分割・左補助閉・右補助開を復元 |
| 内側約850×600へ縮小 | 左右補助を一時折り畳み、両アイコン・ファイル・全工程・ログ・診断入口を保持。領域1/2切替とリセットから単独へ復帰 |
| サンプル門型で既存増分を実行→図＋性能曲線を並置 | 長期初期載荷は設定不足で拒否され、画面確認専用にOFFとして実行。完了してもモデル工程・単独のまま。明示した二分割で左3D・右性能曲線を表示 |
| 作成入口→梁作成→設定内スクロール→取消して閉じる→図内を2回クリック | 対象・適用説明・取消と「始点をクリック」を表示。設定内スクロールで背後の投影は不変。取消後のクリックで部材数・編集世代は不変 |

性能曲線の実機確認は表示接続の確認である。地震前静的識別なし・機構未形成の既存警告を保持し、
計算の設計適合や最大ベースシアの数値照合を確認したものではない。失敗ログ・エラー詳細への到達も確認した。

## 検証の境界

上記は有限のGUI契約と実画面操作のVerificationであり、全入力方式・全結果種別・複数モニタ／
DPI設定・支援技術を網羅した証拠ではない。比較案・採用・外部AI接続の一連操作は後続Issueで扱う。
最終ビルドでの配置の再起動復元、実計算済みグラフ、作成・取消・前面入力の確認結果は、実施した操作に限って追記する。
