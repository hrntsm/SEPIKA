# STB CFT鋼管の標準密度（Issue #741）

## 対象と範囲

[Issue #741](https://github.com/hrntsm/SEPIKA/issues/741)の、既知標準鋼材を指定したCFTの鋼管材料解決を対象とする。
基点は `b292a1cadaa2e7124627a9bcb43edeaa32ffb669`。実行日は2026-10-21。
CFTの主材料を充填コンクリート、鋼管を別材料参照とする構成と、充填部の密度解決・密度不正検査は変更しない。
異厚BOXは[#683](https://github.com/hrntsm/SEPIKA/issues/683)、CFT設計式の原典照合は[#697](https://github.com/hrntsm/SEPIKA/issues/697)で扱う。

## 変更前の公開再現と原因

元Issue本文をGitHub connectorから独自に取得し、独立文書を
`crates/sepika-io/tests/fixtures/cft_standard_steel.stb` に保存した。
300×200 mm、同厚9 mmの組立BOX、鋼管SN400B、充填Fc36、部材長3000 mmで、通常鋼断面は含まない。
元Issueでは独立全文の公式2.0.2 XSD適合が報告されている。今回そのXSD検証は再実行していない。

実装未変更で一時的な `cft_standard_steel_density_baseline` 試験を実行した。
`import_stbridge` と `import_stbridge_with_report` は両方成功し、各モデルの `validate` が成功、
両モデルは `eq_ignoring_dofmap` で一致した。両方でSN400Bの密度は0、
`element_mass_properties` は「CFT断面CFT1の鋼管材料の密度が不正です」を返した（1試験成功）。
コマンドは `cargo test -p sepika-io --locked --test stbridge_cft_steel_density -- --nocapture`。
一時試験は修正後の成功契約を保証する恒久試験へ置き換えた。

原因は `find_or_create_bar_material` がfyを標準材料解決器、E・ν・密度をGUI用プリセットの完全一致で解決していたこと。
SN400Bは標準材料表にあるが、GUI用プリセットのSN400とは完全一致しないため密度0で生成された。
後段の標準材料準備では名前・fc・fyが一致する既存材料を再利用するので、この密度0が残った。
変更後は既知グレードのE・ν・密度を同じ標準材料解決器から取得し、既存材料再利用と未知材の扱いを維持する。

## 独立期待値と公開入口

`tests/stbridge_cft_steel_density.rs` の恒久試験は次を確認する。

| 観測 | 期待値・許容差 |
| --- | --- |
| 両入口のモデル | `validate` 成功、`eq_ignoring_dofmap` 一致 |
| 鋼管物性 | E=205000 N/mm²、ν=0.3、密度=7.85e-9 t/mm³を完全一致 |
| CFT主材料 | コンクリート、Fc36、充填設計重量23e-6 N/mm³を完全一致 |
| 単位長さ質量 | 下記の独立面積期待値と絶対差1e-15 t/mm未満 |
| 通常鋼SN400B混在 | CFT単独と単位長さ質量が完全一致、通常鋼自体も68.1066 kg/m |
| 既知グレード | SN400B、SN490B、STKR400、プリセットにもあるSN400 |
| 未知グレード | SN400UNKNOWNにfy/密度を補完せず、採用鋼管材料なし・診断と質量Err |
| 密度不正 | 0・負・NaN・無限の鋼管密度では質量Errを維持 |

面積は外側300×200、内側282×182から、鋼管8676 mm²、充填51324 mm²と独立に固定する。
期待質量は `8676 × 7.85e-9 + 51324 × cft_core_mass_density()` [t/mm]。
充填部は既存の解決契約を使用し、その設計重量が23 kN/m³のままであることも別に確認する。
当該入力の充填密度は2.345347289849235e-9 t/mm³、合計0.00018847920430422216 t/mm
（188.47920430422215 kg/m）。鋼管だけの68.1066 kg/mは絶対差1e-12 kg/m未満で確認する。
幾何・密度・単位換算のVerificationであり、実物の物理Validationではない。

## 実行履歴

全Cargo実行では `CARGO_TARGET_DIR=C:/Users/hiroa/Desktop/ikkan/target` を明示した。
Rust stable更新は親担当で実行し、rustc 1.99.1 / clippy 0.1.99（58cf8f9c35 2026-10-13）を確認した。
ローカル生ログは同target直下の `issue741-*.log` に保存し、リポジトリへ追加しない。

- 変更前: `issue741-baseline.log`、1成功。両入口のdensity=0と質量Errを出力。
- 恒久試験初稿: `issue741-focused.log`、テスト側の非公開API・存在しないフィールド参照でコンパイル失敗。公開APIへ訂正。
- 次稿: `issue741-focused-fixed.log`、3成功・1失敗。未知材の採用材料がNoneである既存契約を誤ってunwrapしていたため、未設定Err・診断・実材料行のfy/密度に分けて観測を訂正。製品コードは変更しない。

| コマンド | 結果 | 生ログ |
| --- | --- | --- |
| `cargo test -p sepika-io -p sepika-core --locked` | 合計822成功・失敗0・既存ignored3。core lib559＋統合30、IO lib177＋統合56。CFT直接4成功を含む | `issue741-io-core.log` |
| `cargo test -p sepika-io --locked --test stbridge_standard_rebar` | RC/SRC両公開入口matrixへE・ν・密度assertを追加後、4成功。既存の明示任意物性/OVIKA保持も含む | `issue741-rebar-final.log` |
| `cargo clippy -p sepika-io --all-targets --locked -- -D warnings` | 成功 | `issue741-clippy.log` |
| `cargo test -p sepika-app --lib --features gui --locked stbridge` | 6成功・失敗0・846 filtered。読込後DL計算、参照失敗時状態保持、既存書出し契約を含む | `issue741-app-stbridge.log` |
| `cargo fmt --all --check` | 補正後の最終実行成功 | `issue741-fmt-final.log` |
| `cargo run -p xtask --locked -- check-docs` | 成功（summary123・links605・impl refs653） | `issue741-check-docs.log` |
| `mdbook build` | 成功。生成HTMLにCFT鋼管の追記があることも確認 | `issue741-mdbook.log` |

共有helperの利用側であるSRC内蔵鋼・主筋・せん断補強筋にも標準E・ν・密度を照合する。
IO全体の既存未知SRC鋼材・明示物性保持回帰も成功しており、未知名推定や明示材料の上書きは導入しない。
全回帰の開始後に鉄筋matrixへassertを追加したため、その追加観測の証拠は別の最終4成功runとする。
最初の `cargo fmt --all --check` は鉄筋matrix追加箇所の改行差分を検出し、`cargo fmt --all` で補正した。
開発文書3ファイルのローカルリンク先はすべて存在し、新規レポートの参照見出しも存在することを確認した。
GUI featureの利用側確認は上記の6試験に限定し、App全体846 filteredの実行済みとは扱わない。

## 未確認範囲

全形状・全鋼種・全XSD適合、相互運用、手動GUI、骨組応答・設計式の精度、実験Validationは今回の確認対象に含めない。
独立レビュー、親監査、最終headの全CI・通常mergeは公開PRで別途確認する。
