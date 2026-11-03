# App表示と検討入力の整理（Issue1059）

基点は `e82b4e605eddf03f5f8d5ceecbd8731ac3b68bea`。App58候補について不要な一時所有・反復・未観測の前準備を整理し、同一fixtureの観測を共有する。式・係数・浮動小数演算順・エラー順・表示値・ID・保存bytes・Undo/Redo・独立期待値・公差を維持する。これは現役の対応表であり、固定SHA・rawログ・過去の実測を変更しない。

## 現役の保証

台帳keyの接頭辞は、P=`pending_app`、T=`pending_app_tail`、N=`app_checkpoint_candidates`、C=`phase10_coder_checkpoint_candidates`、A=`phase10_auditor_checkpoint_candidates`、AC=`phase10_auditor_checkpoint_conditional_candidates`、AS=`phase10_auditor_checkpoint_separate_findings`、AA=`phase10_auditor_checkpoint_additional_scope_candidates`、AAC=`phase10_auditor_checkpoint_additional_scope_conditional_candidates`、AAS=`phase10_auditor_checkpoint_additional_scope_separate_findings` を省略する。

| 台帳key | 対象（sepika-app） | 元観測から整理先への対応と保持する保証 |
| --- | --- | --- |
| P-003 | `src/viewer/th_detail.rs` | Q0配線済み・Wall対象外という現行契約へ説明だけを一致。実装と非対応範囲は不変。 |
| T-001 | `src/app/actions/design/holding.rs` | capacity_forcesの最後の所有先へmove。全断面力・入力・算式を保持。 |
| T-002 | `src/app/actions/design/holding.rs` | 未読legsだけをtupleから除去。aw_x/aw_yと強弱軸入力・演算を保持。 |
| T-003 | `src/app/actions/design/holding.rs` | 同じpo/elem/ds_stepの最初の壁応答を再利用。先行エラーとRC/SRC分岐を保持。 |
| T-005 | `src/app/panels/analysis.rs` | 荷重ケース/組合せをモデルから同順で借用。ラベル・選択ID・UI順は同じ。 |
| T-006 | `src/app/panels/navigator.rs` | 選択直前の鋼/RC ID列をmove。集合・順序・クリックを保持。 |
| T-007 | `src/app/panels/navigator.rs` | 行のid/kindだけcopy。表示・選択と可変借用前の値確定を保持。 |
| N-008 | `src/app/nav_model.rs` | 小梁→間柱の借用iteratorを既存分類へ。section未設定除外・階分類・順序を保持。 |
| C-001 | `src/app/panels/preparation.rs` | stress_cfg基準は借用、編集用cfgは所有。最終比較後のUndoコマンドと実行順を保持。 |
| C-002 | `src/app/panels/preparation.rs` | Story行は借用。編集コマンドの描画後反映、全入力欄・ID・順序を保持。 |
| C-003 | `src/app/panels/results.rs` | 不変modelのlayersを一回生成。空間/平面/同定/応答の全名前・欠落時表示を保持。 |
| C-004 | `src/app/preparation.rs` | Result<Option>を順番にtranspose/collect。最初のErrとSomeの順、計算入力を保持。 |
| C-005 | `src/app/preparation.rs` | section_appは置換前material/frame_useの2 Copy値だけ保持。断面入力と試験assertは不変。 |
| C-006 | `src/app/rc_wall_direction_tests.rs` | CSV文字列のstdファイル自己往復だけ除去。製品CSV独立値、GUI、OVIKA、編集を保持。 |
| C-007 | `src/app/reference_identity_acceptance_tests.rs` | 既存モデルの節点数だけ同一fixture一回から取得。全参照役割×欠損ID/MAX、入口・原子性を保持。 |
| C-008 | `src/app/references.rs` | Hingeのstep代入を一回に。Noneによるclearと非HingeのSomeフレーム更新を保持。 |
| C-009 | `src/app/references.rs` | 既存digestを利用。bincodeの(source,model,contract,scope)、expect、小文字SHA256を保持。 |
| C-010 | `src/app/references.rs` | 最終WholeModel/Members scopeで一回構築・hash。Membersのsort/dedup、モデルと入力元の所有を保持。 |
| C-014 | `src/app/result_evidence.rs` | 最後のCalculation参照をmove。source clone、input、linesと全検証・診断順は保持。 |
| C-016 | `src/app/result_evidence.rs` | 同一checkを各表示分岐一回評価。complete/known_ng、成分順・全表示文字列を保持。 |
| C-017 | `src/app/study.rs` | 操作確定後・検討ループ前に本案snapshotを一回保持。ループ中はメタ情報のみ変更、switch等は後段実行。 |
| C-019 | `src/app/study/ai/operations.rs` | JSON全値を作ってJob/Submissionを台帳へmove。時刻・last_operation/receipt更新・原子的受理を保持。 |
| C-020 | `src/app/study/ai/operations/tests.rs` | 同一案参照を一回取得。Request全値、UUID、permissionと全呼出入力を保持。 |
| C-021 | `src/app/study/comparison.rs` | 同条件判定はモデルのみclone。先行設定照合、固定断面置換、STB ID付与/bytes照合を保持。 |
| C-022 | `src/app/study/comparison.rs` | 行ごと同一check一回評価。空/負/非有限/partialNG、不正位置のratio消去と表示を保持。 |
| C-023 | `src/app/study/comparison.rs` | 全bucket構築後に各label列を一回sort。containsによる重複除去と最終BTreeMap/昇順を保持。 |
| C-024 | `src/app/study/comparison/gui.rs` | 同フレームのcase/envelope確定とrefresh後に基準行/固定対象/summaryを共有。操作はlocalキュー、比較・根拠を保持。 |
| A-001 | `src/story_copy_view.rs` | preview cacheを借用。summary/created/mismatched/changed表示と保存cacheを保持。 |
| A-002 | `src/story_copy_view.rs` | 実行guard成立後にreportだけ所有化。CopyStory入力、Undo/notice順、保存previewを保持。 |
| A-003 | `src/quantity_view.rs` | 所有小計rowsをmove、総計を借用。全列・集計順・未算定理由を保持。 |
| A-004 | `src/app/workspace.rs` | 読まれないallow_creation初回代入だけ除去。active決定後の代入を保持。 |
| A-006 | `tests/source_story.rs` | 連続する同一export==afterの片方だけ除去。他状態・反復exportの保証を保持。 |
| A-007 | `src/tables/materials.rs` | 描画内draftを一回parse。表示理由/is_okを共有し、commit境界の再parseは保持。 |
| A-008 | `src/tables/members.rs` | 最後のwall_indexをmove。Cow所有/借用、生成壁ID・選択clearを保持。 |
| A-009 | `src/load_editor.rs` | &editor呼出前後の不変clone比較だけ除去。model/history/results/staleness/redo/field診断は保持。 |
| A-010 | `src/section_editor.rs` | 標準Vecだけを操作するpush/remove/assertを除去。build_shape経由の段別筋・径・hoop・円形保証を保持。 |
| A-011 | `src/section_editor.rs` | 完全同本文のChannel/Tee寸法欄を共有。H/B/tw/tfのwidget順/ID、Hの半径欄を保持。 |
| A-012 | `src/section_editor.rs` | 既存rc_rect_dimensionsで同じB/D欄を描画。後続の部材別配筋・widget順/IDを保持。 |
| A-013 | `src/tables/wall_plates.rs` | opening列を借用して既存formatへ。draft/書式と所有が必要なself_weight/loadsを保持。 |
| A-014 | `src/summary.rs` | 同一引用符escapeをcsv_cellへ統合。CSVのbytes/列順を保持。 |
| A-015 | `src/summary.rs` | 不変qのtotalsを一回保持。合計・径別状態の集計APIと加算順を保持。 |
| A-016 | `tests/full_model.rs` | len=4直後のwindows存在だけ除去。層数・層せん断単調性・ベース一致は保持。 |
| A-017 | `tests/full_model.rs` | 同一TH結果の先行applied_long_term assertだけ除去。後段の説明付きassertと全時刻歴保証は保持。 |
| A-018 | `tests/full_model.rs` | 全位置Checked=345/Skipped=0が包含するRC/SRC Checked集計だけ除去。存在・形状・finite・対象件数を保持。 |
| A-019 | `tests/full_model.rs` | 全ResultsBundle/Settings bytes一致が包含するlen/field比較を除去。modal.is_someはNoneを排除する固有保証としてKeep。Model比較も保持。 |
| A-020 | `tests/full_model.rs` | import_anchorizes_secondary_members全assertをimport_builds_expected_modelへ。同一imported入力、56本/全所属/荷重/支点の独立保証を保持。 |
| AC-001 | `src/summary.rs` | 不変&App/内部可変性なしのmetrics/CSV前後serialization比較だけ除去。方向・欠落・識別・設定依存の全期待値を保持。 |
| AS-002 | `src/tables/slabs.rs` | Line/Anchor・Point・Enclosed・Line/Columnsの元4入力を実tip_load_sectionへ。見出し/追加controlの有無と-Yラベルを観測。 |
| AS-003 | `tests/full_model.rs` | 底梁不存在/唯一経路という誤説明だけを現在の観測へ訂正。region生成入力・全assertは保持。 |
| AA-001 | `src/app/tests.rs` | 隣接するsource_story=None同一ループを一回に。native未初期化と3種の初期化順/export/save/undoを保持。 |
| AA-002 | `src/app/tests.rs` | manual_nodal完全一致が包含するany Manualだけ除去。手動値/名称と自動荷重件数を保持。 |
| AA-003 | `src/app/tests.rs` | 連続する同じexplicit_story_inputs代入を一回に。全ダンパー種類×質量法の同期入力/重量を保持。 |
| AA-004 | `src/app/tests.rs` | 同値床fixtureをmake_floor_secondary_beam_test_modelへ。元のJ-edge名へ戻し、別試験のJ1と区別、共有辺1件assertを保持。 |
| AA-005 | `src/app/tests.rs` | 同一全fieldのdummy_th(vec![0])/dummy_lumped(0.52,true)/dummy_pushover(1)へ。独立treeラベル/slot ID期待値は試験側に保持。 |
| AA-006 | `src/app/tests.rs` | active=true/open=true同一末尾反復だけ除去。3固有開閉遷移を保持。 |
| AAC-001 | `src/app/tests.rs` | validate_row_deletion(&self)後のmodel/undo型保証だけ除去。参照拒否・理由/非参照成功を保持、widget削除全体を試したとの過大説明を訂正。 |
| AAS-001 | `src/app/tests.rs` | RC柱の未使用手計算値とFA/FD照合済みという過大説明だけ除去。梁表照合、柱固定FD、全入力/公差・独立整合未確認を保持。 |
| AAS-002 | `src/app/tests.rs` | THがstaleを解除しないという旧説明だけ除去。後続静的解析・保存入力・全assertは維持。 |

## 境界と未確認

AS-002はlocalな同条件filterの件数をUI保証の代用にせず、製品描画のcontrolを4入力で確認する。A-019のmodal存在は全結果bytes比較だけでは包含できないため残す。AA-004のJ-edgeと別試験のJ1の入力差は残す。グリッドの事前検証だけをwidget実削除の原子性保証へ拡張しない。

半径Noneと明示0、Add/Edit/SetRadius/Catalog拒否・原子性の対象外試験は変更しない。RC柱の固定FD回帰をFA/FD算定の独立照合や設計採用根拠と扱わず、#489/#1162のnot_plannedは再開しない。#1160/#1161の調査・計算是正は本整理に含めない。構造Validation・原典照合・安全側判定を追加したとは扱わない。

source追加・削除はなく対象在庫839を維持する。Issue1059全体は、残候補の反映と全台帳・最新版コード・PR履歴に対する別freshの両skill最終確認まで未完了である。

## 検証

同じ上記基点でApp default library/integrationは584成功・失敗0・既存ignored2、GUIは1316成功・失敗0・既存ignored4。初回GUI後のfixture内UUID生成順の維持は、最終GUI AI操作群28件とdefault全試験で確認した。不要import除去とClippyが指摘した同一式の末尾直接return以外に検証中のsource追加変更はない。

両構成all-target Clippy（`-D warnings`）、両構成strict Rustdoc（`RUSTDOCFLAGS=-D warnings`）、fmt、専用target check-docs（141/863/429）・check-deps（46）、mdBook、V&Vローカルリンク503件が成功。共有target、dev/test debug=0、stableを使用した。

```powershell
cargo +stable test -p sepika-app --lib --tests
cargo +stable test -p sepika-app --features gui --lib --tests
cargo +stable test -p sepika-app --features gui --lib app::study::ai::operations::tests -- --test-threads=1
cargo +stable clippy -p sepika-app --all-targets -- -D warnings
cargo +stable clippy -p sepika-app --features gui --all-targets -- -D warnings
cargo +stable doc -p sepika-app --no-deps
cargo +stable doc -p sepika-app --features gui --no-deps
cargo +stable fmt --all --check
cargo +stable run -p xtask -- check-docs
cargo +stable run -p xtask -- check-deps
mdbook build --dest-dir <検証用出力先>
```

実GUI手動操作と既存ignored計測を実施したとは扱わない。新しいValidationや未照合のRC柱算定を検証済みとはしない。
