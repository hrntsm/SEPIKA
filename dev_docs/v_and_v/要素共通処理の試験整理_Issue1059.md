# 要素共通処理の現役試験参照（#1059）

本書は記録時点の検証履歴である。全長ファイバー積分モデルと専用試験 `distributed_curvature_tests.rs` は Issue #1169 で削除しており、現行の解析経路・試験対象ではない。

対象は共通DOF、シェル、節点ばね、履歴ダンパー、Maxwellダンパー、免震支承、要素生成の入力診断である。算定式・公差・状態のcommit/revert順序は変更しない。既存V&Vの状態を変更せず、以下はVerificationの対応表とする。実験ValidationやGUI操作の証拠へ拡張しない。

## 現役の保証

| 元の試験・整理対象 | 現役の試験・実装 | 保持する観測 |
| --- | --- | --- |
| `node_global_dofs_follows_given_order` | `behavior::tests::node_global_dofs_orders_by_node_and_marks_restrained` | 節点順の逆転、各6DOFの順序、自由番号、拘束番兵を同じモデルで照合 |
| `test_hysteretic_active_in_static_no_dt` | `springs::damper::hysteretic::tests::test_hysteretic_bilinear_elastic_then_yield` | 時間刻み未指定で伸び0.05 mmの50 Nと正の接線剛性、降伏後応答を保持 |
| `test_axial_spring_local_stiffness_and_displacement` / `test_rotational_spring_stiffness_components` | `springs::spring::tests::test_axial_and_rotational_spring_stiffness_components` | 軸の対角・交差4成分、曲げ回転2方向の各4成分とねじり0。試験内だけのP/k代入再計算を除く |
| `test_maxwell_state_member_forces_matches_axial_force` | `springs::damper::maxwell::tests::test_maxwell_element_drives_axial_force` | 実update後の引張正負、2評価点の軸力一致とMz=0。commit後の力・緩和・cache・復元の別試験を保持 |
| シェルのドリリング感度 | `shell::tests::test_drilling_stabilization_insensitivity` | 1要素片持ち、節点0/1固定・節点2Uzの単位荷重、係数0.1/1/10で先端変位差1%未満。local_stiffnessの安定化を二重加算せず、既存dense_solveで解く |
| 免震の状態確定共用 | `springs::isolator::tests` | 積層・摩擦・歪依存のtrial後に既存commit_stateを使い、材料確定→変位確定→せん断状態複製の順を維持。既存commit/revert・checkpoint回帰を保持 |
| 行列引数・lookup・生成診断 | `shell::{bmatrix,stiffness,recovery}` / `frame::section_lookup` / `factory::{regime,input_check}` | 未使用引数と不可能なOption枝だけを除去。行列演算、未割当/範囲外/ID不一致の既定物性、Autoと明示レジーム、診断順と文言を維持 |
| `test_laminated_yields_past_qd` | `springs::isolator::tests::test_checkpoint_roundtrip_restores_hysteresis` | 同じ積層支承・150 mm変位で、弾性力を下回ることと120000 Nとの差500 N未満をcheckpoint復元試験へ移設。履歴・復元後の全観測を保持 |
| `fiber_section_states_are_delegated_to_inner_fiber_beam` | `frame::multi_spring::tests::test_ms_phi_positive_objectivity_and_tangent_consistency` | 同じ295 N/mm²・塑性域指定なしの初期モデルで、状態の存在・2点以上・各点のfiber非空を客観性試験の更新前へ移設 |
| `test_nonlinear_panel_tracks_column_without_stiffness` | `springs::panel::tests::test_nonlinear_panel_yields_at_pmy` | 同じ未更新の非線形パネルで14 DOFと柱12 DOFの剛性0を照合。降伏履歴と期待値・公差を保持 |
| `ねじれ項零なら回転自由度の質量は零` | `frame::prismatic::tests::質量行列の並進成分の総和は部材質量に等しい` | 同じ質量12.5・長さ3000・回転慣性0の回転成分0を統合。回転慣性2の追加ケースと全対角・交差成分を保持 |
| 免震の6成分計算・袖壁柱の状態委譲 | `springs::isolator` / `wall::side_column::column` | 免震2出力の共通6成分を一度定義し、式・評価順・符号・座標変換を維持。柱commit/revertは既存innerへ委譲し、checkpoint型診断を維持 |
| 試験中の断面力取得・初期設定 | `frame::fiber::distributed_curvature_tests` / `factory::input_check::tests` ほか | 同一状態の断面力を一度取得して全成分を照合。同値の再代入と観測前に破棄される設定だけを除去し、入力・試行履歴・期待値・公差を保持 |

固定SHAを含む過去のhandoff・raw出力・過去試験件数は履歴として保持する。本表はその記録を過去に遡って変更するものではない。

## 検証状態

専用targetで次を実行した（2026-10-31）。

| 検証 | 結果 |
| --- | --- |
| `cargo test -p sepika-element --locked` | 550成功・失敗0。Rustdoc対象0件も成功 |
| `cargo test -p sepika-solver -p sepika-job --locked` | 単体・統合計635成功・失敗0。既存の `audit_rc_wall_opening_linear_response` 1件はignore（#821の監査用）。両Rustdoc対象0件も成功 |
| `cargo clippy -p sepika-element --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` | 成功 |
| `cargo run -p xtask --locked -- check-docs` | 成功 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-element --no-deps --locked` | 成功 |

対象3クレートには非default feature定義がない。構造計算の適用範囲や物理的妥当性の新規認証は行わない。

### 2026-11-03の状態委譲・同一fixture整理

基点 `8f5079c0de6ced62fca171f6b46fa04760ddd3b9` に対して上表の免震・MS・パネル・線材の4試験を統合した。既存の履歴・全assert・公差を維持する。過去の件数は上書きしない。

| 検証 | 結果 |
| --- | --- |
| `cargo test -p sepika-element --all-targets --locked` | 単体522件・統合1件成功、失敗0・ignore0 |
| `cargo test -p sepika-element --doc --locked` | 対象0件、成功 |
| `cargo clippy -p sepika-element --all-targets --locked -- -D warnings` | 成功（Clippy 0.1.99） |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-element --no-deps --locked` | 成功 |
| `cargo fmt --all -- --check` / `mdbook build` | 成功 |
| `cargo run -p xtask --locked -- check-docs` | 成功（summary 141・links 863・impl refs 429） |

`sepika-element` に独自feature定義はない。共有targetの古いxtask実行ファイルは削除済worktreeを検査ルートとして保持していたため、文書検査だけ専用targetで再ビルドした。開発・試験profileのデバッグ情報を0として実行し、デバッグアサーション等の検査は既定どおり有効とした。
