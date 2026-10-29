# 確認済み断面案の採否・Undo・判断根拠の確認（Issue #981）

## 範囲と状態

2026-10-29、[Issue #981](https://github.com/hrntsm/SEPIKA/issues/981)の断面割当一括採用、既存Undo/Redo、判断時根拠と採否・理由の保存をVerificationした。状態は🔶。自動回帰と、一台のWindowsで専用正常fixtureを使った実機確認を分ける。以下の実機欄は実機担当者の観測報告を受領して記録したもので、記録担当者は再起動・追加操作を行っていない。構造計算式の新しい照合、一般の構造設計適合、実物Validationは対象外である。

製品コードは `9dad259ad52fa31051098b1f849f7a76ab2e9e65`、操作文書を含む確認時HEADは `cc6f338b5e30d064ad3f61d5ee96482c3d0bbe3c`。実機exeのSHA256は `478048D58CE07999F3A0A04D8BBC4AD68129ACE0CCA2ED5D1BFEC3C75571EB92`。本記録の追記では製品コードを変更していない。記録追記後の最終HEADの文書レビュー・PRの全CI・mergeは別途確認し、成功を予記しない。

判断の境界は[ADR0092](../adr/0092-confirmed-study-adoption.md)、操作は[断面案の検討](../../docs/model_edit/10_断面案の検討.md)、比較指標の通常結果との照合は[#980の記録](断面案比較_Issue980_確認記録.md)を参照する。

## 自動回帰と要素からの証拠索引

以下は [adoption.rs](../../crates/sepika-app/src/app/study/adoption.rs) の `sepika_app::app::study::adoption::tests` にある。実機で再実施していない条件も含む。

| 対象 | 直接回帰 |
| --- | --- |
| 確認済み変更の採用、直後Undo、後続編集を先にUndo、案再編集後の固定Redo、結果失効、immutable根拠と保存 | `adoption_undo_following_edit_redo_and_saved_evidence_remain_independent_of_proposal` |
| 確認中の本案・比較案変更、対象断面変更、古い入力、計算失敗、範囲外変更・無効参照の拒否 | `changed_main_or_proposal_and_failed_or_old_calculations_do_not_apply` |
| 未計算・新しい未確認run・再編集後の再計算要求 | `uncalculated_results_and_newer_unreviewed_runs_require_calculation_or_redisplay` |
| 非表示ケース／組合せの実結果だけが欠け、入力記録は残る場合も本案・Undo/Redo・判断記録を未更新 | `missing_unselected_static_or_combination_result_is_rejected_without_any_edit_or_record` |
| 途中の一部割当Noopで全変更を破棄し既存Redo・適用状態を保持 | `partial_assignment_failure_preserves_input_history_and_application_state` |
| 変更なし・重複採用では編集・判断を増やさない | `unchanged_or_duplicate_adoption_does_not_add_edit_or_decision` |
| 不採用理由と判断時入力・結果の保存、本案不変 | `rejection_is_saved_with_immutable_input_and_no_main_edit` |
| OVIKA保存再読込後の本案基準一致、Undo取消の保存、旧セッションUndo非復元 | `save_reload_preserves_main_baseline_equality_and_adoption_cancellation` |
| 動的ケース・静的以外の実設定を照合し、選択表示を入力変更にしない | `main_equality_includes_dynamic_inputs_and_non_static_settings_but_ignores_display` |
| 動的ケースの入力定義を保存しても、存在しない計算結果を復元しない | `study_save_retains_main_dynamic_case_inputs_without_restoring_any_result` |
| 非既定の検定設定を全項目保存復元し、そのまま採用できる | `all_nondefault_design_settings_roundtrip_and_still_allow_adoption` |
| 実計算の長期→短期表示のみでは本案不一致にせず、組合せ係数の実変更は不一致、復帰後は採用可能 | `selecting_long_and_short_results_changes_display_term_without_blocking_adoption` |
| 未準備の質量方式の保存同期で本案基準一致を失わない | `pending_mass_method_save_reload_does_not_change_main_baseline_match` |

GUI構成では代表回帰の取消・再適用を `undo_action` / `redo_action` から行い、通常の結果失効と判断記録を一緒に確認した。本案完全一致と、許可断面差を除く比較条件一致は別である。荷重値・支点・解析設定・組合せ実定義の比較拒否は [comparison/tests.rs](../../crates/sepika-app/src/app/study/comparison/tests.rs) の `conditions_compare_actual_definitions_not_names_ids_or_governing_case` にも対応する。

旧・未来・欠落契約、生成入力不一致、保存結果の失効は既存の結果有効性判定を経由する。動的ケース定義や本案設定の保持を、結果の有効性や最新計算の復元に代用しない。保持結果の失効と原本不変の回帰は [study.rs](../../crates/sepika-app/src/app/study.rs) の `retained_result_views_apply_contract_input_and_panel_gates_without_changing_saved_evidence` と上記#980の証拠に接続する。

## ローカル検証と独立レビュー

最新stableへの更新を実行し、`rustc 1.99.1 (58cf8f9c3 2026-10-13)`、`clippy 0.1.99`を使用した。

| コマンド／構成 | 観測結果 |
| --- | --- |
| `cargo test -p sepika-edit -p sepika-app --lib --locked --features sepika-app/gui` | edit 217成功。初期GUI app 1140成功・3 ignored |
| 再凍結後 `cargo test -p sepika-app --lib --features gui --locked` | 1143成功・3 ignored |
| 再凍結後 `cargo test -p sepika-app --lib --locked` | 464成功・1 ignored |
| 再凍結後GUI採用代表 `study::adoption` | 13成功・1 fixture生成ignored |
| `cargo clippy -p sepika-edit -p sepika-app --all-targets --features sepika-app/gui --locked -- -D warnings` | 成功 |
| 同Clippyのdefault構成 | 成功 |
| `cargo fmt --all --check` | 成功 |
| `target/debug/xtask.exe check-docs` | 成功（summary 125、links 676、impl refs 722） |
| `mdbook build` | 成功 |
| `cargo build -p sepika-app --features gui --bin sepika --locked` | 最終exe生成成功 |
| `git diff --check` | 成功 |

初回の独立レビューで、生成入力記録だけでは必須ケースの実結果存在を保証しない点を修正し、比較画面と採用確認の検査を共通化した。二回目で本案の非既定検定設定の保存不足と、表示由来の長短期区分による誤不一致を修正した。表示切替の追加回帰は初回に1失敗したが、G+Pを含まないfixtureでは対象の検定が成立していなかったため、有効な長短期組合せへfixtureを修正した。再凍結後の全件成功と区別して残す。三回目のfresh独立レビューは修正必須なしだった。

## Windows実機の観測

現在のOS設定のまま、一台の専用プロセスで確認した。OS Settingsの取得・起動・変更は行っていない。

入力は `target/issue-981/normal-adoption.ovika`。生成入口は上記テストモジュールの `export_adoption_gui_fixture`（明示実行専用ignored test）。プロジェクト識別は `9846e359-9d54-440d-9009-af7f240068a1`、部材3・節点4・断面3、固定対象は大梁ElemId2の1部材。検討「比較回帰」に基準案・案A・案Bを持ち、全案の保持計算に警告・診断・notice・errorはない。床板・壁版の未設定領域もない。以下はこの有限入力の観測である。

| 操作・対象 | 実機観測 |
| --- | --- |
| 比較指標と範囲 | 基準の対象鋼材体積0.050022→案A 0.076950 m³、差+0.026928 m³。固定対象最大比0.234658→0.130545。固定対象は検定済6・NG0・未検定12、主架構は検定済26・NG0・未検定52。集計外・未評価説明と「採用≠検定OK」の注意を保持 |
| 変更外への影響 | 柱0/1のDL最大比0.012638→0.015042、差+0.002404、xi位置移動を表示。主架構最大値の差は評価範囲差のため比較しない旨を表示 |
| 変更と明示採用 | 大梁Elem2の「梁 H-400x200x8x13」（S1）→「比較用大梁」（S2、H500x250x10x16）、対象1を変更表で確認。案Aの確認入口から理由「実機確認：断面変更と未検定範囲を確認」を日本語literal入力し、明示採用 |
| 採用直後 | 本案世代1・要再計算。部材0/1はS0のまま、部材2のみS2。判断は「採用済み（現在本案との一致は別）」、同理由・採用時本案一致・節点3/全体X/長期・同ケース集合内の最大・固定1を保持 |
| 直後Undo/Redo | 上部Undoを一度実行し、Elem2はS1、他部材S0不変。Undo無効・Redo有効、判断は「採用をUndoで取消」。検討窓を閉じてRedoを一度実行するとS2、本案世代3へ復帰 |
| 後続通常編集と順序 | 本案Elem2をS2→S1と通常編集。判断は採用済みのまま「現在の本案は採用時入力から変更（Undo取消とは別）」を表示。後続編集へのUndo一回でS2へ戻り、採用取消より通常編集が先に戻る |
| 保存再読込 | 専用 `target/issue-981/manual-saved.ovika` へ別名保存し、Openダイアログから同ファイルを再読込。本案S2、Undo/Redoとも無効。採用済み判断・日本語理由・採用時本案一致・固定条件を復元 |
| 判断時の根拠 | 判断の「準備後入力・結果」から案Aの読取専用ビューへ移動。部材割当操作は無効で、判断時のS2と保持結果を表示。本案へ比較結果を付け替える操作ではない |
| 狭幅の入口 | 外枠1282×827→802×827へ右端resize。ナビ開閉→検討→比較→案A確認→理由欄・採用／不採用ボタン・新基準入口へ到達。高さresizeは反映しなかった |
| 新基準入口 | 「本案を表示して新しい基準の検討へ」で本案に戻り、対象を選び直す通知を確認。旧対象を使った検討の自動作成は行わない |
| 終了 | ウィンドウCloseで終了し、専用PID35136が存在しないことを確認。終了コード0の観測としては記録しない |

専用保存出力のSHA256は `0C478A2D0BA74E172E8E0296E1748A67EA88923485D0807FBD4A9000C834DEA5`。スクリーンショットはツール内で直接確認し、画像ファイルとして保存していない。

初期のuser input検知では再観測し、端末が前面に重なった際はSEPIKAをactivateして復帰した。端末への入力は行っていない。今回の実機でautomatic approval reviewによる拒否、ロック、物理Escapeによる停止は発生していない。先行#979で拒否された未設定R0モデルの続行は再試行せず、間接実行や警告を隠す入力改変も行っていない。

## 未実機・未確認の境界

比較案再編集後のimmutable根拠・固定Redo、未計算・古い・失敗・競合・実結果欠落等の拒否系は直接回帰で確認し、今回の実機では再実施していない。これは実機確認済みという意味ではない。非既定設定・動的ケースの往復も直接回帰の証拠である。

狭幅は外枠802×827の観測に限る。802×631および800×600は未確認であり、幅だけの達成から高さ条件の達成を推定しない。日本語literal入力は確認したが、このIssueでIME変換は確認していない。DPI・OS設定は取得変更せず、先行確認を参照するだけで全条件を再検証済みにはしない。全モデル・全入力方法・全表示条件・支援技術・実物Validationも未確認である。

本Issueの自動回帰と有限な手作業採用確認は、[親#972](https://github.com/hrntsm/SEPIKA/issues/972)および[横断一連操作#985](https://github.com/hrntsm/SEPIKA/issues/985)の全確認を代替しない。外部AIクライアントとの製品接続・権限・AI作成案を使った一連操作は今回実施していない。これらの境界や未実機範囲を後続Issueへ記したことだけで、元の必須条件を検証済みへ読み替えない。
