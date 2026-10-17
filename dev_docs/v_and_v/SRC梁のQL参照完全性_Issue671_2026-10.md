# SRC梁のQL参照完全性（Issue #671）

## 対象と記録時点

対象は SRC 梁の地震時短期せん断で消費する QL、分担、評価位置、QD 選択と不足ゲートの Verification。
現在仕様の正本は[SRC/CFT断面検定](../../docs/calc_basis/06_一次設計/04_SRC_CFT_断面検定.md#src-梁の地震時設計用せん断と重力参照)、保存資格は[OVIKA](../../docs/model_io/01_プロジェクト形式_ovika.md#保存結果の計算契約と再実行)と[MCP](../../docs/mcp_server/04_解析ジョブと結果取得.md#保存成果の生成元と利用可否)に置く。

2026-10-16 の変更前 Source 観測は `e91060d208adaf7ac4c7c0c6abc06f397a3c769d` に対するもの。
恒久修正は #687/PR #688 後の `a1098c55f9ec82e9bed4cee8ab0c4f6bab4984c7` を基点とする。
変更後の直接検証、App公開actions/保存受入、および停止前に実行されたMCP受入の履歴証拠を照合した。
変更前Sourceの値と変更後の恒久回帰を節を分けて記録する。
Source 調査だけで [Issue #671](https://github.com/hrntsm/SEPIKA/issues/671) の原3完了条件を満たしたとはしない。

最新Scopeは既存不具合の修正に限り、MCP固有の既存不具合修正とMCP経由専用受入は今回対象外。
下記MCP受入は停止前の実測履歴として保持し、新たな完了条件には加えない。

## 参照資料と判断可能範囲

- 親取得の RESP-D 公開「SRC大梁断面算定」の退避 HTML を静的照合した。構造規定欄は QL の分担と sQD/rQD を掲載し、後段の SRC 規準方式とは区別する。これは公開ソフトウェア方式説明の一次発信であり、引用する構造規準・技術基準解説書本体を直接閲覧した証拠ではない。URL・版・頁は製品正本に記載。
- 添付 Ver.5.13 は PDF67/本文64 §2.5.10(3) の単純梁 Q0 を使う別方式。Source担当の実画像確認を受領したが、Docs担当による画像再視認はしていない。退避HTML SHA256は `B4208D051EF0F556B3F6FC313CEAB4EC9971DAFA16EC93D1199B77CFEF1CC1CE`、添付PDFは `DA18AFED284B37FB12FBE12765E6485E16484CA4DAA23DE7D54119785BA7EF1E`。
- 採用は既存の QL/share 方式の正常数値を維持し、実際に必要な参照へゲートを整合するもの。Q0 別方式への変更、新しい近似の承認ではない。RC #600 の符号・方向・端合成、SRC柱、CFT、終局需要 #565、地震重量 policy は対象外。
- 両端同一断面・同一設計軸力の近似、絶対値合成、規準本体の適用条件は一般安全性の確認に至っていない。欠落時の需要低下は下記 fixture で観測した範囲であり、全SRC・実建物の規準適合や安全側/危険側を一般判定しない。

## 変更前の実行と証拠

Source担当の `cargo test -p sepika-app --test probe_671_src_shear_contract --locked -- --nocapture --test-threads=1` は、初回2回が probe の型/一時借用コンパイル失敗（exit101）。run03 は1試験成功だが、Appは密度0の前処理拒否で数値未到達。正常密度を与えた run04 は1試験成功、独立値と選択手入力対照を加えた run05 は2試験成功・exit0。
MCP担当の `cargo test -p sepika-mcp --test probe_671_src_q0_ql --locked -- --nocapture --test-threads=1` は default feature、1試験/9公開job呼出成功・exit0。途中失敗なし。

両担当とも rustc1.99.1/cargo1.99.1、CARGO_TARGET_DIR/RUSTFLAGS/RUSTUP_TOOLCHAIN未設定。
Docs担当は最終report、独立期待値JSON、env、観測抜粋とraw末尾の件数/終了結果を照合した。全raw・全42/50証跡を独立再審査したものではなく、自身のCargo実行ではない。
一時probeは各担当が削除し、Source前後のproduction hash一致と本人所有handle終端を報告した。
最終probe SHA256は App `929E55CD017500FCDD41C816B700CA0FB873A68A4893EF75BEBBC6DE02F1F7A4`、MCP `D0D3963BD84A003C3A35716764CC8A2939C4C507FB0511D6C69BA5032EB035D2`。

証拠は TEMP の `sepika671-source-live-report.txt`、`sepika671-source-run01`～`run05` の env/probe/raw/exit、`sepika671-source-independent-values.json`、`sepika671-source-observations.txt`、`sepika671-mcp-source-report.txt`、`sepika671-mcp-source-run01-*` と9call入力/応答/補助対照に保存した。TEMPはローカル監査資料であり、恒久再実行可能なテストの代用ではない。

## 独立fixtureと数値差

L=6000 mm、i固定/j自由の片持ちSRC梁。SRC400×700、内蔵H500×200×9×14、上下3-D22、cover40、あばら筋D10@100の2脚、Fc24/SD345/SN400Bを明示する。材軸参照は(0,0,1)。
手入力Gはi端から1500 mmに全体-Z向き100000 N、Pは空、独立地震ケースはj端全体-Z向き10000 N。保存組合せG+P+Eと同じG/P係数を重力参照へ使う。
静力学による単純梁反力は75000/25000 N、Q0=75000 N。一方、片持ちFEM支持QL=100000 N、支持曲げ=-150×10⁶ N·mm、中央/先端QL=0。組合せ支持Q=110000 N・曲げ=-210×10⁶ N·mm。

App1は正当密度を持ち、選択DLに自動自重w=7 N/mm・総42 kNも含む。App2も正当密度だが自動DLは別非選択ケースで、保存組合せは明示手入力G+空P+独立Eのみ。自重込みApp1をApp2で代替しない。

| 対照 | Q0 [N] | 支持QL [N] | 組合せ支持Q [N] | 支持曲げ [N·mm] |
| --- | ---: | ---: | ---: | ---: |
| App1・自重込み | 96000 | 142000 | 152000 | -336000000 |
| App2・選択手入力のみ | 75000 | 100000 | 110000 | -210000000 |

独立幾何値は鋼I=409631402.666667 mm⁴、Z=1638525.610667 mm³、at=1140.398133 mm²、d=639 mm、j=559.125 mm、share=0.719866922030。採用中の同端近似による鋼/RCの曲げ項は128351.172836/75421.941140 N。
これは近似の数値照合であり、適用可否の新承認ではない。表・下記内訳は丸めて表示する。Source probeのQ0=75000 Nは絶対差10⁻⁸ N未満、支持QL=100000 NとApp1のQ0/QLは絶対差10⁻⁶ N未満のassertで照合した。内訳は公開detailの観測であり、全表示桁の独立assertを行ったとはしない。

## 変更前の直接checkerとrunner

| 条件 | 実観測 |
| --- | --- |
| QL=100000、Q=110000 N、pos0 | sQ=200337.9 N、Qd1のrQ=103435.2 N、Qd2/MinのrQ=0。sQA=576357.2/rQA=400523.9 N、せん断比0.347593221267 |
| Q0だけNone/0/75000/150000/NaNへ変更 | 全QD法で正常結果不変。Q0非消費の証拠であり、不正入力許容の根拠ではない |
| QL=0/50000/100000 N | sQ=128351.2/164344.5/200337.9 N。QL符号反転で同値 |
| 同位置QL欠落/long_at空 | 弾性分担sQ=79185.4/rQ=30814.6 NのCheckedへ戻った |
| 直接QL=NaN | sQ=NaNでもChecked(ok=true)。App入力到達の証拠ではない |
| runnerのQ0キー不足 | Qd1/Minは全3位置Skipped、Qd2はChecked |
| runnerの要求longに部材なし | 全3位置が弾性分担Checkedへ戻った |
| Q0の正常→不足→修復→不足 | Checked→Skipped→Checked→Skipped。古い正常値の再利用なし |

フェースi=300/j=400 mmでは評価位置0.05/0.5/0.933333333、内法5300 mmのみを検定した。支持側face正常のsQ=217289.9/rQ=113396.6 N、せん断比0.377005608081。同位置0.05のQLだけを除くと当該位置が弾性分担Checked・比0.137389378933へ低下し、中央/jfaceは構造規定方式を維持した。Appで同位置だけを欠落させた実験ではない。

## 変更前のAppとMCP実入口

Appの公開 `run_combination(0)` →解析→ `run_design_check`、公開resultsとCSVで確認した。画面操作/描画はしていない。App1正常支持のsQ=230572.3/rQ=115200.8 N、せん断比0.400050984132、全体比0.555340526230。App2正常の比は0.347593221267。
MCPは公開 `compute_job` の DesignCheck/保存組合せindex0を呼び、内部既定Min。Appの明示Qd1とは区別する。正常App1/App2ともn_checks3/n_skipped0、max_ratio=0.555340526230339/0.34759322126721853。
MCP summaryはsQ/rQ・Q0・重力QLを公開しないため、それらは同入力の別公開prepare/重力参照を補助対照とし、MCP取得値へ転用していない。

変更前のdir=[0;3]では、App1は選択自重だけが残りChecked、App2はQ0キーが消えて全位置Skippedだった。各入口で正常→dir0→修復→dir0を反復し、App2の不足時CSVにOKがないこと、MCPの対応する応答全文一致、全9callで元入力serde bytes不変を確認した。これはpure job反復の観測であり、ServerStateキャッシュ検証ではない。
別の有効ゼロ対照は、App2の手入力荷重自体を正規削除した空G/P（地震Eは残る）。補助QL=0/Q0map空、MCPは入口ErrではなくQ0不足で全3位置Skippedだった。

上記dir0でCheckedの履歴は #687 より前のもの。#687/PR #688 後の不正方向は共通入口Errであり、現在の期待値へ流用しない。正常な空重力・QL=0と不正方向を区別する。

## 変更後の直接検証

恒久変更は、SRC梁で消費しないQ0の不足ゲート除去、要求した長期参照の部材欠落と同位置QL欠落・非有限/評価位置非有限の位置全体Skipped、および検定契約版14への更新。
QL/shareの正常式、共有SRC柱helper、RC #600、CFT、ANALYSIS23とモデル/結果schemaは維持した。

| 恒久テスト（`crates/sepika-design-jp/src/srrc/tests.rs`） | 観測可能な契約 |
| --- | --- |
| `src_beam_qd_uses_ql_independently_of_q0_for_every_method` | Qd1/Qd2/Min全てでQ0のNone/0/75000/150000/NaNによらず正常値不変。QL符号反転で同値、有限QL=0も構造規定方式のChecked |
| `src_beam_qd_requires_finite_ql_at_the_evaluation_position` | 空/別位置/許容差外/非有限位置・QLを理由付きSkipped。差0.5×10⁻⁶の有限QL=0はChecked、消費しない他成分のNaNへ検査を拡張しない。ctx無しの通常長短期はChecked |
| `src_beam_public_runner_rejects_requested_gravity_gaps_without_reviving_results` | 全QD法で正常→要求long空→修復→別部材だけのlongをChecked→Skipped→Checked→Skipped。Q0有無で正常値不変、face同位置だけの欠落は当該位置のみSkipped |

独立値は上記Source fixture由来の定数を期待値とし、実装式をテストで再計算していない。
QL100000 N/Q110000 Nの正常支持せん断比0.347593221267、sQ200337.9 N、Qd1のrQ103435.2 NとQd2/MinのrQ0、QL0時のsQ128351.2 N/Qd1のrQ75421.9 Nを確認。
face300/400 mmの支持側比0.377005608081も維持した。人工的なQL欠落の直接checker/runner試験であり、Appで自然発生させた欠落の証拠ではない。

実装担当のrun01は所有4fileのrustfmt成功、run02は `cargo test -p sepika-design-jp src_beam_ --locked -- --nocapture` で19成功、run03は `cargo test -p sepika-core -p sepika-design-jp --locked` で1128成功/既存perf1 ignored/失敗0。
内訳はcore unit492+integration12、design-jp unit619+integration5。RC #600、SRC柱、CFTの既存同crate回帰を含む。
Windows PowerShell、rustc/cargo1.99.1、CARGO_TARGET_DIR/RUSTFLAGS/RUSTUP_TOOLCHAIN未設定、各run exit0。

証拠はTEMP `sepika671-coder-final-report.txt`、`sepika671-coder-run01`～`run03`のenv/raw/exit、`sepika671-coder-before.json`、`sepika671-coder-freeze-hashes.json`、親照合 `sepika671-root-coder-audit.json`。
beforeは実装前のhashであり、各run直前の最終ソースsnapshotではない。担当はrustfmt後からrun02/03間に編集なしと報告し、最終4file hashの親照合一致を確認した。最終snapshotのrun前後一致を取得済みとはしない。

## 変更後のAppと保存受入

恒久2試験は `crates/sepika-app/src/app/src_ql_acceptance_tests.rs` の `public_src_actions_check_real_ql_and_valid_empty_gravity_without_q0` と `src_ovika_design_contracts_keep_analysis23_and_require_explicit_recheck`。
fixture `crates/sepika-app/tests/fixtures/src_ql_contract.json` は正常密度のSourceApp2を用い、自動DLの42 kNは非選択ケースに保持する。
公開 `run_combination` と `run_design_check`、公開結果とCSVを通し、全QD法で以下を確認した。

| 選択手入力G | 組合せ支持Q [N] | 補助Q0 [N] | 公開長期actionの支持QL [N] | 検定/CSV |
| --- | ---: | ---: | ---: | --- |
| 正常100 kN | 110000 | 75000 | 100000 | 3位置Checked/OK、正常支持せん断比0.34759322126721853 |
| 正規削除した空G（E10 kNは保持） | 10000 | map空 | 0 | 3位置Checked/OK |

Q0は公開job helperによる補助値。QLは公開長期actionの観測値で、地震検定はAppが重力参照を再解析する。入力手荷重bytesは保持された。
空Gは正当な荷重削除であり、#687後に入口拒否されるdir0を使っていない。

OVIKAはDESIGN欠落/旧13/未来15/current14の4対照。全てANALYSIS23の静的入力資格・内力・入力bytesを保持し、非current検定はmember_checks/current_design/CSVのOKを復活させなかった。
表示切替・mark_fresh・再保存/再openでも非currentのまま、明示再検定で14へ回復し、その再保存/再open後も資格・内力を保持した。
検定版の不一致だけで解析23を失効させる試験ではない。既存CFT保存回帰の現行版assertも14へ追随した。

run1の `cargo test -p sepika-app --lib src_ --locked -- --nocapture` はテスト側PartialEq比較2箇所でコンパイル失敗・exit101。serialized bytes比較へ修正したrun2は6成功。
その後、再検定回復後の再保存/再open assertを追加してfreezeし、最終run3の `cargo test -p sepika-app --lib --locked` は346成功/失敗0・exit0。既存RC符号付きQ0/方向不正回帰とCFTを含む。
run4の `cargo clippy -p sepika-app --all-targets --locked -- -D warnings` もexit0だが、rustup update stable前の実行であり、正式なWindows横断静的解析を置換しない。
run1/2は途中snapshotで、最終4fileのfreezeと各run前後hashが一致するのはrun3/4。

証拠はTEMP `sepika671-app-final-report.txt`、run1～4のenv/raw/exit/pre/post、`sepika671-app-snapshot-audit.json`、親照合 `sepika671-root-app-audit.json`。各runはrustc/cargo1.99.1、主要3環境変数未設定。

## 停止前のMCP受入履歴

この節は最新Scopeで対象外となったMCP専用受入の既存実測を保持する。今回の追加受入や完了条件として要求しない。
専用integration/fixture、`persist.rs`の専用受入追加、MCP製品文書の説明追補はstash/backupへ保全して今回PRから除外した。共有契約版14への既存assert・名称と製品文書の版表示の最小追随は別扱いで維持する。下記専用テスト位置は停止前の保存済WIPを指し、現在のPRに含まれる恒久回帰とは区別する。既存MCP CIは従来通り維持する。
停止前の `crates/sepika-mcp/tests/src_ql_contract.rs` はSourceApp2と同じ正当fixtureで公開 `compute_job` の保存組合せindex0を正常→有効空G→修復→空Gの各4call、default/`mcp` featureの双方で実行した。
内部既定Minを通り、Appの明示QD法とは区別する。全call n_checks3/n_skipped0、qd_wired=true/gravity_failed0、all_members_checked_and_ok=true。
正常/修復は支持Q110000 N・max_ratio0.34759322126721853、空G/再空Gは支持Q10000.000000000007 N・max_ratio0.2226937858739397。
正常と修復、空Gと再空Gの応答JSON全文はそれぞれ一致し、全callで元入力serde bytes不変。
補助prepare/重力helperで正常Q0=75000 N/QL=100000 N、空GのQ0map空/3位置QL有限0を確認したが、MCP summaryの公開値ではない。

`persist.rs` の保存対照は、上記caseNoneの非永続jobとは別のcaseSome(1) fixture。
ANALYSIS23のNodalDisp ux3.5とDesignCheckのMemberForce n12.5を同一storeに保存し、DESIGN13/欠落/15/14でopen→result_get_json→syncを確認した。
検定はそれぞれunavailable/old_contract、unavailable/unknown_provenance、unavailable/unsupported_contract、current（n12.5）。unavailableにはrowsがなく、全対照で解析はcurrent/ux3.5を保持。
manifestと全Parquet bytesはopen/sync前後で不変。旧解析22/欠落/未来24が解析と従属検定を失効させる既存逆向き対照もdesign14で継続成功した。

| run | `cargo test -p sepika-mcp --locked` の対象（全て `-- --nocapture --test-threads=1`） | 成功数 |
| --- | --- | ---: |
| 01 | `--test src_ql_contract` | 1 |
| 02 | `--lib persist::tests::` | 6 |
| 03 | `--test member_load_direction` | 1 |
| 04 | `--features mcp --test src_ql_contract` | 1 |
| 05 | `--features mcp --lib persist::tests::` | 7 |
| 06 | `--features mcp --test member_load_direction` | 1 |

延べ17試験実行が成功、全6run exit0。構成間の重複を含み、独立仕様17件とは数えない。失敗runなし、rustc/cargo1.99.1、主要3環境変数未設定。
所有3fileは全6run前後と最終freezeのhashが一致、本人所有handleは全終端。
証拠はTEMP `sepika671-mcp-acceptance-final-report.txt`、run01～06のenv/raw/exit/pre/post、`sepika671-mcp-acceptance-runchecks.json`、`sepika671-mcp-acceptance-observations.json`。
停止前に完了した実行の結果であり、その停止状態や過去WIPの保持と今回の採用境界は区別する。

## 証拠照合と残る最終ゲート

Docs担当は各final report・親audit・runchecks/観測JSONと、上記恒久テスト本体のassertを静的照合した。自身のCargo実行や全rawの独立再審査ではない。
各担当の本人所有handle終端とCargo返却報告を確認した。
Windows横断・最新stableでの正式静的解析・文書コマンドは次節の確定証拠を照合した。fresh独立レビュー、親の最終差分監査、最終head全適用CI/docs build、通常merge/main更新は本追記時点未確認。
ローカル直接検証とApp受入の成功だけで原3Doneを完了認定しない。

## Windows横断と文書検証

2026-10-18 JSTのWindows検証担当による確定結果。`rustup update stable` は初回にsandbox内の `.rustup` 一時領域への書込みで失敗（run01 exit1）、権限昇格した再実行で成功（run02 exit0）した。自動承認レビューによる拒否ではない。
更新後はrustc1.99.1、Clippy0.1.99（ともに2026-10-13の `58cf8f9c3` 系）。以下の全コマンドはexit0。

| run | 検証コマンド | 結果 |
| --- | --- | --- |
| 05 | `cargo test --locked -p sepika-core -p sepika-design-jp -p sepika-app` | 1571成功/既存2ignored |
| 06 | `cargo test --locked -p sepika-mcp` | 既存107成功/0ignored。新MCP専用受入を含めない |
| 07 | `cargo test -p sepika-mcp -p sepika-app -p sepika-io --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --locked` | 1274成功/既存3ignored |
| 09 | `cargo test -p sepika-io --locked` | 191成功/既存2ignored |
| 10/11 | `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-app -p sepika-mcp -p sepika-io --all-targets --locked -- -D warnings` と同crate/targetの上記feature有効構成 | 両構成成功 |
| 12 | `cargo check -p sepika-app --features sepika-app/gui --locked` | 成功 |
| 13 | `RUSTDOCFLAGS=-D warnings` で `cargo doc -p sepika-core -p sepika-design-jp -p sepika-app -p sepika-mcp -p sepika-io --no-deps --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --locked` | 成功 |
| 14 | `cargo run -p xtask -- check-docs` | SUMMARY123/links575/実装参照634、成功 |
| 08 | `mdbook build` | 成功。local0.5.4、CI固定0.4.52の版差を保持 |
| 15 | `cargo fmt --all -- --check` | 最終コードsnapshotで再確認成功 |

構成間の重複を含むため、成功数を独立仕様数として合算しない。run05は全コードmanifestに差があるが、並行した親承認のMCP専用差分除外と既存契約版assert追随だけで、対象core/design/Appとその依存ソースは不変だった。
run06～15は最終コードSHAと一致。最終manifest SHA256は `0224DAFA1B4D5782A5723CA94CFFB88D6D6B854062F3CEB6B681F5E5801D3C96`。
mdBook前後は製品/開発文書・theme・book設定のhash不変を確認した。本節の開発文書追補はその実行後であり、追補そのものをmdBook実行済みとはしない。製品文書はfreezeを維持した。

証拠はTEMP `sepika671-windows-final-report.txt`、`sepika671-windows-final-summary.json`、`sepika671-windows-final-ownership.txt`、`sepika671-windows-final-artifact-index.sha256`と各runのenv/raw/result/前後SHA。
summaryはrun14まで、最終fmt run15はfinal reportで確認した。初回rustup失敗を保持し、ソース試験・静的解析・文書buildの失敗は報告されていない。
検証担当は全本人handleを終端し、ownhandles0/残存build process0でCargoを返却した。Docs担当は上記報告・要約・所有記録・証跡索引を照合したもので、自身のCargo実行ではない。
これらはローカルWindows検証であり、remote CIやmergeの成功を意味しない。

## 未確認と対象外

- SRC規準・技術基準解説書本体の直接照合、正負地震・上下非対称・異断面・適用ルートの一般確認、実建物/実験Validationは未実施。
- native GUI手操作、外部stdio/JSON-RPC transport、ServerState更新・キャッシュ、実App/MCPでlong部材/位置のみ欠落させる生成経路は未実測。公開actions/pure jobの成功へ包含しない。
- 直接ctx.seismic_qd=Noneの通常短期契約は維持する。時刻歴の要求QDとlong=Noneの静的候補は未実測の別境界であり、今回の静的入口成功へ包含しない。
- 内法・係数・材料等の全一般validatorやSRC柱/CFTの仕様変更は行わない。既存CFTのF235記述矛盾は #601 の追跡を維持し、本件へ混ぜない。

未確認は[未検証一覧](未検証一覧.md#src梁のql参照完全性671)で追跡し、開発完了の現在状態は元Issueと後続PRを正とする。
