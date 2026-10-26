# SRC内蔵鉄骨の材料解決（#912）

状態は🔶。公開梁柱入口の材料・耐力境界とApp経路のVerificationを対象とする。新材料のSRC規準適用、計算式全体の原典適合、実験Validation・専門家サインオフ・手動GUIは未確認。

## 判断と独立値

未知名の正のfyだけではSRC規準への適用資格を確認できないため、理由付き未算定とする。通常鋼/CFTの直接入力許可をSRCへ拡大しない。既知材種の表と実板厚条件は既存の材料解決を再利用し、確定Fを梁柱へ渡す。既知材種のfyは表を上書きせず、欠落・不正でも表を適用できる場合は算定する。計算式は変更しない。

梁はRC300×600、中心鋭角H400×200×8×12、上下各2-D16、かぶり20、帯筋D8、Fc24、長期SD345 ft215。独立矩形積分Z=1080743.2533333334 mm³、at=402.1238596594935 mm²、j=493.5 mmより、F235のMA=211982789.84174365 Nmm、同需要の比1を確認する。仮にF100を適用するとMA=114715897.04174364 Nmm、比1.8478937558637216となるが、この比較値で直接入力の適用資格は認定しない。

柱は同RC/H寸法・四隅4-D16とし、曲げ0の純引張耐力で確認する。独立H面積7808 mm²、主筋面積804.247719318987 mm²より、長期F235の引張耐力は1396166.5929869155 N。需要を各独立耐力の1.1倍として比1.1・不適合を検証する。

板厚境界はweb/tfの一方ずつを変え、SN400Bの40/40.001/100 mmとSM520Bの40/40.001/75/75.001/100 mmを固定期待耐力で比較する。100.001 mmはfyを明示しても未算定とする。梁は矩形積分Zと鉄筋寄与、柱は実H面積と4-D16から計算した固定値を使用する。

## 要素から製品経路への証拠索引

| 要素・契約 | 恒久テスト | 観測 |
| --- | --- | --- |
| 公開梁柱・長短期の未知名fy100/235/345、欠落/0/負/NaN/±∞、欠落材料・非鋼材 | `srrc::tests::src912_unknown_strength_and_missing_or_nonsteel_material_are_not_checked` | 正fyも適用未確定、他は不備理由を保持し非成功 |
| Issue独立梁値、既知名のfy欠落/不正/矛盾 | `src912_issue_beam_independent_capacity_ignores_known_grade_fy` | MAと比1、表優先 |
| web/tf各不正値 | `src912_each_actual_plate_thickness_must_be_positive_finite` | 有効な他方の厚さに隠さず未算定 |
| 既知表の境界・柱引張/梁曲げ耐力・範囲外 | `src912_known_grade_actual_thickness_boundaries_have_independent_capacities` | 比1.1・NG、範囲外は明示fyでも未算定 |
| 編集→準備→実解析→検定→OVIKA保存/再読込 | `app::src_ql_acceptance_tests::src912_material_edit_preparation_design_save_preserve_beam_and_column_skips` | 梁柱・fy3条件で未算定理由・材料・CSV非OKを保持 |
| 旧検定失効・現行解析保持・明示再検定 | 同App試験 | 検定契約だけ更新し旧検定を除外、再検定後も理由付き未算定を保存 |

実装は `crates/sepika-design-jp/src/srrc/{mod,beam,column,tests}.rs`、App試験は `crates/sepika-app/src/app/src_ql_acceptance_tests.rs`。検定契約を更新し解析版・保存スキーマは据え置く。

## 検証範囲の限界

#884の未知F235観測は調査当時の履歴として保持し、現在の梁柱契約は本記録で置き換える。#886の部材ランク・終局耐力・パネルは本修正の保証対象外。梁N/My/Qz未評価、法定細則、換算・累加/分担式全体の原典照合は#697等の既存追跡を維持する。本修正でSRC全体を検証済みとはしない。

## ローカル検証

2026-10-27、stable rustc/clippy 1.99.1で実施。

- `cargo test -p sepika-design-jp --locked`: 653単体・20統合試験成功。
- `cargo test -p sepika-app src_ql_acceptance_tests --locked`: 既存QL/契約試験を含む3試験成功。新規試験は梁柱×未知fy3条件を通し、現行/旧検定の保存と再検定を確認。
- `cargo clippy -p sepika-design-jp -p sepika-app -p sepika-core --all-targets --locked -- -D warnings`: 成功。
- `cargo fmt --all --check`: 成功。
- `cargo run -p xtask -- check-docs`: 成功（summary124、links662、impl refs706）。
- `mdbook build`: 成功。

App試験の初回は柱fixtureの上下階FL不足で準備停止した。既存の階入力helperで0/6000 mmを明示し、再実行で上記3試験が成功した。製品の入力検証を緩める修正は行っていない。旧契約fixtureには既知F235で算定した成果を載せ、現在の未算定理由への置換を確認する。
