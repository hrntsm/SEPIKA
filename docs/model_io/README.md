# モデル入出力（ファイル形式）

SEPIKA は構造モデルを 2 つのファイル形式で入出力します。

| 形式 | 拡張子 | 用途 | 往復精度 |
|---|---|---|---|
| OVIKA（SEPIKA Project File） | `.ovika` | SEPIKA ネイティブの保存形式。モデルを欠損なく保存・読込する | 完全一致 |
| [ST-Bridge](https://www.building-smart.or.jp/meeting/buildingsmart/st-bridge/) | `.stb` / `.xml` | 他社の一貫計算プログラムや BIM ツールとのモデル受け渡し | サブセットのみ意味的一致 |

いずれも GUI アプリ（`sepika-app`）のファイルメニューから利用でき、実装は `sepika-io` クレートにあります。**日常的な保存・読込は `.ovika`**、**他ツール連携は `.stb`** と使い分けます。

<div class="impl-ref">

**実装参照**：`.ovika` は `sepika_io::ovika`（`save_ovika` / `load_ovika`、`crates/sepika-io/src/ovika.rs`）、ST-Bridge は `sepika_io::stbridge`（`export_stbridge` / `import_stbridge`）が入出力を担います。

</div>

## この章の内容

- [OVIKA プロジェクト形式（.ovika）](./01_プロジェクト形式_ovika.md)
- [ST-Bridge 形式（.stb / .xml）](./02_ST-Bridge_形式.md)
- [ST-Bridge 要素別 変換状況一覧](./03_ST-Bridge_要素別変換状況.md)
- [波形ライブラリ](./04_波形ライブラリ.md)

## MCP サーバでのモデル入力

MCP サーバのモデル入力には制約があります。詳細は [MCP の制約・注意点](../mcp_server/05_制約・注意点.md)を参照してください。
