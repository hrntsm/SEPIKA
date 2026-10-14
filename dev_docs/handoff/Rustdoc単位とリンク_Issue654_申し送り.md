# Rustdoc の単位表記と項目リンク

作成日: 2026-10-15

対象: [Issue #654](https://github.com/hrntsm/SEPIKA/issues/654)（親 [#524](https://github.com/hrntsm/SEPIKA/issues/524)）。

## 修正範囲

既存 Rustdoc の角括弧付き単位・配列添字・区間・行列記号が項目リンクとして解釈される箇所をコード表記にした。
移動済みの公開型・項目は現在の公開パスへリンクし、公開 API 文書に生成されない private 項目はコード表記にした。
モジュールの文書でスコープ依存の参照が解決しない箇所も、公開パスを明示した。

計算実装、公開 API の型・意味、保存契約、CI の警告設定は変更していない。

## ローカル検証

Windows、stable rustc / rustdoc 1.99.0。修正前の通常 default 生成は exit 0 だが、項目リンク警告を 925 件出力した。
修正後は以下の 4 構成で API 文書を生成し、すべて exit 0、警告 0 件を確認した。

```powershell
cargo doc --workspace --no-deps --locked
cargo doc --workspace --no-deps --locked --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet
$env:RUSTDOCFLAGS = '-D warnings'
cargo doc --workspace --no-deps --locked
cargo doc --workspace --no-deps --locked --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet
```

`RUSTDOCFLAGS` は検証後に元の値へ復元した。feature 構成では MCP サーバー、GUI、Parquet 結果 API が生成対象に含まれることを確認した。
生成 HTML で単位のコード表示と、壁版・床板・数量積算・危険断面位置の公開項目リンク先を確認した。
`cargo fmt --all --check` と差分の空白検査も成功した。
Rust の変更行が Rustdoc に限られ、通常コメント・実装が同一であることを差分で確認した。

## 残課題

本 Issue の対象警告は解消した。CI の警告設定強化は本変更の対象外。
