# 階重量と物理質量の現役回帰（#1059）

`sepika-load/src/story_gen/` の試験整理に伴う現役参照を示す。独立期待値、入力ケース、比較フィールド、許容誤差を維持する。計算方式・精度・安全性の変更や Validation の追加を意味せず、既存の V&V 状態は変更しない。

## 現役の保証

以下はいずれも `crates/sepika-load/src/story_gen/tests.rs` の試験である。

| 保証 | 現役試験 |
| --- | --- |
| ダンパー 20000 N が断面自重を置換し、上階の設計重量 10000 N と物理質量が一致 | `test_damper_total_weight_is_common_for_design_and_physical_mass`。ゼロ重量の除外と不正入力は別試験を保持 |
| 密度直接算入と自重同期ケースの両経路で、正の `dynamic_mass` と代表節点質量が一致 | `test_dynamic_mass_matches_between_density_and_synced_paths` |
| 鋼線材の設計重量と物理質量を区別し、両質量方式の総質量が一致 | `test_steel_line_design_weight_and_physical_mass_are_separated` |
| CFT の鋼管にだけ割増を掛け、コアには掛けない。無指定と 1.3 倍、両質量方式、代表節点質量と `dynamic_mass` | `test_cft_steel_weight_factor_applies_to_steel_mass_only`。自重同期経路と角丸・供給断面の試験も保持 |
| 鋼材の補正質点へ残す割増増分も物理密度が基準 | `test_steel_weight_factor_increment_is_physical_based_in_corrected_lumped` |
| 壁の仕上げ荷重を設計重量と補正質点に残す | `test_wall_finish_load_included_in_weight_and_corrected_lumped_mass` |
| GravityCasesOnly は手入力 DL の Other / Dead を自重と推定して置換しない | `test_gravity_cases_only_uses_case_content_without_replacement`。ケースなしの両方式・全階ゼロ重量・非負質量も保持 |
| 柱際・下梁際・上梁際のスリットが、幾何による上下の階帯重量を変えない | `test_wall_slits_preserve_geometric_story_weight`。DL の支持先への配分とは別の保証 |
| Anchor / Columns のどちらも取付き線の高さに壁の全重量を残す | `test_attached_wall_transfers_keep_total_weight_at_anchor_line` |
| 集中・等分布・三角分布の単純梁反力が独立した手計算値と一致 | `test_static_reactions_match_hand_calculated_loads`。符号・範囲検証と公開配分経路の試験は別に保持 |
| 柱脚に下階接続なし・鉛直ブレース・3節点梁なら梁せい分を付加し、2節点柱なら付加しない | `test_base_column_depth_addition_depends_on_lower_connection`。4 ケースの節点・断面・要素の順序を保持 |
| K ブレースの無指定 / InternalNodes / BaseNodesOnly で独立期待重心と重量保存が成立 | `test_k_brace_rules_preserve_weight_and_expected_centroids` |

## 実装整理の範囲

`self_weight_calc.rs` の面積ヘルパーは Shell 分岐からだけ呼ばれていたため、Wall 専用分岐とその索引を除去し、従来と同じ多角形面積を直接使う。物理壁版の `Model::wall_weight` や階帯生成は変更しない。

`generate.rs` の階情報は、検証済み `model.stories` 全件から作った同じ長さの標高列を走査して参照する。存在しない階への既定値補完を除去し、同じ既存階の入力値と剛床を引き継ぐ。公開入力境界の検証は保持する。

共有借用の前後でモデルが不変であることだけを再確認した比較と、明示階を必ず持つ試験 fixture の到達不能な補完処理を除去した。供給断面性能のエラー・修復、FL / ΔSL、所属・剛床・層高さ、被覆の全形状と入力条件の試験は保持する。

## 履歴との区別

過去の固定コミット・raw 出力・件数は当時の記録として保持する。壁スリットの旧「自重を反対側へ全量移動」という記述は支持経路についての過去の仕様であり、現在の幾何階帯の重量を検証する上記試験とは区別する。
