# RC壁のゼロせん断スパンと計算経路（#1288）

2026-11-18、基点 `d251cdc1` と本変更で確認した限定Verification。状態🔶。[#1288](https://github.com/hrntsm/SEPIKA/issues/1288)は未完了で、受理条件を拡張・縮小せず、算定動作は変更しない。現在仕様の正本は[壁脚内力と初期耐力](../../docs/calc_basis/08_部材終局耐力/08_RC壁の壁脚内力と初期耐力.md#平均圧縮応力度とせん断スパン比)と[参考骨格](../../docs/nonlinear/05_RC_部材_非線形復元力特性.md#耐力骨格点単位)に置く。

## 経路ごとの観測

| 入口・試験 | 入力・観測結果 | 実証の限界 |
| --- | --- | --- |
| core `zero_shear_span_has_same_capacity_as_lower_bound_for_both_methods` | min/meanとも比0と1のQuが一致 | 比の直接入力。壁内力の発生・採用条件を検証しない |
| element `selected_method_reaches_both_directions_with_real_area_and_limited_te` | min/meanの正負枝へ比 `[0,1]` を渡すと同じQu | 初期Qu用の下位入口。製品参照求解ではない |
| design-jp `zero_span_capacity_is_available_but_not_a_reference_skeleton_input` | 下位Quは比0で有限、骨格一括APIは理由付き拒否 | 骨格APIへ渡す比の契約 |
| design-jp `wall_reference_entry_limits_zero_face_moment_before_skeleton_validation` | 人工内力バッチの厳密Mface=0・Q=±800000 Nを製品共通の静的参考骨格入口へ渡す。min/meanとも比1と同じ骨格 | 実解析で発生した応答ではない。入口が先に1〜3へ制限してから骨格APIへ渡すことを観測 |
| solver `restrained_wall_ai_reference_can_approach_zero_face_moment_with_nonzero_shear` | 実Ai正負求解でQ=±200000 N、\(\lambda_{raw}<10^{-12}\)。同じモデルの初期Qu参照解決が成功し比1と同じQu | 恒久試験は公差付き近ゼロ。全プラットフォームの厳密0を保証しない |
| job `preparation_and_pushover_accept_wall_with_reference_moment_near_zero` | 未展開壁版を公開準備→壁展開→Ai実静的解析へ接続して近ゼロを確認し、公開増分計算が有限正の初期Quを保持 | 自動剛床・荷重を含む準備後の受理。GUI手入力の一連操作と実建物Validationは未確認 |

製品の静的参考骨格入口が下限1へ制限するため、下位骨格APIの0拒否を製品のMface=0拒否と同一視できない。初期Qu参照もQ=0は拒否するが、Q≠0かつ比が有限なら0を下位へ渡せる。下位APIの受理だけから、任意の製品入力でその状態が発生したとは判断しない。

## 反曲点を壁脚フェイスへ置く独立釣合い

既存の `rc_wall_side_column_model.rs` の矩形1層壁を用いる。柱心間幅4000 mm、高さ \(h=3000\) mm、壁厚180 mm、側柱600 mm角、壁脚face \(f=300\) mm。下側2節点は水平・回転を拘束し、鉛直だけ自由として各節点へ \(k\) の鉛直支持ばねを置く。下梁は両端ピン、上側は鉛直・回転を拘束し水平を自由とする。solver試験は壁面外水平も拘束し、job試験は生成剛床の水平拘束診断を満たすため壁面外水平も自由とする。側柱そのものの元端条件は両端固定を維持する。

この試験は既存の壁置換モデルの弾性釣合いを検証するものであり、剛性モデルの妥当性を新たに承認するものではない。下辺の回転を \(\theta\)、半幅を \(a=2000\) mm、側柱面積を \(A_c=360000\) mm²とする。中央壁柱の曲げ剛性は、既存の縦筋換算を含めて独立に次となる。

\[
EI_w=\{23000+(205000-23000)\times0.002\}
\frac{180\times4000^3}{12}=2.242944\times10^{16}\ \mathrm{N\,mm^2}
\]

上辺の回転を0とし、曲率を高さ方向に積分すると中央壁柱の下辺曲げは \(Qh/2-EI_w\theta/h\)。両側柱の軸伸縮による偶力を加え、壁全体の下辺曲げ \(M_0\) と支持ばね偶力を釣り合わせる。

\[
M_0=\frac{Qh}{2}-C\theta=2ka^2\theta,\qquad
C=\frac{EI_w+2EA_ca^2}{h}=2.955648\times10^{13}\ \mathrm{N\,mm}
\]

フェイス曲げ0の条件 \(M_0-Qf=0\) から、支持ばね定数を決める。

\[
k=\frac{fC}{2a^2(h/2-f)}=923640\ \mathrm{N/mm}
\]

支持ばねと上側鉛直拘束が壁周辺の偶力を受けるため、水平参照力だけでも壁脚faceの曲げは0へ近づきうる。基部固定・自由上辺の片持ち壁だけの \(M=Q(h-f)\) を、任意の支持条件へ一般化できない。

恒久試験は独立値923640を用い、浮動小数点の微小残差を0へ丸めない。Windows/stable 1.100の一時観測では、同じsolver試験の支持ばねだけを923639.9999999986 N/mmとしたとき、生成Aiの正負でそれぞれ \(Q=\mp199999.9999999948\) N、\(M_{face}=0\) N·mm、\(N=0\) Nとなり、初期Qu参照が成功した。これはその入力・環境での厳密0到達の観測で、全プラットフォームで同じ浮動小数点値が出る保証ではない。この微調整値の探索や厳密0のassertは恒久試験に置かない。

jobの初回試行は上側の壁面外水平まで拘束したため、自動生成剛床の水平拘束診断で止まった。上側の両水平を自由とした入力で準備・実求解・増分を再実行した。生成済みモデルを直接solverへ渡す成功と、通常の準備を通る成功は区別する。

## 採用判断と残る確認

比0を下限1へ置換すると、他の入力を固定した式では、制限前の比0をそのまま式へ入れる場合に比べコンクリート項は小さくなる。minの係数比は \(0.12/1.12\)、meanは \(\sqrt{0.12/1.12}\)。横筋・軸力項は変わらない。この単一項の方向だけでは、M=0での引張側の扱い、実際の破壊性状、壁全体の安全性や規準適合を判定できない。

経路差を維持すべきか是正すべきかの採用判断は保留する。計算受理・算術の一致を採用済みと扱わず、次の条件を特定できる根拠が必要である。

- M=0・Q≠0の反曲点断面を、両側柱付き壁の終局せん断耐力式に含めるか。含める場合の引張側主筋・圧縮側柱せいの選択と端部拘束条件。
- 壁脚faceのM/Qを採る初期Qu参照と、静的応答の参考骨格に対して同じ境界条件を使うか、別の条件を置くか。
- 比の下限規定が0までを含む適用承認か、正の短スパン入力に対する数値制限かを判断できる適用範囲・解説、ゼロ近傍の試験条件と破壊形式。
- 採用対象に対応する軸力・配筋・形状・拘束条件での安全性判断。有限値やQu増減だけに依存しないこと。

開口壁、線形換算剛性、Qc、別骨格、軸力上限、側柱なしの適用は本件で再判定しない。実験Validation、GUI手入力、全支持条件・全材料域・全プラットフォームの厳密0到達は未確認。

## ローカル検証

Windows、stable 1.100、clippy 0.1.100で実行。dev/testのデバッグ情報は0、共通targetを使用した。

| 実行 | 結果 |
| --- | --- |
| `cargo test -p sepika-core -p sepika-element -p sepika-design-jp -p sepika-solver --no-fail-fast --locked` | 2325件成功、既存ignored 2件（core641・design-jp733・element516・solver435） |
| `cargo test -p sepika-job --test rc_wall_zero_span --locked` | 最終の下限1耐力同値assertを含む1件成功 |
| `cargo clippy -p sepika-core -p sepika-element -p sepika-design-jp -p sepika-solver -p sepika-job --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all --check`・`git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` | summary142、links1028、impl refs503で成功 |
| `mdbook build docs`・Chrome headless表示 | 成功。変更2節を全体描画から切り出して文面・リンク・既存MathJax数式の表示を確認 |

初回の4クレート広域テストは新規試験の型参照の誤りでコンパイル失敗し、修正後に別実行で再検証した。共有fixtureの入力生成は変更していない。最終のjob同値assertは焦点再実行し、coreの既存テスト説明コメントの削除はfmt/diff確認後に反映した。既存のworkspace未使用依存警告は今回変更しない。CI成功・最終HEADの結果を予記しない。
