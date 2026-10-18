# RC壁の同一face内力と初期耐力 — Issue #617

> **記録時点**：以下の試験値・件数・未実施範囲と「後続」「未完了」は、各節に示すローカル検証・レビュー記録時点の履歴である。
> 最新の最終ゲートは [PR #655](https://github.com/hrntsm/SEPIKA/pull/655) と [索引の最終ゲート参照](README.md#記録時点と最終ゲート)を参照する。CI・mergeの完了は、未実施のGUI・Validation等の完了を意味しない。

## 対象と証拠の範囲

基点mainは `5b7860b8a477c6a97e63a2a5a033d2323520a31c`。対象コードのコミットは `61775a2d0c045caef0bcef6f54952d07da881269`。以下は2026-10-14 WindowsでのローカルVerificationであり、最終headの全構成成功やIssue全体完了を表さない。

製品仕様の正本は[RC壁の壁脚内力と初期耐力](../../docs/calc_basis/04_要素剛性/10_RC壁の壁脚内力と初期耐力.md)、判断は[ADR 0055](../adr/0055-rc-wall-static-face-and-initial-capacity.md)とする。材料入力#535を再利用し、方式選択#572、側柱拡張#496、履歴再構築は対象外である。

## 参照資料と確認限界

[壁耐力の方式説明](https://www.structure.jp/manual/be513.pdf#page=98)と[壁脚内力の公開講習会資料](https://www.unions.co.jp/dqs/seminar/files/hold_seminartext.pdf#page=4)は、2026-10-18に公式配布元へのWebアクセスで公開を確認した。今回の確認は各式の全文再照合ではない。同一faceの内力・容量式のVerificationは以下の独立算術・人工入力・実求解で示す。引用規準原典本体、実験・実測Validationは未確認である。

公開講習会資料の当時取得したPDFのSHA-256は `a29fcad20e8c4373795eb18c02a951fcdb7beaa6ed7b21c897d4a952433dd230` である。当時の照合対象の識別値であり、2026-10-18の再取得ファイルの計算値や、新規の画像照合を示すものではない。

## 独立算術・人工入力・実求解の区別

### 容量式の独立算術

`sepika_core::rc_wall_capacity::tests::independent_600_by_180_capacity_arithmetic_is_distinct_from_square_column_entry` はFc=24 N/mm²、te=180 mm、D=4600 mm、A=828000 mm²、ag=3097 mm²、pwh=.004、横fy=295 N/mm²、N=828000 N、λ=6450/4600を入力する。d=4300 mm、pte=.40012919896640825%、j=3762.5 mm、σ0=1 N/mm²、既存min式の期待Quは1495316.044904383 N。独立Decimal算術の結果を照合し、実装手順をテストへ複製しない。600×180側柱は正方形入口ゲート外なので、実入口の成功証拠としない。meanと方式切替は#572で扱う。

### 人工断面力の同一face合成

共通fixture `test_fixtures/rc_wall617.rs` はlw=4000 mm、t=180 mm、同質左右600×600、高さ3000 mm、下梁共通face距離300 mm。人工入力はNt中央=-400000 N、左=-300000 N、右=-500000 N、中央Mz=-1.75e9 Nmm、中央Qy=200000 N。側柱の面内Q/M=0、腕長は-2000/+2000 mmである。

独立期待値はNcomp=1200000 N、軸力腕長M=4e8 Nmm、梁心全体Mz=-1.35e9 Nmm、face全体Mz=-1.29e9 Nmm、Q=200000 N、物理A=1332000 mm²。解析中央＋側柱A=1440000 mm²を容量式のAへ流用しない。element入口は10本22 mm筋とpwh=.004のQu=2006534.471365995 N、design入口はpwh=.006のQu=2178659.633619860 Nを照合する。二つの配筋条件の期待値を混用しない。

`wall::static_face::tests` の6試験は上記合成、符号反転・90度回転・側柱端順反転、必要行欠落・非有限・近傍行・face不明、剛性剛域長との区別、側柱M再輸送相当の残差、別接続梁支配、直接factoryのNone/零/NaN拒否を検証する。`joint_wiring::tests::reference_skeleton_uses_same_face_total_state_and_keeps_allowable_check_on_missing_side` は壁脚外へ大きいN/Mを置いても壁脚の合成値を保ち、側柱欠落時は参考骨格のみ未算定、正常許容検定はCheckedとなることを検証する。

### 実静的求解

`nonlinear::wall_reference::tests::actual_static_cut_equilibrium_and_reference_capacity_match_independent_values` は既存Analysisを直接解く。GのNcomp=800000 N、上部節点へ各100000 Nの水平荷重を載せた切断面は|Q|=200000 N、|Mface|=540000000 Nmm、Ncomp=0 N。face高さ2700 mmと節点切断面釣合いを照合する。これは人工断面力fixtureと異なる応力状態である。

OFFと正常空Gを明示無載荷として照合し、片側柱の100 N/mm分布軸荷重ではface N=270000 Nを照合する。90度・45度の全体回転後の参照と加力方向非依存、途中失敗時のModel不変を確認する。

active-set fixtureは高さ6000 mm、同質対称柱と斜め引張専用鋼ブレースを持つ。負値を含む複数長期荷重を荷重段階で合成したG、壁方向正負荷重を各々独立Analysisで直接求解した値へ照合する。正負λ/Quの非対称とQyから局所分岐への逆符号対応をassertする。二基底応答の事後合成を参照解にしない。側柱横部材荷重と直接Wall部材荷重のID付き拒否、2壁の片方face不明による全体失敗・入力不変も対象である。

### 数値許容差

solver独立値の照合は相対1e-8（基準値の絶対値と1の大きい方を尺度とする）。解放側柱の回収残差は力尺度max(|中央Q|,|側柱N|,1 N)、モーメント尺度max(|中央M|,力尺度×D,1 Nmm)の各1e-9以下を受け入れる。これは解放の丸め残差の基準であり、Q=0を非零に補完するepsilonではない。必要位置の照合は無次元posの16 machine epsilonで、近い任意位置をfaceとは認定しない。

## 実入口と完了条件の追跡

| 完了条件 | 現時点の証拠 | 残る確認 |
| --- | --- | --- |
| 1 同一バッチ壁脚M/Q/N・極値混用除去 | element人工入力・design同一face試験・App/MCP実case/combo | 親監査・最終head CI |
| 2 中央/側柱所有・腕長・Q・物理A | 独立人工入力、実Analysis切断面釣合い | 親監査・最終head CI |
| 3 一度の符号付きface輸送・位置/端順 | element6試験 | 親監査・最終head CI |
| 4 App/MCP静的バッチ・旧入力・不足診断 | 実case/combo同一バッチ、App側柱欠落・正常許容維持・旧入力拒否、MCP不正case/Fiber拒否 | 親監査・最終head CI |
| 5 G/±Ew荷重合成・active-set直接求解 | solver6試験（filter全7は既存1を含む） | 親監査・最終head CI |
| 6 OFF/空G/分布N/斜め/局所分岐/拒否 | solver上記fixture | 親監査・最終head CI |
| 7 Ds/モデル化表示の採用run Qu | 採用Qu配列完全一致・欠落/NaN/旧入力拒否、実Dsの採用Qu依存・旧TH拒否 | 親監査・最終head CI |
| 8 生成前失敗・状態/台帳非汚染・固定/一回 | 全壁原子性・Model/台帳不変・履歴数と下記実経路 | 親監査・最終head CI |
| 9 App/MCP増分・App非線形TH・対象外維持 | App6試験、MCP86試験（線形TH追加）、GUI純鋼板5試験 | 親監査・最終head CI |
| 10 docs/語彙/ADR/V&Vと共通完了手順 | 本レポート・索引・製品文書・CONTEXT・対象source SHA・文書checks | 親監査、最終head全適用CI、merge |

App3試験は静的台帳不変、採用Qu保存、THのmodel A→B→旧Aの表示拒否と入力記録の個別保持・OVIKA再読込、背景増分/TH計算中の入力編集で結果反映を拒否する。MCP増分は同じjob条件の直接solver run Quへ照合し、参照応答を結果として公開しない。自動App/MCP関数入口であり、手動GUI画面操作・外部MCPクライアント操作は未実施である。

追補App3試験は実静的case20とG＋水平P各係数1のcomboを通し、同じ求解バッチのface N/M/Qを表示detailへ照合する。|Q|=100000 N、物理A=1332000 mm²は独立既知値で、配線照合と独立手算術を区別する。側柱行欠落では参考骨格Skip/許容Checkedを保ち、入力編集後は旧検定を拒否する。MCP指定case/comboの全member-force行は同条件の直接computeと完全一致し、不存在case999と側柱Fiberを具体理由付きで拒否する。MCPは各jobで求解するため、旧保存バッチを受け取る仮想入口をこの証拠に含めない。

モデル化lookupは実増分runのQu配列と完全一致し、未解析・欠落・NaN・旧入力を拒否し、台帳を変えない。実Ds入口は真正RC梁600×600、静的Ai・増分・明示確定評価点・自動rankを使う。モデル・確定壁Qw・群重みを固定し、採用Quの対応分岐だけを|Qw|/.98から|Qw|/1.0へ人工変更すると、既存0.99境界を跨ぎFA→FDとなる。これは採用run値への依存のVerificationであり、その人工変更による構造応答のValidationではない。欠落Quと旧入力も拒否する。群重みは別の明示設計入力を保ち、初期Quへ置換しない。

MCP線形時刻歴はpwh欠落でも正常な実jobを実行し、nonlinear=false、採用耐力空を確認する。GUI純鋼板の既存同期・背景・線形・非線形通知5試験も成功した。

広域回帰で既存 `wall_model` の非線形成功を要求する4試験が開口壁の適用外ゲートへ達した。開口付きの元fixtureは線形・自重・領域・固有値検証に維持し、その4試験だけ別の真正無開口fixtureへ変更した。独立階重量、QuD、Ds=.4、Fes=1.5、QuN=.4×1.5×QuDの関係は維持し、無開口の上帯面積に対応する重量期待値とsnapshot2行を更新した。斜め壁の期待耐力もModelの仮定状態から再計算せず、同runの採用Quへ依存させた。

新しい `nonlinear_opening_wall_refuses_job_without_publishing_partial_results` は元開口fixtureの増分・非線形THを両方実行し、壁ID16付き適用外理由と静的結果bundleのbytes完全不変を検証する。開口ゲート・製品算式・許容差は変更していない。修正後のwall_model統合16件は成功した。

次の広域回帰では既存solverの壁増分2試験が側柱face不明で失敗した。その2試験の専用fixtureで既存の形状由来自動剛域を計算し、実下梁の正faceと左右柱脚の一致をassertした。断面用途・階所属とX/Y参照を載せられる上辺拘束を整え、元の目標変形角、降伏後剛性・細長壁耐力の期待値と許容差は維持した。人工face注入やNoneを零とする救済はしていない。lintは私有型alias・testmodule配置・fixture初期化式だけを整備し、ゲート・計算・輸送は変更していない。整備後element404・solver284の全libと両all-target clippyが成功した。

全workspace clippyで発見したtest-only lintは、MCP共有fixtureの読込みを親testmodule一箇所へ集約し、designの独立期待literalを同一f64の表記へ整理した。製品挙動・入力値・期待値・許容差は変更せず、MCP全86件と該当design人工状態1件を直接再検証した。この修正だけを理由に成功済みの全テストを繰り返さず、両構成のclippyで構成全体を確認する。

## run状態と一回の処理の証拠

`sepika_job::compute_pushover` と `compute_time_history` はrun入力の壁を一回展開してsolverへ渡す。`nonlinear::wall_reference::resolve` は展開済みModelの参照専用cloneから `Analysis::prepare` / `linear_static` を呼び、再展開する静的jobへ渡さない。準備計算の幾何照合用展開とrun求解の展開を混同しない。

増分driverと非線形THは全壁のresolve成功後にbehaviorを新規生成し、容量配列だけを渡して結果にも保持する。参照の変位・内力・要素履歴・外力を渡す経路はない。既存長期集合の外力を各case一度合成し、既存長期載荷を増分driverで一回、THの `apply_long_term_static` で一回行う。参照G応答をその外力へ加算しない。Quをrun中更新する経路もない。

これらのコード経路を、2壁の片方失敗時の全体Err・Model不変、App静的台帳不変、壁履歴数=確定steps数、TH長期載荷フラグ、run耐力保持と保存入力固有性の観測可能試験で補強する。内部call-count計測は導入していない。以上は現在経路のVerificationであり、すべてのモデルの履歴挙動のValidationではない。

## ローカル実行記録

個別の実装・回帰試験はsource担当が実行し、本レポート担当は終了ログと該当sourceを照合した。広域回帰と最終文書checksは文書・検証担当が実行した。ログは作業環境TEMPに表記のファイル名で保持する。

| コマンド | 結果 / ログ |
| --- | --- |
| `cargo test -p sepika-core --lib independent_600_by_180 --locked` | 1成功 / `arithmetic.log` |
| `cargo test -p sepika-element --lib static_face --locked` | 6成功 / `face-tests.log` |
| `cargo test -p sepika-solver --lib wall_reference --locked` | 7成功（本件6＋既存1） / `reference-tests4.log` |
| `cargo test -p sepika-design-jp --lib --locked` | 582成功 / `design-all.log` |
| `cargo test -p sepika-app --lib rc_wall617 --features gui --locked` | 3成功 / `app-tests3.log` |
| `cargo test -p sepika-mcp --lib mcp_rc_wall_initial_capacity --locked` | 1成功 / `mcp-tests.log` |
| `cargo test -p sepika-app --features gui rc_wall617_tests --lib --locked` | 追補後6成功 / `sepika617-regressions-app.log` |
| `cargo test -p sepika-mcp --lib --locked` | 追補後86成功 / `sepika617-regressions-mcp-all.log` |
| `cargo test -p sepika-app --features gui steel_wall_notice_tests --lib --locked` | fmt後5成功 / `sepika617-regressions-steel.log` |
| `cargo test -p sepika-app --test wall_model --locked` | 広域回帰fixture修正後16成功 / `sepika617-fix-wall-model2.log` |
| `cargo test -p sepika-element -p sepika-solver --lib --locked` | solver fixture/lint整備後404/284成功 / `sepika617-fix-element-solver-tests.log` |
| `cargo clippy -p sepika-element -p sepika-solver --all-targets --locked -- -D warnings` | 整備後成功 / `sepika617-fix-element-solver-clippy2.log` |
| `cargo test -p sepika-mcp -p sepika-app -p sepika-io --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --no-fail-fast --locked` | exit0。26 summaries合計1159成功・既存3ignored。App GUI750 / MCP feature116 / IO Parquet183単体を含む / `sepika617-final-feature-test.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 初回exit101。MCP共有fixture重複modとdesign期待値excessive_precisionの2lint。修正前の結果 / `sepika617-final-workspace-clippy.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings`（test lint整備後） | exit0 / `sepika617-final-workspace-clippy2.log` |
| `cargo clippy -p sepika-mcp -p sepika-app -p sepika-io --all-targets --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --locked -- -D warnings` | exit0 / `sepika617-final-feature-clippy.log` |
| `cargo test -p sepika-io --locked` | 独立default exit0。単体172、統合13成功・既存2ignored / `sepika617-final-io-default.log` |
| `cargo check -p sepika-app --features sepika-app/gui --locked` | GUI単体・Parquetなしexit0 / `sepika617-final-gui-check.log` |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | 厳格試行exit101。core/materialの単位表記等がbroken_intra_doc_linksとして拒否された。CIのdocs workflowはこのwarnings denied設定を使わない。通常doc buildと警告の分類は下記に分離 / `sepika617-final-doc-default.log` |
| `cargo doc --workspace --no-deps --locked` | CI相当通常構成exit0。新API単位リンク警告8件をfactory/static_face/WallElement/run採用Quから分類し修正へ戻した。旧変更外の単位・私有mod等の多数警告は別途追跡、警告なしの成功ではない / `sepika617-final-doc-normal.log` |
| `cargo doc --workspace --no-deps --locked`（新単位表記修正後） | exit0。新APIの8単位リンク警告は消失。既存の変更外警告は残る / `sepika617-final-doc-normal2.log` |
| `cargo doc -p sepika-mcp -p sepika-app -p sepika-io --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --no-deps --locked` | exit0。IO/MCP/Appの既存警告は残る / `sepika617-final-doc-feature.log` |
| `cargo fmt --all --check` | 最終sourceでexit0 / `sepika617-final-fmt.log` |
| `cargo run -p xtask -- check-docs` | exit0、summary123・links537・impl refs615 / `sepika617-final-check-docs2.log` |
| `cargo run -p xtask -- check-deps` | exit0、50 upstream checks / `sepika617-final-check-deps.log` |
| `mdbook build`（最終文書） | exit0、生成HTMLのMathJax区切り保持を確認 / `sepika617-final-mdbook2.log` |
| 開発者向け変更文書5ファイルの相対リンク・見出しanchor確認 | リンク400件、うちanchor35件、欠落0 / `sepika617-final-devdoc-links2.log` |
| `cargo test -p sepika-mcp --lib --locked`（test lint整備後） | 86成功 / `sepika617-fix-mcp-lib.log` |
| `cargo test -p sepika-design-jp reference_skeleton_uses_same_face --lib --locked` | 1成功 / `sepika617-fix-design-literal-test.log` |
| `target/debug/xtask.exe check-docs` | 文書担当が実行、利用者導線追記後も成功（summary123、links537、impl refs615） |
| `mdbook build` | 文書担当が実行、HTMLビルド成功 |
| `cargo test --workspace --locked` | 最初の無除外実行はexit101。App単体310成功・full_model37成功等の後、wall_modelの開口壁fixtureで4失敗（11成功）。修正・再実行前の結果であり、sepika-loadの既知#645に未到達 |
| `cargo test --workspace --locked`（fixture修正後再実行） | exit101。wall_model16成功で前4失敗を解消後、sepika-load lib269成功・既知#645一件失敗。`floor/tests.rs:1288` のdot=.000001でfalse/true不一致。対象外コードは変更していない |
| `cargo test --workspace --locked -- --skip floor::tests::issue561_rectangle_classification_thresholds_and_finiteness` | exit101。既知一件をCLI除外して続行し、solver lib282成功・既存pushover fixture2失敗（側柱共通face不一致）。solver統合/doc-tests等の後続へ未到達。無除外成功でも除外後成功でもない |
| `cargo test --workspace --no-fail-fast --locked -- --skip floor::tests::issue561_rectangle_classification_thresholds_and_finiteness` | fixture/lint整備後exit0。69 target summariesの合計3287成功、既存4ignored、既知一件filtered。後続統合/doc-testsも完走。一件除外後の成功であり無除外全成功ではない / `sepika617-final-workspace-skip645-2.log` |

default testsは既知一件CLI除外後に完走し、関連feature testsも成功した。全workspace clippy初回の新テストlintは修正し、workspace/feature両clippy、IO default独立、ParquetなしGUI単体checkも成功した。その後の変更はAPI契約と単位表記のRustdocだけで、実行ロジックは変更していない。最終sourceのfmtと通常default/feature API doc、Cargo経由check-docs/check-deps、mdbook buildは成功した。親担当の`rustup update stable`はrustc 1.99.0（2026-09-28）で成功し、clippy 0.1.99（b940084d7e、2026-09-28）を確認した。既知Windows baseline #645のfloor境界試験は対象外であり、ignore追加や許容差変更は行わない。一件CLI除外後の成功を、未除外の全成功と区別する。

通常API docには既存の単位リンク・私有module参照等の警告が残る。default構成のクレート別件数はcore305、material18、element50、solver86、design301、load53、section22、edit7、skeleton6、IO11、job14、MCP5、App47。feature構成の直接生成対象はIO11、MCP10、App78である。新単位8件の位置（factory190、static_face10、WallElement659、TH result13、pushover types277）に警告はなく、同じファイルの既存警告（factory5〜9の私有module、WallElement54/807、pushover typesの既存単位等）と区別した。`-D warnings`試行の失敗を通常buildの成功で置き換えず、既存警告は[Issue #654](https://github.com/hrntsm/SEPIKA/issues/654)で追跡する。

## 独立レビューの結果

2026-10-14、独立Reviewerが基点 `5b7860b8a477c6a97e63a2a5a033d2323520a31c` からレビュー対象head `90f7eff36fba7bec8934c1bb9c8f4e7927b8e70b` までの変更を確認し、Status done・必須指摘なしとした。Issue原本の完了条件10項目、計算仕様・実入口・失敗契約・適用限界を照合した。対象コードSHAは冒頭の `61775a2d0c045caef0bcef6f54952d07da881269` から変更していない。

この独立レビューの完了は、引用規準原典全文や実験Validationの確認を意味しない。

| 独立実行の範囲 | 成功数 | TEMPログ |
| --- | --- | --- |
| coreの独立容量算術 | 1 | `sepika617-independent-core.log` |
| elementの同一face・符号・不足・直接factory | 6 | `sepika617-independent-element.log` |
| solverのwall_reference（本件6＋既存1） | 7 | `sepika617-independent-solver.log` |
| designの同一face参考骨格・正常許容維持 | 1 | `sepika617-independent-design.log` |
| App GUIの静的case/combo・採用Qu・Ds/表示・状態保持 | 6 | `sepika617-independent-app.log` |
| MCPのRC壁実入口 | 2 | `sepika617-independent-mcp.log` |
| MCPの対象外線形時刻歴 | 1 | `sepika617-independent-mcp-linear.log` |

合計24件はすべて成功し、文書担当もログの成功summaryを確認した。これらの限定再実行を全workspaceの無除外成功や最終CI成功と読み替えない。親監査・最終head全適用CI・mergeはまだ後続ゲートである。

## 未検証・適用限界

引用原典全文、実験の破壊・反転履歴・動的応答・全適用域Validation、中央と側柱の解析軸断面重複を含む軸剛性理論の妥当性は未確認。限定入口のVerificationをこれらのValidationへ拡張しない。手動GUIと外部MCP接続、親監査・最終head CI・mergeは後続ゲートである。
