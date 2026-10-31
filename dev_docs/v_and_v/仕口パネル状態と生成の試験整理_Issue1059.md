# 仕口パネル状態・生成の現役試験参照（#1059）

対象は `sepika-element` の `springs/panel.rs` と `springs/panel_gen.rs` のE052–E056である。算定式・材料・安全性判断・既定値・演算順・公差・状態確定と復元は変更しない。既存の数値Verificationと実験Validation未確認の境界を維持する。

## 現役の保証

| 元の対象 | 現役の実装・試験 | 保持する保証 |
| --- | --- | --- |
| snapshotとcheckpointの重複した状態収集 | `PanelZone::state_data` と同じtupleの型別名 `PanelState` | パネルcommitted/trial各2値、柱committed/trial各12値（columnなしなら同じゼロ配列）、方向順のばね状態bytes2個を同じ順で収集。snapshotのAny型とbincodeのtuple型・項目順は同じ。復元時の型不一致/ばね失敗panic、checkpointの空入力/Decode/MaterialStateErrorはそれぞれ保持 |
| `test_resolve_takes_dc_from_column_and_db_from_beam` / `test_elastic_stiffness_is_g_times_ve` / `test_internal_force_follows_trial_rotation` | `springs::panel::tests::test_linear_panel_geometry_stiffness_and_trial_moments` | 同じcross_joint_modelの未更新Panelでdc379/db583/tp13を絶対1e-9、Veの式を相対1e-12、2DOF・両GVe対角を相対1e-12・非対角絶対1e-6で照合。その後同じPanelに1e-4の回転増分を与え、強軸内力相対1e-12・弱軸内力と公開モーメントの絶対1e-6を保持 |
| `test_generates_panel_at_steel_joint` / `test_is_idempotent` / `test_disabled_mode_removes_panels` / `test_offsets_are_written_into_rigid_zones` / `test_offset_write_is_idempotent` | `springs::panel_gen::tests::test_panel_generation_offsets_and_disabled_mode_lifecycle` | H400×400×13×21の同じL-frame。生成1件、node0、dc379/db583/tp13の絶対1e-9、正K、生成要素数・接続節点・全ID/kind/nodesを保持。梁offset200/柱offset300の絶対1e-9、非接合端ゼロ・元lengthゼロ・rigid_lengthを保持。再適用で全要素数/ID/kind/nodes/offsetの完全一致、OFFで返り値空・要素2件・Panelなし・全offsetゼロを確認。初回/再適用/OFFの各段でmodel.validateを保持 |
| `panel_761_actual_dimensions_control_public_axial_ratio` の観測Vec | 同名試験の各fixture直後のassert | 全5組合せ(L,A,shortening)=(.5,.001,.00025)/(.5,100,.00025)/(3000,.001,1.5)/(3000,100,1.5)/(3000,36000,1.5)。両軸比.5・低減率.8660254037844386を絶対1e-12で保持し、診断出力も維持 |

履歴・失敗・再試行の試験は、同じsnapshotやfixtureが見えても統合しない。

- #751のtrial軸力変更後の独立モーメント比0.61、commit状態0.808へのrevert、5方向とsnapshot/checkpoint後の再試行。
- #724の定軸力11頂点と仕事2.5344、軸力変更での塑性回転保持、両復元API、柱脚剛域と端順反転。
- #753の高圧縮3点、軸耐力到達/超過と非有限の原子性、1e-9境界、正負方向の高圧縮履歴と復元。
- #757の二軸回転・仕事・独立行列と履歴、片方向の材料不正、柱軸・板厚・F区分ごとの保証。
- #761の微小寸法・高軸力拒否・診断・両復元APIによる状態と再試行。
- 生成側のmanual zoneのmax、OFFでも元length/faceを保持する契約、剛域再計算後のoffset保持、追加部材を含むID参照再割当はKeep。

これらの数値・公差・全履歴入力を保持する。過去の固定SHA・raw・実行結果は書き換えない。[既存の剛性照合記録](剛性計算_参照実装照合.md)と併せて参照する。

## 検証状態

2026-10-31、基点 `f91752359c126591a19605a3168bf2c34ebfbe93` に本未commit差分を適用して実行した。

| 検証 | 結果 |
| --- | --- |
| `cargo test -p sepika-element --locked` | 533成功・失敗0・ignore0、Rustdoc0件成功 |
| `cargo test -p sepika-solver -p sepika-job --locked` | 619成功・失敗0、既存監査用ignore1件、両Rustdoc0件成功 |
| `cargo clippy -p sepika-element --all-targets --locked -- -D warnings` | 成功 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-element --no-deps --locked` | 成功 |
| `cargo fmt --all -- --check`、`git diff --check` | 成功 |
| `cargo run -p xtask --locked -- check-docs`、変更V&Vのリンク・実名確認 | 成功 |

全suite成功後、統合先の具体値200/300のassertに包含される「正offsetの存在」だけの重複assertを除いた。正offset保証と全入力は保持し、最終lifecycle exact1件、all-targetsClippy、fmt/diffcheckも成功した。初回全suiteからproductionの変更はない。

対象element/solver/jobに非default feature定義はない。ignoreは `audit_rc_wall_opening_linear_response`（#821監査用）であり成功件数へ含めない。CI・独立レビュー・物理Validationは本ローカル実行と区別する。
