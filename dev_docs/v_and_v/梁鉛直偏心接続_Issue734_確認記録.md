# 梁の鉛直偏心接続（Issue #734）確認記録

## 対象と判定

対象は [Issue #734](https://github.com/hrntsm/SEPIKA/issues/734) の固定初期幾何に対する微小剛体接続である。基点SHAは `49e83a70c451a98e177a1895f6ad30b074a5863e`。検証対象は実装コミット `2c2eaa12c52ea6c293231226df00a6fa58e4cbbf`。実行結果は下記に記録する。現行の計算契約と適用範囲は[座標変換](../../docs/calc_basis/04_要素剛性/08_座標変換.md#梁の鉛直偏心接続)、設計判断は[ADR 0076](../adr/0076-beam-eccentric-connection.md)を正本とする。本記録は数値Verificationであり、実験・実測によるValidationではない。

## 導出と独立期待値

初期偏心rに対し各端のTは、並進ブロックが単位行列、回転から並進へ写すブロックが `−[r]×`、回転ブロックが単位行列である。`δdP=T δdN` を仮想仕事 `fP・δdP=fN・δdN` へ代入すると `fN=TᵀfP`。固定Tで変位微分すると `KN=TᵀKP T`、速度の運動エネルギーから `MN=TᵀMP T` を得る。追加柔性・追加質量を仮定しない。r×Fの符号は右手系の外積から独立に定まる。

| 入力・条件 | 独立期待値 | 試験 |
| --- | --- | --- |
| r=(0,0,+100) mm、F=(1000,0,0) N | My=+100000 Nmm、θy=.001 radでuPx=.1 mm、仕事100 Nmm | `frame::eccentric::tests::vertical_arm_keeps_sign_and_virtual_work_and_parallel_force_has_no_moment` |
| r=(0,0,−100) mm、同じF | uPxとMyの符号反転、e=0は恒等、Fzは移送0 | 同上 |
| E=20000 N/mm²、A=100 mm²、L=1000 mm、端伸び.5 mm | N=1000 N。共有節点のMy=±100000 Nmm、実材軸端の曲げ0 | `eccentric_beam_factory_preserves_work_stiffness_mass_and_committed_member_forces` |
| 微小剛体回転θy=.001、jのuz=−1 mm | 両端に同じ偏心を加えても部材変形・不要な内力0 | 同上 |
| 長さ1000、方向=(.8,0,.6)、正負0偏心、i/j交換、片/両端ピン、剛域0/100 | 軸力の全体成分は(800,0,600) N、追加My=e×Fx、実部材曲げ0。既存軸剛性は全長EA/Lを維持 | `eccentric_sloped_and_reversed_members_keep_axial_force_and_released_physical_moment` |
| w=2 N/mm、L=1000 mm、両端固定、全体X方向部材荷重 | 反力Fx=−1000 Nずつ、My=−1000e Nmmずつ。断面曲げ0 | `eccentric_member_load_reactions_keep_transfer_moment_after_end_releases_and_rigid_zones`、`eccentric_nonlinear_member_loads_keep_section_forces_separate_from_node_transfer`、`eccentric_confirmed_member_load_response_keeps_support_transfer_moment` |
| 仕口パネルi端、e=100、全体X等価端力6000 N | My=600000 Nmm、γY一般化荷重=−300000 Nmm。節点θy=.0005とγY=.001は相殺し内力0 | `eccentric_load_reaches_panel_rotation_once_and_cancelled_rotation_has_no_strain` |
| 密度1e−8 t/mm³、A=100、L=1000 | 総並進質量.001 t、X単位加速度の両端My合計=e×.001。集中/整合両要素質量 | `eccentric_beam_factory_preserves_work_stiffness_mass_and_committed_member_forces` |
| 接続節点Z=3500、SL=3000、e=100 | 実材軸端Z=3600。断面shapeなしでも有効な解析性能なら接続可能 | `eccentric_axis_uses_node_level_and_reports_each_unresolved_input` |
| 不足・不正な偏心/節点/座標/局所軸/断面/材料 | 部材IDと端、項目、理由付きで停止。柱・ブレースへ適用しない | 同上 |
| GUI編集、Undo、OVIKA保存再読込 | 編集で入力識別変更・結果陳腐化。Undoで元応答、再読込で編集後応答へ一致 | `app::beam_placement_tests` |

非線形試験は通常Fiber、MultiSpring、およびBeamの集中ばね/ファイバー表現に対し、接線の差分照合、確定・revert・snapshot復元・checkpoint復号を行う。状態へ外側変位を重ねて保存せず、部材荷重を保持した確定状態も照合する。幾何剛性はN=1000 N、L=1000 mmで、偏心により生じるRx-Rx項=1.2e²、Rx-Uy項=−1.2eを独立値で確認する。

## 層切断の自由体と独立期待値

対象梁のN_i→P_i→P_j→N_j経路を、上側Z>cutと残る側に分ける。無荷重の接続腕の釣合いを消去すると、全交点の符号付き合力は「上側のN端力の和＋P実軸上の上側部材荷重」となる。単一交点なら実軸xiの断面力と同じであり、複数交点・腕だけの交点にも成立する。一要素に対して一つのCutForceへ合算する。N/Pが切断面に一致する場合は上側から除き、通過判定を>と<=へ揃える。集中荷重の既存i端側極限は維持する。

`eccentric_physical_path_cuts_include_arms_actual_load_position_and_plane_endpoints_once` はBeam/Fiber/MS、i/j反転に対して次を照合する。w=2 N/mm、L=3000 mmの全体X分布荷重、両端拘束では接続節点水平反力は−3000 Nずつである。

| N端のZ・実配置 | cut=1500 mmより上の荷重 | 交点合力 |
| --- | ---: | ---: |
| N_z=0/1800、水平距離2400、e=100 | 実軸xi=7/9、1333.333333 N | −1666.666667 N |
| 同じN、e=0 | xi=5/6、1000 N | −2000 N |
| N両端Z=0、P両端Z=2000 | 6000 N | 腕2交点の合計+6000 N |
| N両端Z=3000、P両端Z=1000 | 0 N | 腕2交点の合計−6000 N |
| N両端が切断面上、P両端Z=2000 | 6000 N | +6000 N |
| P両端が切断面上、N両端Z=2000 | 0 N | −6000 N |

同じ水平配置に無荷重の軸伸びを与えた場合、各端の軸力は非零でも二交点合力は0へ相殺する。`eccentric_member_endpoint_load_is_not_a_direct_fixed_node_load` は実P端の1000 Nをe=2000で全支持抵抗・該当層へ1000 N計上し、P=Nなら既存の固定点直接荷重控除により0となることを確認する。これは部材荷重を勝手にNへ移動しないための差である。

## 地動慣性入力の導出

全自由度から自由自由度へ抽出する行列をA、既存の地動剛体影響をrgとすれば、自由方程式への地動慣性入力は `−A M rg ag` である。`−(A M Aᵀ)(A rg) ag` と先に拘束列を消す式は、拘束列から自由行へ伝わる整合質量・偏心質量の寄与を欠く。本変更では前者を用い、その後の拘束縮約・減衰・時間積分を維持した。線形/非線形の共通GroundInfluenceを入力・履歴・層慣性集計へ渡す。

Ryだけ自由で並進を拘束した水平梁（他端全固定）、一律鉛直偏心e、全長L、密度ρ、正方形断面A/I、せん断面積を十分大きくした条件では、次を独立期待値とする。

- 回転剛性 `kθ=4EI/L+e²EA/L`。
- 整合回転質量 `mθ=ρAL³/105+2ρIL/15+e²ρAL/3`。1/105と2/15は三次変位形状・二次回転形状の二乗の区間積分である。
- 一様X地動の回転慣性係数は `eρAL/2`。eの正負で符号反転し、0なら0。
- 固有値は `ω²=kθ/mθ`。
- 初期静止、平均加速度法、地動列 `[0,1,1]` の最初の変位は `−(eρAL/2)/(kθ+4mθ/Δt²)`。

`eccentric_ground_motion_drives_free_rotation_with_restrained_translations` はBeam/Fiber/MultiSpringの公開固有値・線形/非線形時刻歴入口で上記を照合する。

偏心0でj端Uxだけ自由な整合質量の梁では、自由行の質量はρAL/3だが、地動影響係数は固定端列を含めρAL/2である。追加節点質量mnは両方へ同じmnを加える。質点のみ方式なら両方mnであり入力は不変となる。`consistent_ground_motion_keeps_fixed_end_mass_and_nodal_only_input_is_unchanged` が公開線形時刻歴の最初の変位を独立式で確認する。

## 実行結果

- 最終対象SHA: `2c2eaa12c52ea6c293231226df00a6fa58e4cbbf`（コード・試験。文書は後続の別コミット）。
- `rustup update stable`: 成功、stable rustc 1.99.1 (58cf8f9c3 2026-10-13)。`cargo clippy --version`: clippy 0.1.99。
- `cargo check -p sepika-element -p sepika-solver --locked`: 成功。
- `cargo test -p sepika-core -p sepika-element -p sepika-solver --locked --quiet`: core591件・element540件成功後、solverの新しい断面診断文言への期待差分1件で停止。診断期待を改訂。
- `cargo test -p sepika-element -p sepika-solver --locked --quiet`: element541件、solver337単体試験と全統合試験、計962件成功。地動入力・端解放・支持反力・部材荷重・既存動的回帰を含む。
- `cargo test -p sepika-solver eccentric_confirmed --locked --quiet`: 追加した増分確定応答の支持反力試験1件成功。
- `cargo clippy -p sepika-core -p sepika-element -p sepika-solver --all-targets --locked -- -D warnings`: 成功。
- `mdbook build`: 成功。`cargo fmt --all -- --check`、`git diff --check` も成功。
- `cargo test -p sepika-solver eccentric_ --locked --quiet`: 層切断・支持反力等の単体5件と動的統合1件が成功。
- `cargo test -p sepika-app --features gui beam_placement_tests --locked --quiet`: GUI焦点3件成功。
- `cargo test -p sepika-app --features gui --lib --locked --quiet`: 初回925/932成功。断面未設定を前提としたfixture6件はゼロ密度の有効断面を割り当て、STB取込後の旧成功期待1件は偏心未指定拒否を確認してから利用者の明示0入力で解析する契約へ追随。再実行で932件全成功。偏心専用の実OVIKA旧成果拒否と再計算による回復も含む。
- `cargo run -p xtask --target-dir target/issue734-doc-check -- check-docs`: 成功（docsのリンク644件、実装参照687件）。
- `cargo test -p sepika-solver --locked --quiet`: 層切断修正後の340単体試験と全統合試験成功。
- `cargo test -p sepika-app --features gui --test full_model --test wall_model --locked --quiet`: full_model38件、wall_model17件成功。原STBの未指定偏心停止を確認後、明示0によって従来の未解決主筋位置診断を確認するfixtureへ追随。
- `cargo clippy -p sepika-core -p sepika-element -p sepika-solver -p sepika-app --features sepika-app/gui --all-targets --locked -- -D warnings`: 成功。

## 既存時刻歴回帰の意図した変化

全DOFのM・rgを先に乗じる是正は、偏心0でも整合質量の固定端列を含む。独立した一自由度梁で、自由質量ρAL/3に対する地動係数ρAL/2とNewmark最初のstepを照合した後、既存の時刻歴snapshotを2件更新した。静的・固有値・断面検定など他のsnapshot行は不変で、波形のframe数と非線形収束数も不変である。

| 既存回帰 | 旧snapshot | 修正後snapshot | 確認した意味 |
| --- | --- | --- | --- |
| 鋼門型非線形の最大Ux | 0.8625 mm | 0.8700 mm | 約+0.87%、全1001frame収束・長期初期化保持 |
| 同じモデルの層間変形角 | 2.464e−4 | 2.486e−4 | 最大Uxと整合する変化 |
| 実建物線形の最大Ux | 48.95 mm | 49.03 mm | 約+0.16%、固定端側慣性入力を保持 |
| 実建物の層間変形角 | 4.081e−3 / 3.508e−3 / 2.022e−3 / 1.844e−3 | 4.088e−3 / 3.514e−3 / 2.025e−3 / 1.847e−3 | 同じ地動入力是正による変化 |

snapshotの一致自体を独立解との精度保証とはしない。独立一自由度式のVerificationと、実建物の再現性回帰を区別する。

## 未実施範囲と残る制約

実験・実測・実建物によるValidation、手動ネイティブGUI操作、塑性化後の全履歴を網羅する本接続専用の応答比較は未実施。一般XYZ偏心入力、柱/ブレース、二次小梁の新剛性算入、パネル中心偏心、新しい段違い接合式、有限回転・大変形接続、嵩上げ重量や別Issueの重量変更は対象外である。現在の時刻歴結果に独立した支持反力列を新設せず、既存の運動方程式・記録・質量と慣性入力の整合を確認する。PRの最終レビュー・CIは公開PRで別途確認する。
