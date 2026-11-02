# 鋼製間柱の独立検定（#1097）

状態: 🔶。有限なVerificationの記録であり、実建物・実験によるValidationではない。採用範囲・入力・計算方法は[製品文書](../../docs/calc_basis/06_一次設計/05_小梁・床の断面検定.md#鋼製間柱の独立検定)を正とする。

## 独立した期待値

両端ピン・L=3000 mm・横等分布w=2 N/mmは、端せん断wL/2=3000 N、中央曲げwL²/8=2250000 N mm。BOX-200×100×8（明示角R=0）はA=4544 mm²、Iy=23060138.666667 mm⁴、Iz=7578538.666667 mm⁴、E=205000 N/mm²。5wL⁴/(384EI)=0.4462086848252602 mm。

自重78.5e-6 N/mm³から線重量0.356704 N/mm、下端負担100%の中央軸力=-535.056 N。両軸座屈長3000 mmではλ=73.4594514、F=235 N/mm²から長期fc=114.0395563 N/mm²、ft=156.6666667 N/mm²。中央の軸曲げ比は0.06331187630168549。部材全体の最大比は軸力分布との組合せにより中央からずれ得るため、この中央値を部材最大へ読み替えない。

片持ちL=3000 mmの先端P=2000 Nは固定端曲げ6000000 N mm、端せん断2000 N。EI=205000×8000000 N mm²なら先端変位PL³/(3EI)=10.975609756097562 mm。二方向変位の合成は同一位置で行う。

## 現役の保証

- `sepika_load::floor::response::tests`: 単純支持・片持ち・端反転の力と変位。
- `sepika_design_jp::post::tests`: NとMの同位置照合、架け側反力の一度だけの計上・位置でのN跳び・支持面と断面方向の整合、圧縮/引張、負担率、向き、片持ち、長短期、比1前後、座屈長/横補剛の独立、半径不足と不正、限界未設定時の変位保持。
- `sepika_edit::secondary::post_design_tests`: 組合せ削除/Undoによる対象と長短期の保持、局部荷重の参照ケース削除ガードとID追従、所属あり/未割当間柱の削除時の条件除去とUndo/Redo。
- `sepika_edit::section_material::section::corner_radius_input_tests`: 角形鋼/CFTの追加/編集/半径設定/カタログでの未設定拒否、モデルとUndo/Redo不変、明示0と半径不要形状。
- `sepika_app::app::post_design_tests`: 保存再読込、入力編集後の失効、旧計算契約失効、本番egui入力と再検定のポインタイベント、断面入力の半径不足表示。

## 実行状態と境界

2026-11-03、初回実装時に当該worktreeで以下を実行した。共有targetは `C:\Users\hiroa\Desktop\ikkan\target`、Clippyは `0.1.99 (58cf8f9c35 2026-10-13)`。

| コマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-app --features gui --lib --locked --quiet` | 1199成功、失敗0、既存ignore3 |
| `cargo test -p sepika-core -p sepika-load -p sepika-design-jp -p sepika-edit -p sepika-job -p sepika-io -p sepika-mcp --features sepika-mcp/mcp --locked --quiet` | lib・統合・doc test合計2424成功、失敗0、既存ignore3 |
| `cargo clippy -p sepika-core -p sepika-load -p sepika-design-jp -p sepika-edit -p sepika-job -p sepika-io -p sepika-app -p sepika-mcp --all-targets --features sepika-app/gui,sepika-mcp/mcp --locked -- -D warnings` | 成功 |
| `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings` | 最終ポインタ試験の位置取得修正後も成功 |
| `cargo fmt --all -- --check` | 成功 |
| `rustfmt --check --edition 2021 crates/sepika-core/tests/support/steel_secondary_post_model.rs` | 成功 |
| `cargo run -p xtask -- check-docs` | 成功、summary141・links862・impl refs424 |
| `mdbook build` | 成功 |
| `git diff --check` | 成功 |

一括テストのクレート別内訳は次のとおり。統合等にはdoc testを含み、成功数だけを示す。

| クレート | lib成功 | 統合等成功 | ignore |
| --- | ---: | ---: | ---: |
| sepika-core | 593 | 45 | 1 |
| sepika-design-jp | 680 | 28 | 0 |
| sepika-edit | 219 | 29 | 0 |
| sepika-io | 182 | 67 | 2 |
| sepika-job | 100 | 91 | 0 |
| sepika-load | 225 | 21 | 0 |
| sepika-mcp（mcp feature） | 129 | 15 | 0 |

Windows実機ウィンドウの取得・手操作は未実施であり、本番eguiウィジェットへの合成ポインタイベントと区別する。全モデル・全荷重・全GUI画面条件、規準の専門家確認、層間追従・接合部・終局・実験Validationは未確認。


## PR #1120 の既定feature構成の是正

CIの `cargo test --workspace --locked` ではGUI featureが有効にならず、保存・失効の共通試験がGUI限定の編集メソッドを呼んでコンパイルエラーになった。共通試験の編集を、GUI・非GUIで利用できるUndoStack経由の編集と編集済み状態の更新へ置き換えた。保存再読込・入力不一致による失効・明示再検定・旧契約拒否の確認は維持し、本番GUIポインタイベントの試験は変更していない。計算・製品コードの変更はない。

修正後、同じ共有targetで次の構成を直接確認した。先行した他7クレートの2424成功と、その計算・保存・編集の対象ソースは変更していない。CI全workspace実行の成否はローカルの構成別確認とは区別する。

| コマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-app --locked --quiet` | 非GUI：lib481成功・既存ignore1、統合等104成功・既存ignore1。計585成功、失敗0、ignore2 |
| `cargo test -p sepika-app --features gui --lib --locked --quiet` | GUI：1199成功、失敗0、既存ignore3 |
| `cargo clippy -p sepika-app --all-targets --locked -- -D warnings` | 非GUIで成功 |
| `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings` | GUIで成功 |
| `cargo fmt --all -- --check` | 成功 |
| `git diff --check` | 成功 |
