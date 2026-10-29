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
- 受理台帳はADR0087の契約を維持し、参照・照会を含む全成功検討操作の内容・応答を保存する判断に確定した。参照IDの変更要求への再利用拒否、同内容の参照再送、参照の受理照会と再読込の回帰を追加した。ページ応答を複製する容量増加は製品文書へ明記する。
- GUI実機・最終独立レビュー・関連構成の最終回帰は未実施。

## 修正後の自動回帰

GUIの`cargo test -p sepika-app --features gui --lib --locked --target-dir target-issue982`は1150件成功・3件ignore。AI契約7件を含み、実通信の比較案作成・原子的断面割当・既存非同期計算・通常根拠との数値と計算時入力照合・説明提出・旧版明示閲覧・再読込まで通過した。計算時入力には生成節点を含み、本案の4節点で根拠の6節点を置き換えない。GUI表示案と本案入力は通信操作で変更されなかった。

確認依頼の新許可による昇格、範囲外・古い版・無効対象、本案・採用操作、未知操作、現在Review接続での変更要求再送を拒否した。既存の人による確認待ちを観測し、AIの警告承諾・再開・取消を拒否した。解除は計算停止にせず、再読込では許可を破棄し未完了ジョブを中断・結果なしにした。

全成功参照の受理保存・同内容再送・異内容ID再利用拒否・参照の受理照会と保存往復を確認した。旧版参照の保存応答も再承認時のhistory=falseで再送・照会を拒否し、history=trueで元の応答を返した。元の解析契約が失効するとキャッシュ参照を再認証しなかった。

根拠全体の取得不能、取得できた根拠だけの部分集計、確認済みNGと不正成分による未評価の併存を確認した。全範囲件数は取得不能時にnull、取得根拠なしではobserved_countsもnullとした。Static参照によるDesign詳細表取得を拒否した。

`cargo test -p sepika-edit --lib --locked --target-dir target-issue982-edit`は218件成功。固定対象外・断面以外の混在batchを無変更拒否し、変更なし子を含む一括割当も一操作Undo/Redoできた。編集層と共有通信クレートの全ターゲットclippy警告禁止が成功した。MCPの`gui_bridge_stdio`はinitialize・公開ツール一覧・gui_requestの製品パイプ往復1件が成功した。

GUI/MCP全ターゲットclippy警告禁止は先行sourceで成功し、再送開示再検査を含む最終sourceの再実行中。非GUIとMCPの関連全lib、最終独立レビュー、実機は次段で追記する。CI成功とは区別する。

## 実機用の正常fixture

`cargo build -p sepika-app --features gui --example ai_connection_fixture --locked --target-dir target-issue982`で、未設定壁版を持たない専用GUIを生成する。起動対象は`target-issue982/debug/examples/ai_connection_fixture.exe`。MCPの代表操作は`crates/sepika-mcp/tests/support/gui_probe.py`へ、専用MCP実行ファイル、GUI表示のpipe、`--scenario review`または`comparison`、専用出力パスを渡す。外部LLMやAPIキーは使わず、GUIでの明示許可を待つ。通常の計算値・根拠・参照と提出をJSONへ記録する。

## 独立確認後の修正群

9e26a7d4の独立確認ではAI契約7件が成功した。保存済みsubmit応答の無条件開示を修正し、提出時の受理ID・保持計算と既存Quantity根拠を保存して現在の閲覧許可と元根拠を照合する。説明だけの提出にも適用し、後続の無関係な計算で当初の未計算状態を補完しない。計算中の編集拒否は開始proposalに限定し、同じ検討の別案を編集できるようにする。数量のみの提出根拠も案の削除不可規則へ追加した。非GUI構成で数量範囲のlabel取得が欠けるコンパイルエラーを検出し、利用するtest cfgと同じ範囲へ揃えた。

この修正群のAI契約10件（提出の結果参照有無・Quantityの旧版/失効/保存再読込、計算中案と他案の編集を含む）は実行中。直前のGUI全lib成功と区別し、このsourceの最終全lib・静的解析・実機結果は後で追記する。

9e26a7d4の関連確認ではMCP非feature全libが98件成功した。feature全libは並列実行で134成功・既存CFTジョブ待機の1件が時間切れとなった。変更せず単独実行すると2.58秒で成功し、既存MCPテストexeを逐次全135件実行して全成功（12.09秒）した。待機上限や計算式は変更していない。GUI/MCP全target clippy警告禁止もこの段階で成功した。

## 完了状態の保存同期とWindows CI

1fef9194ではGUI全libが1153件成功・3件ignore、非GUI全lib（474対象）が正常終了し、GUI/MCPのfeature・非feature全target clippy警告禁止が成功した。独立確認はAI10件成功のうえ、完了run取込とAI状態同期の間の保存raceを検出した。

修正では完了取込・保存snapshot・復元を同じ状態同期規則に集約した。AI要求job IDを既存実行と保持runへ引き継ぎ、実際の完了runを持つ記録だけを完了へ同期する。中断したジョブを後続の同入力別ジョブの成功で補完しない。状態変更は検討dirtyへ反映し、workerエラーとchannel切断の理由付き失敗も保存する。修正途中のAI契約13件は全成功。計算poll完了直後にAI pollなしで保存・復元、完成runを持つrunning/interrupted記録の復旧、真の未完了、中断後の同入力別ジョブ、dirtyと無変更同期、worker失敗の保持を含む。最終sourceでは通常OVIKA保存APIによる即時保存往復も同じ回帰へ追加し、確定後に再実行する。

Windows専用CIジョブを追加し、PRで共有通信全test・GUI AI契約（逐次）・MCP stdioを実行する。Ubuntu既存4構成は維持し、Windows失敗も必須「テスト」を失敗させる。mainは同じWindows構成のno-runだけで専用cacheを生成し、完全hit時は省略する。debug symbolだけを0としてPR/mainの環境を揃えた。PyYAML構文と、PR/main/cache/gate条件・対象コマンドをローカル確認した。実際のGitHub CI結果はまだ確認していない。
