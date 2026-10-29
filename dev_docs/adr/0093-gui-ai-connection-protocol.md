# GUIの検討操作を限定DACLのパイプと保存受理台帳で中継する

Status: accepted

## 判断

Issue #982では[ADR0087](0087-gui-owned-study-and-local-ai.md)を具体化し、`sepika-ai-connection`をGUIとMCPの共通下層に置く。このクレートはWindowsパイプ、最大1MiBのUTF-8 JSON行、要求・応答の搬送だけを担当する。モデル、許可、検討、受理台帳は所有しない。GUIからMCPへの依存や、モデル・ファイル入出力層への通信処理の混入を避けるための最小分離であり、汎用権限基盤にはしない。

GUIはランダムなパイプ名を生成し、現在のTokenLogonSidだけに保護DACLを付与する。クライアントの許可アクセスは`FILE_GENERIC_READ | FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES`（0x12018b）とし、追加のパイプ作成権を含む`GENERIC_WRITE`は付与しない。クライアントは同じ限定アクセス、overlapped I/O、`SECURITY_IDENTIFICATION`で開く。Tokioの`first_pipe_instance(true)`、`reject_remote_clients(true)`、`max_instances(1)`を使い、切断後は既存server handleを再使用する。OSの恒久設定は変更しない。

パイプスレッドからGUIへ要求と応答用oneshotを渡し、GUIのイベント処理で許可・入力版・対象を照合する。有効化中は短い再描画間隔で未操作中の要求も処理する。比較案の既存Undo/編集トランザクション、既存の検討用一件非同期計算を利用し、表示案・選択・入力ドラフトを切り替えない。計算の確認待ちは既存GUIに残し、AIへ承諾操作を公開しない。

依頼開始、受理IDと要求内容・受理結果、説明提出、SEPIKAが確認した計算・検定状態を検討に保存する。要求内容から接続セッションだけを除いて再送照合し、新たなGUI承認後に受理台帳を照会できる。同じIDの内容違いは拒否する。保存した確認依頼の種別は新しい接続許可で比較依頼へ昇格させない。接続許可と実行中threadは保存せず、再読込の未完了ジョブは「中断・結果なし」とする。

承認後の検討操作では参照・状態照会も含め、成功受理した全要求の内容と結果を保存する。参照IDを後から変更要求へ再利用できる穴を作らず、同内容の再送には当初の応答を返すためである。参照応答の再構成・fingerprintだけの保存・台帳の自動間引きは追加しない。数値ページの応答も複製するため、取得回数と応答量に応じて検討保存容量が増える。最新状態のポーリングは新しい要求IDで行う。

保存応答は新しい権限を作らない。参照の再送と受理照会では現在の保存旧版閲覧許可と元の計算根拠の有効性を再検査し、失効した根拠を保存JSONから再認証しない。検査が成立する範囲で当初の応答を返す。変更要求の同ID再送も現在の比較編集許可を必要とし、確認のみ接続では再実行要求を拒否する。過去の変更の受理結果は明示的な受理照会で確認する。

本案更新・採用・ファイル操作・任意コード・警告承諾・新しい計算取消は公開しない。切断は受理済み計算を停止しない。基準・比較案の全体入力を参照できる範囲と、固定鉄骨大梁だけの編集範囲をGUIで分けて表示する。

## 確認と限界

[製品Rust接続の検証記録](../v_and_v/外部AI接続_Issue982_確認記録.md)で実通信、アクセス権の観測と未検証範囲を区別する。根拠の条件・数値・集計は既存#978/#980の取得口を再利用し、ページの部分行から全体集計を生成しない。

## 参照

- [Tokio ServerOptions](https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ServerOptions.html)
- [Windowsのパイプ権限](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
- [Issue #982](https://github.com/hrntsm/SEPIKA/issues/982)
