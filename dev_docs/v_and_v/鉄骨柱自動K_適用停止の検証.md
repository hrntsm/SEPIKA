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

共有入力は`crates/sepika-design-jp/tests/fixtures/steel_column_k_model.rs`、公開共通検定は`tests/steel_column_k_applicability.rs`、GUIは`sepika-app/src/app/steel_column_k_tests.rs`、MCPは`sepika-mcp/src/job/design_check.rs`の`column_k_mcp_keeps_both_columns_unchecked_for_case_and_combination`で検証する。

## 実行証跡

2026-11-18、`fix/issue-1398-column-k`の変更ソースを再コンパイルし、`cargo test -p sepika-design-jp --locked`成功（単体691件と全統合テスト）。CFT保持試験追加後の`cargo test -p sepika-design-jp --test steel_column_k_applicability --locked`も成功。追加条件とGUI/MCP・静的解析の最終実行は後記する。

## 確認範囲と残る制約

状態🔶。今回のVerificationは、危険側反例を成功として採用しない適用制限と、直接入力経路の有限な契約確認に限る。全架構Validation・全規準適合、実験照合、手動GUI操作・MCP実通信は未確認。全条件に対応する新計算法は追加しない。RC・SRC・CFTへの既存係数供給は変更せず、CFTの設計採用域は[#1400](https://github.com/hrntsm/SEPIKA/issues/1400)で追跡する。

公開一次資料はAISC Engineering Journal [1967年p.43](https://ej.aisc.org/index.php/engj/article/download/72/71)（基礎G10から理想無摩擦ピンを除く）、[1995年p.65式(2)・Fig.4](https://ej.aisc.org/index.php/engj/article/download/641/640/640)（分母6・層内全柱同時座屈仮定）。公開資料の限定照合を、規準全体の承認へ拡張しない。
