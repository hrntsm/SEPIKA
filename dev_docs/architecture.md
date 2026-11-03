# アーキテクチャ

SEPIKA は 14 のクレートから成る階層型アーキテクチャで構成されています。

```
Layer 0: sepika-core（基本データ構造・DOF 管理・荷重組合せ・壁解析モデル生成）、sepika-math（疎行列・ソルバ）、
         sepika-material（一軸材料履歴則）
Layer 1: sepika-section（断面性能算定）、sepika-load（Ai 分布・床/壁/部材の荷重・重量・質量）
Layer 2: sepika-edit（編集トランザクション）
Layer 3: sepika-element（梁・板・仕口パネル要素）
Layer 4: sepika-solver（各種解析）、sepika-io（結果 I/O）
Layer 5: sepika-design-jp（日本仕様設計計算）
Layer 6: sepika-job（解析前処理・解析条件・解析の純粋計算）
         sepika-ai-connection（GUI接続のパイプ・行フレーム・要求応答の搬送）
Layer 7: sepika-mcp（MCP サーバ）、sepika-app（GUI アプリケーション）
```

`sepika-job` は GUI と MCP サーバの共通下層です。
両者は同じ解析を別々の入口から実行するため、前処理（剛域・仕口パネル要素）と解析条件をここに集約し、**同じモデルに対して同じ結果を返す**ことを保証しています。

GUIはモデル・解析状態と表示・選択状態を分離します。編集は編集コマンドを通し、
計算とモデルの処理を画面へ複製しません。所有権と編集経路は
[ADR 0003](adr/0003-single-writer-edit-commands.md)、GUIと外部AIの境界は
[ADR 0087](adr/0087-gui-owned-study-and-local-ai.md)を参照してください。

依存方向は上層から下層のみと定めているため、循環依存が生じていないかを次のコマンドで検出します。

```bash
cargo run -p xtask -- check-deps
```

## クレート一覧

| クレート | 役割 |
|----------|------|
| `sepika-core` | 基本データ構造・DOF 管理・荷重組合せ・壁解析モデル生成 |
| `sepika-math` | 疎行列・ソルバ |
| `sepika-material` | 一軸材料履歴則 |
| `sepika-section` | 断面性能算定 |
| `sepika-element` | 梁・板・仕口パネル要素 |
| `sepika-load` | Ai 分布・床/壁/部材の荷重・重量・質量 |
| `sepika-solver` | 各種解析 |
| `sepika-design-jp` | 日本仕様設計計算 |
| `sepika-io` | 結果 I/O |
| `sepika-edit` | 編集トランザクション |
| `sepika-job` | 解析前処理・解析条件・解析の純粋計算（GUI と MCP の共通下層） |
| `sepika-ai-connection` | GUI/MCP間のローカル通信。モデル・許可・受理台帳はGUIが所有 |
| `sepika-mcp` | MCP サーバ |
| `sepika-app` | GUI アプリケーション |

## 関連情報

- [設計判断](adr/README.md)
- [APIリファレンス](https://hrntsm.github.io/SEPIKA/api/sepika_core/index.html)
