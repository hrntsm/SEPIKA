# 外部AI接続の確認記録（Issue #982）

状態: 🔶。以下は通信・権限・既存計算への接続のVerificationであり、構造計算式・規準適合の新たな認証ではない。Windows実機は具体的共有承認後に再開し、以下の再接続問題を検出した。修正後の最終実機は未完了である。

## 対象と確定版

2026-10-29、Windowsの専用worktreeで検証した。最終ソースは`31e85aa5`、対応文書込みの固定HEADは`8ba9a181637210b27af0c57869d0fdbfe660e5d5`。#981を統合したmain `29c14e5d`に接続している。

対象は`sepika-ai-connection`、GUIの検討処理、MCP stdio薄中継。実編集・実計算には正常な門形架構を使用した。外部LLM・APIキー・生成文章の一致は検証に使わない。実行許可は保存せず、依頼・開始・最終操作・成果・受理台帳を検討へ保存する。切断と計算取消、成果提出と計算成功・検定合格・人による採用を区別する。

## 製品Rustでの方式小実験

専用資源だけで`cargo test -p sepika-ai-connection --target-dir target-issue982`を実行し、次の2件が成功した。

- `same_logon_round_trip_eof_missing_and_exclusive_creation`: 同じログオンから製品TokioパイプへUTF-8日本語行を送受信し、クライアント終了後のEOF、server終了後の不在、同名先行インスタンスの排他拒否を確認した。
- `dacl_excludes_create_instance_and_generic_write`: TokenLogonSid限定DACLに対し、既定TokioクライアントのGENERIC_WRITE要求と追加server作成は失敗した。FILE_CREATE_PIPE_INSTANCEを含まない限定rightsの製品クライアントは接続できた。

リモート拒否オプションとログオンSID限定DACLの組立をコードで確認した。実際の別ログオン・別端末からの接続試行は未実施であり、実測済みとはしない。OS設定・ファイアウォール・レジストリ・サービスは変更していない。#973の.NET小実験とは別の証拠である。

## 最終ソースの直接回帰

コマンドのtargetは全て専用ディレクトリ。次の結果は`8ba9a181`のローカル実行で、GitHub CIとは区別する。

| 検証 | コマンド要部 | 結果 |
| --- | --- | --- |
| GUI全lib | `cargo test -p sepika-app --features gui --lib --locked --target-dir target-issue982` | 1159成功、3 ignore |
| 非GUI全lib | `cargo test -p sepika-app --lib --locked --target-dir target-issue982` | 479成功、1 ignore |
| AI契約 | GUI test exeの`app::study::ai::operations::tests --test-threads=1` | 16成功。独立Reviewerも同じ確定exeで16成功 |
| GUI/MCP関連feature | `cargo clippy -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp --all-targets --locked --target-dir target-issue982 -- -D warnings` | 成功 |
| 非feature | 同clippyの`--features`なし | 成功 |
| MCP stdio | `cargo test -p sepika-mcp --features mcp --test gui_bridge_stdio --locked --target-dir target-issue982` | 1成功。initialize、tools一覧、gui_requestの製品パイプ往復 |
| 専用GUI | `cargo build -p sepika-app --features gui --example ai_connection_fixture --locked --target-dir target-issue982` | 成功 |
| 文書 | `xtask check-docs`、`mdbook build --dest-dir target-issue982-tools/book` | 125項目・678リンク・722実装参照、HTML生成成功 |
| 依存・整形 | `xtask check-deps`、`cargo fmt --all -- --check`、`git diff --check` | 46依存方向、整形・差分とも成功 |

最新stableはrustc 1.99.1、clippy 0.1.99。共有通信クレートの3件（不正・過大・非UTF-8・未完了行・未知フィールドを含む）と全target clippy、変更した編集層の全lib218件と全target clippyも成功した。これらは最終修正群で変更していない。

MCP全libは先行HEADで非feature98件成功。featureは並列負荷中に既存CFTジョブ待機1件が時間切れとなり、変更せず単独では2.58秒で成功した。既存exeの逐次全実行は135件成功（12.09秒）。待機上限や計算式は変更していない。

### 実通信と拒否契約

製品パイプから比較案作成・一括断面割当・既存非同期計算・通常根拠と同じ値と入力の取得・説明提出・保存再読込まで通過した。計算時入力の生成節点を含み、本案4節点で根拠6節点を置き換えない。GUIの表示案・本案入力・未確定draftはAI操作で変更しない。

確認依頼の再承認による昇格、本案・採用操作、範囲外・古い版・無効対象、未知操作を拒否した。断面以外との混在batchや固定対象外は無変更拒否し、有効な一括割当は一操作Undo/Redoできた。人による確認待ちを観測し、AIによる警告承諾・再開・取消は拒否した。計算中案だけを編集拒否し、同じ検討の別案は編集でき、結果を開始版へ固定した。

全成功参照・照会もID・内容・応答を保存し、同内容再送、異内容ID再利用拒否、receipt、保存往復を確認した。旧版readや提出の保存応答でも現在history=falseなら再送・照会を拒否し、history=trueなら元根拠を検査した。結果契約の失効後はキャッシュで再認証しない。全read応答の保存による容量増加は製品文書へ明記した。

新しいReview接続が保存後のproposals→runsページから手計算の旧版を発見し、明示input/run_indexで既存results/evidenceへ到達した。history=falseは旧版metadata・数値・根拠と過去一覧の再送/照会を拒否し、現在版runだけを件数へ含めた。

### 根拠・状態・保存契約

根拠全体取得不能では全範囲件数をnull、有効に取得した範囲だけをscope/count_unit付きobserved_countsへ返し、取得根拠なしは部分件数もnullとした。既知NG・有効最大・未評価を併存させ、構造無効な根拠の残存値を再認証しない。Static参照でDesign詳細を取得する要求を拒否し、詳細もページ取得した。

完了poll直後にAI pollなしで通常OVIKAへ保存し、再読込後も同じrun/calculatedになった。真の未完了はinterrupted_no_result、実worker失敗は理由付きfailedとして区別した。保存対象の同期変更をdirtyへ反映し、元job IDでrunへ帰属させ、後続の同入力別jobで中断を成功へ補完しなかった。

resultsと明示run付き提出のstateはそのrunへ固定した。同入力の後続jobが計算中・確認待ちでも過去runの状態を変えず、AI失敗後の人成功runもcalculatedを保った。提出の異なるrun参照混在を拒否した。run未指定の説明・数量提出は同依頼jobの提出時観測か未計算だけを保存し、後続完了でrun・検定根拠を補完しない。数量のみの提出も元案・根拠を保持し、削除拒否と保存再読込後の取得を確認した。

独立確認で検出したread台帳、提出キャッシュ開示、保存同期race、保持run発見、保持runと後続jobの混在を修正した。最終fresh6は修正必須なしで完了した。

## Windows CI

Windows専用ジョブはPRで共有通信全test・GUI AI契約（逐次）・MCP stdioを実行し、失敗は必須「テスト」を失敗させる。既存Ubuntu4構成は維持した。mainは同じWindows構成のno-runだけで専用cacheを生成し、完全hit時に省略する。PR/mainのdebug symbol設定を揃えた。PyYAML構文とPR/main/cache/gate条件・対象コマンドをローカル確認した。実GitHub CI結果は未確認。

## Windows実機の到達範囲と停止

起動対象は同HEADから生成した`target-issue982/debug/examples/ai_connection_fixture.exe`（タイトル「SEPIKA #982 接続確認」）。実ユーザープロジェクトを使わず、未設定壁版を持たない正常サンプルだけを使用した。代表クライアントは`crates/sepika-mcp/tests/support/gui_probe.py`と専用`sepika-mcp.exe`。外部LLM・APIキー・クラウド接続は使用しない計画だが、以下の拒否時点ではprobeを起動していない。

1. 専用プロセスが存在しないことを確認し、PID36608を可視起動した。初期AX・画像は取得できた。native AXのMaximizeを1回実行した直後にプロセスが終了し、終了codeは`0xC000041D`。stderrは日本語フォント読込infoだけでRust panicはなく、共有有効化前・probe起動前だった。#982起因とは断定しない。同操作は再試行していない。原因未確定の別課題として[#1001](https://github.com/hrntsm/SEPIKA/issues/1001)へ記録した。手操作での再現と、#980の消失事象と同原因かは未確認。
2. 親との調整後、同じexeをPID36824で再起動し、新規inventoryから選び直した。小窓の外枠拡張はAPIのwindow bounds外として拒否され、通常窓のまま継続した。接続パネルのクラウド送信可能性、全モデルの読取範囲、固定鉄骨大梁の編集範囲、対象Study、比較操作許可が未選択のcheckboxを実画像・AXで確認した。
3. 「ローカル接続を有効化」が自動承認レビューに拒否された。理由は、外部クライアントへの共有境界を有効にするアクセス変更について、受信者・データ範囲・権限の具体的なユーザー承認が不足するため。再試行・コード等の間接有効化をせず、依存CU入力を停止して親へ承認確認を依頼した。この時点ではPID36824を共有未有効のまま保持した。

この段階では実機のReview拒否・Comparison往復・解除・通常終了確認は未完了だった。OS設定の取得・起動・変更、過去に拒否された未設定壁版の警告続行は行っていない。

## 具体承認後の実機と再接続の修正

ユーザーから、専用正常サンプルの全モデル・条件・結果をこのPC内の専用MCPブリッジとgui_probe.pyだけへ共有し、Reviewの参照・説明提出、Comparisonの固定大梁への既存断面割当・既存計算・根拠提出、最後の解除を行う具体的承認を得て再開した。既存PID36824をfresh inventoryで選び直し、二重起動していない。

有効化とReview承認を実施し、probeはexit0で参照・説明提出を完了した。作成と計算はpermission_denied、提出はnot_calculated/source=none/runなし/検定件数null。GUI表示先は本案のままだった。続くComparison用の新クライアントは初回connectでos233になり、許可された明示再接続1回は承認できたが、begin_requestにconnect応答、studyにstarted応答、fixed_targetsにstudy応答という一応答ずれを記録してprobeが停止した。Comparisonのモデル変更には到達していない。

同一Listener/serverでclient1の複数要求・切断観測後にclient2へ接続する小さな回帰でも、2クライアント目の最初の要求がpremature disconnect/os233で失敗した。これは[mio #1983](https://github.com/tokio-rs/mio/issues/1983)の同インスタンス再使用時のstale read報告と、手元mio1.2.2の状態管理に一致する。上流の修正案は未release。応答ずれの実測と上流報告との一致を、OS設定や一般的なGUI終了の原因特定とは扱わない。

修正前のGUIでは接続を明示解除し、通常Save Asで専用target内のpre-phase6-review.ovikaへ保存した。AXの保存成功・dirty消失とファイル生成を確認し、Alt+F4で通常終了した。保持した起動プロセスのexit0と専用GUI/MCPプロセス不存在を確認した。破棄選択はしていない。

修正は一有効化・一クライアントへ限定した。切断後はserverと旧endpointを廃止し、GUIも許可・有効化を終了する。再接続は人が再有効化して新しいランダム接続先を渡し、新規承認する。EOFの読み飛ばし、不確実なdrain、ACL緩和、同名再bind、未merge上流fork、自動retryは追加しない。bridgeは要求IDと応答IDの不一致を成功扱いせず、受理不明として閉じる。probeも外側JSON-RPCと内側要求IDを検査する。

新しいtransport回帰は3回の新endpoint・旧endpoint不在・各3要求の応答ID一致を確認し、全4件とclippyが成功した。Appの実通信回帰は切断後のGUI有効化と許可の終了、旧許可拒否、新endpoint・新承認・旧session拒否を含めAI16件が成功した。fake応答ID不一致の拒否を含むMCP stdio2件も成功した。最終構成の広い検証と実機は次段で確認する。probeに任意の専用marker待機を追加し、提出後のGUI根拠確認・明示解除・旧context一回の拒否を同じclientで記録できるようにした。専用fixtureの初期サイズは通常製品と同じ1280×800、最小800×600へ揃えた。#1001の修正や解消とは扱わない。