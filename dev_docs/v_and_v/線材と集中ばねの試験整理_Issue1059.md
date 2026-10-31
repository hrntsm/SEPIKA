# 線材・集中ばねの現役試験参照（#1059）

E011–E025の保守整理に対するVerificationの対応表である。計算式・演算順・許容差・近似・安全側判断は変更せず、既存の✅/🔶を格上げしない。実験ValidationやGUI実行の証拠へ拡張しない。

## 現役の保証

| 整理対象 | 現役の試験・観測 |
| --- | --- |
| RC/SRC弾性係数の同値分岐、入力モデルのreadonly assert | `test_beam_new_src_cft_composite_props` とtorsion_contract_testsの独立J/GJ・実Beam/Fiber応答、およびRC床板の実剛性・質量・供給性能を維持。共有Modelの値を読み直すだけのassertを除く |
| `test_local_stiffness_cache_is_bit_exact` | `test_fixed_ends_exact_equals_raw`にcold/warm同一行列をビット一致で移設。rawとの全144成分比較と元公差1e-9を保持。derive Cloneと同じ新規fixtureだけを比べる反復は除く |
| `test_torsion_not_stiffened_by_rigid_zone` | `test_rigid_zone_preserves_rigid_body_rotation`にGJ/Lの対角・交差の相対1e-9を移設。両曲げ方向の剛体回転も保持 |
| 薄壁・壁考慮offの単独試験 | `test_auto_rigid_zone_considers_attached_wall`で(150mm,on)/(90mm,on)/(150mm,off)を保持。λi=325/125/125、λj=125、facei/j=300 mmを1e-9で観測 |
| 標準材のN-M表示 | `concentrated_spring_backbone_matches_material`にmy0/n_allowの1e-9照合を移設。RC武田型は`concentrated_spring_history_returns_backbone_without_mn_linear`として別に残す |
| 断面なし・強度なしの表示 | `concentrated_spring_missing_section_or_strength_returns_no_backbone`の2条件でmodel/骨格なし/相関なしを保持 |
| 同じばね材料の重複生成 | `mn_limit_replaces_yield_points`で最初に作ったsjへset_yieldし、軸力200000 Nと全折れ点・実probe照合を維持 |
| 偏心の試験内仕事再計算 | `vertical_arm_keeps_force_sign_and_parallel_force_has_no_moment`の独立符号・平行力を保持。実ラッパの仕事整合は既存`eccentric_beam_factory_preserves_work_stiffness_mass_and_committed_member_forces`の剛体回転、軸1000 N・偏心モーメント、有限差分接線、両質量と履歴復元で確認 |
| 集中ばねのcommit/revert荷重clone重複 | elastic要素の既存commit/revertを使用。材料・回転状態の順序とN-M再適用を維持。`member_load_failed_trial_and_checkpoint_restore_every_material_and_load_state`等のrollback/記録試験を保持 |
| `test_state_member_forces_matches_internal_force` | `test_internal_force_no_double_count`へ3評価点・材端Mz符号・非零・絶対1e-6を移設 |
| `test_mn_interaction_yield_moment_in_response` | 要素外でclone材料だけをtrialする観測を整理。`test_mn_interaction_reduces_spring_yield`の両端半減と圧縮/引張、`member_point_load_yields_springs_and_matches_bilinear_equilibrium_and_tangent`の実部材応答を保持 |
| factoryのprivate snapshot型判定 | 元のdefault集中ばね/明示Fiberの両入力を保持し、公開end_spring_rotations/fiber_section_statesで生成結果を観測 |
| SM520の同じ範囲外拒否 | [現役ファイバー回帰](SM520告示基準強度と範囲外拒否_Issue629_2026-10.md#現役ファイバー回帰の参照1059)の全材名・形状・正常対照を保持 |

READMEのティモシェンコ梁✅はBernoulli極限と剛域GJ/Lの数値照合を保持する。敵対的レビュー記録の両曲げ剛体回転の保証も同じ試験へ残る。readonly assertの削除はRC床板の独立断面二次モーメント・bareとの他性能比較・供給性能2倍の保証を削らない。固定SHA/raw履歴は変更しない。

## 検証状態

2026-10-31、初回ローカル検証は基点 `70fc8a0ba42a9031e9f53a3398170e98ee580a19` にE011–E025の変更を載せ、同worktreeで利用したtargetで次を確認した。以下の547件と利用側635件、および静的検証はこの初回基点の結果である。

| 検証 | 結果 |
| --- | --- |
| `cargo test -p sepika-element --locked` | 547成功・失敗0、Rustdoc対象0件成功 |
| `cargo test -p sepika-solver -p sepika-job --locked` | 単体・統合635成功・失敗0。既存の#821監査用1件はignore、Rustdoc両0件成功 |
| `cargo clippy -p sepika-element --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` | 成功 |
| `cargo run -p xtask --locked -- check-docs` | 成功 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-element --no-deps --locked` | 成功 |

対象element/solver/jobに非default feature定義はない。CI・独立レビューと、構造計算の物理Validationはこのローカル結果と区別する。

### 最新baseでの独立検証と追加整理

その後、E001–E010を含むmainを取り込み、基点 `e3380f0c47d25e225987ff3d0415f672f3f393c5` に更新した。親経由の独立レビュー担当の実行報告はelement全543成功・失敗0である。初回547件との差4件はmainへ取り込まれたE001–E010の試験統合による。この独立検証は下記の期待値生成の追加整理前であり、初回の利用側635件・clippy等を最新baseで再実行した証拠とは扱わない。

追加整理では `concentrated_spring_backbone_matches_material` の同一入力に対する `yield_moment_and_axial` の再呼出しだけを除去し、最初の降伏モーメントをN-M相関と全骨格点・probeの両方へ使用する。期待値・観測・公差・production計算は変えない。追加整理後、最新基点で `cargo test -p sepika-element --lib --locked factory::hinge_view::tests::concentrated_spring_backbone_matches_material -- --exact` は1成功・542除外・失敗0。`cargo fmt -p sepika-element -- --check`、check-docs、diffcheckも成功。全543件・利用側・clippyの再実行はしていない。

### 重複assertの最終整理（同基点e3380f0c）

SRC複合性能試験の同一area_ax再計算を除去し、先行する同じ出力・同公差1e-6の比較を保持した。ねじりhelperは同じKから逆算したthetaの再比較だけを除き、thetaは実update入力に保持する。独立GJ/Lの全4成分1e-10と実要素の±1000000 Nmmの照合1e-8は残る。

変更後、`test_beam_new_src_cft_composite_props`とtorsion_contract_testsのH・丸角Box・CFT角/円の3試験をそれぞれexact実行し、計4成功・失敗0（各1成功・542除外）を確認した。fmt対象クレートcheck・check-docs・diffcheckも成功。全543件や利用側・clippyの再実行結果ではない。
