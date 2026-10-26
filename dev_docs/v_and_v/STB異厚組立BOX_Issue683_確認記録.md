# STB異厚組立BOXの読込確認（Issue #683）

状態: 🔶。2026-10-26。STB 2.0.2の組立BOXから汎用断面モデルへの結合を対象とする。
[Issue #683](https://github.com/hrntsm/SEPIKA/issues/683)と[汎用形状の確認記録](異厚組立BOX_Issue831_確認記録.md)を参照。
入力条件と計算の現在仕様は[製品文書](../../docs/model_io/02_ST-Bridge_形式.md#組立boxの読み込み)を正とする。

## 独立期待値

外高さ300・外幅200・左右板厚9 mm。外形から内法長方形を差し引いた独立算術を用いる。
物理質量は鋼密度7850 kg/m³、充填部はFc36普通無筋コンクリート23 kN/m³、重力加速度9.80665 m/s²による。
鋼質量は鋼面積×0.00785、CFT質量は鋼質量＋充填面積×23/9806.65（kg/m）。

| 上下板厚 mm | 鋼面積 mm² | Iy mm⁴ | Iz mm⁴ | 充填面積 mm² | 鋼質量 kg/m | CFT質量 kg/m |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 6 | 7584 | 87700608 | 55314368 | 52416 | 59.5344 | 182.4681235447375 |
| 9 | 8676 | 109875852 | 58328652 | 51324 | 68.1066 | 188.47920430422213 |
| 12 | 9768 | 131127264 | 61342936 | 50232 | 76.6788 | 194.49028506370677 |

## 要素→テスト索引

| 対象 | 自動回帰 | 観測する契約 |
| --- | --- | --- |
| 通常鋼・CFTの両公開入口 | `crates/sepika-io/tests/stbridge_built_box.rs` | 6/9/12 mmの両板厚と上表のA/I/物理質量、両入口のモデル一致 |
| 近接異厚 | 同上 | 9と9.000000000000002を区別し、J未算定を維持 |
| 未算定境界 | 同上 | 異厚J・せん断の理由付きエラー、CFT等価剛性の拒否 |
| 同符号・同階・同用途の断面 | 同上 | 通常鋼・CFTのt2だけ異なる2断面、部材参照、A/B/t1/t2の取込済み報告、OVIKA再読込でのモデル一致 |
| 不正寸法・参照境界・同厚・圧延 | 同上 | 欠落/非数値/非有限/非正/内法不成立、未割当断面の検査と未参照ライブラリ不検査、従来の同厚性能と圧延形状 |
| Appの読込と失敗時原子性 | `crates/sepika-app/src/app/reference_identity_acceptance_tests.rs` | 6/9/12/近接異厚の正常置換、不正寸法でモデル・結果・履歴・保存先・入力/保存ファイル保持、GUI featureでは診断Text描画 |

## 実行結果と限界

2026-10-26、stable更新確認後のRust 1.99.1 / Clippy 0.1.99で、次をローカル実行した。

| コマンド | 結果 |
| --- | --- |
| `rustup update stable` / `cargo clippy --version` | stableは最新、Clippy 0.1.99を確認 |
| `cargo test -p sepika-io --locked` | 244成功・2 ignored・失敗0。対象の組立BOX試験7成功 |
| `cargo clippy -p sepika-io --all-targets --locked -- -D warnings` | 成功 |
| `cargo test -p sepika-app --lib built_box_ --locked` | 2成功 |
| `cargo test -p sepika-app --lib built_box_ --features gui --locked` | 2成功（診断Text描画を含む） |
| `cargo clippy -p sepika-app --lib --tests --features gui --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` / `git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | summary124・links654・impl refs697すべて成功 |
| `mdbook build` | 成功 |

初回の対象試験コンパイルで参照IDのOption展開漏れを修正した。
IO全試験の初回実行は、同符号の出力断面名を等しいとした試験1件が失敗した。
既存の競合名への`#2`付加規則に合わせて期待値を修正し、IO全試験を再実行して成功した。
修正後のfresh独立レビューは必須指摘なし。最終head CIは未確認。

今回の確認は読込と汎用形状・質量・保存への結合Verificationである。
STB出力、全XSD適合・完全相互運用、異厚BOX骨組解析の精度、J/せん断/耐力式の採用、実物Validation、手動GUIは未確認。
既存のJ・せん断・CFT等価剛性の理由付き未算定を成功計算へ変更していない。
独立レビュー・最終head CIは公開PRで別に確認する。
