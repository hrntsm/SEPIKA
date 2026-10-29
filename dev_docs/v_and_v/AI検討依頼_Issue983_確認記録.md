# AI検討依頼の確認記録（Issue #983）

## 対象と境界

2026-10-29〜2026-10-30。依頼準備・明示開始・実計算・成果提出・接続終了の区別、元入力・方向・成分の保存、通常の根拠と比較・人の採否への入口を対象とする。計算式は変更していない。構造設計の妥当性・AI回答の正しさ・クラウド送信・全クライアントとの相互運用を確認したものではない。

状態は🔶。実行した自動回帰、Windows実機、CI・独立レビューの範囲を分ける。Review・Comparisonの開始・提出・切断と保存、限定修正後の最終実機・独立freshを確認した。最新HEADのCIは未実施であり、PRで確認する。

## 自動回帰と要素からの証拠索引

直接回帰は `crates/sepika-app/src/app/study/ai/operations/tests.rs`。既存接続・job・提出は[Issue #982の記録](外部AI接続_Issue982_確認記録.md)から引き継ぐ。

| 観測する契約 | 直接回帰 |
| --- | --- |
| 一般節点のReview、コピー・接続では未開始、beginで開始、昇格拒否、説明・注記保存、read-only元対象復帰 | `prepared_review_keeps_generic_targets_unstarted_and_read_only_after_copy_connection_and_reload` |
| 選択非追従、確認時の入力版変更拒否、切断後の手編集・Undo | `prepared_request_rechecks_changed_input_without_following_later_selection` |
| 保持結果の明示方向・成分、旧版許可、提出、OVIKA再読込から元根拠・入力表 | `review_from_retained_results_keeps_original_reference_and_allows_only_explicit_old_input` |
| 本案実結果を元proposalのまま保存し、形状選択から結果を創作しない | `main_review_preserves_explicit_result_source_while_shape_selection_does_not_invent_results` |
| 入力取消後も視点・配置・選択・無関係なUI状態を保持 | `cancelling_uncommitted_request_inputs_preserves_view_selection_and_unrelated_ui_data` |
| WholeModel/Membersの明示数量scope、選択不要、copyの範囲・識別、同ID再送・receipt・別ID取得・提出・保存再読込の同一性 | `explicit_quantity_scope_without_selection_survives_copy_replay_receipt_submission_and_reload` |
| 数量根拠の元入力不一致を編集後・再読込後に拒否し、元の保存入力を明示表示した場合だけ依頼できる | `quantity_request_rejects_edited_input_before_and_after_reload_until_saved_input_is_explicit` |
| 変更済み比較案の依頼元入力・対象を基準と分け、その後の再編集・保存再読込でも依頼時断面を読取専用表示する | `comparison_request_keeps_the_modified_source_input_separate_from_the_baseline` |
| 実際の本案変更で未開始の依頼を拒否し、開始済み成果を古い版とする。元copyの保持と現行性を保存再読込後も分ける | `main_review_rechecks_live_main_before_start_and_marks_submissions_old_after_edit_and_reload` |
| 依頼一覧と同じactionで材料・荷重ケース・未使用断面の保存入力表へ戻り、現行入力を変更せずread-onlyを保つ | `request_list_target_action_opens_material_load_case_and_unused_section_in_their_saved_tables` |
| 同じ元入力・部材の複数依頼があっても、本案や別依頼から選んだ比較依頼の固定入力を読取専用で開き、元断面と依頼文脈を保つ | `request_target_action_keeps_the_chosen_request_when_sources_are_shared` |
| 本案変更後のRequestInput派生Reviewでhistory=falseの開始・新規取得・同ID再送・receiptを拒否し、history=trueで元応答と取得を許可する。OVIKA復元後も同じ許可規則 | `request_input_derived_review_requires_current_history_permission_for_start_reads_and_saved_replies` |
| 実worker失敗・人の確認待ち・中断の表示元 | 既存の `worker_errors_and_disconnections_remain_failed_with_their_reason_after_reload`、`confirmation_wait_is_observable_and_cannot_be_acknowledged_by_ai_or_resumed_by_reload` |
| 拒否はモデル・受理台帳を変更せず、確認依頼は後の比較許可でも編集不可 | 既存の `rejected_scopes_versions_targets_and_replays_leave_models_unchanged`、`review_request_remains_read_only_after_new_comparison_permission_and_reload` |

途中のGUI AI回帰で、表由来の図種未指定参照をテスト側で図へ直接渡していた1件を、製品と同じ根拠・固定対象表への復帰へ修正した。fresh1前のsourceで、検討全体64件、全GUIライブラリ1164件、標準featureの検討44件が成功した。

fresh1は数量の初回応答と再送・受理照会の範囲相違、数量根拠と現在入力の不一致を検出した。取得・提出・再検証を同じ固定数量参照へ統一し、明示数量scopeでは現在選択を要求・補完しないよう修正した。修正後のAI直接回帰23件はすべて成功した。元入力表の復帰先は内部の表示先種別に固定せず、同じ保存元のInputReferenceとread-onlyを検証する。

fresh2は変更済み比較案の依頼元snapshotが基準へ置換される点と、本案由来の開始前照合・成果現行性が固定copyだけへ照合される点を検出した。依頼元入力・依頼元対象を基準の編集対象から分け、開始前の本案由来照合と本案と同じ案識別を持つ成果の現行性を実際の本案入力へ照合した。修正後のAI直接回帰25件はすべて成功した。

fresh3はAI25件・検討68件の独立回帰成功を確認したうえで、図を持たない対象も依頼一覧から必ず図へ渡す問題を検出した。対象種別に応じ図・表・詳細へ渡す一覧共通actionへ修正し、UIと同じactionの直接回帰を追加した。修正後のAI直接回帰26件はすべて成功した。

fresh4は、RequestInputから作った依頼の旧版許可が、検討内コピーとの入力一致で省略される点を検出した。実現行版の判定と明示保存入力由来の依頼を共通の履歴許可規則へまとめ、開始・参照・保存応答再照合へ適用した。修正後のAI直接回帰27件はすべて成功した。明示保存入力由来の依頼は許可検査が入力競合検査より先に働くため、#983の保存結果Reviewテスト1件の拒否codeをpermission_deniedへ更新した。#982の既存拒否・変更受理結果再送回帰は成功している。

検討の無視指定は `export_adoption_gui_fixture`（親エージェントの実機検証用fixtureを明示生成）とGUIの `write_issue980_manual_fixture`（Issue980実機確認専用の正常入力artifactを明示生成）。全GUIライブラリでは加えて `issue978_gui_fixtures`（Windows実機確認用プロジェクトを明示生成）を含む。既存の専用artifact生成用であり、失敗したテストを無視へ変更したものではない。

## 実行記録

- `rustup update stable`: stable 1.99.1で最新。`cargo clippy --version`: clippy 0.1.99。
- `cargo test -p sepika-app --features gui --lib app::study --locked`: 64成功・失敗0・既存無視2。
- `cargo test -p sepika-app --features gui --lib --locked`: 1164成功・失敗0・既存無視3。実行10.08秒。
- `cargo test -p sepika-app --lib app::study --locked`: 44成功・失敗0・既存無視1。
- fresh1修正後の `cargo test -p sepika-app --features gui --lib app::study::ai --locked`: 23成功・失敗0・無視0。`target/issue-983-ai-fresh1-tests.log`。
- fresh2修正後の同じAI直接回帰: 25成功・失敗0・無視0。`target/issue-983-ai-fresh2-tests.log`。同sourceのGUI all-targets Clippy、fmt、文書リンク、mdBook、差分の空白確認も成功。
- fresh3修正後の同じAI直接回帰: 26成功・失敗0・無視0。`target/issue-983-ai-fresh3-tests.log`。同sourceのGUI all-targets Clippy、fmt、文書リンク、mdBookも成功。
- fresh4修正後の同じAI直接回帰: 27成功・失敗0・無視0。`target/issue-983-ai-fresh4-tests.log`。同sourceのGUI all-targets Clippy、fmt、文書リンク、mdBook、差分の空白確認も成功。
- `cargo check -p sepika-app --features gui --locked`: 成功。
- `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings`: 成功。
- fresh1修正後も同じClippy、fmt、mdBookと文書リンク確認が成功。
- `cargo fmt --all -- --check`: 成功。
- `cargo run -p xtask --locked -- check-docs`: 成功（summary125・links678・impl refs722）。
- `mdbook build`: 成功。
- `cargo run -p xtask --locked -- check-deps`: 成功（46 upstream checks）。
- `cargo build -p sepika-app --features gui --example ai_connection_fixture --locked`: 成功。実機前exeのSHA256は `3A52BBF6060204DF5FA9CA2AA15BC37162551CE996072CEE88EF395FE3475F74`。
- `gui_probe.py` は準備依頼ID・操作対象入力を明示指定し、提出前の実状態を確認できるoptional引数を追加した。Pythonの `py_compile` と `--help` が成功した。後述の専用fixtureでMCP実通信も成功した。
- `cargo build -p sepika-mcp --features mcp --locked`: 成功。MCP exeのSHA256は `AC7444588B1239C812BFB437CD8514E781D187ED5154006806C2ED8133BFA3D2`。
- fresh1修正後のGUI exeの再ビルドも成功。SHA256は `9011BFDC9EE7897A14D93F714748F845B3C29BA080B176E2E002686790291748`。最後の回帰assert変更を含むClippyも再実行して成功した。
- fresh2修正後のGUI exeの再ビルドも成功（1分22秒）。SHA256は `1802978218C375891C80C17C740DDFEAD1D980C68EFB32A0D84D137A9D504EA9`。先の2つのGUI hashをfresh2後sourceの実機確認へ流用しない。
- fresh4修正後のGUI exe再ビルドも成功。限定修正前の実機exeのSHA256は `21DB7003DF84A01C19697BEDCA257CC69AB681093D4C592528FED42BE85E400A`。限定修正後の実機確認には再ビルドしたexeを使う。
- fresh5は修正必須なし。限定修正前のAI直接回帰27件・検討全体70件、GUI all-targets Clippy・fmt・文書リンク・mdBook・差分確認が成功した。限定修正後の結果とは分ける。
- 依頼文脈の限定修正後はAI直接回帰28件成功・失敗0、検討全体71件成功・失敗0・既存無視2。ログは `target/issue-983-ai-context-tests.log`、`target/issue-983-study-context-tests.log`。
- 同じ限定修正後sourceでGUI all-targets Clippy・fmt・文書リンク（summary125・links678・impl refs722）・mdBook・差分確認が成功した。ログは `target/issue-983-{clippy,fmt,docs,mdbook,diff}-context.log`。
- 限定修正後の `cargo build -p sepika-app --features gui --example ai_connection_fixture --locked` は成功（48.09秒）。最終実機に使ったexeのSHA256は `7BDD585E5C7698C06845192404EAB6273277E2DB7C370BC9C95D169A0B1C2123`、54,725,632 bytes。ログは `target/issue-983-gui-context-build.log`。
- fresh6は修正必須なし。独立したAI直接回帰28件・検討全体71件（既存無視2）・差分確認も成功した。
- ビルド先は当該worktreeの `target`。開発ログは `target/issue-983-*.log`。他worktreeのビルド成果を更新していない。

## Windows実機

専用の正常サンプルだけを対象とし、未設定壁版の警告続行・本案採用・実プロジェクトやクラウドへの送信・OS設定操作は含めない。全モデル・条件・保持結果は利用者の明示承認を得て、同じPC内の `sepika-mcp.exe` と `gui_probe.py` だけに共有した。

Reviewでは部材2から依頼を準備・保存・コピーし、コピーと接続許可だけでは未開始であることを実画面で確認した。開始受理後は作業中と最終確認操作を表示し、本案世代0・部材2の選択を維持した。実通信で比較案作成と計算がともにpermission_deniedとなり、未計算の説明提出が成功した。実画面でAI説明・未確認事項2件・現行入力との一致・未計算を別々に確認した。解除後は接続中断・継続未確認を表示し、説明を保持した。旧sessionはpipe終了で拒否され、helperは正常終了した。

Comparisonは別依頼として部材2から準備した。初回のhelperは依頼元の本案を操作対象へ誤指定したためproposal_missingで拒否され、未開始を維持した。コピー文の「操作対象案・入力版」が示す検討内の基準コピーへ修正し、新接続先で再承認した。実通信で比較案作成・断面2割当・変更取得・計算・結果と根拠と数量の取得・提出が成功した。runningはhelper応答で観測したが、短時間のためGUIの計算中表示は実測していない。GUIでは計算済みを確認し、検定済26・未検定39・NG0と採用を区別した。AI通知で本案世代0・選択・作業画面を切り替えなかった。提出根拠から世代1の比較結果を読取専用表示し、依頼固定入力から元の世代0・断面1へ戻れた。解除後の旧session拒否とhelper正常終了も確認した。

実測artifactは当該worktreeの `target` 内に保持する。

| artifact | bytes | SHA256 |
| --- | ---: | --- |
| `issue-983-review-live.json` | 12,669 | `48DA8A6E5547505832578A760055043B2B676C8A98E41C4ADE35EE3DFB995093` |
| `issue-983-comparison-retry.json` | 240,982 | `9A759E51657BDABC22860B60D531F9F200C04E3A9E957F230A0D48B91A5B1680` |
| `issue-983-final-gui.ovika`（人の補足追記後の最終保存） | 62,927 | `63FDB8B01D11BC2575ADDBE77FC4DF5ED6BE9F70C6DF7D33B58569DAD8379B33` |

初回保存時はReview・Comparison両依頼、AI説明、比較の保持計算1件、接続中断、本案世代0を持ち、保存後の未保存表示消失を確認した。初回保存の62,827 bytes・SHA256 `07FBE35100994F548B5741378E11EF5D51472B8FD3782611CE4BC523C11A243F` は追記前の測定であり、前表の最終保存とは区別する。

同じ元入力を使うComparisonの対象確認で、部材・断面は正しいが先行Reviewの目的へ表示文脈が切り替わることを実機で検出した。一覧actionが検討・依頼の識別を保持して指定の固定入力へ切り替える限定修正と直接回帰を追加した。最終exeで本案からComparisonの部材2を開き、Comparisonの目的をタイトルに持つ読取専用の世代0・元断面1を維持することを実画面で確認した。

最終exeで専用OVIKAを通常の「開く」から読み込み、本案世代0、両依頼のAI説明・固定条件、比較の保持計算1件、検定済26・未検定39・NG0を確認した。再読込後は継続未確認であり、接続許可はなかった。人の補足欄だけに「専用サンプルで元入力と保持根拠を確認した。未検定項目が残るため本案には採用しない。」を追記すると未保存表示が付き、通常のCtrl+Sで消えた。同じ保存物を再度開き、人の理由とAI説明が別欄に残り、計算・継続未確認・許可なし・本案世代0を保持することを確認した。

接続なしでComparisonの比較表を開き、未計算の基準との結果差を判定せず、人による採否への入口が表示されることを確認した。採用は実行していない。「MCP実通信比較を手作業で表示」から編集対象の比較案世代1へ移れた。最後は保存済みで通常終了した。手作業での再計算・Undoと失敗・人の確認待ちは前記の自動回帰で検証し、今回の実画面確認へ読み替えない。

### fixtureの区別

- #973の確定fixtureは仕様検討と接続小実験のための構造入力であり、現在の製品接続・Task保存形式の互換性を確認した成果ではない。
- #982の引継物は `target/issue-982-handoff/final-ai-connection.ovika` と対応するReview/Comparison JSON。正常fixtureの過去の製品実測であり、OVIKAのSHA256は `25CE073D5F2568455A161F908CCA7A518BA3584A62E54EAECC8FE929DAC82F27`（57,368 bytes）。旧開発スキーマの可能性があるため、新Task形式での読込成功を仮定せず、互換性の補完も行わない。
- #983は当該worktreeの `ai_connection_fixture` と `sepika-mcp.exe`、同じ `gui_probe.py` で前記の新しい専用保存物を生成した。#982の保存物は今回の実測証拠として流用していない。

## 未検証

全入力フォーム・全モデル・全DPI・全狭幅条件の網羅、AI回答の内容保証、実物Validationは対象外。GUIの計算中表示の瞬間は未観測。最新base/HEADのCIはPRで実行証拠を確認するまで未確認とする。
