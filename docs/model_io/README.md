# モデル入出力（ファイル形式）

SEPIKA は構造モデルを 2 つのファイル形式で入出力します。

| 形式 | 拡張子 | 用途 | 保持・受け渡しの範囲 |
|---|---|---|---|
| OVIKA（SEPIKA Project File） | `.ovika` | SEPIKA ネイティブの保存形式。モデルを欠損なく保存・読込する | 完全一致 |
| [ST-Bridge](https://www.building-smart.or.jp/meeting/buildingsmart/st-bridge/) | `.stb` / `.xml` | 他社の一貫計算プログラムや BIM ツールとのモデル受け渡し | 書出は開発中。対応要素でも欠落・近似・再読込の制限あり |

ST-Bridgeは[取込前・直後の確認・再出力](02_ST-Bridge_形式.md#取込前guiから読み込む)の順に確認してください。注意と属性の取込状況は下ドックの「ログ」に表示します。失敗時は現在のモデルを置き換えません。

いずれも GUI のファイルメニューから利用できます。**日常的な保存・読込は `.ovika`**、**他ツール連携は `.stb`** と使い分けます。

<div class="impl-ref">

**実装参照**：`.ovika` は `sepika_io::ovika`（`save_ovika` / `load_ovika`、`crates/sepika-io/src/ovika.rs`）、ST-Bridge は `sepika_io::stbridge`（`export_stbridge` / `import_stbridge`）が入出力を担います。

</div>

本案の有効な結果・過去履歴・検討の保持計算は保存条件が異なります。[OVIKAの保存対象と再読込](01_プロジェクト形式_ovika.md#保存対象と再読込)を確認してください。外部波形は別途管理します。

## この章の内容

- [OVIKA プロジェクト形式（.ovika）](01_プロジェクト形式_ovika.md)
- [ST-Bridge 形式（.stb / .xml）](02_ST-Bridge_形式.md)
- [ST-Bridge 要素別 変換状況一覧](03_ST-Bridge_要素別変換状況.md)
- [波形ライブラリ](04_波形ライブラリ.md)

## MCP サーバでのモデル入力

MCP サーバのモデル入力には制約があります。詳細は [MCP の制約・注意点](../mcp_server/05_制約・注意点.md)を参照してください。
