# 平面凸四角形シェルの面積（Issue1154）

## 対象と独立期待値

基点 `78c7265a6e5ef4971478e0467dba26345877c1a0` における、面積の重複・欠落する三角分割を是正する。新しい近似・係数・定式化は採用せず、平面凸四角形だけを検証対象とする。計算方式の正本は [MITC4 シェル要素](../../docs/calc_basis/04_要素剛性/04_MITC4_シェル要素.md)。

公開試験 `crates/sepika-element/tests/shell_quadrilateral.rs` の材料は E=23000 N/mm²、ν=0.2、密度2.4×10⁻⁹ t/mm³、板厚100 mm。

| 平面座標 [mm] | 独立面積 [mm²] | 各方向の総質量 [t] | 法線回転剛性の対角 / 非対角 [N·mm] |
| --- | ---: | ---: | ---: |
| (0,0), (4000,0), (3000,3000), (0,2000) | 9,000,000 | 2.16 | 6,468,750,000 / −2,156,250,000 |
| (0,0), (4000,0), (4000,2000), (0,2000) | 8,000,000 | 1.92 | 5,750,000,000 / −1,916,666,666.6666667 |
| (0,0), (4000,0), (5000,2000), (1000,2000) | 8,000,000 | 1.92 | 5,750,000,000 / −1,916,666,666.6666667 |

第1行の面積は靴紐公式 `(18,000,000−0)/2`、他2行は底辺×高さで独立に求めた。総質量は密度×板厚×面積、法線回転剛性は既定の γ=0.001 と G=23000/2.4 により、対角0.75γGtA・非対角−0.25γGtAから固定期待値を得た。試験では面積関数や剛性の実装を使って期待値を再計算しない。

## 公開入口と観測

`Model::validate`・`validate_local_frames` に成功する入力から `factory::build_behavior` → `try_mass_matrix` と `tangent_stiffness` を実行する。

- 全4巡回とその逆順を検査し、旧実装が面積11,000,000 mm²へ過大評価した元順と8,000,000 mm²へ過小評価した1巡回を含む。
- 水平板に加え、剛体回転・平行移動 `(x,y,0)→(1000+0.6x,−2000+y,3000+0.8x)` を検査する。法線は (−0.8,0,0.6)。3形状×2姿勢×2周回方向×4開始位置の48入力。
- 集中・整合の両質量の全3並進方向を固定総質量と比較する。集中質量の均等配分・非対角ゼロ、方向間結合と回転慣性ゼロを確認する。
- 全体回転剛性の法線への射影を4×4全成分で固定数値と比較し、面積を使うドリリング安定化を観測する。全剛性成分が有限であることも検査する。
- 既存の矩形・歪みメッシュの剛性、局所基底、整合質量の保証は既存element全試験で確認する。

## 保存成果の扱い

解析式が変わるため解析契約を67から68へ更新する。既存の完全一致契約に従い、旧解析成果とそれに依存する検定成果は再計算を必要とする。`ovika_missing_old_exact_future_contracts_preserve_model_and_only_exact_results` に旧67を追加し、実OVIKA読込でモデル・設定を保持したまま結果を失効させる既存試験で保証する。

検定契約49・結果スキーマ1・モデルスキーマを変更しない。保存形式の変更や版間移行は不要であり、[ADR0014](../adr/0014-schema-compatibility.md)のモデル保存スキーマ方針と、計算契約の失効を区別する。新語・新たな設計判断はない。

## ローカル実行記録

2026-11-03、Windows、`stable-x86_64-pc-windows-msvc`、`rustc 1.99.1 (58cf8f9c3 2026-10-13)`、`clippy 0.1.99 (58cf8f9c35 2026-10-13)`。作業開始前に親エージェントがstable更新済みで、実装担当が使用版を確認した。上記基点へ本変更を適用した作業ツリーを検証した。`CARGO_TARGET_DIR=C:\Users\hiroa\Desktop\ikkan\target`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`。

| コマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-element --test shell_quadrilateral --locked` | 1試験成功、48入力の質量・剛性を照合 |
| `cargo test -p sepika-element -p sepika-core --locked` | element 527件・core 634件成功、core既存ignored 1件、doc-test失敗なし |
| `cargo clippy -p sepika-element -p sepika-core --all-targets --locked -- -D warnings` | 成功 |
| `cargo test -p sepika-app --features gui --lib ovika_missing_old_exact_future_contracts_preserve_model_and_only_exact_results --locked` | 1件成功、1222件filtered。旧67・現行68を含む実OVIKA保存成果の利用可否を確認 |
| `cargo clippy -p sepika-app --all-targets --features gui --locked -- -D warnings` | 成功 |
| `cargo run -p xtask --locked -- check-docs` | 成功（summary 141、links 863、impl refs 429） |
| `cargo fmt --all -- --check` | 成功 |
| `git diff --check` | 成功 |
| `mdbook build` | 成功 |

## 検証範囲と残る限界

これは有限fixtureの幾何と要素行列のVerificationである。反り・凹形状、任意建物の固有値・動的応答・設計結果の定量影響、手動GUI操作、実験Validationは確認していない。質量・安定化剛性の増減を全体応答について一律に安全側とは扱わない。fresh独立レビューと最終CIはPRで別途確認する。
