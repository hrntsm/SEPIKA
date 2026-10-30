# dev_docs — 開発者向けドキュメント

## ドキュメント配置ルール（正本）

どの情報をどこへ書くかは、本書を唯一の索引・配置ルールとする。
1 つのルールは 1 か所だけを正本とし、詳細はリンク先で管理する。

| 置き場 | 正本とする内容 |
| --- | --- |
| [README.md](../README.md) | 初見の読者向けのプロジェクト紹介と各正本への入口 |
| [AGENTS.md](../AGENTS.md) | エージェントの実装・判断原則。コード・テスト・コメントの原則、安全性判断、語彙・ADR、Goal / Scope の扱い |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | 開発参加の基本フロー。言語、コミット形式、ローカル検証の考え方、Issue / PR の手順 |
| [GitHub Issues](https://github.com/hrntsm/SEPIKA/issues) | 未完了の開発課題・不具合報告・提案 |
| [.github/ISSUE_TEMPLATE/](../.github/ISSUE_TEMPLATE/) | Issue 起票時の入力項目 |
| [.github/pull_request_template.md](../.github/pull_request_template.md) | PR で記録する内容。実際にローカルで実行した検証と結果 |
| [.github/workflows/ci.yml](../.github/workflows/ci.yml) | CI で実行する網羅的検証のコマンド・構成 |
| [CONTEXT.md](../CONTEXT.md) | ドメイン語彙 |
| [docs/](../docs/) | GUI から利用する構造設計者向けの製品ドキュメント。現在仕様・理論・計算根拠と、分離した実装参照 |
| [docs_authoring.md](docs_authoring.md) | 製品ドキュメントの執筆・検証・プレビュー手順 |
| [adr/](adr/README.md) | 設計判断と理由。現在有効かどうかは Status で管理 |
| [handoff/](handoff/README.md) | 過去の実装経緯・判断の履歴。未完了作業の状態はGitHub Issuesで管理 |
| [v_and_v/](v_and_v/README.md) | V&V の証拠・未検証項目。目録・要素→テスト索引と更新手順はリンク先で管理 |
| [architecture.md](architecture.md) | クレート構成・依存方向等のアーキテクチャ |
| [GUIの文字と状態表示](ui_readability.md) | 共通の文字・余白・選択・フォーカス・状態表示規約 |
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

## 資料と比較の記載ルール（正本）

`docs/` と `dev_docs/` では、SEPIKA と同種の構造解析・構造設計ソフト名（製品名・略称を含む）と、
その文脈で挙げる他社名を、見出し・本文・表・脚注・リンク表示名に記載しない。
必要な技術比較は方式・適用条件・制限の違いを匿名で説明し、有用な比較や独立した検証結果を維持する。
材料・構造部材を識別する固有商品名・材種・型番・必要なメーカー名は保持する。
一般の開発ツール・エディタ・表計算ソフト・MCP クライアント等の名称も削除対象に含めない。
法令・告示・規準の必要な特定情報や、学術的・技術的な標準名称は区別して扱う。

- `docs/` には他の構造解析・構造設計ソフトの資料紹介・リンク・照合経緯を残さず、SEPIKA の採用方式と適用条件を説明する。
  GUI 利用者向けの具体的な書き方は [docs_authoring.md](docs_authoring.md#本文) に従う。
- `dev_docs/` では、公開元で公開を実際に確認した資料の URL を記載してよい。
  表示名には上記の削除対象の名称を含めない。URL 自体にそれらを含む場合は例外として許容するが、完全な匿名化にはならない。
  URL が記載されているだけで公開済みとは判断しない。
- 提供・添付された資料など公開可否を確認できない資料は、非公開と断定せず、
  資料名・ファイル名・ページ・引用・受領や照合の履歴・ハッシュ等、その資料への言及を残さない。
  同じ内容を公開元で確認できる場合は、公開 URL に基づく説明へ置き換えてよい。
- 資料本文の抽出テキスト等を保存する場合は、公開元と転載・利用条件を確認する。
  必要な条件を確認できない本文はリポジトリに保存・転載しない。
- 整理対象の資料だけに依存した計算根拠・検証結果を、独立した根拠なしに確認済みとしない。
  公開原典・コード・独立したテスト等で裏付けられる範囲と、未確認の範囲を明記する。

## 開発作業と検証状態への入口

- [未完了の開発作業（GitHub Issues）](https://github.com/hrntsm/SEPIKA/issues)
- [handoffのIssue移行判定記録](handoff/handoffのIssue移行_Issue430_判定記録.md)（移行時点の履歴）
- [未検証一覧](v_and_v/未検証一覧.md)

## 利用マニュアルの配置

- [章再編のページ・節対応](handoff/利用マニュアル章再編_Issue1016_配置対応.md)：各内容の正本と、操作・剛性・復元力・耐力・建物評価の境界。
