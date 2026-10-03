# OVIKA の named MessagePack 検証

作成日: 2026-10-03

## 対象と条件

Issue #374 の named 化について、保存・復元の正しさとサイズ・codec 時間を確認しました。
比較の基点は `92153a9f`、作業ブランチは `refactor/374-ovika-named-msgpack` です。
版番号据え置きと開発中ファイルへの互換層を設けない条件は、Issue 原文よりユーザー合意を優先しました。
現行仕様は [OVIKA](../../docs/model_io/01_プロジェクト形式_ovika.md)、方針は [ADR 0014](../adr/0014-schema-compatibility.md) を参照してください。

## 保存・復元の直接検証

| 検証 | テストと結果 |
|---|---|
| モデルの named 保存、入れ子のフィールド順序独立、追加フィールドの default | `sepika-io::ovika::tests::saved_model_fields_are_named_and_order_independent`：保存した実バイトを宣言順の異なる型で復号し、モデル復元後の `validate()` と全体一致を確認 |
| 実際のモデル型の default 追加フィールド | `named_node_reads_added_default_field`：並びを逆にした名前付き節点を `Node` へ復号し、欠落した `support_spring` を補完 |
| 壊れたモデル | `corrupt_model_with_matching_hash_returns_decode_error`：ハッシュが一致する不正 MessagePack を `load_ovika` が `Decode` エラーにすることを確認 |
| 4 payload の実モデル round-trip | `sepika-app/tests/full_model.rs::ovika_roundtrip_preserves_model_and_results`：STB 取り込み・準備計算・静的・固有値・設計検定後にアプリの保存・読込入口を実行。モデル全体、準備計算全体、設定全体、結果 bundle 全体を比較し、復元結果の件数・固有値結果も確認 |
| 任意 payload の map・順序独立・追加 default | 同テスト：3 payload のトップレベルを map として復号。準備計算・結果・設定を宣言順の異なる型へ復号し、追加 default を確認。設定の入れ子 `cfg` も順序を変えて復号 |
| 保存設定の default フィールド | `ovika_reads_defaulted_settings_fields`：質点系波形選択フィールドを省いた named 設定を実際の保存型へ復号し、アプリからも復元 |
| 任意 payload の破損 | `ovika_corrupt_optional_payloads_report_notices`：3 payload の復号失敗を注意として報告し、モデル読込は継続、準備計算・結果は未復元、設定は変更しないことを確認 |
| 復元モデルの検証 | `ovika_invalid_restored_model_is_not_installed`：節点 ID 重複を持つファイルをアプリが拒否し、元モデルを保持 |

既存の版不一致拒否・ハッシュ不一致・必須エントリ・任意エントリの opaque bytes 往復テストも維持しています。
旧位置形式の拒否や migration 専用のテストは追加していません。

## 実モデルのサイズ・時間比較

### 測定対象

入力は `crates/sepika-app/tests/fixtures/model.stb` を `import_stbridge_with_report` で取り込んだモデルです。
節点 166、解析要素 115、二次部材 56、床領域 26、断面 37 を測定時に確認しました。
**サイズ比較はモデル単体の OVIKA**（`manifest.json`・`model.msgpack`・`settings.json` の 3 エントリ）で、準備計算・結果・解析設定は同梱していません。
準備計算・解析を行う前の同一 `Model` に対し、変更前と同じ `to_vec` と変更後の `to_vec_named` を比較しました。

named 側は実際の `save_ovika` で保存しました。
位置形式側は既存のテスト用 ZIP writer を利用し、named 側の manifest と settings を保持したままモデルのバイト列と SHA-256 のみを置き換えました。
両側でエントリ順、Deflate 圧縮の既定オプション、JSON の整形を揃えています。
この writer は fsync を行わないため保存処理時間の比較には使用していません。
両形式の最終ファイルを `load_ovika` で読み、`validate()` と元モデルとの全体一致を確認しました。

### 実行環境・方法

- OS：Windows、`x86_64-pc-windows-msvc`
- CPU：Intel Core Ultra 7 258V（8 コア・8 論理プロセッサ）
- Rust：`rustc 1.99.0 (b940084d7 2026-09-28)`
- 依存：`Cargo.lock` 固定、rmp-serde 1.3.1、zip 2.4.2
- ビルド：release、テストスレッド 1、他の検証コマンドとは同時実行しない
- 計測：各形式で encode/decode を 10 回ウォームアップ。各 1,000 回を 7 バッチ測り、1 回あたりの平均時間の中央値と最小〜最大を記録
- 最適化対策：入力と出力を `std::hint::black_box` に渡す
- 時間の範囲：MessagePack codec と出力の確保・破棄。STB 読込・解析・ZIP 圧縮展開・SHA-256・ファイル I/O・fsync・`validate()` は含めない

再実行コマンドは以下です。
初回は release ビルドが 120 秒の実行上限を超えましたが、600 秒の上限で再実行し完了しました。

```text
cargo test -p sepika-io --release --locked measure_real_model_msgpack -- --ignored --nocapture --test-threads=1
```

### 結果

| 形式 | MessagePack [byte] | 最終 OVIKA [byte] | serialization 中央値 [µs]（範囲） | deserialization 中央値 [µs]（範囲） |
|---|---:|---:|---:|---:|
| 位置形式（`to_vec`） | 51,323 | 8,237 | 43.808（41.672〜44.629） | 101.811（93.305〜110.143） |
| named/map（`to_vec_named`） | 111,641 | 10,441 | 77.817（76.338〜81.768） | 136.746（136.039〜139.675） |

名前を保持することで MessagePack 自体は約 2.18 倍になりましたが、最終 OVIKA は 2,204 byte（約 26.8%）の増加でした。
このモデルでは serialization は約 1.78 倍、deserialization は約 1.34 倍でした。

### 限界

これは 1 モデル・1 環境・1 回の測定系列であり、統計的な性能保証ではありません。
4 payload 込みのサイズ・時間、保存／読込のエンドツーエンド時間、巨大な時刻歴結果は測定していません。
4 payload の正しさは別の実モデル round-trip テストで検証しているため、この性能値から結果 payload の性能を推定しないでください。
測定テストは明示実行用の `#[ignore]` とし、通常テストに時間の閾値は設けていません。

## 検証コマンド

以下の検証はいずれも成功しました。
clippy 前に `rustup update stable` を実行し、stable が 1.99.0 で最新であることを確認しました。

```text
cargo test -p sepika-io --locked
cargo test -p sepika-app --locked
cargo test -p sepika-app --test full_model ovika_ --locked
cargo test --workspace --locked
cargo test -p sepika-app -p sepika-mcp -p sepika-io --features sepika-app/gui,sepika-mcp/mcp,sepika-io/parquet --locked
cargo check -p sepika-app --features sepika-app/gui --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p sepika-app -p sepika-mcp -p sepika-io --all-targets --features sepika-app/gui,sepika-mcp/mcp,sepika-io/parquet --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p xtask -- check-docs
mdbook build
cargo run -p xtask -- check-deps
git diff --check
```

通常テストで ignore される性能 probe は、今回の codec 計測のみを明示実行しました。
`cargo audit`・`cargo deny` は実行していません。
