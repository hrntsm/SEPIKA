# 壁幾何と側柱の試験整理（Issue #1059）

## 対象と現役の保証

基点 `368308878de40a88e1310a33b4a8d340976094f1` のE057–E065を整理した。変更はテスト領域のみで、本番の算式・診断・状態確定と復元・物理力の所有は変更しない。既存のVerification/Validationの状態は引き継ぎ、固定SHAと過去の実行結果は履歴として保持する。

| 保証 | 現役の試験と判定 |
| --- | --- |
| 台形壁の上下辺平均壁長と開口周比 | `wall/misc_wall.rs::台形壁の開口周比は壁エレメント幾何の壁長を用いる`。bottom4000/top3000/平均3500/h3000を絶対1e-9で確認。開口1.8e6に対し包絡長ならr0≤0.4、平均長ならr0>0.4と不成立を保持。fixtureだけを共用。 |
| 柱際スリットを持つ雑壁の収集と板厚不明の除外 | `wall/misc_wall.rs::test_plate_without_thickness_is_not_counted_as_misc_stiffness`。板厚除去前に収集1件・両柱際スリットtrueを確認し、算入壁版ID0とsection=None後の除外を確認。旧`test_column_face_slit_wall_is_collected_as_misc_wall`から移設。 |
| スリットによる耐震壁不成立 | `wall/misc_wall.rs::test_any_slit_breaks_seismic_wall`が柱際・下梁際・上梁際を確認。`test_wall_is_seismic_judgement`末尾の同じ柱際入力は除去し、同試験の正常150mm・板厚100mm拒否・開口3.0e6拒否は保持。 |
| 局所釣合いと失敗診断 | `wall/side_column/local_equilibrium/tests.rs::quartic_energy_has_independent_positive_and_negative_force_tangent_and_work_anchors`が±0.02の±1MNを絶対1e-3で確認。失敗群の末尾にあった無状態の同じ正入力再試行だけ除去。全失敗診断と`callback_failure_after_a_successful_newton_evaluation_is_propagated`は保持。 |
| 面内解放・面外と軸剛性 | `wall/side_column/mod.rs::test_release_zeroes_inplane_bending_plane`。LocalY/Zの両方で面内ゼロ・面外Timoshenko剛性・EA/Lを元の相対1e-6で確認。旧`test_release_keeps_axial_stiffness`を統合。単位DOF二次形式の代わりに実行列対角を観測し、同じ両行列の全成分有限も明示する（旧0×NaN/Infによる失敗を保持）。 |
| 側柱判定の要素種・左右・拒否 | `all_three_member_kinds_share_detection_and_index_selection`がBeam/Fiber/MultiSpring×左右について両入口の期待軸Some([0,1])とbinding1件を確認。既存の対角材・無関係柱のNone、純鋼境界の独立分類は保持。同一委譲処理の自己比較と単独is_someを除去。 |
| 解放質量の回転写像 | `test_consistent_mass_has_the_same_released_rotation_mapping`のConsistent2対角・Lumped1対角は絶対1e-12のまま。未使用Model/Ctxだけ除去。 |
| 同じ壁faceの全力・物理面積と両耐力方式 | `wall/static_face.rs::frame_representations_preserve_common_face_and_reject_invalid_bindings`のBeam行に旧`same_face_total_forces_and_physical_area_keep_distinct_owners`を統合。N1200000/Q200000/M−1.29e9/A1332000/depth4600の厳密値、min/meanの独立Qu2006534.471365995/2676420.1354640694の絶対1e-6と両方向一致を保持。Fiber/MSの五fieldと全kindの不正binding拒否も保持。方式を元に戻してから拒否入力を作る。 |

実装参照: [雑壁](../../crates/sepika-element/src/wall/misc_wall.rs)、[側柱](../../crates/sepika-element/src/wall/side_column/mod.rs)、[局所釣合い試験](../../crates/sepika-element/src/wall/side_column/local_equilibrium/tests.rs)、[壁面応答](../../crates/sepika-element/src/wall/static_face.rs)。

## 実行結果

次は同じ基点 `368308878de40a88e1310a33b4a8d340976094f1` に今回の差分を適用したローカル結果である。初回elementの未使用Ctx参照残存によるコンパイル失敗は修正済みで、成功結果と区別する。

| 検証 | 結果と実行範囲 |
| --- | --- |
| `cargo test -p sepika-element --locked` | 532成功、失敗0。Rustdoc試験0件成功。最後の有限確認明示・方式復元の2修正より前に実行。 |
| `cargo test -p sepika-solver -p sepika-job --locked` | 合計613成功、失敗0、既存ignore1。Rustdoc試験各0件成功。本番コードはこの実行以降も不変。 |
| `cargo clippy -p sepika-element --all-targets --locked -- -D warnings` | 全件試験段階でexit0。最終2修正後の再確認は下記。 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-element --no-deps --locked` | exit0。最後の変更はprivate試験assertと試験設定復元のみ。 |
| `cargo fmt --all -- --check` | 全件試験段階でexit0。最終差分も再確認。 |
| `cargo run -p xtask --locked -- check-docs` | exit0、summary141 / links860 / impl refs419。開発者文書の相対リンク・現役symbolは別途確認。 |

最終の全成分有限確認・方式復元後に、`test_release_zeroes_inplane_bending_plane`と`frame_representations_preserve_common_face_and_reject_invalid_bindings`を各`--exact`で実行し各1件成功した。同じ最終差分でall-targets Clippy -D warnings・fmt・diffcheckもexit0。全件試験を最終2修正後に再実行したとは主張しない。実験Validation・手動GUIは今回の確認対象に含めない。


fresh指摘後、同じ基点36830887で`test_wall_is_seismic_judgement`末尾の重複柱際ケースを除去した。`test_wall_is_seismic_judgement`と`test_any_slit_breaks_seismic_wall`を各`--exact`で確認し各1件成功。all-targets Clippy -D warnings / fmt / diffcheck / check-docsは全exit0（summary141 / links860 / impl refs419）。この最終修正後に全suiteを再実行したとは主張しない。
