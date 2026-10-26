# CFT充填コンクリートFc下限の回帰検証（Issue #916）

## 根拠と保証範囲

[国土交通省公開の平成14年告示第464号](https://www.mlit.go.jp/notice/noticedata/pdf/201703/00006524.pdf)第三2の充填コンクリートの設計基準強度24 N/mm²以上を境界の独立根拠とする。第十・第十一の適用範囲は第三2の下限を除外しない。#887時点の公開入口での未判定は[原典照合記録](CFT適用材料許容耐力_Issue887_原典照合.md)を参照。

現行製品仕様の正本は[SRC/CFT断面検定](../../docs/calc_basis/06_一次設計/04_SRC_CFT_断面検定.md#cft-の適用条件と別途確認)。Fcの補完、別認定の例外、終局式・施工条件・他の適用資格の判定は追加していない。本検証はFc下限のVerificationであり、CFT全体の規準適合、耐力式の物理Validation、専門家サインオフや手動ネイティブGUIの証拠ではない。

## 独立境界と製品入口

| Fc [N/mm²] | 期待する公開入口・製品検定結果 |
| ---: | --- |
| 21 | 下限と入力値を含む理由付き未算定。比の合格にしない |
| 23.999 | 同上。許容差で24へ丸めない |
| 24 | 既存の許容検定を継続でき、小需要は合格 |
| 24.001 | 同上 |

| 要素・入口 | 永続テストと観測 |
| --- | --- |
| 公開`CftDesign::check` | `crates/sepika-design-jp/tests/cft_concrete_strength.rs::cft_concrete_strength_statutory_lower_bound`。等厚400×300×12 mm・角R0と円形400×12 mm、L3000 mm、SN400B/Es205000 N/mm²、Nraw=−1000 N・他0、長短期×4値の16条件。下限未満は`Skipped`の条項・下限・実入力値、以上は`Checked`の`ok()`、入力Fc保持を検証 |
| 実App求解→検定→レポート | `app::result_contract_tests::cft916_app_analysis_and_report_distinguish_concrete_strength_lower_bound`。既存の1柱fixture（400×400×12 mm・角R0、L7200 mm）に圧縮1000 Nを与え、Fc変更後に`run_linear_static`と`run_design_check`を実行。現行表示用成果と実CSVで下限未満の理由付き「検定不能」、以上の比0・OKを4値で検証。入力Fcを保持 |
| 旧保存済み合格の再利用拒否 | `app::result_contract_tests::cft916_old_saved_passing_design_expires_and_recheck_reports_low_fc`。Fc21の解析成果に旧合格結果を置くfixtureを設計契約33で実OVIKA保存・読込。現行解析と内力は保持し、旧合格・表示/CSVを失効。再保存・再開後も復活せず、明示再検定は理由付き未算定、設計契約34となる |

App試験は製品の実求解・検定・CSV生成を直接呼ぶ自動Verificationである。マウス操作によるGUI表示や任意建物の適合を確認したものではない。設計契約版の更新は[ADR 0014](../adr/0014-schema-compatibility.md)に従い保存済み旧判定の迂回を防ぐもので、解析式・解析契約は変更しない。

## ローカル検証

| コマンド | 結果 |
| --- | --- |
| `rustup update stable`、`cargo clippy --version` | stable更新確認（rustc 1.99.1、Clippy 0.1.99） |
| `cargo test -p sepika-design-jp --test cft_concrete_strength` | 1試験・16条件成功 |
| `cargo test -p sepika-app --lib cft916` | 2試験成功。境界4値と実OVIKA旧合格の失効・再検定 |
| `cargo test -p sepika-design-jp -p sepika-core --locked` | 21 targets合計1319成功・既存1 ignored・失敗0 |
| `cargo test -p sepika-app --locked` | 16 targets合計519成功・既存1 ignored・失敗0 |
| `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-app --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check`、`git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | 成功（summary124・links659・impl refs706） |
| `mdbook build` | 成功 |

既定featureの直接経路を検証した。GUI featureの全テスト、手動GUI、外部MCP transport、実験Validation、独立レビューと最終CIは本記録時点では未確認。
