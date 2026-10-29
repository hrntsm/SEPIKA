# 本案から分離した断面案検討の確認（Issue #979）

## 対象と証拠の区別

2026-10-29に、本案・基準案・比較案の編集先、計算開始時入力と完了結果の帰属、固定数量根拠、OVIKA保存・再読込を有限な専用入力で確認した。状態は🔶（限定Verification）。一般の構造設計適合、検定合格、実物に対するValidationの証明にはしない。

Windows実機の観測は実機担当者から受領した操作記録に基づく。下記のRust回帰・静的解析・文書検査は実装担当者の実行結果であり、実機操作の代用として扱わない。実装はmain `e62b56cf` を統合したPR #997の作業差分。記録作成時は未コミットで、最終HEADのCI成功は予記しない。

実機本確認は数量表の集計元ラベル修正前に行った。固定の「現在の本案」表示を現在案名へ直した後の最終fresh `release`は修正必須事項なしで完了し、担当者からGUI検討12件・根拠16件（fixture1件ignored）・非GUI検討6件の成功報告を受領した。最終ラベルの実機表示は、後述の物理Escapeによるユーザー停止で未確認である。

## 入力モデルと観測の許容差

- 通常入力は既存 `sepika_app::sample::portal_frame` の3部材。鉄骨大梁は部材2・断面S1（H400）で、比較用の有効な大梁断面S2（H-500x250x10x16）を追加し、S1と同じ材料を割り当てた。パネルゾーンは生成なし。
- 続行確認用入力は同じ架構に、節点0–1を結ぶ下梁・部材3を追加した4部材。割当領域を再生成し、壁版の未設定領域R0が実際に残る入力とした。警告を回避するための領域削除や近似は行わない。
- 対応する再現入口は `app::study::tests` の共通fixtureと `unset_regions_require_confirmation_on_calculation_copy_and_save_prompt_survives_switch`。専用出力コードは追跡ソースへ残さず、一時exampleは生成後に削除した。
- 所有先・部材断面・世代・履歴件数・入力識別は一致を観測する。数量は画面の表示桁（体積m³・鉄骨重量tの小数6桁）で往復一致を確認した。独立した手計算や規準例題との数量照合ではない。

ローカルartifactは追跡外の `target/issue-979-manual.ovika` と `target/issue-979-confirmation.ovika`。実機で保存した `target/issue-979-cua-output.ovika` は26,932 bytes、`target/issue-979-cua-recalc.ovika` は13,565 bytesだった。これらを恒久的な規準適合資料にはしない。

## Windows実機条件

SEPIKAの通常窓1282×831で操作した。OS設定・Settingsの取得・起動・変更は一切行っていない。表示倍率の条件は[#976の既存受入記録](UI可読性_Issue976_確認記録.md)を参照し、本件では変更していない。全DPI・複数モニタ・支援技術を網羅する試験ではない。

本確認後にPID36116は正常終了した。stderrにはフォント情報のみで、panicは記録されなかった。

最終exeはPID18396で起動したが、最初の `sky.list_windows` が物理EscapeによるCUユーザー停止を返したため、数量ラベルの最終実機表示は確認できなかった。停止後のCUによる取得、アプリ入力、終了操作は行っていない。PID18396は未終了として記録する。主要な実機本確認の観測とは区別し、最終ラベルの確認済み扱いにはしない。

## 実機で確認した所有先と計算帰属

| 操作 | 観測 |
| --- | --- |
| 4部材入力で大梁2・S1を固定し、基準案・案A・案Bを作成・命名 | 基準案は読取専用。案Aと案Bは独立した比較案として一覧から再表示できた |
| 案AだけS2へ割当、Undo、Redo | S2世代1 → S1世代2 → S2世代3。案B・基準案・本案はS1で、Undoは空のまま |
| 案Aで対象外の部材0にS2を指定 | 拒否。部材0はS0、案Aの世代3も変わらなかった |
| 基準案を全ケース・全組合せで計算 | R0未設定壁版による影響の全文を確認して続行。Run1は基準案の開始世代0へ帰属し、警告本文も残った |
| 案AのS2・世代3で開始し、確認待ち中に基準再計算を要求 | 「別の解析実行中」として拒否し、Run1を維持した |
| 確認待ち中に案Aを表示し、S1・世代4へ再編集してから続行 | Run2は案Aの開始世代3へ帰属。編集中の案AはS1・世代4を維持した |
| Run2の開始時入力／準備後入力を再表示 | 開始時はS2・世代3、準備後はS2・世代4。いずれも読取専用で、編集中案Aへ読み替わらなかった |
| 全4部材の数量を確認 | Run2案Aは鋼材体積0.209888m³・鉄骨1.647624t、基準案／案Bは0.182960m³。案Bの固定根拠は1.436239tだった |
| Run2案Aの数量と未計算案Bの数量を明示保存 | 固定数量根拠が2件残った。解析未実行の案Bでも保存できた |
| 数量根拠から本案の材料入力へ移り、Fy235→236を確定 | 本案だけ世代1・Undoあり。保存数量入力のポップアップはFy235を保持した |
| 元の結果へ戻る | 案Aの読取専用ビュー・元の数量表・1.647624tへ復帰し、検討には本案変更を表示した |
| 案B表示中に別名保存 | 保存後は未保存印が消えた。保存対象は本案を含むプロジェクト全体だった |
| `issue-979-cua-output.ovika`を再読込 | 本案は世代1・Fy236、現在の案AはS1・世代4・Fy235。基準案＋2比較案、Run1＋Run2、固定数量根拠2件を保持し、Undo履歴は永続化しなかった |
| 固定数量根拠とRun2の開始時入力を再表示 | 数量1.647624tと入力識別が一致し、開始時S2も読取専用で復元した |

R0の確認では、未設定の領域が床荷重・躯体自重と壁の剛性に入らず、応力・変形を過小評価し得る旨の注意を確認した。警告の内容を解消したことや、そのモデルの適合を確認したものではない。

既存の検定表ではEYの12位置が未検定だった。これは対応範囲の表示確認であり、架構の合格確認にはしない。Design根拠A/B等の実行識別・保存入力・表示値の契約は、次節の直接回帰と独立レビューの証拠に分ける。

## 続行拒否と別入力による再計算の区別

追加のR0付き再計算で「続行」を押す操作は、automatic approval reviewが危険側結果に対する明示承認を必要とするとして拒否した。再試行、間接実行、入力を変更して同じ警告を回避する操作は行っていない。確認待ちのままCtrl+S、取消、再読込を行い、成功履歴はRun1・Run2だけでRun3はなく、自動再開もしなかった。拒否対象のモデルを適合確認済みと扱わない。

保存再読込後のAIなし再計算は、未設定領域のない別の3部材通常入力を新規プロジェクトとして開いて確認した。固定対象0・比較案0の基準だけの検討を作成し、`issue-979-cua-recalc.ovika`へ保存・再読込した後、明示した基準計算が警告の続行操作なしに成功してRun1を保持し、再保存した。これは拒否された4部材モデルの承認や再実行ではない。

## 途中入力の観測

本案の節点0・X欄へ途中値「-」を入力して切り替えると、既存のセル確定検証が明示的に拒否した。値「-」、節点0のXに有限数値が必要な理由、モデルを変更しなかった旨が本案のログに残った。座標0・世代0・Undo空を維持し、他案の入力として確定しなかった。

合成GUI／状態回帰は案ごとの途中ドラフトの分離保持を確認する。一方、実機のセル確定が走る経路では上記の明示検証がある。すべての途中文字列が、あらゆる切替後も文字列のまま残るという無条件の保証にはしない。

## 自動回帰と要素からの証拠索引

| 対象 | 直接の観測・回帰 |
| --- | --- |
| 基準＋2比較案・限定編集・Undo・途中ドラフト分離 | `proposals_isolate_input_undo_and_incomplete_drafts` |
| 計算中の再編集・二重要求・全体派生値・保存参照 | `calculation_retains_start_input_after_edit_and_roundtrip_resolves_history` |
| 準備警告本文と診断の保持 | `all_preparation_warnings_and_diagnostics_survive_completion_roundtrip_and_history_display` |
| 元／別保持Design根拠A/B、静的結果、入力popup、保存往復 | `saved_design_references_retain_only_their_recomputed_evidence_across_selection_save_and_reload`、`result_evidence::tests`の`issue979_`直接回帰 |
| 契約欠落・旧・未来／生成入力欠落・不一致／未対応パネル | `retained_result_views_apply_contract_input_and_panel_gates_without_changing_saved_evidence`（本体／追加根拠×12条件、原本不変） |
| 比較案ゼロ・未計算数量根拠 | `quantity_only_study_roundtrip_keeps_fixed_source_without_analysis`、`result_evidence::tests`の未計算基準・比較の直接回帰 |
| 本案／基準／比較／開始時／準備後の表示名・描画非変更 | `workspace_search_and_study_headers_name_the_same_active_input_without_editing_it`（通常図表・検討窓・検索元・数量集計元） |

ラベル最終修正前の確定ソースでは、`cargo test -p sepika-app --features gui --lib --locked --quiet`が1102件成功・fixture生成1件ignored、非GUIのapp441・edit217・IO175件成功（IO1件ignored）。GUI／非GUIの4クレートall-targets clippy、fmt、check-docs、mdBook、exe buildも成功した。独立fresh `integrated_final`は修正必須なしで、担当者から非GUI検討6件・GUI検討12件・根拠16件（fixture1件ignored）の成功報告を受領した。数量ラベル最終修正後は、次の実行結果を確認した。計算・保存形式・編集制限は変更せず、無関係な全件テストの再実行は追加しない。

| 実行コマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-app --features gui --lib workspace_search_and_study_headers --locked --quiet` | 1件成功。5Locationの通常図表・検討窓・検索元・数量集計元と、案識別・入力世代の一致、モデル・結果・世代・Undo非変更 |
| `rustup update stable` / `cargo clippy --version` | stable変更なし、rustc 1.99.1 / clippy 0.1.99 |
| `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` | 成功 |

`cargo run -p xtask -- check-docs`（125章・676リンク・722実装参照）、`mdbook build`、`git diff --check`、`git diff --cached --check`も成功した。`cargo build -p sepika-app --features gui --bin sepika --locked`は成功し、最終exeは `target/debug/sepika.exe`（52,137,984 bytes、2026-10-29 12:39:44 JST生成）。最終fresh `release`は完了した。数量ラベルの最終実機表示は物理Escapeによる停止で未確認のため、PR #997はDraftを維持し、最終HEADのCIは後続ゲートとして区別する。

## 未確認・対象外

全モデル・全断面・全ケース・全入力方法・全画面条件の網羅、独立した構造計算式や数量の数値照合、実物Validation、専門家サインオフは未実施。本案への採用・外部AI通信・接続追加は本Issueの確認範囲外で、それぞれ後続Issueの証拠を要する。最終freshは完了したが、最終数量ラベルの実機表示は物理Escapeによる停止で未確認。PR #997はDraftを維持し、最終HEADのCIは後続とする。

実機で別途見つかった材料名編集が元へ戻る現象は[Issue #998](https://github.com/hrntsm/SEPIKA/issues/998)で追跡し、本件の表示・保存修正へ混ぜない。
