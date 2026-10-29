# 外部AI接続の確認記録（Issue #982）

状態: 🔶。構造計算式や規準適合を新たに認証する検証ではない。最終実機・関連回帰の結果は実行後に追記する。

## 製品Rustでの方式小実験

2026-10-29、Windowsの専用worktreeと専用target内で`cargo test -p sepika-ai-connection --target-dir target-issue982`を実行し、次の2件が成功した。

- `same_logon_round_trip_eof_missing_and_exclusive_creation`: 同じログオンから製品TokioパイプへUTF-8日本語行を送信し、応答を受信。クライアント終了後のEOF、server終了後の不在、同名の先行インスタンスの排他拒否を確認。
- `dacl_excludes_create_instance_and_generic_write`: TokenLogonSid限定DACLに対し、既定TokioクライアントのGENERIC_WRITE要求と追加server作成が失敗し、FILE_CREATE_PIPE_INSTANCEを含まない限定rightsの製品クライアントは接続できた。

リモート拒否のオプション指定とログオンSID限定DACLの組立をコードで確認した。実際の別ログオン・別端末からの接続試行は未実施であり、その拒否を実測済みとはしない。OS設定、ファイアウォール、レジストリ、サービスは変更していない。#973の.NET小実験とは別の証拠である。

## 実装と検証対象

`sepika-ai-connection`、`sepika_app::app::study::ai`、`sepika_mcp::gui_bridge`を対象とする。実編集・実計算には既存の正常な門形架構を使い、未設定壁版の警告を承諾する操作は行わない。外部LLM、APIキー、生成文章の一致は検証に用いない。

最新mainの#981を統合し、採否とAI観測データを同じ検討保存へ接続する。実行許可は保存せず、受理・開始・提出・結果検証を保存する。切断と計算取消、提出と計算成功・検定合格・人による採用を区別する。

## 実装途中の確認

- 最新stableはrustc 1.99.1、clippy 0.1.99。共有通信クレートの3件と`clippy --all-targets -- -D warnings`が成功した。
- `mdbook build`、`xtask check-docs`（125項目・678リンク・722実装参照）、`xtask check-deps`（46依存方向）が成功した。
- 初回GUI契約テストは確認依頼の再承認・再読込拒否が成功し、実編集を含む2件が既存の単一断面割当制限によるComposite拒否を検出した。編集層へ全断面割当の識別を追加し、原子性・混在拒否・一操作Undo/Redoの回帰を追加した。修正後の再実行とMCP stdioはビルド中で、成功をまだ主張しない。
- 受理台帳は現時点では開始・変更・計算開始・提出だけを保存する。参照操作の応答も全件複製保存するか、固定版の再取得とするかはADR0087との整合判断を照会中で、完了条件の未確定事項として残す。
- GUI実機・最終独立レビュー・関連構成の最終回帰は未実施。
