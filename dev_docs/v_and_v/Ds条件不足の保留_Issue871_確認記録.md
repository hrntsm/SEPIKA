# Ds条件不足の保留（Issue871）

対象: [Issue #871](https://github.com/hrntsm/SEPIKA/issues/871)。2026-10-27。実装コミット: `538347c5`。

原典の画像照合は[Issue719調査](../handoff/Ds崩壊機構補正_Issue719_調査記録.md)の記録を引き継ぐ。本件で原典を新たに閲覧したとはしない。製品仕様の正本は[構造特性係数Ds](../../docs/calc_basis/07_二次設計/02_構造特性係数_Ds.md)。

## 修正の判断と境界

層崩壊の柱梁群を一律一段階悪化する処理は、確認した部材・接合部・柱脚の条件を代替しないため削除した。補正全体が安全側という保証もない。単なる無補正への変更ではなく、現行データで未確認の対象条件を理由付き保留にした。新しい応力推定・法規ルート・保証設計の入力システム・近似は追加していない。

通常RC/Sラーメンも現在の入力だけでは確認情報が不足するため広く保留となる。RC梁はS柱の存在と組み合わせて層外も確認し、基礎梁用途自体も未確認とする。RC梁を基礎梁と断定せず、地域限定のRC→S換算を一般適用しない。S柱についても露出柱脚と断定せず、柱脚の有無・形式から確認不足とする。

規準条件が成立しているとする新たなデータ入力は追加していないため、条件充足時の新たな成功経路はない。Ds/Qu採用状態の補完は[#872](https://github.com/hrntsm/SEPIKA/issues/872)の別範囲。全規準適合・実建物Validation・専門家サインオフ・OS手操作のGUI確認は未実施。

## 実入口と観測契約

| 対象 | 独立した期待・観測 | 試験・経路 |
| --- | --- | --- |
| 実S架構・自動/手動・FA/FD | 入力と実解析を満たしても不足条件を補完せず、Ds判定保留。前の候補から採用情報・βuを残さない | `app::tests::ds871_public_entry_holds_both_rank_modes_and_clears_candidate_source` → 公開 `compute_holding_capacity` |
| 実RC壁付き架構・自動/手動 | Ds階崩壊時応力・破壊形式、非ヒンジ柱、接合部の対象最低種別を不足理由として示す | `rc_wall_619_existing_ds_entry_uses_adopted_run_wall_response_and_reports_inputs` の実解析後の公開入口確認 |
| 方向別接続梁 | X方向ではY梁を除き、Y方向では含める。接合部はヒンジ/脆性対象の識別と対象だけの最低種別が不足 | `ds871_rc_conditions_exclude_orthogonal_beams_and_identify_joint_targets` |
| S柱脚 | 柱脚形式・伸び能力・保有耐力接合・認定条件が未確認。保有耐力接合の柱ヒンジ仮定/上部Ds、非保有耐力接合の伸び能力別措置、群Dの0.05割増除外を区別して案内 | 上記S架構公開入口試験 |
| S造のRC基礎梁候補 | 基部節点間のRC梁を層の群集計対象外でも検出し、用途・ヒンジ・限定換算・FD回避を未確認とする | `ds871_s_frame_detects_rc_foundation_beam_outside_layers_without_conversion` |
| CSV/GUI | 対象部材・不足条件の文を表示し、正常なQun/合否の表を出さない | 公開入口試験のCSV、`ds871_gui_renders_hold_reason_without_capacity_verdict` の実egui描画 |
| 保存成果 | 旧検定契約は失効。解析は維持し、再読込後も不足条件を保留 | `ds871_old_design_is_invalidated_but_analysis_remains_and_ds_stays_on_hold` |
| 候補数値計算 | 既存の世代・力釣合い・Fes・壁応答・材料強度の観測を維持 | `compute_unconfirmed_holding_capacity` を明示して既存App数値試験を維持 |
| 剛域と解析機構 | 降伏4材端・未降伏時の機構未形成、剛域300mm/高さ3000mmのせん断力比1.25との5%以内照合 | `sepika-design-jp/tests/vnv_rigid_zone_mechanism.rs` の3試験 |

外部の`full_model`・`wall_model`試験では公開入口を保持し、正常なDs/Qunの期待を保留契約へ変更した。QuとFesの既存スナップショット値、Qudの独立期待値、壁のQu/βu/力釣合いは公開solver・summaryの観測として維持した。これらをAppの確定Ds計算の証拠とはしない。斜め壁の局所せん断を用いる候補種別FD/FD/FBの3条件は、App内の`ds871_diagonal_wall_candidate_uses_local_shear_without_confirming_conditions`へ移し、矩形RC単独壁の共有fixtureを45度回転して確認する。外部integrationは斜め壁の実応答と公開入口の保留を確認する。

## 誤ったDs経路の整理

Appで使わなかった`member_rank::story_ds`は互換性のために残さず削除した。旧`vnv_rigid_zone_ds`は解析機構の観測を維持して改名し、一律悪化を告示上の規則とする主張と計算呼出だけを削除した。これら3試験の成功を製品Dsの規準適合とは扱わない。

既存Appの数値・入力世代試験は、条件確認前の候補計算に対するVerificationとして残す。製品画面・結果画面は公開入口だけを呼び、CSVは同じ読み取り専用の不足理由を使う。候補を表示・保存する製品呼出は追加しない。公開入口は計算を一回だけ行い、条件不足時には採用表示用情報を消去する。

## 実行した検証

初回の標準App試験は425成功・1失敗で、断面未設定モデルのCSV出力時に診断がpanicした。診断を失敗可能な用途解決へ変更して修正した。開発中に存在しない壁用部材用途を参照したコンパイル失敗もあり、壁要素・壁版を別途識別する処理へ修正した。これら途中結果は成功の根拠に含めない。

GUIの初回は992成功・3失敗。旧正常値表示を期待した2試験は、候補数値・目的別点のassertを維持して保留表示を期待する契約へ更新した。新保存試験の1件は、旧契約でも意図して残すパネルのSkipped診断まで消去する誤った期待を、通常検定消去・Skippedのみ残存へ修正した。修正後のGUI995件は全成功した。

検定契約は42へ更新した。実OVIKAの保存39・40・41と未来版（現行+1）は通常検定を利用不可とし、現行42だけを再利用する。現行でもDs条件不足は保留のままとし、旧成果の再保存・再読込・一次検定の再実行でDsの保留が解除されないことを確認した。解析契約64は維持した。

全cargoコマンドに専用`CARGO_TARGET_DIR=C:\Users\hiroa\Desktop\ikkan\.worktree\.cargo-target-871`を指定した。ツールチェインは2026-10-27に`rustup update stable`で更新確認し、rustc1.99.1・Clippy0.1.99（2026-10-13）。

| コマンド・検証 | 実測結果 |
| --- | --- |
| `cargo test -p sepika-design-jp --locked` | 単体663、統合21（剛域機構3を含む）成功。doc-testは0件 |
| `cargo test -p sepika-app --lib --locked` | 修正後427成功（先行main取込・斜め壁追加前の全体回帰） |
| `cargo test -p sepika-app --features gui --lib --locked` | 修正後995成功（同上、GUI旧契約更新後） |
| `cargo test -p sepika-app --features gui --lib ds871 --locked` | 最終差分6成功・991除外。自動/手動・GUI・直交梁・層外梁・保存39/40/41/現行/未来・斜め壁候補を含む |
| `cargo test -p sepika-app --lib ds871 --locked` | 最終差分5成功・424除外。GUI機能なし構成のコンパイルと同契約 |
| `cargo test -p sepika-app --features gui --lib rc_wall_619_existing --locked` | 最終差分1成功・996除外。実RC壁付き架構の自動/手動の公開入口 |
| `cargo test -p sepika-app --features gui --test full_model --test wall_model --locked` | 最終差分38+17成功。Qu/Fesの既存数値、Qud独立値、実応答と保留の組合せを維持 |
| `cargo clippy -p sepika-core -p sepika-app -p sepika-design-jp --features sepika-app/gui --all-targets --locked -- -D warnings` | 成功。追加の定数assertへの指摘は不要なassertを除いて修正 |
| `cargo fmt --all -- --check`・`git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | summary124・links667・impl refs707、成功 |
| `mdbook build` | 成功 |
| 変更した開発文書3件の相対リンク・見出しアンカー | 解決不能0 |

最終差分の基準mainは`a71c5a4a`。#901の別改修を含む最終統合・freshレビュー・全CIは後続であり、本記録の成功件数へ含めない。ローカルログは専用worktreeの`target/issue871/`へ保存した。
