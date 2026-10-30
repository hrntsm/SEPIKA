# OVIKA・波形ライブラリの内部形式とAPI

保存対象・読込・再利用の条件は[OVIKAの製品仕様](../docs/model_io/01_プロジェクト形式_ovika.md)を正本とする。本書は内部形式と呼出側の実装用情報を扱う。互換方針は[ADR 0014](adr/0014-schema-compatibility.md)に従う。

## コンテナと直列化

`.ovika` の内部は現在 ZIP コンテナです。`manifest.json`・`model.msgpack` は必須エントリ、
残りは任意エントリ（同梱がなければ読込側の該当項目が `None` になる）です。

| エントリ | 必須 | 内容 |
|---|---|---|
| `manifest.json` | ○ | スキーマ版、単位系、各エントリの SHA-256 ハッシュ |
| `model.msgpack` | ○ | 内部モデル本体（MessagePack でシリアライズ） |
| `preparation.msgpack` | - | 準備計算の結果（GUIの同梱条件は製品仕様を参照） |
| `results.msgpack` | - | 解析・検定結果（GUIの同梱条件は製品仕様を参照） |
| `studies.msgpack` | - | 検討と保持した計算根拠 |
| `analysis_settings.msgpack` | - | 解析タブの設定値（立体時刻歴と質点系それぞれの波形パラメータ・減衰、固有値／質点系のモード数など） |

任意エントリを含む MessagePack エントリでは、struct をフィールド名付きの map（named MessagePack）として保存します。
入れ子の struct も名前を保持するため、Rust のフィールド宣言順序を変更しても読み込みは順序に依存しません。
任意エントリの直列化はアプリ側が行い、IO 層は内容を解釈しないバイト列として保存・返却します。

型の定義とAPI契約は[ovika.rs](../crates/sepika-io/src/ovika.rs)、付随データの生成・復元は[app/actions/io.rs](../crates/sepika-app/src/app/actions/io.rs)、検討の直列化は[app/study.rs](../crates/sepika-app/src/app/study.rs)を参照する。

## スキーマ変更の確認

`manifest.json` の `schema_version` は、モデルと任意エントリを含む永続化スキーマの版です。
現行値は `1` で、読み込み時にはこの値との一致を確認します。
ただし初回リリースまでは、版を `1` に据え置いたまま破壊的変更を行い、開発中に保存したファイルの後方互換は保証しません。
同じ版でも過去のファイルを読み込める保証はなく、旧形式への変換や互換層は提供しません。

named 形式はフィールド順序への依存をなくしますが、フィールド名・型・意味の変更まで自動的に互換にするものではありません。
たとえば `#[serde(default)]` を持つ追加フィールドは欠落時に補完できますが、その値が計算上も適切かは別に確認する必要があります。
struct の未知フィールドは通常無視されますが、フィールド削除に伴う情報の喪失が許されることを意味しません。

初回リリースは Git タグで判定します。
初回リリース後に永続化スキーマを変更するときは版を上げ、リリース済みファイルからの明示的な migration を実装します。
順序だけの変更は named 表現のスキーマ変更に当たりません。
それ以外の変更は、以下の観点でデータの保存・復元と計算上の意味を確認します。

| 変更 | 確認する事項 |
|---|---|
| フィールド追加 | 欠落を default 等で読めるか、補完値が適切か |
| フィールド削除 | 未知フィールドを無視しても必要な情報が失われないか |
| フィールド名変更 | 旧名から新名への明示的な変換が必要か |
| 型変更 | 値の範囲・単位・表現を保って変換できるか |
| enum variant の追加・削除・意味変更 | 未知 variant や廃止値をどう扱うか、計算の意味が保たれるか |
| 構造の分割・統合 | 対応する値を欠落・重複なく移せるか |
| 任意エントリの変更 | producer 側の具体型の変更と、同梱がない場合の挙動が整合するか |

リリース済みスキーマを移行する場合は、その版の型へ decode してから現行型へ変換します。
現行型への直接 decode を migration の代用にはしません。
現在は版間の変換を行わず、未対応版は MessagePack の decode 前に `UnsupportedVersion` として拒否します。

<div class="impl-ref">

**実装参照**：`sepika_io::ovika::CURRENT_SCHEMA_VERSION` と `load_ovika`（`crates/sepika-io/src/ovika.rs`）が現行版と版一致チェックを定義します。

</div>

## OVIKA API利用例

<details>
<summary>Rust コード例（開発者向け）</summary>

`Model::section` と `Model::element_section`、二次部材の断面用途検査は、添字位置の ID が一致する断面を優先し、
なければ同じ ID を持つ先頭の断面を選びます。未割当・参照先欠落は断面取得で `None` になります。
この検索規則は不整合なモデルの保存許可を意味せず、保存・読込では参照 ID の検査を別に行います。

```rust
use sepika_io::ovika::{save_ovika, load_ovika, OvikaExtras};
use std::path::Path;

// 保存: 内部モデル＋任意エントリ（各バイト列は呼び出し側が事前に
// rmp_serde::to_vec_named で直列化したもの。io 層は中身を解釈しない）→ .ovika
save_ovika(Path::new("model.ovika"), &model, OvikaExtras {
    studies: Some(&study_bytes),
    preparation: Some(&preparation_bytes),
    results: Some(&results_bytes),
    analysis_settings: Some(&analysis_settings_bytes),
})?;

// 読込: .ovika → 内部モデル＋同梱されていれば各任意エントリ
let contents = load_ovika(Path::new("model.ovika"))?;
let model = contents.model;
```

</details>

## 波形ライブラリAPI利用例

<details>
<summary>Rust コード例（開発者向け）</summary>

```rust
use sepika_io::wave_library::{wave_library_dir, list_wave_library, register_wave, wave_sha256};

let dir = wave_library_dir().expect("ユーザーデータディレクトリを特定できません");
let names = list_wave_library(&dir)?;          // 登録済みの波形ファイル名一覧
let name = register_wave(&dir, src_path)?;     // 波形 CSV をライブラリへコピー（同名は上書き）
let hash = wave_sha256(&dir, &name)?;          // 内容の SHA-256（16進小文字）
```

</details>

## コンテナ検査と保存処理

- **ハッシュ検証**: 読込時に各エントリの SHA-256 を `manifest.json` の記載と照合し、不一致ならエラーにします。必須エントリ（`model.msgpack`）が manifest に列挙されていない場合も拒否します（未検証のまま読み込ませないためです）。
- **余分なエントリ**: 利用対象外のエントリは設定として取り込みません。manifest に列挙されている場合は、同じ SHA-256 検証と展開サイズ上限を適用します。
- **スキーマ版チェック**: 本ソフトはリリース前で後方互換を持たないため、現行スキーマ版（`1`）以外は拒否します。
- **原子的な保存**: 一時ファイルへ書き出して `fsync` した後に `rename` することで、電源断でファイルが破損しないようにしています。
- **ZIP 爆弾対策**: 1 エントリあたりの展開サイズ上限（4 GiB）を超える書庫は拒否します。現在の `.ovika` は単層 zip（deflate）で展開率は理論上約 1032 倍が上限のため、絶対サイズ上限がメモリ保護として機能します。ヘッダの申告サイズは信用せず、実際の展開バイト数でも検査します。

## MCPとの境界

MCP の DesignCheck は毎回新しい解析を行い、OVIKA の保存応力を再利用しません。
MCP の Parquet/FsResultStore の result_get は別の保存経路で、この OVIKA の版・入力照合の対象には含みません。
