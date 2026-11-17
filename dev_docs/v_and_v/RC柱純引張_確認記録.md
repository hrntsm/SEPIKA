# RC柱の純引張と曲げ算定範囲（#1393）

2026-11-18。状態🔶。純引張の軸力釣合いと製品入口のVerificationを対象とし、全配筋・全軸力域・円形近似・二軸合成の一般的妥当性や実験Validationは確認していない。

現在仕様は[RC柱の純引張](../../docs/calc_basis/06_一次設計/02_RC_断面検定.md#柱の純引張)、[二軸曲げの合成](../../docs/calc_basis/06_一次設計/02_RC_断面検定.md#柱の二軸曲げの合成)、[保存成果の契約](../../docs/model_io/01_プロジェクト形式_ovika.md#保存結果の計算契約と再実行)を正とする。

## 独立した期待値

矩形500×500 mmまたは円形D600 mm、Fc24・SD345、径19 mm主筋8本、かぶり40 mm、帯筋径10 mm・間隔100 mmとする。矩形は各辺3本の外周配筋で隅角筋を重複させない。

コンクリートの引張抵抗を除き、一様引張で全主筋が許容引張応力度に達する状態の軸力釣合いを用いる。主筋総面積は8×π×9.5²=2268.229895892 mm²、許容引張力は長期215×As=487669.427616744 N、短期345×As=782539.314082682 Nである。1 MN純引張の独立期待比は長期2.050569388545、短期1.277891068223。入力モデルとは別の`rc_column_tension_reference.rs`へ期待値を置く。

純引張限界と一般のN–M境界は分けて確認する。現在のN–M点列は正の中立軸位置のみを走査し、純引張上限に至る全境界を表さない。引張側の点列範囲外の曲げ耐力を端点値で代用せず、非零曲げがあれば既存の許容曲げ0の理由付き未検定とする。純引張上限との新しい直線補間や相互作用式は導入しない。

## 回帰の入口と観測

| 入口 | 保証する契約 |
| --- | --- |
| design-jp `tests/rc_column_zero_moment_capacity.rs` | 公開`run_member_design_checks`で矩形・円形、長期・短期、3位置の純引張0/0.25/0.99/1/1.01倍と圧縮0.1倍、曲げ0/各軸/二軸を検証。純引張の等号適合・超過不適合、引張範囲外の非零曲げ未検定、算定範囲内の曲げ合成を保持 |
| 同上の既存圧縮境界試験 | 既存400×600 mm矩形と円D600、圧縮上限0.99/1/1.01倍、許容曲げ0×非零需要の理由未検定を維持 |
| app `src/app/rc_column_boundary_tests.rs` | 準備・実解析・自動検定から純引張の3位置不適合、現行OVIKA保存再読込、旧検定55の失効と現行解析内力からの明示再検定を確認 |
| app `src/viewer/check_ratio.rs` | 実解析後の純引張をNG、曲げを伴う範囲外を未検定としてツールチップ・検定比図へ描画し、0.00 OKを表示しない |
| mcp `src/job/design_check.rs` | DesignCheckジョブの単独ケース・有効な保存G+P組合せで3位置NG、全件適合false、最大比のJSON往復を確認。引張範囲外の非零曲げは未検定3件として伝達 |

GUI/MCPの実解析では準備による自重が純引張を減らすため、1 MNを直接与える独立入口の比と同一値とはしない。現行解析72を保ち検定契約を55から56へ更新する。旧検定を再保存だけで現行と認定しない。

## 検証実行

Windows、stable Rust 1.100、dev/test debug=0、共有targetで実施。対象コードを各構成で再ビルドし、以下を実行した。引張側範囲外の判定に負の設計軸力のガードを明示した後、単体境界1件・公開入口2件・app4件・mcp4件・Clippy・fmtを再実行して成功した。

- `cargo +stable test -p sepika-design-jp --locked`：単体690件と全統合テスト成功。独立圧縮値・共有入力補助の整理後、`--test rc_column_zero_moment_capacity`の2件も再実行して成功。
- `cargo +stable test -p sepika-job --test rc_column_axial_boundary --locked`：1件成功。
- `cargo +stable run -p xtask -- check-docs`：成功（summary142、links1031、impl refs505）。
- `cargo +stable clippy -p sepika-core -p sepika-design-jp -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp --all-targets --locked -- -D warnings`：成功。
- `cargo +stable test -p sepika-app --lib --features gui rc_column_ --locked`：4件成功。
- `cargo +stable test -p sepika-mcp --lib --features mcp rc_column_ --locked`：4件成功。
- `cargo +stable test -p sepika-mcp --lib --features mcp contract_versions_and_missing_metadata_preserve_analysis_and_design_rows --locked`：1件成功。
- `cargo +stable fmt --all -- --check`：成功。
- `mdbook build docs`：成功。生成HTMLとChromeのMathJax描画画像で、純引張式・軸力比の3分岐・負号・不等号・最大比と本文の折返しを確認し、崩れなし。

## 確認範囲の限界

引張側点列範囲外は未検定であり、全引張域の複合応力状態を数値上適合にする機能は提供しない。未検定を不適合件数へ混ぜず、全件適合とも扱わない。点列範囲内の既存集約配筋・円形等価矩形・二軸合成の全域精度、刊行規準全適用条件、手動GUI・MCP実通信、実験Validationは未確認である。構造規定警告の条件を扱う#1392とは分離する。
