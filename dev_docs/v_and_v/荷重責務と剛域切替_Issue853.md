# 荷重責務と剛域切替（Issue #853）

2026-10-26。状態は🔶。通常梁・壁側柱・集中ばね・Fiberの既存荷重応答を正本として接続を確認するVerificationであり、規準原典の全面照合や実物Validationではない。

## 独立期待値

全長1000 mm、i/j剛域200/100 mm、全長等分布2 N/mmの両端固定梁を用いる。可撓長700 mmの固定端値はせん断700 N、モーメント81666.6666667 N·mm。剛域内荷重はi側400 N（節点腕100 mm）、j側200 N（節点腕50 mm）。節点への移送後は反力1100/900 N、固定端モーメント261666.6666667/−161666.6666667 N·mmとなる。剛域OFFは反力1000/1000 N、固定端モーメント166666.6666667/−166666.6666667 N·mm。いずれも合力2000 N、i端まわりの一次モーメント1000000 N·mmを保存する。

部分台形は区間[100,800] mm、強度1/3 N/mmで、合力1400 N、一次モーメント711666.6666667 N·mm。全長三角形0/3 N/mmは1500 N、1000000 N·mm。i剛域内の[0,150] mmの2 N/mmは300 N、22500 N·mm。点荷重1000 Nは両端・剛域内・可撓区間・両境界を含め、一次モーメントを作用位置×1000 Nとして独立に照合する。

## 要素→試験索引

| 契約 | 恒久試験 | 状態 |
| --- | --- | --- |
| 設定既定ON・保存OFF・壁設定との独立 | core `rigid_zone_switch_defaults_on_and_roundtrips_off`、app `test_save_and_open_project_roundtrip` | 最終ゲート成功 |
| 剛性と独立CMQ・手動値/フェース/仕口偏心の保持 | solver `building_rigid_zone_switch_preserves_inputs_and_changes_stiffness_and_cmq` | 直接・最終ゲート成功 |
| 荷重形状・両端非対称・剛域境界・方向・材端反転・局所軸回転・端解放と力/一次モーメント保存 | solver `building_rigid_zone_cmq_keeps_load_force_and_moment_for_all_shapes_and_releases`、element `frame::member_load::tests` | 直接・最終ゲート成功 |
| 増分/時刻歴の集中ばね/Fiber実荷重状態とOFFの剛域0一致 | solver `building_rigid_zone_switch_reaches_incremental_and_timehistory_member_load_states` | 直接・最終ゲート成功 |
| 設定→共通準備→job実解析→CMQの各評価断面一致 | job `rigid_zone_setting_reaches_common_preparation_static_analysis_and_cmq` | 最終ゲート成功 |
| C/M/Q実描画のON/OFFと集中荷重左右 | app `building_rigid_zone_setting_changes_all_rendered_cmq_components`、`cmq_m_drawing_preserves_closed_form_ordinates_and_single_outline`、`cmq_q_fill_stays_inside_signed_curve_and_point_load_jump`、`eccentric_cmq_uses_actual_axis_for_point_jump_length_and_local_components` | 最終ゲート成功 |
| 設定変更後の旧結果失効 | app `各計算入力の編集後は保有耐力判定が無効になる` | 最終ゲート成功 |
| 壁の生成件数・ID・由来・skip・入力保全 | core `wall_expand::tests` | 最終ゲート成功 |
| 壁重量/物理質量・壁解析・診断/数量・GUI選択 | job `wall_weight_partition`、`wall_side_column_entry`、design-jp数量/壁検定、app壁試験群 | 最終ゲート成功 |
| ゼロ床荷重の境界検証・小梁伝達/残量 | load `floor::tests`・`floor::beam_design::tests` | 最終ゲート成功 |

## 実行履歴

初回の4クレート試験では、既存試験成功に対して新規CMQ試験2件が「梁の荷重応答を持たない」で失敗した。通常弾性梁は実荷重応答を所有せず全長legacy荷重へ分岐しており、壁側柱のみが既存の荷重分解へ接続していた。通常梁を既存の荷重応答・状態保持へ接続した後、`cargo test -p sepika-solver --lib building_rigid_zone --locked` は3成功・失敗0となった。記録は `target/issue853-core-load-element-solver.log` と `target/issue853-switch-direct.log`。

所有経路への接続後の既存試験では、線形Beamを非所有とする旧assert1件、荷重状態を設定せずsourceだけを渡す旧準備とlegacy RHSの分岐を期待する7件が失敗した。物理期待値を変更せず所有要素として状態を設定し、legacyへの重複計上がない期待へ揃えた。`cargo test -p sepika-solver --lib --locked` は354成功・失敗0（`target/issue853-solver-owner-regression2.log`）。

GUIを有効にした関連試験の初回実行ではapp lib985件が成功し、実建物の`full_model`は37成功・スナップショット1失敗となった。通常梁の部材荷重を剛域・端解放込みで扱う接続により、固定荷重の最小鉛直変位は−1.017→−1.008 mm、RFLの偏心率は0.005642→0.005572へ変わった。荷重・柱脚軸力総和・固有値・地震水平応答等のスカラは同じで、独立UDL端力・合力/一次モーメント保存と共通準備→実解析照合の成功を根拠に、この2値のみスナップショットへ反映した（`target/issue853-related-gui.log`）。これは壁の定式化・重量/質量式を変更したものではない。

CMQ公開入口は、当該梁・壁側柱の既存診断と、接続端の明示パネルの既存解決を通す。不正な壁側柱の断面参照や、対象梁とは別材料の接続柱でGが非正となるパネルを、factoryのpanicへ進まず同じ理由付きエラーとして返す。対応試験はelement `steel_plate_boundary_clones_preserve_registered_section_ownership_without_rc_release` と `panel_757_valid_steel_joint_couples_each_factory_representation_once`。

4クレート結合試験の後続integrationでは、公開全体荷重組立が所有判定なしで旧全長荷重を積分していたため、`panel_759_public_solver_preserves_original_span_loads_reactions_and_shear_work`が主解析との変位照合で失敗した。公開組立も所有梁の零変位荷重応答へ接続した後、`cargo test -p sepika-solver --test panel_zone --locked`は21成功・失敗0となった（`target/issue853-panel-public-load.log`）。パネルMS=−60050000 N·mm、柱反力19000 Nと曲げ60050000 N·mm、せん断仕事の独立期待値は変更していない。

壁モデルの後続試験では、開口壁のDL断面力の絶対値和（4桁）が81330→81300 Nへ変わったが、重量・固有値3モード・EX断面力と頂部変位は同じだった。無開口壁の長期G+Pの壁脚圧縮力は35718.3405661085→35707.88729066197 Nで、ΔN=−10.45327544653 N。壁全断面A=735000 mm²、D=4300 mm、te=A/D=170.9302325581 mm、j=7(4300−150)/8=3631.25 mmは同一である。荒川meanの軸力項による係数は0.1te·j/A=0.0844476744186で、ΔQu=−0.8827548016 N。既存の独立値1717733.52160923 Nへこの差を反映した1717732.638854428 Nは、実際の正負Qu1717732.6388544284 Nと一致した。試験には長期結果の壁脚Nの独立期待値と、非軸力項1714717.1908143304 N＋0.1N·j/Dによる期待Quを残す。一時的な再解析と診断出力は削除した。壁生成の演算・ID/対応、重量・質量と壁剛性を変えた差ではなく、通常梁からの重力荷重伝達の是正により壁脚参照Nが変わったものである。診断時の証拠は `target/issue853-wall-capacity-diagnostic.log`。

## 最終検証結果

main `b8d61a04` 上の変更に対するローカル検証。件数はunit・integration・doc-testの合算である。

| コマンド | 結果 | ログ（target/） |
| --- | --- | --- |
| `cargo test -p sepika-core -p sepika-load -p sepika-element -p sepika-solver --locked` | 1932成功・失敗0・ignored2。core648、element552、load287、solver445 | `issue853-required-final.log` |
| `cargo test -p sepika-job -p sepika-design-jp -p sepika-app -p sepika-mcp --features sepika-app/gui --locked` | 2071成功・失敗0・ignored1。app1090、design-jp683、job190、mcp108 | `issue853-related-gui-final2.log` |
| `cargo check --workspace --all-targets --locked` | exit0 | `issue853-workspace-check.log` |
| `cargo run -p xtask -- check-deps` | exit0、45方向 | `issue853-check-deps.log` |
| `cargo run -p xtask -- check-docs` | exit0、summary124・link655・impl704 | `issue853-check-docs.log` |
| `cargo clippy -p sepika-core -p sepika-load -p sepika-element -p sepika-solver -p sepika-job -p sepika-design-jp -p sepika-app -p sepika-mcp -p sepika-edit --all-targets --features sepika-app/gui --locked -- -D warnings` | exit0、Clippy 0.1.99 | `issue853-clippy-final2.log` |
| `mdbook build` | exit0 | `issue853-mdbook-final.log` |
| `cargo fmt --all -- --check` | exit0 | `issue853-fmt-check.log` |

ignoredはcore/appの既存性能probe各1件と、Issue #821の開口壁監査1件である。後者を開口モデルの妥当性保証として扱わない。

Clippy初回は公開CMQ関数をtest moduleの後に置いた宣言位置、次回は冗長closureを指摘した。allowを加えず、関数宣言をtest moduleの前へ移動し、`.map(trace_max_shear)`へ等価置換して最終成功となった（`issue853-clippy.log`、`issue853-clippy-final.log`）。この2つだけが全自動試験の成功後に行ったコード変更で、最終Clippyは全targetを確認した。式・入力・出力・期待値の変更はない。

## 未実施・限界

手動GUI操作と実物Validationは未実施。増分・時刻歴は各解析種別の実要素生成・部材荷重状態を照合し、全時刻の応答履歴の独立理論解を新規に検証するものではない。壁のモデル化式・重量/質量の算定式・床分配式は変更しない。旧荷重レビューの未接続記述は当時の履歴として保持する。
