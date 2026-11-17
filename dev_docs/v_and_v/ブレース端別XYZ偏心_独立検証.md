# ブレース端別XYZ偏心の独立検証

2026-11-18、[Issue #1364](https://github.com/hrntsm/SEPIKA/issues/1364)の検証。仕様は[端別XYZ偏心](../../docs/model_edit/09_梁の鉛直偏心.md)、[座標変換](../../docs/calc_basis/04_要素剛性/08_座標変換.md)、[鋼構造の断面検定](../../docs/calc_basis/06_一次設計/03_鋼構造_断面検定.md)を正本とする。

## 独立期待値

- 基準端(0,0,0)・(3000,4000,0) mmの軸方向は(0.6,0.8,0)、長さ5000 mm。両端に同じ(100,200,300) mmを与えても長さ・方向を維持する。i端(100,200,300)、j端(1100,2200,3300) mmの偏心では実端差が(4000,6000,3000) mmとなり、長さは1000√61 mm。端反転と偏心端の交換でも同じ物理応答となる。
- E=205000 N/mm²・A=2000 mm²からEA/Lを求め、並進と回転だけから独立に実端の伸びを与える。部材軸力と内力回収・確定状態を比較し、軸力以外の部材内力が0であることを確認する。各端で軸力ベクトルとr×F、元節点座標を用いる全体力・モーメントの釣合いを照合する。変位と剛性による仕事がEA/L×伸び²に一致する。
- 質量は物理密度7.85×10⁻⁹ t/mm³と実材軸長から求める。集中質量の各回転慣性m(r²)/2と、整合質量に一様加速度を与える各端のr×maを確認する。地動の公開解析内共通経路では、並進拘束・回転自由の各端にX加速度によるMy=300、Mz=−200、Y加速度によるMx=−300の影響値が残る。
- 引張専用反復では実端差(600,0,800) mm、L=1000 mm、軸方向(0.6,0,0.8)とし、j端のθyだけを自由とする。腕(600,0,−200) mmの実端伸びは−600θy。回転ばね2×10⁸と軸剛性200000から、正負の節点モーメントに対するθy=M/(2×10⁸+軸剛性係数×200000×600²)、N=−軸剛性係数×200000×600θyを照合する。
- 部材等分布荷重の既存独立反力試験へBraceを加え、各端1000 Nと腕±100 mmによる移送モーメントを、両端ピン・剛域・端反転で確認する。層切断の独立外力100/50 kNと支持ばねの既存試験へBraceを加え、腕が切断面を横切るときも元節点側の合力と層釣合いを確認する。
- 自重はA=10000/20000 mm²、実長3000/1000 mm、鋼の設計単位体積重量78.5 kN/m³から総量3925 N。元接続先によるK型配分を保ち、内部節点方式の重心1800 mm、基準節点方式1600 mmと照合する。荷重は実軸全長の分布とし、元節点座標・接続を変更しない。

主な試験入口はelement `frame::truss::tests::xyz_brace_*`、solver `xyz_brace_*`・`eccentric_member_load_reactions_*`・`確定記録は独立外力100と50を上層累積し支持ばねを外部作用として含める`、load `xyz_brace_self_weight_*`。

## 入力・検定・操作

- 低位トラス構築と公開静的解析入口で、零実長・非有限偏心/座標/参照軸・不正節点数/参照を理由付きで拒否する。解放により不安定な構成へ微小回転剛性を加えない。
- `steel_slenderness::eccentric_brace_compression_requires_both_explicit_buckling_lengths`は偏心0/非0、引張/圧縮、両軸/片軸/未指定を組み合わせ、非0偏心の圧縮だけに両軸直接指定を要求する。
- app/MCPの`xyz_brace_*`でGUI共通編集・JSON・OVIKA・Undo/Redo・成果入力識別/失効・実配置選択・剛体表示切替を確認する。偏心欠落を保存JSONの既定0へ置換しない。検討は既存の大梁断面割当だけという制限を保ち、偏心入り入力を固定複製し、比較案内での偏心編集を原子的に拒否する。本案を更新しても固定入力へ混入しない。
- 小梁・間柱の表示偏心による力学成果の保持、既存の成果契約版不一致による利用停止は、GUI/MCP/IOの既存広域試験で確認する。

## 検証環境と実行記録

Windows、Rust stable 1.100.0、Clippy 0.1.100。コンパイル停止後の `rustup update stable` は同版unchangedで完了した。dev/test debug情報を0とし、共有targetを使う。実行ログは作業環境の `target/tmp/issue1364/` に保存する。ログそのものとブラウザ画像はGitで追跡しない。

広域初回は `--no-fail-fast` で全失敗を回収した。core/element/edit/solver/designは2566成功・1失敗（当時の偏心0Brace実軸の扱いの不一致）、GUI/MCP/IO featureは1733成功・1失敗（断面未割当・偏心0Braceのダンパー重量準備入力）。共通実軸APIの偏心0None経路を維持し、低位トラス構築と公開解析側で必要な入力検証を行う修正とした。既存fixture・既存期待は変更しない。job/load広域初回444成功。追加試験のMCP零実長の拒否形式と、自重の設計重量期待値は契約を確認して修正し、直接再検証する。

数式はmdBook生成HTMLのMathJax入力を確認し、ブラウザ表示で式・添字・改行・収まりを目視確認した。検定適用条件と操作説明もブラウザで確認した。

最終確認は以下のとおり。初回広域後は、変更・失敗の直接対象だけを再実行した。公開入力診断を追加した後は、その影響を受けるelement/solverのlib全体を再確認した。

| 対象 | 実行入口 | 結果・ログ名 |
| --- | --- | --- |
| 要素・solver最終 | `cargo test -p sepika-element -p sepika-solver --lib --locked --no-fail-fast` | 515 + 340成功、`analysis-entry-final.log` |
| 自重・K型配分 | `cargo test -p sepika-load --lib --locked xyz_brace_self_weight` | 1成功、`self-weight-final.log` |
| 圧縮座屈長さ | `cargo test -p sepika-design-jp --locked eccentric_brace_compression` | 1成功、`brace-design.log` |
| GUI/MCP新入口 | `cargo test -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp,sepika-io/parquet --locked --no-fail-fast xyz_brace` | 4成功、`entry-final.log` |
| 偏心0の途中重量入力 | app lib `ダンパー総重量はgui共通同期経路` | 元fixture・元期待の1成功、`damper-final.log` |
| 部材荷重移送 | solver `eccentric_member_load_reactions` | Brace追加後成功、`brace-load.log` |
| 層切断・支持ばね | solver `確定記録は独立外力100` | Brace追加後成功、`brace-story.log` |
| core/element/edit/solver/design/load/job静的解析 | 各クレートを `cargo clippy -p ... --all-targets --locked -- -D warnings` | 成功、`clippy-core-final.log` |
| MCP/GUI/Parquet静的解析 | `cargo clippy -p sepika-mcp -p sepika-app -p sepika-io --all-targets --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --locked -- -D warnings` | 成功、`clippy-feature-final.log` |
| 文書リンク・実装参照 | `cargo run -p xtask -- check-docs` | summary142・links1027・impl refs503成功、`check-docs-final.log` |
| HTML・整形 | `mdbook build docs`、`cargo fmt --all --check` | 成功、`mdbook-final.log`・`fmt-check.log` |

最終の公開拒否試験は静的両入口7入力、増分5入力、線形/非線形時刻歴各4入力を実行し、Resultの理由付きエラーとして返ることを確認した。工事途中の未割当・偏心0モデルを許す重量準備と、トラスを構築する解析入口の検証責務を分離している。

## 質量源と公開入口の入力責務の対照

ブレースの各入口で必要な入力と、不使用の情報を区別して検証する。この表はブレースを対象とし、梁の既存診断を緩和するものではない。

「用」は算定・配置に使う量、「構」は既存トラス構築時の入力制約、「全」は従来から残る全体局所基底検証、「識」は編集対象を識別する入力、「—」はその入口で新しい検査を課さない量を表す。用途・生成記録・ダンパー配置など、独立した既存モデル検証まで省略する意味ではない。

| 公開入口・条件 | 部材節点 | 実長 | 局所軸 | 断面 | A | 密度 | E | 総重量 | 偏心 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 静的once | 用 | 用 | 用 | 用 | 用 | 構※1 | 用 | 構※1 | 用 |
| Analysis prepare | 用 | 用 | 用 | 用 | 用 | 構※1 | 用 | 構※1 | 用 |
| 増分解析 | 用 | 用 | 用 | 用 | 用 | 構※1 | 用 | 構※1 | 用 |
| 線形時刻歴 | 用 | 用 | 用 | 用 | 用 | 用/構※1 | 用 | 設定時に用 | 用 |
| 非線形時刻歴 | 用 | 用 | 用 | 用 | 用 | 用/構※1 | 用 | 設定時に用 | 用 |
| 固有値解析（両公開入口） | 用 | 用 | 用 | 用 | 用 | 用/構※1 | 用 | 設定時に用 | 用 |
| 質量のみ・通常要素質量 | 用 | 用 | 用/構※3 | 用 | 構※3 | 用 | — | — | 用 |
| 質量のみ・LumpedOnly | 集中質量の節点のみ | — | — | — | — | — | — | 設定時に生成照合 | — |
| 質量のみ・総重量置換・CorrectedLumped | 全・生成照合 | 全 | 全 | 全※2 | — | — | — | 用 | 全※2 |
| 質量のみ・総重量置換・LumpedOnly | 生成照合 | — | — | — | — | — | — | 用 | — |
| 重量列挙・自動重力同期・通常材 | 用 | 用 | 非0偏心の実配置制約 | 用 | 用 | 用※4 | — | — | 用 |
| 重量列挙・自動重力同期・総重量置換 | 用 | — | — | — | — | — | — | 用 | — |
| GUIの準備・階生成 | 上記重量入力に全体局所基底検証を追加 | 全 | 全 | 全※2 | 重量源による | 重量源による | — | 重量源による | 全※2 |
| SetMemberEndOffsets | 用 | 用 | 非0偏心の実配置制約 | 識 | — | — | — | — | 用 |

- ※1: 静的・増分で密度を剛性式に使うことはないが、既存トラス構築は物理質量も解決する。LumpedOnlyの動的解析も剛性要素を構築するため、同じ構築上の制約を保つ。総重量置換では断面由来の密度を物理質量へ加えず、設定総重量の有効性を検査する。断面と材料は軸剛性に必要であり、総重量置換によって解析時に省略できるものではない。
- ※2: CorrectedLumpedとGUI準備が持つ従来の全体局所基底検証は残す。偏心0ブレースは断面・材料未指定でもこの幾何検証を通る一方、非有限な局所軸は拒否する。非0偏心には共通実配置の節点・断面参照等の入力制約を維持する。総重量置換時に新しいトラス構築・E・A・密度の診断を重ねない。
- ※3: 基準コミットのトラス構築もAの解決を行い、集中要素質量を選んでも局所基底を構築していた。Aは軸剛性用であり、質量自体の面積は共通材料領域から求める。Eは既存構築で格納するだけだったため、純粋要素質量へ新しい正値検査を加えない。節点参照・実長・密度・断面由来質量は実際の要素質量とその散布に必要であり、未解決参照を黙って原点や0物性へ置換することは本Issueの拒否契約に沿って停止する。
- ※4: DLは設計単位体積重量を使い、同じ列挙で計算する物理質量相当重量・解析行列負担は密度を使う。総重量指定材はそれらを指定総重量で置き換える。Eの正値診断を重量同期へ追加しない。

解析用の質量入力検査は、全質量方式でブレースの弾性入力を確認する。純粋な質量行列では、実際に要素質量を構築するブレースだけを既存のchecked constructorで診断する。この分離により、静的・増分・動的な剛性計算の不正入力を拒否しつつ、不使用の弾性特性を純粋な質量行列へ要求しない。

コードの対照位置は `sepika-element/src/frame/truss.rs`、`sepika-element/src/factory/input_check.rs`、solverの `common/assemble.rs`・`statics/linear/mod.rs`・`statics/analysis/precheck.rs`・`nonlinear/pushover/driver.rs`・`dynamic/timehistory/{linear,nonlinear}.rs`・`dynamic/eigen/mod.rs`、`sepika-load/src/story_gen/self_weight_calc.rs`、`sepika-app/src/app/actions/loads.rs`、`sepika-edit/src/beam_placement.rs`（いずれも `crates/` 以下）。SetOffsetsの断面用途は対象識別であり、E・密度・Aを計算するための条件ではない。

追加確認は以下のとおり。既存fixture・期待値は有効入力への置換で回避せず保持した。

| 対照試験 | 結果 |
| --- | --- |
| `weight_mass`全体 | 14成功。`mass-input-scope-final.log` |
| 元の節点質量専用・不使用局所軸試験をBeam/Braceへ同条件適用 | LumpedOnlyで節点質量2を保持、CorrectedLumpedと公開固有値両入口では従来の局所軸エラー |
| 総重量置換の断面未指定・不正断面参照・材料なし | 両質量方式で有効入力時と質量行列全成分が完全一致。同じfixtureの公開固有値解析は断面/材料の理由付きエラー |
| 純粋要素質量のE=0/−1/NaN/Inf | 有効E時と質量行列全成分が完全一致。同じfixtureの公開固有値解析はEの理由付きエラー |
| 公開静的両入口・増分・線形/非線形時刻歴の不正幾何 | 3回帰成功。`public-input-scope-final.log` |
| solver全target Clippy、MCP/GUI/Parquet全target Clippy、fmt | 成功。`clippy-mass-input-scope-final.log`・`clippy-mass-scope-feature-final.log`・`fmt-mass-scope-final.log` |

## 検証範囲

ここで確認したのは固定初期幾何・微小変位/回転の剛体接続、既存弾性トラス、既存引張専用反復と非線形解析内の弾性部材応答。接合部・剛体腕の耐力、新しい材料履歴、大回転・P–Δ/P–δ、STB配置変換のValidationを示すものではない。
