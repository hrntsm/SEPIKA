# 外部AI接続の確認記録（Issue #982）

状態: 🔶。最終ソースの自動回帰・独立確認と、正常サンプルによるWindows実機の参照・実編集・既存計算・根拠提出・解除・正常終了を確認した。通信と権限・既存計算への接続の有限なVerificationであり、構造計算式・規準適合の新たな認証ではない。

## 対象と確定版

2026-10-29、Windowsの専用worktreeで検証した。最終ソースは`03c9a6d9`、対応文書込みの検証HEADは`a5af25eec1422293e51a7ffaf2906390bf2e6c20`。最終実機もこのソースから生成したexeを使った。#981を統合したmain `29c14e5d`へ接続している。

対象は`sepika-ai-connection`、GUIの検討処理、MCP stdio薄中継。正常な門形架構の専用fixtureを使用し、実ユーザープロジェクト、外部LLM、APIキー、生成文章の一致を検証に使っていない。実行許可は保存せず、依頼・開始・最終操作・成果・受理台帳を検討へ保存する。切断と計算取消、成果提出と計算成功・検定合格・人による採用を区別する。

## 製品Rustでの方式小実験

専用資源だけで`cargo test -p sepika-ai-connection --target-dir target-issue982`を実行し、次の2件が成功した。

- `same_logon_round_trip_eof_missing_and_exclusive_creation`: 同じログオンから製品TokioパイプへUTF-8日本語行を送受信し、クライアント終了後のEOF、server終了後の不在、同名先行インスタンスの排他拒否を確認した。
- `dacl_excludes_create_instance_and_generic_write`: TokenLogonSid限定DACLに対し、既定TokioクライアントのGENERIC_WRITE要求と追加server作成は失敗した。FILE_CREATE_PIPE_INSTANCEを含まない限定rightsの製品クライアントは接続できた。

リモート拒否オプションとログオンSID限定DACLの組立をコードで確認した。実際の別ログオン・別端末からの接続試行は未実施であり、実測済みとはしない。OS設定・ファイアウォール・レジストリ・サービスは変更していない。#973の.NET小実験とは別の証拠である。

## 最終ソースの直接回帰

targetは全て専用ディレクトリ。次の結果は`a5af25ee`のローカル実行で、GitHub CIとは区別する。

| 検証 | コマンド要部 | 結果 |
| --- | --- | --- |
| 共有通信 | `cargo test -p sepika-ai-connection --locked --target-dir target-issue982-transport` | 4成功 |
| GUI全lib | `cargo test -p sepika-app --features gui --lib --locked --target-dir target-issue982` | 1159成功、3 ignore |
| 非GUI全lib | `cargo test -p sepika-app --lib --locked --target-dir target-issue982` | 479成功、1 ignore |
| AI契約 | GUI test exeの`app::study::ai::operations::tests --test-threads=1` | 16成功。独立Reviewerも同じ確定exeで16成功 |
| 関連feature | `cargo clippy -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp --all-targets --locked --target-dir target-issue982 -- -D warnings` | 成功 |
| 非feature | 同clippyの`--features`なし | 成功 |
| 共有通信の静的解析 | `cargo clippy -p sepika-ai-connection --all-targets --locked --target-dir target-issue982-transport -- -D warnings` | 成功 |
| MCP stdio | `cargo test -p sepika-mcp --features mcp --test gui_bridge_stdio --locked --target-dir target-issue982` | 正常中継・不一致応答の拒否の2成功 |
| 専用GUI | `cargo build -p sepika-app --features gui --example ai_connection_fixture --locked --target-dir target-issue982` | 成功 |
| 文書 | `xtask check-docs`、`mdbook build --dest-dir target-issue982-tools/book` | 125項目・678リンク・722実装参照、HTML生成成功 |
| 依存・整形 | `xtask check-deps`、`cargo fmt --all -- --check`、`git diff --check` | 46依存方向、整形・差分とも成功 |

最新stableはrustc 1.99.1、clippy 0.1.99。変更した編集層の全lib218件と全target clippyも成功し、最終修正群では変更していない。MCP全libは先行HEADで非feature98件成功。featureは並列負荷中に既存CFTジョブ待機1件が時間切れとなり、変更せず単独では2.58秒で成功、既存exeの逐次全実行は135件成功（12.09秒）。待機上限や計算式は変更していない。

### 実通信と拒否契約

製品パイプから比較案作成・一括断面割当・既存非同期計算・通常根拠と同じ値と入力の取得・説明提出・保存再読込まで通過した。計算時入力の生成節点を含み、本案4節点で根拠6節点を置き換えない。GUIの表示案・本案入力・未確定draftはAI操作で変更しない。

確認依頼の再承認による昇格、本案・採用操作、範囲外・古い版・無効対象、未知操作を拒否した。断面以外との混在batchや固定対象外は無変更拒否し、有効な一括割当は一操作Undo/Redoできた。人による確認待ちを観測し、AIによる警告承諾・再開・取消は拒否した。計算中案だけを編集拒否し、同じ検討の別案は編集でき、結果を開始版へ固定した。

全成功参照・照会もID・内容・応答を保存し、同内容再送、異内容ID再利用拒否、receipt、保存往復を確認した。旧版readや提出の保存応答でも現在history=falseなら再送・照会を拒否し、history=trueなら元根拠を検査した。結果契約の失効後はキャッシュで再認証しない。全read応答の保存による容量増加は製品文書へ明記した。

新しいReview接続が保存後のproposals→runsページから手計算の旧版を発見し、明示input/run_indexで既存results/evidenceへ到達した。history=falseは旧版metadata・数値・根拠と過去一覧の再送/照会を拒否し、現在版runだけを件数へ含めた。

### 根拠・状態・保存契約

根拠全体取得不能では全範囲件数をnull、有効に取得した範囲だけをscope/count_unit付きobserved_countsへ返し、取得根拠なしは部分件数もnullとした。既知NG・有効最大・未評価を併存させ、構造無効な根拠の残存値を再認証しない。Static参照でDesign詳細を取得する要求を拒否し、詳細もページ取得した。

完了poll直後にAI pollなしで通常OVIKAへ保存し、再読込後も同じrun/calculatedになった。真の未完了はinterrupted_no_result、実worker失敗は理由付きfailedとして区別した。保存対象の同期変更をdirtyへ反映し、元job IDでrunへ帰属させ、後続の同入力別jobで中断を成功へ補完しなかった。

resultsと明示run付き提出のstateはそのrunへ固定した。同入力の後続jobが計算中・確認待ちでも過去runの状態を変えず、AI失敗後の人成功runもcalculatedを保った。提出の異なるrun参照混在を拒否した。run未指定の説明・数量提出は同依頼jobの提出時観測か未計算だけを保存し、後続完了でrun・検定根拠を補完しない。数量のみの提出も元案・根拠を保持し、削除拒否と保存再読込後の取得を確認した。

独立確認で検出したread台帳、提出キャッシュ開示、保存同期race、保持run発見、保持runと後続jobの混在を修正した。再接続修正後の最終fresh7は修正必須なしで完了した。

## 同一パイプ再使用の失敗と製品制約

先行sourceの実機ではReviewが完了した後、Comparison用の新クライアントが初回connectでos233になった。許可された明示再接続1回は承認できたが、begin_requestにconnect応答、studyにstarted応答、fixed_targetsにstudy応答という一応答ずれでprobeが停止した。Comparisonのモデル変更には到達していない。

同一Listener/serverでclient1の複数要求・切断観測後にclient2へ接続する小さな回帰でも、2クライアント目の最初の要求がpremature disconnect/os233で失敗した。[mio #1983](https://github.com/tokio-rs/mio/issues/1983)の同インスタンス再使用時のstale read報告と、手元mio1.2.2の状態管理に一致する。上流修正案は未releaseであり、OSや一般的なGUI終了の原因特定とは扱わない。

最終実装は一有効化・一クライアントへ限定した。切断後はserverと旧endpointを廃止し、GUIも許可・有効化を終了する。人が再有効化して新しいランダム接続先を渡し、新規承認する。EOFの読み飛ばし、不確実なdrain、ACL緩和、同名再bind、未merge fork、自動retryは追加しない。bridgeは要求IDと応答IDの不一致を成功扱いせず、受理不明として閉じる。probeも外側JSON-RPCと内側要求IDを検査する。新回帰では3回の新endpoint・旧endpoint不在・各3要求のID一致と、GUIの許可失効・再承認を確認した。

## 最終Windows実機

ユーザーから、専用正常サンプルの全モデル・条件・結果をこのPC内の専用MCPブリッジとgui_probe.pyだけへ共有し、Reviewの参照・説明提出、Comparisonの固定大梁への既存断面割当・既存計算・根拠提出、最後の解除を行う具体的承認を得て実施した。

対象は`target-issue982/debug/examples/ai_connection_fixture.exe`（タイトル「SEPIKA #982 接続確認」）、PID8808。初期設定は内側1280×800・最小800×600で、実画像の外枠1282×802を確認した。Maximizeは使用していない。MCPは専用`sepika-mcp.exe`と`crates/sepika-mcp/tests/support/gui_probe.py`で起動し、Comparisonは任意の`--release-marker`で提出後に同clientを保持した。

- Review: GUIで対象Studyと確認のみを許可し、参照・説明提出が成功した。create_comparisonとcalculateはpermission_denied、提出はnot_calculated/source=none/runなし/検定件数nullだった。probe exit0後、操作なしでGUIが接続先・許可を終了し、有効化ボタンへ戻った。
- Comparison: 人が新endpointを作り、比較権限で新clientを明示承認した。固定部材2の断面は1→2、変更件数は1。作成・割当・既存計算・結果・根拠・数量取得・run_index=0の提出が成功し、marker待機へ進んだ。外側・内側の要求応答IDを全て照合した。AI操作中、GUI表示は本案・編集世代0のままだった。
- 保持計算と同じGUIの照合: 人の操作で比較案の準備後入力・結果を開き、案dfa664c7-032a-4453-b5d8-39c053297bdd、世代1、入力識別b939949a…ce2af、6節点・静的5ケース・検定結果9件を確認した。長期ケースの部材0の検定比はGUIで0.086／0.074／0.121、MCPで0.08610299045044587／0.07413204054251889／0.12096722602267818で、表示精度で一致した。通常の検定根拠窓でも同じ案・版と長期比0.0861、軸曲げ・せん断の内訳、計算時部材・断面・材料を確認した。節点変位の実機数値照合は行っていない。
- 件数とページ: 提出はsource=run/run_index=0/calculated、checked=26・confirmed_ng=0・unchecked=39。全体合格とは扱わない。結果ページはtotal155/limit100/omitted=true、節点根拠ページはtotal6/limit1/omitted=trueであり、部分ページを全体集計へ置き換えない。
- 解除: marker待機中にGUIで接続を明示解除した後、同clientの旧context読取1回がos232（pipe is being closed）で拒否され、probeはexit0で終了した。新規接続や再送はしていない。保持した計算は残り、接続UIは有効化ボタンへ戻った。
- 保存・終了: 本案表示へ戻し、編集世代0・4節点・未計算を確認した。通常Save Asで専用target内のfinal-ai-connection.ovikaへ保存し、AXの保存成功・dirty消失と57368bytesのファイル生成を確認した。Alt+F4で通常終了し、起動時から保持したプロセスのexit0、専用GUI/MCPプロセス不存在を確認した。

専用target/cuのfinal-review.jsonとfinal-comparison.jsonに各応答を記録した。これらは検証生成物であり、製品や実ユーザープロジェクトの共有先ではない。実機での保存再読込・全DPI/最小寸法・全結果行・全根拠の網羅は未実施で、保存再読込と境界の直接回帰と区別する。

### 停止と再開の記録

先行fixtureではnative AX Maximizeを1回実行した直後、共有前・probe起動前に終了code 0xC000041Dで終了した。stderrはフォント読込infoだけでRust panicなし。同操作は反復せず、原因未確定の別課題[#1001](https://github.com/hrntsm/SEPIKA/issues/1001)へ記録した。手操作再現・#980の消失と同原因かは未確認。先行プロセスの再起動後は解除・専用保存・通常exit0を確認した。

共有の具体承認がない段階では自動承認レビューが有効化を拒否したため停止し、迂回しなかった。受信者・データ・権限を特定したユーザー承認後に正規操作で再開した。最終fixtureの初回activateと回復1回も失敗したため停止し、ユーザーの手動前面化回答後にfresh inventoryから選び直した。実画像の重畳を確認後、通常activate1回は成功し、その後の上記最終操作を完了した。追加起動・別経路前面化は行っていない。

OS設定の取得・起動・変更、過去に拒否された未設定壁版の警告続行、クラウド送信、実ユーザープロジェクト、本案更新・採用は行っていない。

## Windows CI

Windows専用ジョブはPRで共有通信全test・GUI AI契約（逐次）・MCP stdioを実行し、失敗は必須「テスト」を失敗させる。既存Ubuntu4構成は維持した。mainは同じWindows構成のno-runだけで専用cacheを生成し、完全hit時に省略する。PR/mainのdebug symbol設定を揃えた。PyYAML構文とPR/main/cache/gate条件・対象コマンドをローカル確認した。実GitHub CI結果はPRで別途確認する。
