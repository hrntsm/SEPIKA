# ピン端ファイバー梁の数値釣合い（#1182）

2026-11-03、Windows、rustc 1.99.1で確認した数値的なVerificationを記録する。
物理モデル・収束判定の正本は[非線形梁要素のモデル化](../../docs/nonlinear/04_非線形梁要素のモデル化.md#整合接線と履歴)、課題は[#1182](https://github.com/hrntsm/SEPIKA/issues/1182)を参照する。

## 再現と原因の切分け

基点 `ad38801f` で、500×500 mm、長さ3000 mm、始端ピン・終端固定、E=205000 N/mm²、fy=235 N/mm²、G=78846.15 N/mm²、Lp=250 mmの人工矩形断面を、終端y方向へ2 mmずつ40回載荷した。
最初の2 mmで内側断面釣合いが80反復停止し、始端My残差は−2.277229886545171×10⁻¹⁰ N·mm、正規化残差は2.277229886545171×10⁻¹⁰だった。Issue記載値と一致する。

一時診断では始端Myの残差尺度が1 N·mm、終端曲率κzが−6.541639310476461×10⁻⁷ /mmだった。
初期断面剛性のMy–Mz成分には0.013671875、終端の断面Myには−1.862645149230957×10⁻⁹ N·mmが残り、初期弾性分の控除からy方向の追加回転−5.431954023984867×10⁻²² radが生じていた。
これらは載荷面と直交する微小成分であり、許容値だけを変えてよい根拠にはならない。

断面力と断面剛性の逐次加算だけをNeumaierの補償和に変更すると、初回の内側停止は解消した。一方、60 mm時には外側の全Newton補正が大きな回転へ振れ、20反復で停止した。
この時点の始端回転は−0.14704771729954885 rad、Mz残差は−1.4108006875883595×10¹⁰ N·mmだった。こちらは微小な丸め残差とは異なる、実際に大きい釣合い残差である。
補償和に加えて外側の補正に残差減少線探索を用いると、元の40回載荷が収束した。

## 数値精度の扱い

補償和は各加算で失われる低位の寄与を別に保持して最後に戻す。断面力とその完全接線・初期断面剛性へ同じ方式を適用し、材料応答・積分点・実断面座標は変えない。
初期剛性の連成成分を強制的にゼロにしたり、断面力を対称化したりはしない。したがって非対称断面の実在する連成も保持する。

残差から丸め誤差を差し引く分岐、微小剛性、正則化、弾性応答への代替を追加していない。
内側の80反復・外側の20反復、相対許容値10⁻¹⁰、解放成分の厳しい尺度も保持した。
線探索中は材料を確定せず、棄却候補の局所解を次候補へ持ち越さない。同じ確定材料履歴・同じ元局所解から候補を評価する。
有限な減少候補が得られたこと自体を収束とはせず、次の反復で元の釣合い判定を満たした場合だけ成功する。

この変更は既存の[ADR 0064](../adr/0064-fiber-ms-axial-biaxial-consistency.md)を維持する数値求解の修正であり、新しい物理モデルの選択ではない。
加算誤差以外の有限精度限界を除去した保証、任意の断面・載荷での収束保証、構造計算上の一律の安全側判定には拡張しない。

## 現役の回帰と独立期待値

| 入口 | 保証 |
| --- | --- |
| `sepika-section` の `fiber::tests::cancelling_fiber_contributions_retain_small_force_and_coupling` | 寄与10¹⁶、1、−10¹⁶、1の数学的な合計2を断面力・連成剛性で保持する。実装の加算手順をテストで再現しない |
| `sepika-element` の `frame::fiber::tests::ピン端の500角断面は降伏後の釣合いと接線と履歴を保持する` | 元の40×2 mm、ピン解放、塑性化、初期比95%未満への剛性低下を確認。78/60/0/−40/−80/−60/0/80 mmの除荷・反転で断面釣合い、物理材端モーメント、同一確定状態からの軸・両曲げ・回転の中央差分接線、checkpoint/revert/retry一致を照合 |
| `frame::fiber::constitutive_tests::same_commit_finite_difference_includes_all_axial_biaxial_couplings` 等 | 軸塑性と二軸曲げ連成の内力・整合接線を維持 |
| `frame::fiber::independent_plastic_reference::independent_plastic_axial_event_partition_preserves_work_at_every_history_commit` 等 | 材料履歴・仕事を独立な塑性参照と照合 |
| `frame::fiber::independent_plastic_reference::independent_plastic_perfectly_plastic_nonunique_end_partition_fails_atomically_and_retries` | 有限の完全塑性入力で解が非一意となる場合は成功にせず、試行状態を原子的に復元して再試行できる |
| `frame::fiber::constitutive_tests::similar_sections_retain_independent_coupled_elastic_solution_and_failed_trial_state` 等 | 非対称断面の実連成と寸法尺度、非有限・特異な入力の失敗を保持 |

500角の追加履歴では物理ピン端曲げと断面曲げ釣合いを固定端曲げの10⁻¹⁰、軸釣合いを絶対10⁻⁵ Nで検査する。
中央差分の刻みは並進10⁻⁴ mm、回転10⁻⁷ rad、接線照合は絶対0.5・相対2×10⁻⁶とする。
この試験の絶対接線公差は単位の異なる行列各成分に共通するため、あらゆる単位系・寸法の精度保証としては扱わない。
実験・実建物とのValidationはこの作業に含めていない。

## ローカル検証

- `cargo test -p sepika-section -p sepika-element --locked`：成功（section単体37件・統合1件、element単体514件・統合1件）。
- `rustup update stable`：成功、rustc 1.99.1のまま最新。
- `cargo clippy -p sepika-section -p sepika-element --all-targets --locked -- -D warnings`：成功（clippy 0.1.99）。

- `cargo test -p sepika-solver --locked`：成功（単体334件、統合91件、既存ignore 1件）。
- `cargo run -p xtask -- check-docs`：成功（summary 143、links 885、impl refs 453）。
- `mdbook build docs`：成功。
- `cargo fmt --all -- --check`、`git diff --check`：成功。

CI結果とは区別して、PRの検証欄にも記録する。
