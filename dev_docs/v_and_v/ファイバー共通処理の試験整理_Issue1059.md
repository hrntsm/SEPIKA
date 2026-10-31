# ファイバー共通処理の現役試験参照（#1059）

対象はファイバー要素の材料解決、公称初期剛性、局所釣合いと部材荷重応答、および同じ経路の試験整理である。算定式・積分と加算の順序・収束条件・公差・材料判定・状態確定と復元順序を維持する。既存の✅/🔶と実験Validationの状態を変更しない。

## 現役の保証

参照先はすべて `crates/sepika-element/src/` 配下である。

| 整理対象 | 現役の試験・実装 | 保持する保証 |
| --- | --- | --- |
| SRC/CFT鋼材Eの同値分岐 | `frame/fiber/mod.rs::fiber_young_moduli`、`tests.rs::srcファイバーは内蔵鋼材のstrength_factorを使う`、`cftファイバーは鋼管と充填コンクリートの初期接線を分離する` | 鋼材の初期接線、未割当と不正参照のゼロ。CFTコアのコンクリートE算定は別分岐のまま |
| CFTの重複したF適用範囲ガード | `frame/fiber/mod.rs::resolve_steel_fiber_fy`、`factory/hinge_view.rs::sm520_fiber_rejects_explicit_steel_and_main_fy_before_surface_generation` | 同じ鋼材・Box最大板厚/円管板厚による先行拒否を維持。明示fyと主材料fyで適用範囲外を救済しない |
| hingeなしの早期return後と常時trueの所有判定後の到達不能枝 | `frame/fiber/mod.rs::solve_internal_dofs`、`frame_load_response`、`state_frame_load_response`、`member_load_tests.rs` | 分布/端部ヒンジ/壁接続の分岐、残差尺度・20反復上限・診断、線形応答の未対応Errと実trial荷重応答、失敗時rollbackを維持 |
| 2構築経路の公称初期K | `frame/fiber/mod.rs::nominal_elastic_stiffness`、`tests.rs::test_plastic_zone_phi_positive_timoshenko_behavior`、`剛域ありの弾性剛性は軸以外が弾性梁と厳密一致する`、`test_torsional_stiffness_and_internal_force` | 2点Gauss・12×12・断面3成分の元のループ順、せん断・ねじり加算順を共用。塑性域構築経路の最終軸剛性上書きを維持 |
| 2つの `CompressionYield` | `physical_curvature_tests.rs::CompressionYield` を `distributed_curvature_tests.rs` から再使用 | 圧縮降伏境界−0.00005、応力−10、接線200000/20000の人工材料を保持。両モジュールの全試験・正負曲率・非対称配置・境界力・仕事の独立期待値を維持 |
| 部材断面力のDebug文字列比較 | `member_load_tests.rs::flex_point_wall_equilibrium_keeps_nonzero_raw_moments_and_zero_released_face_moments` | `MemberForces` の値を直接比較し、評価位置と6成分、評価点数を同じ2入口間で照合。生モーメント・解放面モーメント・節点力の別assertも保持 |
| `noncentroidal_initial_section_retains_full_axial_biaxial_coupling_without_extra_elastic_compliance` | `constitutive_tests.rs::similar_sections_retain_independent_coupled_elastic_solution_and_failed_trial_state` の尺度1 | 同一偏心(3,7)と軸変位1.5でN=100000の絶対1e-6、K=66666.66666666667の絶対1e-5、初期Dの3非対角非ゼロ、両端塑性ひずみゼロの絶対1e-12をcommit前に照合。ηの元の絶対1e-12もcommit前に照合し、commit後のN/K/ηは全4尺度の既存相対1e-9で照合。Dゼロ/NaN失敗・理由・checkpoint一致も保持 |
| 独立primal試験のcheckpoint非空だけのassert | `independent_plastic_reference.rs::coupled_wall_partial_yield_unload_history_matches_uncondensed_primal_state_tangent_and_energy` | 常に固定構造を直列化する非空確認だけを除去。独立primal解・部分降伏/除荷/再載荷・力・接線・η・塑性ひずみ・材料応力・仕事/散逸・snapshot復元と確定後の力の完全一致は維持 |

統合先の偏心4ファイバーの独立期待値と4尺度の根拠は[既存のIssue #783記録](微小相似Fiber初期断面_Issue783_2026-10.md#独立期待値と観測範囲)を参照する。固定SHA・過去の試験件数・raw出力は履歴として保持する。

## 検証状態

基点 `e9ddd75c4c4dcb2dfa5ba7cae45b06c4287bb90e` に本整理を適用した未commit差分で、2026-10-31に次を実行した。

| 検証 | 結果 |
| --- | --- |
| `cargo test -p sepika-element --locked` | 最終549成功・失敗0・ignore0。Rustdoc0件も成功 |
| `cargo test -p sepika-solver -p sepika-job --locked` | 単体・統合計630成功・失敗0、既存監査用ignore1件。両Rustdoc0件も成功。以後の変更はconstitutive試験のcommit前後観測保持だけで、利用側productionは不変 |
| `cargo clippy -p sepika-element --all-targets --locked -- -D warnings` | 成功 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-element --no-deps --locked` | 成功 |
| `cargo fmt --all -- --check` | 成功 |
| `cargo run -p xtask --locked -- check-docs` | 成功 |
| `git diff --check`、変更V&Vのリンク先・試験名確認 | 成功 |

対象element/solver/jobには非default feature定義がない。既存のignoreは `audit_rc_wall_opening_linear_response`（#821の監査用）であり、成功件数へ含めない。CI・独立レビュー・実験Validation・手動GUIは本ローカル検証とは別の証拠である。


## 最新main取込後の限定確認

独立レビューで指摘された単独中継 `bilinear_member_at` を削除し、`bilinear_member` から同じ入力 `(1.0, 0.0, 0.0)` で `similar_bilinear_member` を直接呼ぶ。数値・状態・期待値・公差は変更しない。

基点を `eadc4b9307b2f2fba73c660f1a77b380cf062e7c` へfast-forwardした後、`cargo test -p sepika-element --locked frame::fiber::constitutive_tests::` は16成功・失敗0（対象外526件）。fmt・diffcheck・check-docsと現役文書のリンク/試験名確認も成功した。上表の全要素549件・利用側630件と静的解析は旧基点 `e9ddd75c` での実行証拠であり、最新基点の全suite再実行を意味しない。Rust6ファイルの本整理差分は、中継helperの削除後からfast-forward前後で完全一致することを確認した。
