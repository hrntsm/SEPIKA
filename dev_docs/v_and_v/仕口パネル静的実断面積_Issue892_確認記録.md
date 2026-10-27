# 仕口パネル静的検定の実断面積（Issue892）

## 対象と判定範囲

Issue #892 のS/CFT静的仕口パネル検定を対象とする。正の有限な断面積を実値で除算し、面積・基準軸耐力・軸力比が表現不能なら理由付き未算定とする算術のVerificationである。状態は🔶。
N/A/Fの原典定義、現行降伏式・CFTへの採用根拠、全形状・実建物・実験Validation、手動GUIは検証していない。[原典調査#723](仕口パネル耐力式_Issue723_原典調査.md)の残課題を解消したとはしない。

## 独立期待値

A=5×10⁻¹⁰ mm²、F=200 N/mm²、圧縮N=5×10⁻⁸ Nなら、FA=10⁻⁷ N、n=0.5、降伏低減係数√(3/4)=0.8660254037844386となる。Aを10⁻⁹ mm²へ引き上げるとn=0.25、係数√(15/16)=0.9682458365518543となり、耐力を過大評価する方向である。

通常対照はA=24576 mm²、F=200 N/mm²、圧縮N=2457600 Nで同じn=0.5とする。旧下限ちょうどA=10⁻⁹ mm²、圧縮N=10⁻⁷ Nも同じ値となる。
検証用の角形柱は400×400×16 mm、梁はH-600×200×11×20とし、dc=384 mm、db=580 mm、Ve=7127040 mm³、κ=37/33である。既存降伏式へ代入したn=0の耐力は733991077.0990952 N·mm、n=0.5では635654918.918919 N·mm。需要10⁸ N·mmの検定比はそれぞれ0.13624143824094354、0.15731806208638105となる。テストの期待値はこれらの固定値とし、本体の耐力関数を呼んで再生成しない。
微小面積例では形状寸法を保持して検定用断面積だけを与える。これは公開関数の数値処理の検証fixtureであり、実接合部の成立性や実求解の検証ではない。

## 要素から証拠への索引

| 対象 | 直接入口・テスト | 観測 |
| --- | --- | --- |
| S/CFT、微小・下限境界・通常面積、ゼロ・引張、n=1・2 | `sepika-design-jp::joint_wiring::tests::panel_892_actual_positive_area_matches_independent_capacity_for_steel_and_cft` | 公開 `collect_joint_checks_with_long` の材端力組立とMS応答の両入口、28条件・両軸56結果。n=0.5の固定期待値、n≥1のゼロ耐力/無限大比を区別 |
| A=0/負/NaN/±∞、FA=∞/underflowゼロ、比=∞/underflowゼロ、N=NaN/∞ | `panel_892_invalid_area_capacity_and_unrepresentable_ratio_are_skipped_with_reason` | S/CFT×両入口×11境界、計68結果で接合部と理由を保持。不正面積5条件は既存の参照柱ゲートによる接合部単位1診断、残りは方向別2結果 |
| 旧検定38失効・解析保持・明示再検定 | `sepika-app::app::result_contract_tests::panel_892_design38_expires_checks_but_keeps_analysis_until_explicit_recheck` | 実OVIKA保存/読込後、旧検定を非公開とし部材応力・変位・パネル応答を保持。明示再検定で現行契約と強弱両軸を復旧 |
| 現行成果保持 | `panel_758_current_design_save_open_display_and_recheck_preserve_both_principal_axes` | 現行保存/読込・表示・再検定で両軸の結果保持 |

## 実行記録

初回の直接2テストは容量の独立期待値1成功・境界1失敗だった。不正断面積を既存の参照柱ゲートが接合部単位1診断にするところを、試験側で両軸2結果と期待したためである。公開契約に合わせて試験の結果件数を修正した。FAのoverflow自体に到達する例は、A=2 mm²・F=f64最大有限値とし、先行するEA/Lを有限に保つ人工fixtureを使う。直接Fの実材適用性を確認する試験ではない。

2026-10-27、実装コミット `751d902fa3d7ab6c7dea47618470e29e3fc0ac38` で次を確認した。各Cargoコマンドは `CARGO_TARGET_DIR=C:\Users\hiroa\Desktop\ikkan\.worktree\.cargo-target-892` を明示した。

| コマンド | 結果 |
| --- | --- |
| `rustup update stable` | stable 1.99.1のまま最新。`cargo clippy --version` は0.1.99 |
| `cargo test -p sepika-design-jp --locked` | unit663＋統合21=684成功、失敗0・ignored0。追加の公開入口2テストを含む。doc-test0件は成功件数へ算入しない |
| `cargo test -p sepika-app --lib panel_ --locked` | 15成功、失敗0・ignored0。旧検定38と現行保存保持を含む |
| `cargo test -p sepika-app --lib --locked` | 421成功、失敗0・ignored0。前行15件を含むため別々に加算しない |
| `cargo clippy -p sepika-design-jp -p sepika-app -p sepika-core --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` | 成功 |
| `cargo run -p xtask -- check-docs` | 成功（summary124、links664、impl refs706） |
| `mdbook build` | 成功 |
| `git diff --check` | 成功 |

coreは契約定数のみの変更で、無関係な全テストを再実行せず、Appの実保存・失効・再検定の直接回帰で契約更新を確認した。解析契約は63のまま、検定契約は39へ更新する。旧検定38以前は明示再検定が必要であり、モデル・結果の保存スキーマは変更しない。
