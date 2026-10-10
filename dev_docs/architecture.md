# アーキテクチャ

SEPIKA は 14 のクレートから成る階層型アーキテクチャで構成されています。

```
Layer 0: sepika-core（基本データ構造・DOF 管理・荷重組合せ）、sepika-math（疎行列・ソルバ）、
         sepika-material（一軸材料履歴則）
Layer 1: sepika-section（断面性能算定）、sepika-load（Ai 分布・床荷重）
Layer 2: sepika-edit（編集トランザクション）、sepika-skeleton（スケルトン曲線）
Layer 3: sepika-element（梁・板・仕口パネル要素）
Layer 4: sepika-solver（各種解析）、sepika-io（結果 I/O）
Layer 5: sepika-design-jp（日本仕様設計計算）
Layer 6: sepika-job（解析前処理・解析条件・解析の純粋計算）
Layer 7: sepika-mcp（MCP サーバ）、sepika-app（GUI アプリケーション）
```

`sepika-job` は GUI と MCP サーバの共通下層です。
両者は同じ解析を別々の入口から実行するため、前処理（剛域・仕口パネル要素）と解析条件をここに集約し、**同じモデルに対して同じ結果を返す**ことを保証しています。

`sepika-app` は `App { core: AppCore, ui: UiState }` を持ちます。`AppCore` はモデル・解析状態、`UiState` は選択や表示状態を保持し、モデル差し替え時は `ModelScoped` と `UiModelScoped` を破棄します。幾何選択と個別の注目対象は `UiModelScoped::selection` にまとめ、グリッドの編集範囲や Navigator のケース・断面などの表示文脈とは分けます。egui View は表示と入力取得を担当し、保留編集・Undo/Redo・保存確認・ログ・幾何選択同期は `app/actions/` の App 操作へ渡します。計算とモデルのドメイン処理は Core/Job に置き、View へ複製しません。

材料・断面の再採番情報は `sepika-edit::UndoStack::id_changes` が直前の成功した編集・Undo/Redo の適用順で提供します。逆操作の生成時に実際の挿入・削除を記録し、複合編集は受理した子操作の情報を合成します。`sepika-app` はその情報を使って各表示対象を更新します。表の保留表示対象は描画時の ID が有効な間に設定し、確定した編集を通して最終 ID へ追従させます。編集層は画面状態や Navigator に依存しません。

依存方向は上層から下層のみと定めているため、循環依存が生じていないかを次のコマンドで検出します。

```bash
cargo run -p xtask -- check-deps
```

## クレート一覧

| クレート | 役割 |
|----------|------|
| `sepika-core` | 基本データ構造・DOF 管理・荷重組合せ |
| `sepika-math` | 疎行列・ソルバ |
| `sepika-material` | 一軸材料履歴則 |
| `sepika-section` | 断面性能算定 |
| `sepika-element` | 梁・板・仕口パネル要素 |
| `sepika-skeleton` | スケルトン曲線 |
| `sepika-load` | Ai 分布・床荷重 |
| `sepika-solver` | 各種解析 |
| `sepika-design-jp` | 日本仕様設計計算 |
| `sepika-io` | 結果 I/O |
| `sepika-edit` | 編集トランザクション |
| `sepika-job` | 解析前処理・解析条件・解析の純粋計算（GUI と MCP の共通下層） |
| `sepika-mcp` | MCP サーバ |
| `sepika-app` | GUI アプリケーション |

## API リファレンス

各クレートの API ドキュメント（rustdoc）は、CI で `cargo doc` から生成され、このサイトの [`api/`](https://hrntsm.github.io/SEPIKA/api/sepika_core/index.html) 以下に併設されます。
