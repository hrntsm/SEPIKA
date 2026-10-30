# 非線形モデルの内部契約とGUI未接続計算

Issue #1022で製品本文から分離した開発者向けの補助情報。GUIの現在仕様・既定値・計算根拠は [非線形モデル](../docs/nonlinear/README.md) と [免震・制振](../docs/calc_basis/09_免震制振/README.md) を正とする。以下のライブラリ専用式をGUI採用則と解釈しない。

## 材料の内部契約

このソフトは一軸材料を `UniaxialMaterial` トレイト（`trial`/`commit`/`revert`）として実装し、ファイバー断面積分と集中ばねに用います。
塑性率算定の基点には `reference_stress`（鋼・鉄筋は降伏強度、コンクリートはシリンダー強度）と `reference_strain` を用います。降伏強度に 1e18 以上を与えた実質弾性ダミーは塑性率評価の対象外とします。


## コンクリート一軸履歴モデル

このモデルは、圧縮を放物線上昇からピーク、線形軟化、残留へと進め、引張を弾性からひび割れ、指数減衰（テンションスティフニング）へと進めます。

**算定式**

初期接線:

\\[ E\_0 = 2 \cdot f\_c/|\varepsilon\_{c0}| \\]

圧縮上昇（\\( \varepsilon \ge \varepsilon\_{c0} \\)、\\( r = \varepsilon/\varepsilon\_{c0} \\)）:

\\[ \sigma = -f\_c \cdot (2r - r^2) \\]

圧縮軟化（\\( \varepsilon\_{c0} > \varepsilon \ge \varepsilon\_{cu} \\)）:

\\[ \sigma = -f\_c + \text{slope} \cdot (\varepsilon - \varepsilon\_{c0}) \\]

\\[ \text{slope} = f\_c \cdot (1 - \text{残留比})/(\varepsilon\_{cu} - \varepsilon\_{c0}) \\]

引張（\\( \varepsilon > \varepsilon\_{cr} = f\_t/E\_0 \\)、β = テンションスティフニング係数）:

\\[ \sigma = f\_t \cdot \exp(-\beta \cdot (\varepsilon/\varepsilon\_{cr} - 1)) \\]

除荷・再載荷は最大経験ひずみへの原点指向割線とします。

既定値は \\( \varepsilon\_{c0} = -0.002 \\), \\( \varepsilon\_{cu} = -0.0035 \\), テンションスティフニング係数 0.5, 残留比 0.0 とします。
これらの既定値の規準原典との照合は未確認です。

<div class="impl-ref">

**実装参照**：`sepika_material::uniaxial::Concrete`（`crates/sepika-material/src/uniaxial/concrete.rs`）が算定します。

</div>


## Karsan–Jirsa材料のライブラリ専用包絡線

修正 Kent–Park（圧縮ひずみの大きさ \\( x \ge 0 \\)、\\( r = x/\varepsilon\_{c0} \\)）:

\\[ \sigma = -f\_c \cdot (2r - r^2) \qquad (x \le \varepsilon\_{c0}) \\]

\\[ \sigma = -f\_c + \text{slope} \cdot (x - \varepsilon\_{c0}), \quad
   \text{slope} = \frac{f\_c - f\_{cu}}{\varepsilon\_{cu} - \varepsilon\_{c0}} \qquad (\varepsilon\_{c0} < x \le \varepsilon\_{cu}) \\]

\\[ \sigma = -f\_{cu} \qquad (x > \varepsilon\_{cu}) \\]

NewRC包絡線のGUI採用仕様は[一軸材料構成則](../docs/nonlinear/01_一軸材料構成則.md)を正とする。

## 履歴状態の再評価

### 確定点の再評価と初期接線

武田型、最大点指向型、原点指向型、標準型、逆行型と鉄骨横座屈型では、確定した変形と同じ変形を再評価・確定しても、力・接線・載荷方向・反転点・経験最大点を保持します。変形差の厳密なゼロを対象とし、正負のゼロを同じ点として扱います。有限の非ゼロ微小変形は載荷増分として評価します。

生成直後の原点では、力は0、接線は各骨格の初期剛性です。試行を取り消した場合は確定状態へ戻ります。応答の照会だけでは、別の変形で試行中の状態を変更しません。


## ファイバー梁の履歴確定

各試行状態と局所反復は同じ確定済み材料履歴を基準とし、局所収束だけでは履歴を確定しません。準静的増分解析で全体が未収束の試行を破棄するときは、材料履歴と断面ひずみ、弾性梁の変位、内力・接線を一体で確定状態へ戻します。時刻歴のステップ確定の扱いは[非線形時刻歴](../docs/calc_basis/05_構造解析/05_時刻歴応答解析.md#非線形時刻歴newmark-β--newton-raphson)に従います。


## GUI未使用の全長ファイバー積分

- 塑性化域を設けない全長ファイバー積分モデル（2 点ガウス積分、\\( \xi = \mp 1/\sqrt{3} \\)、重み 1）も要素として構築できますが、非線形解析の要素生成では用いません。

## パネルの状態復元

未確定の試行を取り消すと、せん断変形角・軸力状態・降伏耐力・塑性履歴は直前の確定状態へ戻り、内力・接線と再試行の応答も同じ確定状態に対応します。この復元の整合は、変動軸力下の除荷・再載荷や連成接合部モデルの妥当性確認を意味しません。
## GUIから自動適用しない免震ライブラリ計算

**記号・単位**

| 記号 | 意味 | 単位 |
|------|------|------|
| \\( n \\) | マルチシア配置の方向別ばね本数 | - |
| \\( Q\_{\max} \\) | すべり支承の最大水平力 | N |
| \\( \mu \\) | 摩擦係数 | - |
| \\( N \\) | 長期軸力（引張・浮上りは 0） | N |
| \\( \gamma \\) | せん断ひずみ | - |
| \\( C\_{Kd}(\gamma) \\) | 降伏後剛性のひずみ依存係数 | - |
| \\( C\_{Qd}(\gamma) \\) | 切片荷重のひずみ依存係数 | - |
| \\( K\_d(t) \\) | 温度 \\( t \\) における降伏後剛性 | N/mm |
| \\( K\_{d20} \\) | 20℃ における降伏後剛性 | N/mm |
| \\( Q\_d(t) \\) | 温度 \\( t \\) における切片荷重 | N |
| \\( Q\_{d20} \\) | 20℃ における切片荷重 | N |
| \\( t \\) | 温度 | ℃ |
| \\( k\_{eq} \\) | 等価水平剛性 | N/mm |
| \\( H\_{eq} \\) | 等価粘性減衰定数 | - |
| \\( \delta \\) | 設計変位 | mm |
| \\( \beta \\) | 初期剛性と降伏後剛性の比 \\( K\_1/K\_d \\) | - |
| \\( P\_v \\) | 転がり支承の長期軸力 | N |
| \\( P\_o \\) | 転がり支承の静定格圧縮荷重 | N |
| \\( \sigma \\) | 球面すべり支承の長期支持面圧 | N/mm² |
| \\( V \\) | 球面すべり支承の層間速度 | mm/s |
| \\( \mu\_0 \\) | 球面すべり支承の標準摩擦係数 | - |

## マルチシア（多層積層ゴム）低減率

多層積層ゴム（マルチシア配置）の剛性および耐力の低減率を、配置の幾何（せん断ばねの方向余弦の和）に基づいて算定します。

**算定式**

剛性低減率:

\\[ = 2/n \\]

耐力低減率:

\\[ = \frac{1}{\sum\_{i=1}^{n/2} \left( \cos(\pi/n \cdot (i-1)) + \sin(\pi/n \cdot (i-1)) \right)} \\]

<div class="impl-ref">

**実装参照**：`sepika_design_jp::isolator`（`crates/sepika-design-jp/src/isolator.rs`）が剛性低減率（`multi_shear_stiffness_reduction`）と耐力低減率（`multi_shear_strength_reduction`）を算定します。

</div>

## 摩擦（すべり）支承

すべり支承の最大水平力を、クーロン摩擦則により次式で算定します（N は長期軸力、引張は 0）。

\\[ Q\_{max} = \mu \cdot N \\]

<div class="impl-ref">

**実装参照**：`sepika_design_jp::isolator::friction_max_force` が最大力を算定し、要素側では 2 次元摩擦として合力を Qmax に射影します（`sepika_element::springs::isolator`）。

</div>

## 鉛プラグ入り積層ゴム（LRB）統一型・歪依存

鉛プラグ入り積層ゴム（LRB）の剛性と切片荷重は、以下の歪依存モデルにより、ひずみ γ への依存性と 20℃基準の温度換算を算定します。採用係数は対応する製品別モデルの適用条件に限り、他製品へ一般化しません。

**算定式**

- 剛性ひずみ依存係数 \\( C\_{Kd}(\gamma) \\)（\\( \gamma < 0.25:\ 0.779 \cdot \gamma^{-0.43} \\)、\\( 0.25 \le \gamma < 1.0:\ \gamma^{-0.25} \\)、
  \\( 1.0 \le \gamma:\ \gamma^{-0.12} \\)。第1分岐の指数は分岐点 \\( \gamma = 0.25 \\) での連続性から −0.43）
- 切片荷重ひずみ依存係数 \\( C\_{Qd}(\gamma) \\)（3 分岐）

温度換算:

\\[ K\_d(t) = K\_{d20} \cdot \exp(-0.00271(t-20)) \\]

\\[ Q\_d(t) = Q\_{d20} \cdot \exp(-0.00879(t-20)) \\]

等価水平剛性、等価粘性減衰:

\\[ k\_{eq} = Q\_d/\delta + K\_d \\]

\\[ H\_{eq} = \frac{2}{\pi} \cdot \frac{Q\_d \left( \delta - \frac{Q\_d}{(\beta - 1) K\_d} \right)}{k\_{eq} \cdot \delta^2} \\]

<div class="impl-ref">

**実装参照**：`sepika_design_jp::isolator`（`crates/sepika-design-jp/src/isolator.rs`）がひずみ依存係数・温度換算・等価剛性／等価粘性減衰（`lrb_stiffness_strain_factor`・`lrb_intercept_strain_factor`・`lrb_stiffness_at_temperature`・`lrb_intercept_at_temperature`・`equivalent_stiffness`・`equivalent_damping`）を算定します。

</div>

## 転がり支承・球面すべり支承・高減衰ゴム

転がり支承と球面すべり支承には以下の摩擦モデル、高減衰ゴムにはブリヂストン E6/E4 のひずみ多項式を採用します。係数は対応する製品別モデルの適用条件に限って用います。

**算定式（代表）**

転がり支承摩擦:

\\[ \mu = \frac{1.2 + 7.8 \cdot P\_v/P\_o}{1000} \\]

球面すべり支承 標準摩擦係数（MN型ほか）:

\\[ \mu\_0 = 0.043(2.03\sigma^{-0.19} + 0.068) \\]

速度依存:

\\[ \mu = \mu\_0(1 - 0.55 \cdot e^{-0.019|V|}) \\]

- 高減衰ゴム（ブリヂストン E6/E4）: Geq・Heq・U のひずみ多項式。登録された製品別係数の適用条件に限り、同じ構造形式の製品全般には一般化しません。

<div class="impl-ref">

**実装参照**：`sepika_design_jp::isolator`（`crates/sepika-design-jp/src/isolator.rs`）が各支承の摩擦係数・ひずみ多項式（`rolling_bearing_friction`・`spherical_bearing_mu0_mn`・`spherical_bearing_mu_mn`・`hdr_bridgestone_e6`・`hdr_bridgestone_e4`）を算定します。

</div>


一方、塑性化域を設けない全長ファイバー積分モデルは、通常の非線形解析の要素生成とは別のモデルです。こちらは可撓長にわたる断面積分から軸応答を得るため、一様弾性断面の軸剛性は \\( EA/L\' \\) となります。上記の塑性化域モデルの節点間長基準とは区別します。

