# ctutils 推移依存の監査（Issue #775）

## 対象と根拠

基準は main `6810dbee70db43f44beb75cdcd33e035d29bc7d8`。 [失敗した CI](https://github.com/hrntsm/SEPIKA/actions/runs/39956133587) では [cargo audit](https://github.com/hrntsm/SEPIKA/actions/runs/39956133587/job/120422314401) が `ctutils 0.4.3` の RUSTSEC-2026-0385 を 1 件検出し、[cargo-deny](https://github.com/hrntsm/SEPIKA/actions/runs/39956133587/job/120422314179) も同勧告で失敗した。

[公式 RustSec 勧告](https://github.com/RustSec/advisory-db/blob/main/crates/ctutils/RUSTSEC-2026-0385.md) の修正版は `>=0.4.4`。[上流変更履歴](https://github.com/RustCrypto/utils/blob/master/ctutils/CHANGELOG.md) は 0.4.4 の定数時間実装修正と、同日公開の 0.4.5 を記載する。SEPIKA での秘密値への到達や悪用可能性は判定していない。

更新前後の `cargo tree -i ctutils --locked` では、`zip 9.0.5` の `pbkdf2`・`hmac`・`sha1` と `lzma-rust2` の `sha2` が `digest 0.11.3` を介して `ctutils` に到達し、SEPIKA 側は `sepika-io` とその利用側 `sepika-app`・`sepika-mcp` に到達する。`cargo update -p ctutils --precise 0.4.5` 後の `Cargo.lock` 差分は `ctutils` の版と checksum の 2 行だけで、依存辺・他パッケージは不変。

## 実行証拠

2026-10-23、専用 `CARGO_TARGET_DIR=C:/Users/hiroa/Desktop/ikkan/target/issue775-isolated` で実行。ログは `C:/Users/hiroa/Desktop/ikkan/target/issue775-evidence/` に保存した。RustSec DB は `f7b2f6bc`。

| 検証 | 結果 |
| --- | --- |
| `cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195` | exit 0。既存の保守終了・yanked 警告 8 件を維持 |
| `cargo deny check` | exit 0。advisories、bans、licenses、sources が成功。既存設定の警告を維持 |
| `cargo test -p sepika-io --locked` | exit 0。240 件成功、既存の計測用 2 件は ignored。OVIKA の保存・読込、ZIP 破損・サイズ境界の拒否を含む |
| `cargo test -p sepika-io --features parquet --locked` | exit 0。251 件成功、既存の計測用 2 件は ignored。parquet 有効時も同じ OVIKA/ZIP 経路を通過 |

本変更は推移的依存のパッチ更新であり、数値式・GUI・モデル・保存形式・解析39／検定25の契約を変えない。製品利用者向けの現在仕様に変更がないため `docs/` は更新しない。

## 検証の限界

監査はロック解決と既存ポリシーを検査する。定数時間特性の独自測定、悪用可能性の判定、構造計算の Validation は含まない。App/GUI/MCP の全構成はローカルでは実行しておらず、最終 head の全 CI・独立レビューを PR で確認する。
