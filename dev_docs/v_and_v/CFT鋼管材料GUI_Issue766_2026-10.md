# CFT 鋼管材料の GUI 割当（Issue #766）

## 対象と証拠の範囲

[Issue #766](https://github.com/hrntsm/SEPIKA/issues/766) の既存 CFT 角形・円形の `Section.steel_material` を対象とする。
計算式・スキーマ・材料モデルは変更しない。製品仕様は[断面の符号と階](../../docs/model_edit/02_断面の符号と階.md#材料は断面が持つ)を正とする。
以下は Windows 上のヘッドレス egui と App の公開操作・実 OVIKA ファイルによる Verification であり、ネイティブ GUI の手動操作・実験 Validation ではない。

## 変更前の再現と役割差

基点 main `062ec3dc` の `tables/sections.rs::role_applies` は Steel 役割を SRC 矩形梁・矩形柱だけに許可していた。
実際の表が使う `material_cell` を egui で描画する試験を追加し、この適用条件を変更前へ戻して実行した。
`cft766_material_cell_shows_unassigned_instead_of_non_applicable` は、角形・円形の順に `[false, false]` を観測して失敗した。
両形状とも、未割当の選択欄 `―` ではなく非適用の `—` になった。修正後は `[true, true]` で成功した。
初回の同種試験は角形で直ちに失敗する構成だったため、両方の描画を収集する構成で再確認した。

| 形状 | 主材料 | 主筋・せん断補強筋 | 内蔵鉄骨・鋼管 |
| --- | --- | --- | --- |
| RC 梁・矩形柱・円形柱 | 有効 | 有効 | 非適用 |
| SRC 矩形梁・矩形柱 | 有効 | 有効 | 有効 |
| CFT 角形・円形（変更前） | 有効 | 非適用 | 非適用 |
| CFT 角形・円形（修正後） | 有効 | 非適用 | 有効 |

既存 `new_rc_and_src_shapes_expose_material_roles` が RC/SRC の対照を保証する。新規の表操作試験が CFT の鉄筋欄の非適用も保証する。

## 直接入口と独立期待値

| 試験 | 観測する契約 |
| --- | --- |
| `tables::sections::cft_material_tests::cft766_material_cell_shows_unassigned_instead_of_non_applicable` | 両 CFT の鋼管セルに未割当を表示する（変更前の赤試験） |
| `tables::sections::cft_material_tests::cft766_table_assign_clear_and_undo_redo_both_shapes` | 実 `sections_table` へポインタの押下・解放を渡し、専用材料名表示、解除、Undo/Redo、再割当を実行。コンクリート候補を鋼管 popup に出さず、主材料を保持する |
| `app::cft_material_tests::cft766_material_edit_save_reopen_invalidation_and_recalculation` | 両形状で既存 `SetSectionMaterial` による Concrete/Rebar の拒否と元参照・鮮度保持、鋼材変更・解除、Undo/Redo、割当済み／未割当それぞれの実 OVIKA 保存・再開、重量と解析・検定成果の失効と明示再計算を確認 |

App 試験は既存の柱 fixture `cft_long_case2_app`（柱長 7200 mm、圧縮 1000 kN、充填 Fc30）を使う。
角形は 400×400×12 mm・角R=0、円形は外径 400 mm・板厚 12 mm。
鋼管を F=235 から未知名称の明示 F=345 N/mm² の材料へ変更し、再解析後の CFT 軸圧縮耐力が増すことを確認する。
密度は 7.85e-9 から 3.925e-9 t/mm³ に変え、独立面積差から単位長さ質量の差を照合する。

- 角形鋼管面積：400²−376² = 18624 mm²。
- 円形鋼管面積：π(200²−188²) = 4656π mm²。
- 質量差：各面積 × 3.925e-9 t/mm³（絶対許容差 1e-12 t/mm）。

充填コンクリートは不変。鋼材の設計単位重量は既存の固定値なので、再生成後の階地震用重量は変更前と一致する（絶対許容差 1e-6 N）。
`weights_are_current` が変更直後に false、再準備後に true となることも確認する。
解除後は質量取得・CFT 終局検定がエラーとなり、保存・再開後も鋼管未設定を保持し、準備計算は「鋼管」を含む理由で停止する。検定実行も停止する。
主材料への読み替えや既定 F による成功は許容しない。

## ローカル検証

- `rustup update stable`：stable 更新不要。`rustc 1.99.1`、`cargo clippy --version`：`clippy 0.1.99`。
- `cargo test -p sepika-edit --locked`：unit 207、統合 20、計 227 成功。
- `cargo test -p sepika-app --locked`：unit 396 と全統合試験が成功（既存 ignore 1）。
- `cargo test -p sepika-app --features gui --locked`：unit 892 と全統合試験が成功（既存 ignore 1）。
- `cargo test -p sepika-app --features gui cft766 --lib`：直接 3 試験成功。
- `cargo clippy -p sepika-edit -p sepika-app --all-targets --locked -- -D warnings`：成功。
- `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings`：成功。
- `cargo run -p xtask -- check-docs`：成功（リンク・実装参照エラー0）。
- `mdbook build`：成功。
- `cargo fmt --all -- --check` / `git diff --check`：成功。

最後の試験調整後にも GUI 構成の全試験（unit 892 と全統合試験）、GUI Clippy を再実行して成功した。default と edit 全試験はその試験調整前の実行であり、実装コードは同一である。

途中の試行では円形の fixture フィールド名・GUI feature 条件の誤りによるコンパイル失敗があった。
重力解析をケース0だけ再実行した試行は、既存の長期組合せが必要とするケース1の入力鮮度で停止したため、全非零 terms の解析へ修正した。
再開後の成果がない状態に `design_stale=true` まで要求した試行は失敗し、成果がある変更直後の失効と、成果がない再開後の検定停止を区別する assertion にした。
これらを材料編集の製品不具合や成功の証拠として扱わない。

## 未実施と範囲外

- ネイティブ GUI の目視・手動操作は、この実行環境で対話的なデスクトップ操作を行っていないため未実施。egui の実表描画・ポインタイベント・App 編集・保存・計算を代替とする。ウィンドウ、OS 入出力ダイアログ、DPI・文字の視認性は未検証。
- STB 元強度を持つ CFT の専用追加 fixture は今回追加していない。既存の強度元指定更新処理を使い、App の全既存回帰を確認した。標準密度は [#741 の検証](STB_CFT鋼管標準密度_Issue741_2026-10.md)と区別する。
- RC/SRC 等の他材料欄には全材料を提示する既存仕様が残る。今回の材料区分拒否は CFT 鋼管に限定し、全役割の再設計は行わない。コード監査上の観測であり、その不正入力の計算影響は今回実行確認していない。
- CFT の許容圧縮に使う E の既存不整合は [#767](https://github.com/hrntsm/SEPIKA/issues/767) の別範囲。
- 物理モデルの Validation、独立レビュー、最終 head の全適用 CI 成功・通常マージは未実施。コミット・push・PR 作成も行っていない。ローカル成功で Issue 全完了とはしない。
