# STB断面系列別ID（Issue #571）

## 記録の状態と対象

2026-10-18、基準main `cf2c4ec9597ce8659f0b9e60b6e95299c1e70225` からの読込限定変更を対象とする。
採用namespace・具体的断面系列・正規化した元数値IDによる定義/部材/強度の対応と、内部モデル/OVIKA保持を検証する。
Windowsローカル検証は、初回統合IO191成功/36失敗・App自動受入8成功/1失敗、第2統合IO225成功/2失敗・App9成功、関連feature回帰1321成功/1失敗を経て、最終ローカルでfeature1322成功/0失敗・IO default227成功/0失敗となった。異なるWIP snapshotの結果を分け、初期失敗を取り消さない。今回の文書最終修正後の再検査とfreshレビュー、親監査・最終head全CI・mergeは未確認であり、元5Done全体は完了認定しない。
最新の開発状態は[Issue #571](https://github.com/hrntsm/SEPIKA/issues/571)を正とする。

製品仕様は[ST-Bridge形式](../../docs/model_io/02_ST-Bridge_形式.md#断面系列と数値id)と[系列対応表](../../docs/model_io/03_ST-Bridge_要素別変換状況.md#断面参照の系列対応)、判断は[ADR0036](../adr/0036-stbridge-section-use-explicit-mapping.md#入力識別の判断issue-571)、語彙は[CONTEXT](../../CONTEXT.md#断面と材料)を正本とする。

## 元5完了条件との対応

| 完了条件 | 観測と現時点の証拠 | 未確認 |
|---|---|---|
| 独立入力→公開import→OVIKAでC1→30、G1→24を直接assertし、順序/内部再採番でも保持 | series12成功。各部材の参照/Fc、同系列内順序変更、実SectionId/ElemId再採番と実OVIKA保存/読込を観測。最終feature全体も成功 | 親の全Done監査 |
| 同Column_RC:1/:01重複をXSD/アプリで別確認し、Column_RC欠落をBeam_RC:1へ結ばない | series12/boundary6成功。固定XSDの両重複違反と、要求系列欠落のschema適合/アプリ拒否を分離。最終ローカル再検証成功 | 親監査・最終head全CI |
| Column/Post・Girder/Beam共有、RC/S/SRC/CFT・Slab_RC/Deck同ID、未対応Precast、元ID101/701、同名/同形状/異Fc | series12/boundary6が全対応部材系列とmissing/invalid/unsupported kind、全19系列1/01重複を観測。最終feature全体も成功 | 親監査・最終head全CI |
| schema適合と内部の意味対応を別確認し、両公開入口/GUI失敗時原子性・ID付き診断・docs入力制限 | namespace9、App自動受入9、IO default227成功。現5sourceの独立XSD124派生は期待外0。最終ローカルのClippy両構成・check-docs/mdBook成功 | 本文書修正後の文書再検査・freshレビュー。手動GUIは未実施 |
| #524共通手順の独立レビュー・親監査・最終head全適用CI・通常merge | namespace/key、組立/強度、full_model差分のfresh独立レビューは必須指摘なし。文書レビューのADR統合条件1件を修正 | 修正後のfresh文書レビュー・親監査・最終head全CI・通常merge/main |

元5Doneの一部を省略した完了認定は行わない。強度優先順位member > section > source story > common、原階/指定節点、配筋、材料、任意物性、通常参照不正の原子性の既存契約も維持対象である。

## 固定標準の根拠

[公式2.0.2 XSD配布](https://www.building-smart.or.jp/wp-content/uploads/2026/04/ST-Bridge_v202.zip)の原本SHA-256は `56cc1b80062c2385c15f8ab0745f4e96ac5b9400a4743307031e813d0253a1fa`。
先頭ASCII `o;?` の3 bytesだけを除いた宣言無変更の検証派生 `ST-Bridge_v202-normalized.xsd` のSHA-256は `d854bf431d395ace4a7917a63ebed431d251e5bb2fbf7fbea26e9e195fdfa2b6`。
今回の文書作業でも派生のhashと952–1027行の19個の具体系列key、XSD全文のkeyref不在を静的確認した。

19系列はColumn RC/S/SRC/CFT、Beam RC/S/SRC、Brace S、Slab RC/Deck/Precast、Wall RC、Foundation RC、Pile RC/S/Product、Open RC、Parapet RC、Undefinedである。
同系列の`1`と`01`は同数値として重複し、別系列の同数値IDは独立する。keyrefがないため、要求系列欠落の意味検査はアプリ側にも必要となる。
数値の既存u32字句・範囲契約とXSDのpositiveInteger適合は別であり、この変更によって0や空白等の既存字句境界を独断で変更しない。

[公式仕様説明書2022-06-15配布](https://www.building-smart.or.jp/wp-content/uploads/2022/06/ST-Bridge_XML%E4%BB%95%E6%A7%98%E8%AA%AC%E6%98%8E%E6%9B%B8%EF%BC%88ver.2.0.2%EF%BC%89_20220615.pdf)は本文64頁§5.2.1補足10、68頁§5.3.1、73頁§5.4.1補足16、75頁§5.5.1、77頁のBrace注記を根拠とする。
事前調査はPDF76/80/85/87/89頁を描画して具体系列、Post/ColumnとBeam/Girderの共有、Braceの当面Sのみを確認した記録である。本文headerの2020/12/16 Revision2と配布URLの日付を区別する。
Slab/WallはXSDの種別宣言も照合対象とする。これらは資料根拠であり、今回の実装成功や全XSD対応の証拠ではない。

## 独立入力と予定観測

自らのexportから生成しない入力を用いる。節点10=(0,0,0)、20=(0,0,3000)、30=(6000,0,3000) [mm]。
柱C1は10→20、RC、Column_RC:1、500×500 mm、Fc30。梁G1は20→30、RC、Beam_RC:1、300×600 mm、Fc24。部材Fcは双方省略する。
同じ元IDで定義した断面から、C1とG1がそれぞれ30/24 N/mm²を採ることを直接確認する。

事前独立XSD検証（lxml 6.1.1/libxml2 2.14.6）の `xsd-proof.json` を今回静的に読み合わせた。基本文書は適合、同系列`1`追加/`01`追加はkey重複違反、Column_RCだけを除いた文書は適合である。
初回の重複派生には要素順違反もあったため、同系列要素を隣接配置した派生でkey重複を分けて確認した記録を保持する。
この事前記録と、下記の今回のXSD独立検証/公開import実行結果は別の証拠である。

| 観測群 | 入力・期待結果 | 今回実行 |
|---|---|---|
| 意味保持 | 基本、定義/部材順序変更、同名同形状異Fc、ID101/701、内部再採番、OVIKA保存/再読込で各部材の断面/強度対応を保持 | series12成功 |
| 系列境界 | 柱/間柱と大梁/小梁共有、RC/S/SRC/CFT別系列同ID、Slab_RC/Deck同ID、Wall RC、未対応Precast/Undefined/Brace RC・SRCを理由付きで区別 | series12/boundary6成功 |
| 不正入力 | 同系列重複、要求系列欠落、他系列だけ同ID、kind_structure欠落/不正/未対応、ID不正で全体拒否し、元部材ID/要求系列/id_sectionを診断 | series12/boundary6・既存external_ids8成功 |
| namespace | 正式URIの既定/接頭辞を同等に扱い、foreign/no-namespace/未束縛要素・属性prefixを拒否。属性QNameを短縮せず、標準StbExtensions配下をモデル/強度へ混入させない | namespace9成功 |
| 状態と回帰 | import_stbridge / import_stbridge_with_report、App失敗時のモデル・Undo・結果世代不変、強度優先順位、原階/指定節点、配筋、native任意物性/OVIKAの保持 | App自動9・IO default227成功。最終feature1322成功。途中1失敗は下記で分離 |

系列の順序変更はXSD sequenceを保つ同family定義内/同member tag内とし、逆family順を正常入力へ固定しない。namespace無しの既存fixture移行57 root/7 filesは数値・強度等の期待を正式URIへ移した作業であり、全fixtureのXSD適合を保証しない。

## 実行証拠と未確認範囲

| 検査 | 確認内容 | 状態 |
|---|---|---|
| 固定XSDの静的確認 | 派生SHA-256、19具体系列key、keyref無し | 文書担当が今回確認 |
| 独立XSD事前証拠の読合せ | `xsd-proof.json` の基本/重複1/重複01/要求系列欠落を区別 | 文書担当が今回確認。検証器の今回再実行ではない |
| 初回統合 | `cargo check -p sepika-io --locked` | exit0 |
| 初回統合IO | `cargo test -p sepika-io --locked --no-fail-fast` | exit101、191成功/36失敗/既存ignored2 |
| 初回統合App | `cargo test -p sepika-app --features gui --locked --lib reference_identity_acceptance_tests` | exit101、8成功/1失敗 |
| 初回fmt | `cargo fmt --all -- --check` | exit1、5箇所差分 |
| 第2統合IO | 上記IOコマンド | exit101、225成功/2失敗/既存ignored2 |
| 第2統合App/fmt | 上記App/fmtコマンド | ともにexit0、App9成功（830 filtered） |
| 関連feature回帰 | `cargo test -p sepika-mcp -p sepika-app -p sepika-io --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --locked --no-fail-fast` | exit101、1321成功/1失敗/既存ignored3。App lib839成功、full_model36成功/1失敗 |
| 後続IO default | 上記IOコマンド | exit0、227成功/既存ignored2 |
| GUI構成/fmt | `cargo check -p sepika-app --features sepika-app/gui --locked`、上記fmtコマンド | ともにexit0 |
| 現5source固定XSD | 無変形抽出した124派生XMLを.NET XmlSchemaSetで独立検証 | 適合48/意図した不適合76/期待外0、6RC const INPUTは全適合。5source前後hash差分0 |
| fresh独立レビュー | namespace/key、組立/強度の2範囲 | 必須指摘なし。Cargo未実行の静的監査と6RC独立XSD確認であり、試験結果/最終ゲートの代用ではない |
| 最終ローカルfeature/IO | 上記feature/IOコマンドを再実行 | ともにexit0。feature37 suite・1322成功/0失敗/既存ignored3、IO12 suite・227成功/0失敗/既存ignored2 |
| 最終ローカル静的解析 | `cargo clippy -p sepika-io --all-targets --locked -- -D warnings` と3クレート同feature構成の `cargo clippy --all-targets --locked -- -D warnings`、fmt | すべてexit0。事前rustup更新成功、stable1.99.1 / Clippy0.1.99 |
| 最終ローカル文書 | `cargo run -p xtask -- check-docs`、`mdbook build` | ともにexit0。ただし本記録追記とADR訂正より前のsnapshot |
| full_model直接実行/独立レビュー | 修正対象1試験と符号+floorによる一意識別・4断面の観測仕様 | 直接1成功、freshレビュー必須指摘なし。最終featureでもfull_model37成功 |
| 残る確認 | 今回文書修正後の文書再検査・fresh文書レビュー、親監査・最終head全CI・通常merge/main | 未確認 |

実ログはTEMPの `sepika571-validation/{first-integration,second-integration,feature-regression}/report.md` と `current-schema-final/{report.txt,results.json}` を文書担当が読み合わせた。初回/第2統合/feature回帰は各1161入力の実行前後hashが一致し、検証中の入力変更と実行結果を混ぜていない。異なる段階どうしのsnapshot同一性を主張するものではない。

最終ローカルの `final-local/{report.json,summary.json}` は全9コマンドexit0、全18 snapshotで1161入力が初期hashと一致した。初回rustupのsandbox書込み失敗後、限定昇格で更新を完了しており、利用者判断待ちは残っていない。この最終ローカル実行も、以後の文書変更や将来の最終head CIの成功を認定する証拠ではない。

初回の新3targetは23成功/4失敗だった。要求系列欠落の診断語句、定義無しUNDEFINEDと定義済未対応の区別、namespace alias fixtureの置換がapp_versionへ作用した未束縛prefix、braceのmember強度metadata観測等を補正した。既存失敗を未変更baselineでも起きる問題とは認定せず、欠落kindのproduction救済や新skip/ignoreは追加していない。

6RC既存試験はSN400B主材料とRC形状の不整合fixtureがkind=S出力へ依存した限定原因であり、全RC writerの不具合と断定しない。元writer assertと数値/形状/材料の期待を保持し、RC入力復元は独立正式XML、内部完全保存は実OVIKAへ分離した。export文字列の書換えwrapperや出力実装の修正は追加していない。

第2統合の残り2失敗は、新追加assertがsecondary-only Beamにも主架構用途Girderを要求したことと、kind=RCを正しく明示した未割当壁のFc未指定警告を旧期待リストが含めなかったことである。前段の別family統合期待の改訂とは分ける。補正後のIO default227成功で解消を確認した。

関連feature回帰の残り1失敗は `full_model::original_fixture_retains_unresolved_positions_and_analysis_derivative_is_explicit` の固定断面添字に依存した期待だった。`sections[25]`/`[8,16,24]`を符号とfloorの存在・一意性をassertする確認へ変更し、直接1試験と最終feature全体が成功した。B1/FB1/FGY1/FGX1の4断面について形状寸法・配筋・原coverと明示した派生cover、診断、全断面/材料/部材/荷重の比較を維持し、fresh独立レビューも必須指摘なし。productionや元fixtureは変更していない。

今回の独立XSD124派生では、空の19系列38重複は内容不足を含む診断境界であり、完全なXSD正例ではない。参照欠落・未対応kind・内部u32上限超はXSD適合しうる。防御的extension入力や余分なqualified属性のアプリ受理も、schema適合とは分ける。初回のschema派生に残った要素順/属性不足を補正した最終採取で期待外0となり、6RC INPUTのXML hashは前回採取と全6件一致した。

過去WIPでのcargo check成功・限定5試験成功・IO122成功/50失敗/1ignoredは別実装の履歴であり、今回の証拠にしない。既存の「系列同IDを拒否する」試験も本件の成功実装を示さない。
全標準要素/全XSD/全shape対応、外部ソフトとの完全相互運用、手動native GUI、実験Validationを確認済みとしない。
本件は読込と内部/OVIKA保持のみであり、出力写像修正・自己export再importの成立・外部断面ID永続保存・新しい物理形状/近似は対象外とする。既存出力と開発中表示の契約は変更しない。
