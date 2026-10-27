# コンクリート長期許容せん断の是正（Issue #922）

## 照合範囲と安全性

2026-10-27、[Issue #922](https://github.com/hrntsm/SEPIKA/issues/922)に従い、普通コンクリートの長期許容せん断応力度の過大を是正した。現在仕様と適用経路は[材料の許容応力度](../../docs/calc_basis/06_一次設計/01_材料の許容応力度.md)を正本とする。

ユーザー指定の『2025年版 建築物の構造関係技術基準解説書』第2版第1刷、9.3 pp.584–585の原画像をレンダリングして目視確認した。p584の令91条表の長期せん断欄と短期2倍、p585の平成12年建設省告示第1450号第2のF>21と0.49項を照合した。実験値による別値の但し書は本変更では利用しない。書誌・公開訂正の先行確認は[Issue881記録](RC材料構造細則_Issue881_原典照合.md#原典版刷公開訂正)を参照し、今回の原画像照合と区別する。原画像・抽出本文はリポジトリへ収載しない。

変更前のFc24長期0.74に対して原典の0.73 N/mm²を採用する。短期倍率1.5及び軽量1種・2種の係数0.9は変更せず、基準の長期値の訂正だけを各経路へ伝播する。短期1.5は法令2倍と同じ値ではなく、その75%である。軽量係数及び部材式全体への適用根拠を確認済みとはしない（[照合待ち](原典照合リスト.md#照合待ち)、[#697](https://github.com/hrntsm/SEPIKA/issues/697)）。材料の許容値を大きくする変更・危険側の近似・解析式変更はない。

## 独立期待値と要素からの証拠索引

以下の期待値は原典へ先に代入した固定値であり、実装関数の呼出しやコピーを期待値生成に用いない。応力度の単位はN/mm²。

| Fc | 長期 | 採用短期 | 材料試験 |
| --- | --- | --- | --- |
| 20.999 | 0.6999666666666667 | 1.04995 | `material_strength::tests::concrete_shear_922_matches_independent_legal_boundary_values` |
| 21 | 0.7 | 1.05 | 同上 |
| 21.001 | 0.70001 | 1.050015 | 同上 |
| 24 | 0.73 | 1.095 | 同上 |
| 36 | 0.85 | 1.275 | 同上 |

軽量1種・2種のFc24は長期0.657、短期0.9855で、普通コンクリート・圧縮値とともに`test_concrete_fc24_representative_values`で確認する。

| 経路 | 入力と独立期待値 | 実入口・検証 |
| --- | --- | --- |
| RC梁・柱 | Fc24、b400×D700、主筋径20、かぶり40、補強筋径10・脚数0、d640・j560 mm。M=0で梁α2/柱α1.5。梁Q329280 Nと柱Q246960 Nに対して長短期とも検定比1.0068493150684932、NG | `RcDesign::check` → `rc::allowable::rc_allow`。`rc::tests::concrete_shear_922_rc_beam_and_column_reject_old_passing_shear` |
| SRC梁 | 同じRC寸法、主筋上下4本、H500×200×50×14。sZ3075630.9333333336 mm³、at1256.6370614359173 mm²。弾性分担のRC負担率0.1862005860660579、Q882583.6882258701 N。RC長期QA163520 N/短期245280 N、検定比長期1.005(NG)/短期0.67(OK) | `SrcDesign::check` → `srrc::beam::src_beam_check`。`srrc::tests::concrete_shear_922_src_beam_uses_reduced_long_and_short_allowables` |
| SRC柱 | Fc24、500角、主筋径20・各辺4本、かぶり40、補強筋径10・脚数0、H300×200×50×14。d440・j385 mm、n15、β1.1142857142857143。α上限2・3b′/b=1.8支配、長期QA534798 N、Q537471.99 Nで比1.005(NG)。短期比0.6827523092941661(OK) | `SrcDesign::check` → `srrc::column::src_column_check`。`srrc::tests::concrete_shear_922_src_column_uses_reduced_long_and_short_allowables` |
| RC壁 | 長短期の共通材料値を使用 | `rc::wall::rc_wall_shear_check`（既存単体・結線回帰） |
| RC接合部 | 短期の共通材料値を使用 | `rc::joint::rc_joint_shear_check`（既存回帰） |
| SRC仕口パネル | 長短期の共通材料値を使用 | `srrc::panel_zone::src_panel_zone_check`（既存回帰） |
| 保存済み検定 | 旧検定35・36を除去、解析63の内力を保持。再保存・再読込みでも旧OKを公開せず、明示再検定で現行値を採用。現行版は保持、未来版は失効 | `app::src_ql_acceptance_tests::src_ovika_design_contracts_keep_current_analysis_and_require_explicit_recheck`、実OVIKA往復 |

RC長短期の上記許容せん断力は梁327040 N/柱245280 Nである。訂正前は梁331520 N/柱248640 Nのため同じ需要を許容内としていた。SRCの長期比1.005は訂正前0.9914189189189189であり、RC負担の支配を確認する。これらは材料値の差で旧合格となる境界のVerificationであり、脚数0のfixtureを実建物の適合配筋例とはしない。

当初実装は検定計算契約版を35から36へ更新し、解析契約62とスキーマを維持した。その後、#910を含むmain（`6c911bc2`）を統合し、mainの解析契約63を保持、検定契約を37へ更新した。解析63の保存内力を保持し、旧検定35・36のみ失効して再検定を要求する。スキーマ変更はない。

## 実行結果と未確認

### 当初実装・main統合前の記録

追加SRC柱試験の初回は期待値側でα′を1.5としたため失敗した（観測比0.8375）。SRC柱のα上限2と3b′/b=1.8の条件を手計算で見直し、期待値のQA・Qを訂正した。実装側は変更していない。

共有ビルド先は `.worktree/.cargo-target` を使用。

| ローカルコマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-design-jp --locked` | 成功。656単体、21統合、doc-test 0。SRC柱の期待値訂正後の最終実行 |
| `cargo test -p sepika-core --locked` | 成功。604単体、45統合、1 ignored、doc-test 0 |
| `cargo test -p sepika-app --lib src_ql_acceptance_tests --locked` | 成功、3件。公開SRC・旧35/現行36/未来37の実保存往復 |
| `cargo test -p sepika-app --lib result_contract_tests --locked` | 成功、36件 |
| `rustup update stable` / `cargo clippy --version` | stable最新確認、rustc 1.99.1 / clippy 0.1.99（2026-10-13） |
| `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-app --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` / `git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | 成功（summary124・links662・impl refs706） |
| `mdbook build` | 成功 |
| 変更Markdownの相対リンク先検査 | 全件の実在を確認。開発文書も対象 |

### main統合後の確認

共有targetを専有した状態で、変更したcore・element・solver・design-jp・App等のworkspaceクレートを再ビルドし、実行件数を確認した。旧35・36、現行37、未来38を、解析63の実保存・再読込み・再保存・再検定へ通した。旧版の期待失効状態は現行定数との比較から生成せず、明示したfalseで確認した。前表の解析62・検定36時点の全core試験は履歴であり、今回の版統合を理由に全件を繰り返さない。

| ローカルコマンド | 統合後の結果 |
| --- | --- |
| `cargo test -p sepika-design-jp --lib concrete_shear_922 --locked` | 4 passed、654 filtered。独立材料境界・RC梁柱・SRC梁柱 |
| `cargo test -p sepika-app --lib src_ql_acceptance_tests --locked` | 3 passed、417 filtered。10種類の保存契約条件を含む。明示期待状態への最終修正後も成功 |
| `cargo test -p sepika-app --lib result_contract_tests --locked` | 36 passed、384 filtered |
| `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-app --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` / `git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | 成功（summary124・links663・impl refs706） |
| `mdbook build` | 成功 |

専門家サインオフ、実建物・実験Validation、手動GUI操作、全法令の改正履歴監査、部材式全体及び軽量係数の別規準原典照合は未実施。限定したVerificationをもって全規準適合とは扱わない。#881時点の相違は履歴を維持し、本記録へ接続する。
