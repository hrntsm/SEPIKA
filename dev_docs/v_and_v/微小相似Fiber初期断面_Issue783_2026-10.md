# 微小相似Fiber初期断面の検証（Issue #783）

2026-10-24、Windows・Rust stable 1.99.1で実行した数値Verificationを記録する。対象は[Issue #783](https://github.com/hrntsm/SEPIKA/issues/783)。実験・実測によるValidationと、独立レビュー・最終head CI・mergeは別の証拠とする。

## 変更前の再現と原因

公開 `nonlinear_time_history_analysis` を使う既存 `panel_753_public_nonlinear_time_history_propagates_axial_limit` から、微小尺度だけの集中ばね指定を除去した。通常寸法は成功し、尺度0.0001（柱長0.3 mm・軸力比0.5）はFiberの初期断面剛性のpivot=0でpanicした。

原因と適用範囲は次のとおり。

- 形状付き解析ファイバーの目標寸法に1 mm下限があり、微小H形の各板が中央の1点に縮退して弱軸の二次モーメントがゼロになった。絶対下限を除去し、最大断面寸法/16で分割する。形状なしの既定格子・円環の分割数は変更しない。
- 下限だけを除去すると、初期断面剛性の断面接線求解で後退誤差1としてpanicした。対角平方根による無次元化で軸・曲げの単位尺度を分離し、共通密行列LUは各右辺の無限大ノルム後退誤差を検査する。元の成分別検査は、ゼロ近傍の非対角成分に残る丸め残差だけで誤差1となる。取得した3×3行列を独立の恒等解テストに固定し、旧検査で失敗することも実行確認した。
- 尺度1e-8の偏心断面では、局所残差の尺度に加える1によりη=0の早期収束が生じた。絶対下限を除去し、初期断面対角平方根で残差と作用尺度を共通仕事尺度へ写して6成分の最大ノルムで相対評価する。各成分を別々に相対化する途中試行は、ゼロ近傍の他軸応答の丸め誤差で通常寸法solver15件を過剰拒否したため採用しない。

初期3×3を必要とする公開非線形factoryのAuto/Fiber/MSに、初期断面が特異・非有限であれば部材IDと理由を返す準備検証を追加した。全長分布Fiberの直接構築にこの可逆性条件を追加していない。任意の剛性や寸法による下限補完・正則化・弾性代替は導入していない。

## 独立期待値と観測範囲

| 試験 | 独立期待値・観測 |
| --- | --- |
| H形256×128、tw=tf=16 mm、直角隅 | 各矩形の厳密Iからセル自身のIを控除する中点則の閉形式。A=7680 mm²、配置交換後I=(5505024, 73891840) mm⁴。尺度1、1e-4、1e-8、1e4でA∝s²、I∝s⁴と初期Dを確認 |
| 偏心4ファイバー断面 | E=200000 N/mm²、A=1000 mm²、L=3000 mm、偏心(3,7) mm。軸変位1.5 mmからN=100000 N、η=(0.00060625,-8.75e-6,15e-6)、軸剛性EA/L。4尺度で力∝s²、曲率∝1/s、剛性∝sを確認 |
| 純二軸曲げ | 偏心なしの独立断面I=(400000,100000) mm⁴。両端逆向きの回転θ=(1e-5,2e-5) radにM=2EIθ/L、両端逆符号。4尺度で各軸M∝s³を確認し、共通LUのノルム検査が弱い成分の有意な応答を隠さないことを確認 |
| 失敗時の状態 | 上記偏心断面を確定後、初期Dをゼロ又はNaNとしてtrialを失敗させ、対象節点と初期剛性の理由、checkpoint完全一致を確認 |
| 公開非線形時刻歴 | 通常寸法対照と尺度1e-3、1e-4、1e-5の軸力比0.5をAutoのまま受理。既存n=1/1.1はパネル軸耐力の理由付きErr、初期断面が空となる形状なしfixtureは部材ID付きErr。時刻0の記録変位は与えた軸ひずみからの独立変位と一致 |

対応テストは `frame/fiber/constitutive_tests.rs::similar_*`、`dense_lu.rs::near_diagonal_coupling_roundoff_passes_normwise_backward_error`、`panel_zone.rs::panel_753_public_nonlinear_time_history_propagates_axial_limit`。

公開solverの時刻歴確認は初期時刻の観測であり、微小モデルの全時刻応答・塑性履歴を包括的に保証しない。実験Validation、他の全断面形状・極限寸法・全荷重履歴での精度保証は未実施。

## 保存成果と回帰

断面配置・局所釣合いの収束条件・密行列求解の受理条件と公開解析の開始条件が変わるため、解析計算契約を41から42へ更新する。検定計算契約は26を維持する。旧解析41の保存成果は読込で解析・依存検定を失効し、明示再計算で現行版となることをAppの実OVIKA回帰で確認する。旧40の既存パネル回帰も保持する。

既存input_checkの形状なし正常fixtureは、形状だけを削除して形状由来Jの基準を残していたため、明示諸元の基準と矩形寸法を設定した。旧巨大荷重1e300の局所失敗伝播fixtureは、相対残差修正で全体Newton非収束へ変わったため、有限の1e307で構成則の非有限失敗を確実に励起し、要素・解析フェーズの診断契約を保持した。

## 実行記録

以下は本修正の最終ローカル実行であり、CIと区別する。

| コマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-math -p sepika-element -p sepika-solver --locked` | math24・element518・solver319+統合70、計931件とdoc2件成功 |
| `cargo test -p sepika-app result_contract_tests --locked` | 32件成功 |
| `cargo test -p sepika-app --lib --features gui result_contract_tests --locked` | 33件成功 |
| `rustup update stable`、`cargo clippy --version` | stable 1.99.1は最新、clippy 0.1.99 (58cf8f9c35 2026-10-13) |
| `cargo clippy -p sepika-core -p sepika-math -p sepika-element -p sepika-solver -p sepika-app --all-targets --locked -- -D warnings` | 成功 |
| `cargo clippy -p sepika-app --all-targets --features gui --locked -- -D warnings` | 成功 |
| `cargo run -p xtask -- check-docs` | 全リンク・実装参照OK |
| `mdbook build` | 成功 |
| `cargo fmt --all -- --check`、`git diff --check` | 成功 |

途中の変更前panic・下限除去だけの失敗・個別相対残差の過剰拒否は上記の履歴として保持する。独立レビュー・親監査・最終head CI・mergeの記録は公開PRを参照する。
