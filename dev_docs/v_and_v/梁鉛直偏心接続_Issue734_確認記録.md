# 梁の鉛直偏心接続（Issue #734）確認記録

## 対象と判定

対象は [Issue #734](https://github.com/hrntsm/SEPIKA/issues/734) の固定初期幾何に対する微小剛体接続である。基点SHAは `49e83a70c451a98e177a1895f6ad30b074a5863e`。検証対象は初回実装 `2c2eaa12c52ea6c293231226df00a6fa58e4cbbf` と、独立レビュー修正 `193ca93c76be92295553377236bba4df7546e088`。実行結果は下記に記録する。現行の計算契約と適用範囲は[座標変換](../../docs/calc_basis/04_要素剛性/08_座標変換.md#梁の鉛直偏心接続)、設計判断は[ADR 0076](../adr/0076-beam-eccentric-connection.md)を正本とする。本記録は数値Verificationであり、実験・実測によるValidationではない。

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

## 初回実装の実行結果

- 初回対象SHA: `2c2eaa12c52ea6c293231226df00a6fa58e4cbbf`（コード・試験。文書は後続の別コミット）。
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

## 独立レビューによるフランジ位置ゲートの是正

初回head `593cf346` の独立レビューで、フランジ位置の標準形式判定へ渡す実材軸の相対鉛直位置が0固定のままであることを確認した。既存の共有接合節点から各梁の実材軸端までのZ差を同じ位置判定へ供給し、判定式・公差・適用範囲は維持した。共通の偏心を新しいパネル中心位置や耐力式へ読み替えない。

`eccentric_panel_flange_applicability_uses_actual_end_heights_without_extending_staggered_scope` は、せい600 mm・フランジ厚17 mmの相対する同断面梁に対する独立期待値を確認する。梁の一律鉛直偏心−100/0/+100 mmなら上フランジ位置は191.5/291.5/391.5 mm、下フランジ位置は−391.5/−291.5/−191.5 mm。追加試験の最初のredは、−100 mm入力に対して上フランジ191.5 mmの期待に旧291.5 mmが返ることだった。

- 同偏心の両梁は標準形式に適合し、既存PanelJointのdb・実効体積等は偏心0と同じ。公開線形解析が成功する。
- 異偏心の組0/+100、0/−100、+100/−100では、上下の位置差がそれぞれ−100/+100/+200 mmとなり、接合部・梁・上下差付きの段違い診断で公開適用判定と解析を拒否する。
- 一方の偏心未指定を0補完せず、公開適用判定と解析が未指定理由で停止する。
- Beam/Fiber/MultiSpringと両梁の端順4組に対して同じ判定を照合する。Beamの既存自動生成は同偏心で生成、異偏心では生成しない。

Fiber/MSの試験は、既存のBeam経路で生成した明示パネルを保持し、対象梁の要素表現を切り替えて行う。NodeAdjacencyの自動生成対象は既存RustdocどおりBeamであり、本件でFiber/MSの自動パネル生成を新対応としたものではない。

修正後の対象SHAは `193ca93c76be92295553377236bba4df7546e088`。計算契約版は未公開Issue内の修正として53を維持する。

- `cargo test -p sepika-solver --test panel_zone eccentric_panel_flange --locked --quiet`: 追加受入1件成功。
- `cargo test -p sepika-core -p sepika-element -p sepika-solver --locked --quiet`: 1598件成功、既存ignored 1件。core591・element541・solver340の単体試験と、panel_zone21件を含む統合試験が成功。
- `cargo test -p sepika-design-jp panel --locked --quiet`: 既存パネル検定34件成功。
- `cargo test -p sepika-app --features gui --lib panel --locked --quiet`: GUI/保存契約等の既存パネル関連29件成功。
- `cargo clippy -p sepika-core -p sepika-element -p sepika-solver -p sepika-design-jp -p sepika-app --features sepika-app/gui --all-targets --locked -- -D warnings`: 成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- `cargo run -p xtask --target-dir target/issue734-doc-check -- check-docs`、`mdbook build`: 成功。リンク644件・実装参照687件。

## default構成のCI検証とGUI試験の境界

PR814のCI実行40176922975で、default構成のlib試験からGUI専用の編集・Undoメソッドを呼び出すコンパイルエラーE0599を確認した。STB取込試験はGUI限定にせず、feature非依存の既存編集コマンドで偏心0を明示し、未指定拒否とDL解析成功を両構成で引き続き確認する。編集・Undo・保存再読込による旧成果拒否の専用試験は、その操作が存在するGUI構成で実行する。製品コード・計算契約版53は変更しない。

修正対象SHAは `2dca3cf2`。次の結果はCI結果とは分けたローカル実行である。

- `rustup update stable`: stable 1.99.1のまま最新。`cargo clippy --version`: 0.1.99。
- `cargo test -p sepika-app --lib --locked --quiet`: default構成413件すべて成功。
- `cargo test -p sepika-app --features gui --lib --locked --quiet`: GUI構成932件すべて成功。
- `cargo clippy -p sepika-app --all-targets --locked -- -D warnings`: default構成成功。
- `cargo clippy -p sepika-app --features gui --all-targets --locked -- -D warnings`: GUI構成成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- `cargo run -p xtask --target-dir target/issue734-doc-check -- check-docs`、`mdbook build`: 成功（summary124・リンク644件・実装参照687件）。

## MCPと設計入口のfixture追随

CI実行40177947981で、MCPの標準配筋507関連3試験が取込梁の鉛直偏心未指定診断で先に停止し、workspaceの設計入口試験が断面未指定診断で先に停止することを確認した。いずれも製品ゲートを緩和せず、試験用入力を明示した。

MCPでは取込直後に梁の偏心Noneをassertして欠落保持を確認してから、既存編集コマンドで0を明示する。標準配筋の元情報・再出力保持、未対応配筋を消費する入口の拒否理由、正常な接合部検定比の既存期待は変更しない。設計入口では既存鉛直部材へ柱用途の有効断面と材料を与え、不正局所軸と部材ID付きの元診断を維持する。製品コード・計算契約版53は変更しない。

design-jp全試験の初回実行で、鋼材入力拒否試験の意図的な断面欠落・参照切れ2ケースに対し、共通ゲートの診断が先行して「部材 0」ではなく「部材 ID 0」を返す期待文言不一致を1件回収した。当該2ケースはIDと断面診断を期待し、他8ケースの期待は維持した。

修正対象SHAは `deaeb16a`。次はmain統合前のローカル実行結果である。

- `cargo test -p sepika-mcp --features mcp --locked --quiet`: lib131件・統合13件、計144件成功。
- `cargo test -p sepika-mcp -p sepika-design-jp --locked --quiet`: 再実行で計777件成功。design-jp単体656件・MCP default単体95件と両crateの統合試験を含む。
- `cargo clippy -p sepika-mcp -p sepika-design-jp --all-targets --locked -- -D warnings`: default構成成功。
- `cargo clippy -p sepika-mcp --features mcp --all-targets --locked -- -D warnings`: feature構成成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- `cargo run -p xtask --target-dir target/issue734-doc-check -- check-docs`、`mdbook build`: 成功（summary124・リンク644件・実装参照687件）。

## 先行Issue618とのmain統合

main `d00b3c293e364070345de49490b136c2071b46e4` を統合コミット `85f88a90` で取り込んだ。先行Issue618の解析契約53と本接続を区別するため、最終の解析契約版は54とする。上記の53での実行結果は各対象SHA当時の記録として保持する。

RC床板協力剛性の左右独立・材長加重平均厚と、承認済みの危険側注意・ADR0075は保持する。実P軸での要素構築と偏心接続は維持し、床板協力剛性のResult診断を合成する。full_modelの矩形RC梁に指定されたe=−D/2は、梁上端と床上面を一致させる先行Issueのfixture入力として保持し、解析応答への作用を含めて検証する。

`length_weighted_rc_slab_and_eccentric_connection_keep_independent_stiffness_and_mass_couplings` は、先行Issueの左右別の分割・異厚床板群fixtureを使用し、実P軸と床位置を変えずに共有N高さとe=±300 mmを変更する。E=23000、A=300×600、L=10000よりEA/L=414000 N/mm、結合Kux,θy=±124200000 N、回転Kの追加分=e²EA/L=37260000000 Nmmを独立値とする。床協力後I=10307037269.681742 mm⁴は元の独立値を保持する。既存RC質量契約のγRC=24 kN/m³、g=9806.65 mm/s²から端軸質量m/3=1.4683913466882166 t、結合Mの絶対値440.51740400646503 t·mm、回転Mの追加分132155.2212019395 t·mm²を得る。Beam/Fiber/MultiSpringの線形factory経路でこれらを照合した。試験初回の質量期待は材料に直接指定した密度を使用して不一致となったため、既存RC質量契約の単位体積重量を用いる独立値へ訂正し、製品質量は変更していない。

full_modelはGUI lib932件成功後、統合試験37/38件成功・代表値snapshotのみ不一致となった。矩形RC梁のe=−D/2が本Issueで初めてK・Mへ作用するため、上記の独立写像と床剛性の合成を確認してから、当該snapshot1件だけを更新した。これは実建物の独立解やValidationとはせず、明示fixtureに対する再現性回帰として扱う。

| 代表値 | 統合前 | 統合後 |
| --- | --- | --- |
| 固有周期T[1]/T[2] | 0.4647 / 0.4199 s | 0.4646 / 0.4198 s |
| EXの2FL/3FL/RFL最大Ux | 6.556 / 11.95 / 15.65 mm | 6.548 / 11.94 / 15.64 mm |
| 最大検定比 | 0.3700 | 0.3698 |
| 線形時刻歴最大Ux | 49.03 mm | 48.86 mm |
| 同層間変形角4層 | 4.088e−3 / 3.514e−3 / 2.025e−3 / 1.847e−3 | 4.070e−3 / 3.503e−3 / 2.019e−3 / 1.842e−3 |

同じsnapshotの層剛性・偏心率等にも微小差がある。準備重量・DL最小Uzと柱脚軸力和・検定件数と未検定0・履歴1001frameは不変。上端一致を取り消してe0へ戻す変更は行わない。更新後のfull_model38件・wall_model17件は成功した。

統合版54のコード・試験の対象SHAは `b2ad23c6`。ローカル検証は次のとおり。

- `rustup update stable`: stable rustc 1.99.1のまま最新。`cargo clippy --version`: 0.1.99。
- `cargo test -p sepika-core -p sepika-element -p sepika-solver -p sepika-design-jp --locked --quiet`: 計2276件成功、既存ignored1件。core591・element550・solver340・design-jp656の単体試験と全統合試験を含む。
- `cargo test -p sepika-element length_weighted_rc_slab_and_eccentric --locked --quiet`: 追加した床板平均厚と偏心接続の独立合成試験1件成功。
- `cargo test -p sepika-app --features gui --lib --test full_model --test wall_model --locked --quiet`: GUI lib932件成功、full_modelは前記snapshot1件の差分で停止。
- `cargo test -p sepika-app --features gui --test full_model --test wall_model --locked --quiet`: snapshot限定更新後、full_model38件・wall_model17件成功。
- `cargo test -p sepika-app -p sepika-mcp --lib --locked --quiet`: app default413件・MCP default95件成功。
- `cargo test -p sepika-mcp --locked --quiet`: default単体・統合計108件成功。
- `cargo test -p sepika-mcp --features mcp --locked --quiet`: feature単体・統合計144件成功。
- `cargo clippy -p sepika-core -p sepika-element -p sepika-solver -p sepika-design-jp -p sepika-app -p sepika-mcp --all-targets --locked -- -D warnings`: default構成成功。
- `cargo clippy -p sepika-core -p sepika-element -p sepika-solver -p sepika-design-jp -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp --all-targets --locked -- -D warnings`: GUI・MCP同時feature構成成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- `cargo run -p xtask --target-dir target/issue734-doc-check -- check-docs`、`mdbook build`: 成功（summary124・リンク646件・実装参照688件）。

## 未実施範囲と残る制約

実験・実測・実建物によるValidation、手動ネイティブGUI操作、塑性化後の全履歴を網羅する本接続専用の応答比較は未実施。一般XYZ偏心入力、柱/ブレース、二次小梁の新剛性算入、パネル中心偏心、新しい段違い接合式、有限回転・大変形接続、嵩上げ重量や別Issueの重量変更は対象外である。現在の時刻歴結果に独立した支持反力列を新設せず、既存の運動方程式・記録・質量と慣性入力の整合を確認する。PRの最終レビュー・CIは公開PRで別途確認する。
