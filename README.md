# SEPIKA

Integrated Structural Analysis

<div align="center">
<img src="./crates/sepika-app/assets/squid.png" width="25%">
</div>

SEPIKA（セピカ）は、日本の建築構造計算一貫プログラム。Rust で実装。
SEPIKA は Sepia（コウイカ）と IKA を組み合わせた名前で、日本語の「一貫計算（IKKAN）」に由来する。
モデル作成 → 荷重 → 解析（静的・固有値・地震・プッシュオーバー・時刻歴）→ 検定・設計 → レポートまでを
デスクトップ GUI または MCP サーバから扱う。

## アーキテクチャ

**14** のクレートから成る階層型アーキテクチャ（詳細は [dev_docs/architecture.md](dev_docs/architecture.md)）:

```
Layer 0: sepika-core（基本データ構造・DOF 管理・荷重組合せ）、sepika-math（疎行列・ソルバ）、
         sepika-material（一軸材料履歴則）
Layer 1: sepika-section（断面性能算定）、sepika-load（Ai 分布・床荷重）
Layer 2: sepika-edit（編集トランザクション）、sepika-skeleton（スケルトン曲線）
Layer 3: sepika-element（梁・板・パネルゾーン要素）
Layer 4: sepika-solver（各種解析）、sepika-io（結果 I/O・ST-Bridge）
Layer 5: sepika-design-jp（日本仕様設計計算）
Layer 6: sepika-job（解析前処理・解析条件・解析の純粋計算）
Layer 7: sepika-mcp（MCP サーバ）、sepika-app（GUI アプリケーション）
```

`sepika-job` は GUI と MCP の共通下層。依存方向は上層から下層のみ。
循環依存は `cargo run -p xtask -- check-deps` で検出する。

## ビルド・開発

手順の詳細（テスト・静的解析・機能フラグ）は [CONTRIBUTING.md](CONTRIBUTING.md) を参照。

```bash
# ワークスペース全体ビルド
cargo build --workspace

# GUI 起動（egui/eframe）
cargo run -p sepika-app --features gui

# MCP サーバ起動（stdio）
cargo run -p sepika-mcp --features mcp
```

## ドキュメント

### 利用者向け（mdBook）

計算根拠・理論・入出力・MCP の使い方は [docs/](docs/) を mdBook でビルドした
[ドキュメントサイト](https://hrntsm.github.io/SEPIKA/) に公開している
（`main` への push で GitHub Pages に自動デプロイ）。
ローカルプレビューは [CONTRIBUTING.md](CONTRIBUTING.md#ドキュメントサイトmdbook) を参照。

主な章: [はじめに](docs/introduction.md) · [モデル入出力](docs/model_io/README.md) ·
[MCP サーバ](docs/mcp_server/README.md) ·
[計算根拠](docs/calc_basis/README.md)

### 開発者向け（`dev_docs/`）

設計判断（ADR）・アーキテクチャ・検証記録・申し送りは [dev_docs/](dev_docs/README.md) に集約（ドキュメントサイトには含めない）。

| 一覧 | 内容 |
|------|------|
| [dev_docs/handoff/残課題一覧.md](dev_docs/handoff/残課題一覧.md) | 実装残りの集約チェックリスト |
| [dev_docs/v_and_v/未検証一覧.md](dev_docs/v_and_v/未検証一覧.md) | V&V 未完了（❌/🔶）の集約チェックリスト |
| [dev_docs/architecture.md](dev_docs/architecture.md) | クレート階層・依存方向 |
| [dev_docs/v_and_v/](dev_docs/v_and_v/README.md) | 検証レポート・要素→テスト索引 |
| [dev_docs/handoff/](dev_docs/handoff/README.md) | 申し送り目録（時系列・カテゴリ別） |

## ライセンス

MIT License (see [LICENSE](LICENSE))

Copyright (c) 2026 Hiroaki NATSUME
