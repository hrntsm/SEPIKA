# dev_docs — 開発者向けドキュメント

## ドキュメント配置ルール（正本）

どの情報をどこへ書くかは、本書を唯一の索引・配置ルールとする。
1 つのルールは 1 か所だけを正本とし、詳細はリンク先で管理する。

| 置き場 | 正本とする内容 |
| --- | --- |
| [AGENTS.md](../AGENTS.md) | エージェントの実装・判断原則。コード・テスト・コメントの原則、安全性判断、語彙・ADR、Goal / Scope の扱い |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | 開発参加の基本フロー。言語、コミット形式、ローカル検証の考え方、Issue / PR の手順 |
| [.github/ISSUE_TEMPLATE/](../.github/ISSUE_TEMPLATE/) | Issue 起票時の入力項目 |
| [.github/pull_request_template.md](../.github/pull_request_template.md) | PR で記録する内容。実際にローカルで実行した検証と結果 |
| [.github/workflows/ci.yml](../.github/workflows/ci.yml) | CI で実行する網羅的検証のコマンド・構成 |
| [CONTEXT.md](../CONTEXT.md) | ドメイン語彙 |
| [docs/](../docs/) | GUI から利用する構造設計者向けの製品ドキュメント。現在仕様・理論・計算根拠と、分離した実装参照 |
| [docs_authoring.md](docs_authoring.md) | 製品ドキュメントの執筆・検証・プレビュー手順 |
| [adr/](adr/README.md) | 設計判断と理由。現在有効かどうかは Status で管理 |
| [handoff/](handoff/README.md) | 実装経緯・申し送り・残課題。目録と更新手順はリンク先で管理 |
| [v_and_v/](v_and_v/README.md) | V&V の証拠・未検証項目。目録・要素→テスト索引と更新手順はリンク先で管理 |
| [architecture.md](architecture.md) | クレート構成・依存方向等のアーキテクチャ |
| [theme.rs](../crates/sepika-app/src/theme.rs)・[table_util.rs](../crates/sepika-app/src/table_util.rs) | UI のテーマ・frame helper・表の規約と実装 |
| [full_model.rs](../crates/sepika-app/tests/full_model.rs)・[wall_model.rs](../crates/sepika-app/tests/wall_model.rs) | 実モデル統合テスト固有の情報・実行方法。既知不具合・経緯は [既存 handoff](handoff/実モデル統合テスト_申し送り.md) |
| [book.toml](../book.toml)・[theme/](../theme/)・[docs workflow](../.github/workflows/docs.yml) | mdBook 設定、表示・フッター、公開と API リファレンス生成の実装 |

`docs/` は **GUI から SEPIKA を利用する構造設計者向け**の製品ドキュメントとする。
本文は、理論・計算根拠・利用者から見える現在仕様・制約を中心とする。
実装上の注意事項も「関数 A が X を Y として扱う」ではなく「この機能では X は Y として扱われる」
という利用者向け仕様として説明する。
OSS としてコードまで追えるよう、既存の `実装参照` を本文と分離して維持する。
これは実装位置への導線であり、API ドキュメントや実装詳細の解説、本文の代わりではない。
具体的な執筆規約は [docs_authoring.md](docs_authoring.md) を正とする。

現在の製品仕様・既定値・制約・計算根拠は `docs/` を唯一の正本とし、`dev_docs/` に複製しない。
`dev_docs/` は判断・検証・経緯・アーキテクチャを扱い、製品ドキュメントサイトには含めない。

## 未完了項目へのショートカット

- [残課題一覧](handoff/残課題一覧.md)
- [未検証一覧](v_and_v/未検証一覧.md)
