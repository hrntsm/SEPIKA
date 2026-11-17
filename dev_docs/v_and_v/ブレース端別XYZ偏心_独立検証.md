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

## 検証範囲

ここで確認したのは固定初期幾何・微小変位/回転の剛体接続、既存弾性トラス、既存引張専用反復と非線形解析内の弾性部材応答。接合部・剛体腕の耐力、新しい材料履歴、大回転・P–Δ/P–δ、STB配置変換のValidationを示すものではない。
