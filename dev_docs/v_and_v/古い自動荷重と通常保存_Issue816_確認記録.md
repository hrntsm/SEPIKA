# 古い自動荷重と通常保存（Issue #816）確認記録

## 対象と再現条件

2026-10-25、調査元main `c0d56741`。最終検証は `a286a18d` のmain変更を統合して実施した。
未統合のIssue #731の自動区間DL機能は使用していない。

既存の門型ラーメンサンプルから手入力ケースを外し、準備計算で既存の自重DLを生成する。
初期入力は仕口パネルなしとし、片側梁端の移動に伴う斜柱パネル未対応と保存の検証を分離する。
公開GUI編集経路の `App::apply_model_edit(SetNodeCoord)` で梁長6000 mmを3000 mmへ変更し、通常の `save_project_to` と `open_project_from` を呼ぶ。

調査時の実行結果は次の通り。

| 条件 | 修正前mainの観測結果 |
|---|---|
| 準備計算なしで梁を短縮 | 通常保存・再読込に成功し、3000 mmの入力を保持 |
| 準備計算後に梁を短縮し、再準備せず保存 | 自動自重DLの旧区間 `[0,6000]` mmが材長3000 mmに収まらず保存拒否 |
| 拒否した保存の既存ファイル・Appモデル・Undo | 既存ファイル・モデル・履歴を保持。既存ファイルは元の6000 mmとして再読込可能 |
| Undo/Redo後に再準備して保存 | 通常保存・再読込に成功し、3000 mmの入力を保持 |

初報の「保存成功・再読込失敗」はmainでは再現しない。mainのIO保存・読込はともに同じ部材荷重区間検証を行う。
一方、編集後の入力を通常保存して再開する期待は、main既存DLの保存拒否でも満たされず、#731から独立して修正できる。
最初の調査試験では斜柱パネル未対応で再準備対照が失敗し、初期パネル入力を上記の通り分離した後に調査試験が成功した。

## 修正の判断

準備計算が古く、実際に範囲不正な自動部材荷重がある場合だけ、App保存候補から全自動節点・部材荷重を除く。
候補の入力が元成果と変わるため解析・検定成果を同梱せず、生成入力識別を付け直さない。
保存時に準備計算を実行すると未完成モデルの保存が計算条件に依存するため行わない。
準備が古いだけで一律除去すると、有効な自動荷重を含む直接検定単独保存の入力識別を変えるため、範囲不正が実在する場合に限定した。
IO単独保存には準備計算の鮮度情報がないため、渡されたモデルの厳格検証を維持する。
計算式・計算契約版・保存スキーマの変更や近似の導入はない。

## 要素とテストの索引

対象ファイル: `crates/sepika-app/src/app/stale_auto_load_save_tests.rs`。

| テスト | 保証する観測結果 |
|---|---|
| `shortened_beam_normal_save_preserves_inputs_without_repreparation_and_supports_undo_redo` | 既存DL生成・静的計算後の公開短縮編集→再準備なし通常保存→読込。座標・手入力節点/部材荷重保持、旧Auto/成果除外、メモリモデル・履歴保持、Undo/Redo、読込後再準備と再保存 |
| `unprepared_reprepared_and_unfinished_models_keep_normal_save_available` | 未準備・再準備ありの正常対照、断面未割当を含む未完成入力の保存・再読込 |
| `manual_out_of_range_load_still_rejects_edit_and_save_without_changing_file_or_app` | 手入力固定距離荷重を範囲外にする公開編集の拒否。手入力だけ不正・Autoと手入力とも不正の保存拒否、既存ファイル・モデル・保存先・履歴・未保存状態保持 |
| `candidate_write_failure_keeps_existing_file_and_in_memory_derived_data` | 候補作成後の実ファイル書込失敗で既存ファイル・モデル・準備/解析成果・保存先・履歴・未保存状態保持 |
| `valid_generated_loads_keep_direct_design_input_identical_when_preparation_is_stale` | 有効な既存Autoを含む直接小梁/床板検定単独保存で荷重・生成入力識別・検定を復元 |

IO直呼びの不正Auto拒否は最初のテスト、不正手入力拒否は手入力テストでも照合する。
既存の `app::tests::test_floor_design_checks_secondary_member_beam_high_density_steel` による直接検定単独保存も正常対照とする。
`app::rc_wall534_tests::crack_reference_input_edits_undo_redo_and_save_never_reauthenticate_old_results` は、短縮後の保存拒否の期待を、再準備なし保存後も旧検定を復活させない期待へ更新した。
座標・荷重・生成入力識別・モデル・履歴は厳密一致、失敗時の既存ファイルはバイト一致で照合する。

## 実行記録

- 調査用一時テスト `issue816_main_investigation`: 成功（1件、exit 0）。調査後に除去。
- 修正直後の対象回帰: 成功（3件、exit 0）。後から書込失敗・有効Auto付き直接検定対照を追加した。
- 最新main統合後の初回GUI全件: 936件成功、1件失敗（既存RC壁534の保存拒否期待）。新5件と既存直接検定正常対照は成功。
- `cargo test -p sepika-app --features gui --lib --locked`: 旧期待の更新後、937件成功（exit 0）。
- `mdbook build`: 成功（exit 0）。
- `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings`: 成功（exit 0）。
- `cargo clippy -p sepika-app --all-targets --locked -- -D warnings`: 成功（exit 0）。
- 静的解析前に `rustup update stable` を実施。stableは更新不要、`cargo clippy --version` は `0.1.99`。
- `cargo run -p xtask -- check-docs`: 成功（summary 124、links 647、impl refs 688、exit 0）。
- `cargo fmt --all -- --check`、`git diff --check`: 成功（exit 0）。

## 検証範囲

状態・入出力契約の自動Verificationである。手動ネイティブGUI操作と物理的な実験・実測Validationは実施していない。
最終独立レビュー・全適用CIは公開PRの最終ゲートで確認する。
未統合の自動区間DLを用いた初報の保存成功・読込失敗は、本検証のmain再現と区別する。
