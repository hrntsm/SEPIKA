# 正定値求解の数値条件（Issue #818）確認記録

2026-10-25、Windows・Rust stable 1.99.1・faer 0.24.0。起点は `59f2deb7237ebc505d4726b65d6c75bb883aff8c`。本記録は当該起点に対する作業差分のVerificationであり、最終headの独立レビュー・CI・mergeは公開PRで確認する。現行式と適用条件は[線形代数ソルバ](../../docs/calc_basis/05_構造解析/01_線形代数ソルバ.md)、方式選択は[ADR 0078](../adr/0078-equilibrated-cholesky-condition-estimate.md)を正とする。

## 再現と原因

無階・無剛床。固定柱脚 `(0,0,0)` から柱頭 `(0,0,3000)` のRC600角、柱頭から自由先端 `(6000,0,3000)` のRC400×700梁。E=23000 N/mm²・ν=0.2、梁鉛直偏心を明示0とする。自動剛域は柱0/200、梁125/0 mm。柱脚以外は全自由、既定のねじり解放。先端荷重は `[0,500,-1000,0,0,0]`（N・Nmm）。

修正前の `Analysis::prepare` と直接静解析は成功し、柱頭Rz=`98303999999.9999 rad`、先端Uy=`589823999999999.5 mm`を返した。これは独立した機構運動と整合しない正常終了である。

独立変位は柱頭Rz=θ、先端Uy=6000θ・Rz=θ、その他0。柱の解放ねじりと梁の剛体回転だけで構成される。θ=0.001で要素剛性と独立変位の積を確認し、全要素端力の絶対値が `1e-7` 未満（並進N・回転Nmm）となる。機構運動は求解結果や因子から作らない。

未平衡のKでは並進・回転の単位と規模が混在する。faerの通常のLLᵀ成功判定はpivotが正かどうかであり、理論上の零剛性が組立・変換・消去の丸めで微小正値になれば成功する。分解成功は正定値の数値的信頼性を保証しない。物理的なねじり解放や拘束を変えて原因を回避していない。

## 独立期待値と対応テスト

| 対象 | 独立期待値・許容 | テスト |
| --- | --- | --- |
| RC機構の存在 | 指定零エネルギー運動の要素端力 <1e-7 | `mechanism_diagnosis::rc_torsion_release_has_independent_zero_energy_motion` |
| 同じ機構の公開入口 | ゼロ荷重・鉛直のみ・面外込みとも静的準備と直接静解析を拒否 | `rc_torsion_release_is_rejected` |
| 直接固有値入口 | 反復開始前に数値条件を含む不安定診断 | `standalone_eigen_entry_rejects_the_same_mechanism_before_iteration` |
| RC柱ねじり保持 | 柱頭Rz=PLH/(GJ)=5.145356315924878e-5 rad、相対1e-9 | `retained_column_torsion_matches_independent_flexibility` |
| 幾何・剛性規模 | 長さ倍率0.001/1/1000、E倍率1e-6/1/1e6の9条件で、柱頭ねじりが倍率の二乗とEに反比例 | 同上 |
| 丸め限界の連成 | 2自由度のほぼrank1、厳密2進の座標倍率・符号反転で拒否 | `cholesky::acceptance_tests::rounded_mechanism_is_rejected_for_scaled_and_sign_reversed_coordinates` |
| 有限弱剛性 | 連成ばねの弱剛性1e-10、自由度数2/10000、座標倍率1e±60・独立剛性1e-200を受理、既知解相対1e-5 | `finite_weak_stiffness_is_accepted_independently_of_other_blocks_and_units` |
| 自由度置換 | 既知解を持つ弱連成・独立剛性1e120の3通りの置換を受理、相対1e-5 | `coordinate_permutations_preserve_weak_finite_solutions` |
| 密な疎行列分解 | 192自由度、rank1+1e-8 Iの有限剛性を受理し既知解相対1e-5、機械精度の摂動は拒否 | `dense_factor_preserves_finite_weak_stiffness` |
| 下三角契約 | 上三角の0/NaN/1e300に依存せず同じ解、下三角NaNは拒否 | `lower_triangle_is_the_only_input_to_factorization_and_condition_diagnosis` |
| 分解失敗後 | 同じ寸法・異なる寸法の失敗とも旧因子での求解を拒否、正常な再分解で回復 | `failed_refactorization_invalidates_all_previous_solutions` |
| 標準推定 | 対角・負成分を含む既知1ノルム、単一次元、初期/座標probeから隠れた部分空間を最後の交互符号probeで検出 | `condition::tests` |

RC保持例のJは採用する正方形600mmのねじり定数 `18,252,000,000 mm⁴`。既存仕様でねじりは全長H=3000mmへ分布する。剛域による曲げの可撓長を、ねじり長に取り違えない。寸法倍率の試験は幾何寸法の数値変更であり、製品のmm入力単位を変更する機能ではない。別に行列座標の対角合同変換で単位・自由度規模への頑健性を確認する。

閾値付近の一般的な小摂動SPDを、任意の十進座標倍率で常に拒否する保証は置かない。実際、`1+ε`の2自由度例では合同変換の丸めで推定値が判定境界へ移る条件があった。これを隠すための閾値倍率は追加しない。

## 共有経路と確認範囲

静的準備・長期剛性・引張専用反復、直接固有値、増分接線、線形/非線形時刻歴の有効剛性、および初期加速度の質量行列は共通Choleskyを呼ぶ。全ての数値分解で診断が働く。既存の反復終了・増分失敗/戻し・非線形履歴契約は呼出側へ残す。質量のLUへの既存切替を削除せず、静的機構であることだけから動的有効剛性を拒否する新しいゲートは設けない。

既存の `sepika-solver` 全テストで静的・非線形・固有値・時刻歴を回帰確認する。LUの不定値・非対称の公開求解契約もmath既存試験で維持する。これは任意モデル・全塑性経路・全ての丸めや並べ替えでの保証ではない。

## 性能比較

再現用コマンド：`cargo test -p sepika-math condition_estimation_cost --locked -- --ignored --nocapture --test-threads=1`。

3次元の等間隔格子の各自由度に対角6.1、各隣接自由度間に-1を与えた正定値の7点差分行列。行列値は無次元で、8³と16³自由度を用いた。旧方式相当は同じfaer高水準LLᵀの数値分解のみ、新方式は合同平衡化・同じ数値分解・逆条件数診断の合計。両者とも記号解析をウォームアップ後再利用し、faer逐次実行で各5回の平均を比較した。既存起点の旧方式APIを、今回作業差分中の同じプロセスで呼び出しており、別SHAのバイナリを横並び比較したものではない。

| 自由度 | 行列格納成分数 | 旧方式相当 | 診断込み | 比 |
| --- | --- | --- | --- | --- |
| 512 | 3200 | 0.252 ms | 0.434 ms | 1.724 |
| 4096 | 27136 | 5.109 ms | 8.870 ms | 1.736 |

Windowsの当該端末・devプロファイル（faer最適化3）での単回計測。OS負荷・並列性・疎構造・fillによる変化を含む性能保証ではない。追加分解・密逆行列はないが、特に軽い数値分解では数回の三角求解の比率が増える。

## 実行証拠と未実施範囲

共有target内の`issue818-*.log/.exit`をローカル実行証拠に保存した。

| コマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-math -p sepika-core --locked` | 合計667成功・2 ignored、exit0。math33+doctest2、core632。ignoredは既存長時間試験と今回の手動性能計測 |
| `cargo test -p sepika-solver --locked` | 13suite合計429成功、exit0 |
| `cargo test -p sepika-solver --test mechanism_diagnosis --locked` | 幾何・E倍率9条件の追加後も4成功、exit0 |
| 手動性能計測コマンド（上記） | 1成功、exit0 |
| `cargo clippy -p sepika-math -p sepika-solver -p sepika-core --all-targets --locked -- -D warnings` | exit0 |
| `cargo run -p xtask --locked -- check-docs` | summary124・links647・impl refs688、exit0 |
| `mdbook build` | exit0 |
| `cargo fmt --all -- --check` | exit0 |

初回テストfixtureの未割当材料・配筋型の構築誤り、想定した超節点サイズ80の誤り、ねじり長を曲げ可撓長に取り違えた独立期待値、および最初のclippyの固定配列指摘は修正して再実行した。任意の十進倍率で境界SPDを一律拒否する旧試験は、上記の契約上の限界を区別する試験へ変更した。

手動GUI・実験/実測Validation・任意行列の機構検出の数学的保証は対象外。標準推定は逆ノルムの下界を返すため、全機構を検出する証明とはしない。最終headの独立レビューと全適用CIは公開PRの証拠と区別する。

## 最終main統合（解析57・検定30）

`origin/main` の `27fa9502`（#786・#731・#652の文書を含む）を `88bb0d34` で統合し、解析契約版を57へ進め、検定契約版30を保持した。未検証索引の追記競合は双方の記録を保持して解消した。保存形式ページに既存の2箇所の現行版表記も57/30へ整合し、旧解析56以前の再解析を明記する。診断の式・閾値・物理モデルは変更していない。

統合直後、共有targetを再利用した最初の複合テストではRC機構の拒否2試験が失敗した。そのログにはmathの実compileがなく、ソース内容が本件と一致するartifactの使用を確認できなかった。診断を一時観測すると、実経路の平衡化行列の1ノルムは2.938365726322542、逆1ノルム推定は7.551400522552195e15、推定逆条件数は4.506782971324298e-17となり、12列を全て求解した独立の逆1ノルムとも一致した。一時観測を除去して元コードへ完全復元し、source mtime更新後にmathの実compileを確認して既定構成を再実行すると、同じ機構を含め全試験が成功した。

これは共有targetの再利用と再buildの観測として保持し、元artifactの内容を遡って確認できていないため原因を断定しない。閾値の増加や試験条件の緩和で解消していない。package限定cleanはtargetのCACHEDIR.TAG不備によりCargoが削除前に停止したため、全target削除や他cacheへの操作を行わず、元sourceの明示再buildを用いた。最終CIの新しいrunnerで確認する範囲と区別する。

| 統合後コマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-math -p sepika-core -p sepika-solver --locked` の初回 | core632・math33は成功、solverの新RC拒否2件でexit101。全成功とは扱わない |
| 元math sourceの明示再build後 `cargo test -p sepika-math -p sepika-solver --locked` | math33+doctest2・solver429成功、exit0 |
| `cargo test -p sepika-app --features gui --lib --locked` | 943成功、exit0。#786の既定解放機構拒否/Keep独立変位・支持釣合い、#731の準備・編集・Undo/Redo・保存再読込、保存成果の版失効を含む |
| `cargo test -p sepika-load --test beam_upstand --locked` | #731の14成功、exit0 |
| `cargo clippy -p sepika-math -p sepika-core -p sepika-solver -p sepika-app --features sepika-app/gui --all-targets --locked -- -D warnings` | exit0 |
| `cargo fmt --all -- --check` | exit0 |
| `cargo run -p xtask --locked -- check-docs` | summary124・links650・impl refs691、exit0 |
| `mdbook build` | exit0 |

一時probe sourceと各段階の`issue818-*.log/.exit`は共有targetへ保存し、製品コード・テストから一時観測を除去した。既定構成とGUI構成は元の診断コードで成功している。最終headの独立レビュー・CIはこのローカル結果へ予記しない。

## 文書・テストのみの後続main統合

#808の調査PRを含むmain `0f3d622f` を `e4792ba0` で統合した。製品ロジックの変更はなく、mathソースと解析57・検定30を維持した。未検証索引の競合は両節を保持して解消した。新しい塑性ばねの確定接線と公開時刻歴の回帰を含む `cargo test -p sepika-solver --test general_member_loads --locked` は12成功・exit0。先の全回帰は反復せず、追加範囲と文書・fmtを確認した。
