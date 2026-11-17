# 鉄骨柱自動Kの適用停止と直接指定の検証（#1398）

## 対象と独立期待

[Issue #1398](https://github.com/hrntsm/SEPIKA/issues/1398)の理想ピン柱脚・剛接柱頭の1層1スパン平面ポータルを対象とする。柱・梁長3000mm、2柱1梁、角半径0の溶接箱400×400×10mm、A=15600mm²、Iy=Iz=395720000mm⁴、E=205000N/mm²、F=235N/mm²。柱頭各2215200Nの鉛直下向き荷重で、一次弾性解析は柱軸力−2215200N、他断面力ほぼ0となる。

対称swayの柱脚曲げ0・柱頭横せん断0と梁の同向回転抵抗から、独立境界釣合いはx tan(x)=6/Gt、Gt=1、0<x<π/2となり、K=2.327876759160841、強軸有効長6983.630277482522mmを得る。これは実装の柱脚G=10式の再計算ではない。弱軸は3000mmを直接指定する。

同じ採用圧縮式で全荷重の軸力曲げ成分比1.014743328291276、両柱3位置の6結果はNG。独立長を保持して荷重のみ半減すると同比0.507371664145638、6結果は有限OKとなる。圧縮式そのものの規準適合、設計荷重での弾性座屈発生は検証対象としない。

## 検証する契約

- 通常S柱は両軸直接指定のいずれかが不足すれば対象柱・不足軸・理由付き未検定。支持・梁を剛にしただけで層内軸力分布・同時座屈成立としない。
- 非有限・負の直接値を自動Kまたは部材長へ補完しない。モデル直接入力0は明示的な座屈無視の既存契約を保持する。
- GUIの解析結果・保存と再読込、MCPの単独ケース・保存組合せは、未検定理由を保ち全体成功へ変換しない。
- 検定契約56の保存結果は失効し、解析結果を保持して新契約57で再検定できる。
- primary材がSteelのCftBoxは通常S柱に分類せず、既存の自動K長による6位置の比を保持する。これはCFT係数法の安全性を承認する試験ではない。

共有入力は`crates/sepika-design-jp/tests/fixtures/steel_column_k_model.rs`、公開共通検定は`tests/steel_column_k.rs`・`tests/steel_column_k_applicability.rs`、GUIは`sepika-app/src/app/steel_column_k_tests.rs`、MCPは`sepika-mcp/src/job/design_check.rs`の`column_k_mcp_keeps_both_columns_unchecked_for_case_and_combination`で検証する。

## 実行証跡

2026-11-18、#1399のmerge `7e228bd3`を取り込んだ `fix/issue-1398-column-k` の変更ソースを再コンパイルした。

| 実行 | 結果 |
| --- | --- |
| `cargo test -p sepika-design-jp --locked` | 単体691件＋統合53件、計744件成功。旧観測2試験は是正後契約へ更新 |
| `cargo test -p sepika-app --features gui --lib --locked` | 初回1228成功・3失敗・3ignore。柱未指定をCheckedと仮定した比較2件・荷重状態1件を更新し、該当3filterの再実行は各1件成功 |
| `cargo test -p sepika-app --features gui --lib column_k --locked` | 新GUI保存失効と既存primaryCFTの計2件成功 |
| `cargo test -p sepika-mcp --features mcp --lib --locked` | 初回135成功・2失敗。横補剛試験の全柱Checked前提を更新し、`lateral_bracing_mcp` filter再実行は2件成功 |
| `cargo test -p sepika-mcp --features mcp --lib column_k --locked` | 単独ケース・保存組合せの新契約1件成功 |

比較試験は、両案の未検定柱を数値差0へ変換せず、不足理由・不完全状態を保持することへ更新した。component破損試験は実際にCheckedの梁を対象に維持し、荷重状態の固定自由単柱は当該境界のK=2に対応する両軸6000mmを直接指定した。横補剛試験は梁のreferenceと検定を維持し、未検定が柱だけで全体成功しないことを確認した。任意の短い直接長による一括補完は行っていない。

- `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp --all-targets --locked -- -D warnings`：成功。Clippy 0.1.100（2026-11-09）。既存の未使用workspace依存schemarsのmanifest警告のみ。
- `cargo run -p xtask -- check-docs`：成功（summary142・links1034・impl refs506）。
- `mdbook build docs`：成功。生成鋼03 HTMLで両軸直接指定・0のAPIとGUIの区別・CFT境界を静的確認。数式追加はなく、通常S柱に適用しない旧数式の表示を削除した。
- `cargo fmt --all -- --check`・`git diff --check`：成功。
- 生成鋼03をheadless Chromeで描画し、変更したK節の本文・適用条件表・実装参照が欠けず表示されることを目視確認した。

## 独立レビュー後の危険断面位置の是正

新しいS柱の早期未検定処理が、生の内力評価位置をそのまま結果へ追加していた。フェースと節点芯が異なる部材では、直接指定時の危険断面選別と一致しなかったため、早期処理にも共通の危険断面選別を適用した。既存の他材種の係数供給・未対応条件は変更していない。

柱長3000mm、始端フェース150mm・終端フェース300mmを明示した公開求解で、内力に節点芯0/1が含まれることを確認した。その同じ内力に対し、未指定による未検定と独立直接長による検定の位置が、両柱とも[0.05, 0.5, 0.9]の3位置で一致する。節点芯はどちらにも含まれない。

実建物fixtureは実際に通常S柱40本を含む。未指定柱の生内力5位置による200行へ期待値を合わせず、フェースと中央の120未検定位置・他部材の225Checked位置を確認した。各未検定は対象の通常S柱・不足理由・フェース位置に対応し、snapshotの最大検定比等の数値は不変で、Checked/Skipped件数だけを225/120へ更新した。柱40本・大梁75本という既存のモデル説明は保持した。

リップ材の実検定・帳票試験は、柱の直接指定不足と梁の局部・ゆがみ・全体座屈未対応理由を区別する。混在描画試験は正常な梁断面と未検定柱を含む入力を用い、実際のChecked/Skipped位置数と表示が一致することを確認する。柱へ根拠のない短い長さを入れて混在状態を作らない。

- `cargo test -p sepika-design-jp --test steel_column_k --test steel_column_k_applicability --locked`：8件成功。
- `cargo test -p sepika-app --features gui --test full_model design_check_covers_every_member --locked`：1件成功。
- `cargo test -p sepika-app --features gui --test full_model snapshot_key_scalars --locked`：1件成功。
- `cargo test -p sepika-app --features gui --test lip_channel --locked`：7件成功。
- `cargo test -p sepika-app --features gui --lib column_k --locked`：2件成功。新GUI試験の柱位置数を各3位置に固定した。
- `cargo test -p sepika-mcp --features mcp --lib column_k --locked`：1件成功。両柱の計6位置・各3位置を厳密に確認した。
- `cargo test -p sepika-mcp --features mcp --lib lateral_bracing_mcp --locked`：既存の2件成功。
- 位置選別修正後の`cargo clippy -p sepika-design-jp -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp --all-targets --locked -- -D warnings`成功。`check-docs`（summary142・links1034・impl refs506）、`mdbook build docs`、fmt・diff確認も成功。

## 確認範囲と残る制約

状態🔶。今回のVerificationは、危険側反例を成功として採用しない適用制限と、直接入力経路の有限な契約確認に限る。全架構Validation・全規準適合、実験照合、手動GUI操作・MCP実通信は未確認。全条件に対応する新計算法は追加しない。RC・SRC・CFTへの既存係数供給は変更せず、CFTの設計採用域は[#1400](https://github.com/hrntsm/SEPIKA/issues/1400)で追跡する。

公開一次資料はAISC Engineering Journal [1967年p.43](https://ej.aisc.org/index.php/engj/article/download/72/71)（基礎G10から理想無摩擦ピンを除く）、[1995年p.65式(2)・Fig.4](https://ej.aisc.org/index.php/engj/article/download/641/640/640)（分母6・層内全柱同時座屈仮定）。公開資料の限定照合を、規準全体の承認へ拡張しない。
