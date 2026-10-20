# 側柱の物理分類・回転接続・局所釣合いを要素表現から分離する

- Status: accepted
- Date: 2026-10-18
- Related: [#496](https://github.com/hrntsm/SEPIKA/issues/496)、[ADR 0013](0013-adopt-wall-element-model.md)、[ADR 0019](0019-material-wall-section-and-directional-strength.md)

## 背景と判断理由

側柱の物理的役割を Beam の検出処理と一体にすると、Fiber／MultiSpring への表現変更で壁側の断面算入や解放判定が変わる。
また、壁面法線に合わせて材料の断面軸まで回すと、接続条件のために材料モデルの意味を変えてしまう。
非線形要素では、線形剛性の縮約だけでは解放回転の釣合いと収束状態の内力を得られない。
この三つを独立した基盤に分け、分類・接続・数値求解それぞれの契約を検証できる配置を採用する。

## 決定

### 物理分類は core で全候補と理由を保持する

`sepika-core` の `WallSideColumnIndex` に、要素表現に依存しない物理分類を置く。
明示材料・断面用途・鉛直側縁・両端節点の一致を使い、RC／SRC 置換壁、純鋼板の境界、成立しない壁、未確定を区別する。
全一致候補と関連 ID・理由を保持し、重複候補や非平行共有を最初の一致で隠さない。反転した平行法線は同じ解放面として扱う。
材料不明を RC へ推定せず、材種の認識と耐力・解析種別の適用可否を分ける。

利用側ごとの Beam 限定走査の追加は、同じ部材の役割が利用経路で異なるため採用しない。
core の分類は計算要素を生成せず、解放・耐力成立・解析適用の成功を意味しない。

### 回転接続を材料の断面軸と塑性モデルから独立させる

`ReleaseConnection` は、元の部材局所軸を保った可撓端変位と内部回転を結ぶ。
壁面法線の断面内射影から回転だけの写像を作り、内力・残差・全接線ブロックを同じ写像で仕事共役に変換する。
壁法線まわりの自由解放は元の等方端部回転ばねとの直列接続として表し、保持する回転方向には元の端部条件を残す。
固定は拘束関係、自由解放は零ばねとして表す。

材料軸や並進軸を一括して回転する案は、材料構成則と接続方向を混同するため採用しない。
大数の penalty で固定を近似する案は、有限値の選択が条件数と応答へ影響するため採用しない。
この接続は既存材料モデルの入力・出力を扱う基盤であり、材料構成則そのものを決定しない。

### 局所釣合いと線形求解を分離する

`solve_local_equilibrium` は、同じ commit 履歴から各 trial を評価する呼出側の契約の下で、内部回転の荷重込み残差 `r_b = 0` を解く。
収束した同じ trial の保持側内力 `f_a` を返し、接線は `Kaa − Kab solve(Kbb, Kba)` とする。
非線形内力を `Kcondensed * u` や収束前の内力で置き換えない。
荷重の変位依存性がある場合は、呼出側が残差とその微分を同じ trial に含める。

線形求解は `sepika-math` の `CheckedDenseLu` に分け、既存依存の faer による部分ピボット LU と複数右辺求解を使う。
逆行列の明示生成や対称正定値の仮定を置かず、特異・非有限・不収束・評価失敗を明示する。
零剛性や弾性への fallback、正則化によって成功へ読み替えない。
材料と内部回転の commit／revert、失敗 trial の破棄は呼出側が一体で所有し、局所収束だけでは commit しない。

## 記録の範囲と限界

`accepted` は上記基盤の設計判断の採用を表す。
基盤の単体 Verification、実 Fiber／MultiSpring・壁系の独立解析比較による Verification、実験等による物理 Validation を区別する。
検証結果は対象のコードと固定時点に対応付け、分離前の成果をそのまま現在の計算経路の成功根拠にしない。
基盤の単体 Verification の成功はこれらの達成を示さず、本 ADR は [#496](https://github.com/hrntsm/SEPIKA/issues/496) の全 7 完了条件の達成や計算経路の適用成功を示さない。
準静的な局所縮約だけでは、動的解析・P–Δ・非平行共有柱の定式化、全状態での解の一意性や収束性を保証しない。
現在の役割・解放方向・解析別適用条件は[壁エレメントモデル](../../docs/calc_basis/04_要素剛性/05_壁エレメントモデル.md)を正本とし、本ADRは判断理由と検証の境界を記録する。

## 実装参照

- [物理分類](../../crates/sepika-core/src/model/wall_side_column.rs)
- [回転接続](../../crates/sepika-element/src/wall/side_column/release_connection.rs)
- [局所釣合い](../../crates/sepika-element/src/wall/side_column/local_equilibrium.rs)・[密行列求解](../../crates/sepika-math/src/dense_lu.rs)

Legacy source:
- [Issue #496](https://github.com/hrntsm/SEPIKA/issues/496) の物理分類・解放方向・局所釣合い契約と、本 ADR 記載の基盤分離の採用判断
