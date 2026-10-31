# OVIKA参照ID整合性（Issue #678）

記録日: 2026-10-16。対象は [#678](https://github.com/hrntsm/SEPIKA/issues/678)、親は [#524](https://github.com/hrntsm/SEPIKA/issues/524)。
base は `7dcc54e3a2d78e5014c89514153521cff7cbe73f`、作業ブランチは `fix/678-ovika-reference-identity`。
Source freeze は 2026-10-15 16:13:54 UTC の未コミット差分で、コミット済み HEAD の検証とは区別する。
本記録は参照選択・拒否・保持の Verification であり、構造計算の数値精度や安全側／危険側の Validation ではない。
製品仕様は [OVIKA](../../docs/model_io/01_プロジェクト形式_ovika.md#未完成の入力と参照の整合性)、判断は [ADR 0059](../adr/0059-incomplete-model-reference-integrity.md) を正とする。

## 証拠と実施者

Source 担当が直接テストと独立コンテナ生成 probe を実行し、Docs 担当はソース・raw ログ・exit・JSON/CSV・freeze manifest を照合した。
Docs 担当自身による Cargo 実行や独立レビューではない。TEMP 証拠はローカルの引継ぎ資料であり、再現可能な恒久回帰は以下の repo 内テストに置く。

| 証拠 | 内容 |
| --- | --- |
| `sepika678-probe-handoff.md`、`sepika678-probe-results.json` | 修正前41ケース。head `b1a4119c` は今回 base と全ツリー一致 |
| `sepika678-source-handoff.md`、`sepika678-source-freeze.json` | 実装7パスの SHA256/Git blob、直接コマンドと終端 |
| `sepika678-source-direct-final.log/.exit` | 最終 core/io 直接テスト、exit 0 |
| `sepika678-after-probe-run.log/.exit`、`sepika678-after-probe-supplement.log/.exit` | 修正後39＋2ケース、各 exit 0 |
| `sepika678-after-probe-results.json/.csv`、`sepika678-after-probe-summary.json` | 全41件の個別値と集計 |
| `sepika678-acceptance-handoff.md`、`sepika678-acceptance-run2-raw.log/run2-exit.log`、`sepika678-acceptance-freeze.log` | 別担当の App 4件、exit 0、テスト SHA256/Git blob |

前後で probe 入力ソースの SHA256 は `b6989a4464a46389b1d25ec6842bfd85693b6d95fa8b7551ff6c9369ebf753f4` と同一。
修正後の生成ファイルは別ディレクトリへ置き、修正前のコンテナを上書きしていない。

## 修正前後の実観測

参照元は主架構、床領域の小梁、壁領域の間柱、未所属小梁、未所属間柱の5種。
二次部材の ID はすべて安定 `SecondaryMemberId(674)`。断面は非 RC の数値入力を基本とし、幾何や実建物の成立を認定する入力ではない。
主架構は2節点線材、二次部材は有限な Detached 端点を持つ。

| 入力 | 件数 | 修正前 save / 正hash load | 修正後 save / 正hash load |
| --- | --- | --- | --- |
| 5参照元 × 正規、未割当 `None` | 10 | 成功 | 成功 |
| 5参照元 × 範囲外同ID実在、欠落、位置不一致、重複2種 | 25 | 成功 | 構造参照エラーで拒否 |
| 空モデル | 1 | 成功 | 成功 |
| 主架構断面用途 `frame_use=None` | 1 | 成功 | 成功 |
| 参照先と異なる添字位置に RC 梁型を置いた間柱 | 1 | 誤った断面の用途エラーで拒否 | ID 位置不一致で拒否 |
| `ElemId(674)` の添字不一致 | 1 | 成功 | 拒否 |
| 全所属横断の二次安定 ID 重複 | 1 | 成功 | 拒否 |
| 未参照の非正規 `SectionId(7)` | 1 | 成功 | 拒否 |

修正後は正常12件が保存・正しい manifest/hash の読込をともに通過し、不整合29件はともに理由付きで拒否した。
不正読込は hash 破損による拒否ではなく、モデルを直接格納して正しい hash を付けた独立コンテナで観測した。
主架構の用途未設定は前後とも全 `Model.validate` だけが拒否する。IO の未完成受理から、GUI の「開く」の全検証通過を推定しない。

### 公開断面検索と用途検査

以下は主架構7形態の直接観測。断面名は同 ID の別実体を区別する入力上のラベルである。

| 形態 | `Model.section`（前後同じ） | 修正前 `element_section` | 修正後 `element_section` |
| --- | --- | --- | --- |
| 正規 | canonical | canonical | canonical |
| `None` | `None` | `None` | `None` |
| 範囲外同 ID 実在7 | fallback7 | `None` | fallback7 |
| 欠落7 | `None` | `None` | `None` |
| 位置不一致 ref1 | fallback1 | wrongslot9 | fallback1 |
| 重複、添字一致あり1 | canonical1（先頭ではない） | canonical1 | canonical1 |
| 重複、添字一致なし7 | first7 | `None` | first7 |

二次用途検査の wrong-slot 入力は `[id1 numeric1, id9 RC梁]` を ref1 の間柱から参照する。
修正前は無関係な id9 の RC 梁を選んで拒否したが、修正後は numeric1 を選んで用途検査は成功した。
その入力自体は構造参照検査が拒否し、保存を許可する理由にはしない。
RC 用途 validator の条件や二次 CFT 禁止を変更した観測ではない。

### IO の保持

- 全41件で保存呼出前後の借用モデルの named MessagePack バイト列が一致。
- 全41件で独立正hashコンテナの読込前後のファイルバイト列が一致。
- 正常12件でモデルの往復一致と、不透明な preparation/results/analysis_settings の3バイト列を保持。
- 保存拒否29件で既存保存ファイルのバイト列を保持し、元の空モデルを通常読込できた。

修正前は40成功がモデルと付帯3データを保持し、用途誤拒否1件は既存ファイルを保持した。
修正後 CSV の拒否行の `roundtrip=false` / `extras=false` は、候補が保存されず元の保存が残るという意味であり、既存成果の消失ではない。
IO の opaque bytes 保持から、成果の計算契約や現入力への利用可能性は認定しない。

## 恒久回帰と実行結果

Source の最終コマンドは `cargo test -p sepika-core -p sepika-io --locked`（exit 0）。
core は unit 490、integration 6＋4 が成功、外部検証用1件は ignored。
IO は unit 177 成功・1 ignored、integration 9＋1＋1＋2 が成功・外部 XSD 出力1件は ignored。両 crate の Doc tests も失敗0。

| テスト | 観測する契約 |
| --- | --- |
| `model::tests::element_section_uses_canonical_id_then_first_matching_id` | 7形態で同 ID 実体の優先順位、欠落・未割当 |
| `model::tests::secondary_purpose_selects_canonical_then_first_fallback_section_without_wrong_slot_rejection` | 二次用途の canonical 優先・先頭 fallback、wrong-slot 誤拒否解消 |
| `ovika::reference_tests::all_member_locations_reject_noncanonical_section_references_and_keep_incomplete_models` | 5参照元×7形態、未完成正常保存、安定 ID 674 と全所属横断重複拒否 |
| `ovika::reference_tests::indexed_ids_and_existing_reference_families_are_checked_before_persistence` | 正常対照と24変異、9種添字 ID、材料4役・節点/版/断面/一本部材の参照、元ファイル/モデル/正hash入力保持 |

実装位置は `crates/sepika-core/src/model/tests.rs`、`crates/sepika-io/src/ovika_reference_tests.rs`。
24変異は集合の全組合せや未網羅フィールドまでを検証する件数ではない。

最終成功前に4回の exit 101 があった。初回は既存 panel-zone fixture の3番目断面が既定 ID0のままで参照2と不一致、
次は新 IO テストの `unwrap_err` が返却型に Debug を要求するコンパイルエラー、3回目は既存非正規用途テストの拒否理由と新 fixture の残参照、
4回目は新 fixture の正常一本部材追加に伴う group 番号の期待不一致だった。
fixture の ID・参照・期待理由を新契約へ整合し、最終コマンドを再実行した。これらを成功に置き換えず、Source の各 raw/exit に残す。

## 検査範囲の静的照合と所有

`Model::validate_reference_ids` は既存全 validate の構造ブロックを抽出し、未参照実体を含む9種の添字 ID と、
要素→節点/断面、断面→材料4役、通り芯→節点、床/壁領域→節点/版、版→既存 helper が得る節点/断面、
二次部材→断面、一本部材→要素を対象にする。二次部材 ID は全所属横断で一意とし、添字一致は要求しない。
囲まれた版の境界が解決できず helper が空を返す場合もあり、幾何や割当完成度の検査とは別である。
割当領域の安定 ID、非連番、親 WallRegion のない有効ケースへ新条件は課していない。後述の App 受入では親なし Enclosed 壁版を IO 往復する正常対照も確認した。

Story 所属、Constraint/Mpc、荷重、生成記録等の未網羅は [#681](https://github.com/hrntsm/SEPIKA/issues/681) の独立調査対象。
根拠は `aggregate.rs` の `visit_node_ids` / `visit_story_ids` / `visit_elem_ids` と今回集合の差であり、実到達・数値影響は未確認。
二次 CFT 禁止は #539、RC 専用用途は #674、板 RC 参照は #677、STB 外部系列 ID は #571 が所有し、今回再実装しない。

## App 実状態の直接受入

別の acceptance 担当が `cargo test -p sepika-app --lib reference_identity_ --locked -- --nocapture` を実行し、
4 passed / 0 failed / 335 filtered out、exit 0。Docs 担当は handoff 全文、raw/exit、freeze、該当テストの assert を照合した。
対象は `crates/sepika-app/src/app/reference_identity_acceptance_tests.rs` の次の4件であり、ネイティブ GUI の操作や独立レビューではない。

| テストの `reference_identity_` 以降 | 観測 |
| --- | --- |
| `rejected_load_and_correct_hash_open_preserve_app_state_and_history` | `Elem0→Section678` を直接差し替えと正hashファイル開くで拒否。元モデル/実線形解析成果/準備計算の bytes、Modal生成入力、revision、Undo/Redo両stack、selection、dirty/path/stalenessを保持。入力ファイルも不変。拒否後の実Undo/Redoで節点X座標0→−100→−200 mmの履歴内容も確認 |
| `save_rejection_precedes_result_sync_and_preserves_existing_file` | 同じ不正参照を持つ App の保存を拒否し、既存ファイルと上記状態を保持。pushover窓口Qu11・Xslot22・Yslot33・表示Yの非空成果を置き、拒否後は成果bytes不変。その後に同期だけを実行するとQu33へ変化する対照で同期前拒否を確認 |
| `incomplete_models_save_and_load_without_gui_completion_requirements` | `Section=None` と `frame_use=None` を構造検査/直接App差し替え/App保存/IO読込で受理しモデルbytes保持。後者は全validateと実Appファイル開くが拒否し、空の元Appモデルを保持 |
| `wall_plate_without_parent_wall_region_keeps_stable_assignment_id` | #632対照。長方形4節点・支持部材を持つ有効Enclosed壁版、親WallRegionなし、非連番の割当領域安定ID632、quad境界成立を確認し、構造検査と通常IO往復を通過。GUI完成度受理は主張しない |

初回は新テストの `WallPlateId` / `WallPlateAssignmentRegionId` の import 漏れ2件で exit 101。
明示 import だけを追加して上記を再実行し、製品コードの追加変更はない。
テストの `rustfmt --check --edition 2021` も exit 0。freeze 時刻は 2026-10-15 16:15:43 UTC、
テスト blob は `bca52532d549c87844a6e527c3c0c1fdb1a21db6`、SHA256 は `85219905969350e6b101de108bb1bf14382692562cd3d7a279c459b3a86a5cde`。
未コミット共有 working tree に対する実行であり、将来の最終 head 全体の成功とはしない。

## 版判断と未検証

永続化フィールド・型・直列化形式、数式・定数・solver・RC 用途条件の非変更と、canonical 入力で同じ断面を選ぶことは静的に確認した。
不正旧保存は正しい hash でも `OvikaContents` の返却前に拒否し、付帯成果をアプリへ返さない。
親が製品差分と修正後 workspace 75 suite・3414 passed / 0 failed / 既存4 ignored を確認し、
schema 1・解析契約22・検定契約13の据置を採用した。正常数値解の独立比較・実験 Validation を実施したという意味ではない。

広域検証担当の `cargo test --workspace --locked --no-fail-fast` は最終 exit 0（`sepika678-validation-workspace-fix2.log/.exit` と summary）。
先行 workspace の2失敗は、通常保存で不正 Node ID を使う full_model fixture と、床追加後の境界要素に固定 ID1 を使う quantity fixture による。
同 Source Coder が前者を非RC断面の用途未設定へ、後者を現在の要素数に対応する ID へ最小修正した。
前者は IO成功後のGUI全validate拒否・元model/path/file保持、後者は元の8組合せと三節点拒否の assert を維持し、対象各1件は exit 0。
最終 workspace は full_model 37/37 の成功も含む。

GUI feature の先行 App unit は814成功/6失敗。共通座標編集 fixture がコピーした通り芯所属と独自4節点が不一致だったため、
Source が4軸の節点所属と読込成功 assert だけを整合した。対象 GUI 7件（813 filtered）は exit 0。
これらは製品検査を緩和する変更ではない。根拠は Source handoff 追補1〜3と `source-fix{1,2,3}-freeze.json`、
`source-fix-full-model-final` / `source-fix-quantity-final` / `source-fix-coordinate-final` の各 log/exit（いずれも `sepika678-` 接頭辞）。
その後の関連 feature 全体の再実行は29 suite・1262成功/0失敗/既存3 ignored、IO default は6 suite・190成功/0失敗/既存2 ignored、各 exit 0。
コマンドは `cargo test -p sepika-mcp -p sepika-app -p sepika-io --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --locked --no-fail-fast` と
`cargo test -p sepika-io --locked`。Docs は `sepika678-validation-features-final` / `sepika678-validation-io-default` の command/raw log/exit を照合した。
GUI の Parquet なし構成も `cargo check -p sepika-app --features sepika-app/gui --locked` が exit 0。
`sepika678-validation-handoff.md` と GUI command/raw/exit を照合した。関連構成の件数は重複するため独立ケース数として足し合わせない。
広検証終端時（2026-10-15 16:40:18 UTC）の `sepika678-validation-end.json` で、GUI fixture 修正後の Rust 12パスが検証前後で不変と固定されている。
workspace 成功後の追加差分は default で除外される GUI test helper のみで、workspace 全体の反復はしていない。

MCP transport、手動 GUI、OS 障害注入、全一般参照の網羅、解析数値精度・実験 Validation は未実施。
静的解析・文書検査、fresh 独立レビュー・最終親監査・最終 head 全適用 CI・通常 merge は本記録時点では未確認。
最終ゲートの最新状態は [#678 の原完了条件](https://github.com/hrntsm/SEPIKA/issues/678) と、そこから参照する関連 PR・CI に記録する。
ここでの未確認は実行時点の記録であり、現在も未完了であるという認定ではない。
原4 Done 全体の達成をこの Source 区切りだけで認定しない。
