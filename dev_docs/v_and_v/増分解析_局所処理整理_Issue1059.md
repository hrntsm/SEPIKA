# 増分解析の局所処理整理（Issue1059 S6a）

## 対象と保証

基点`a2a8f9fcdc3b75c5157a8ebbd5616675141a2e6f`で、増分解析の同じ確定応答を求める局所処理を整理した。物理式・演算順・収束判定・公差・入力診断・trial/commit/rollback・公開結果の項目は変更していない。

| 対象 | 維持する観測 |
| --- | --- |
| 2節点部材の応答抽出 | 2節点以外の除外、部材端力・曲げ・せん断・軸力・変形角 |
| 屋根の代表自由度 | 剛床master優先、最大質量・同点最小ID、X/Y、欠落・拘束・変位範囲外の0 |
| 部材荷重の倍率 | 集中荷重と分布荷重、重力係数の後にμ、基準荷重にλ、元入力の保持 |
| Newton修正量の長さ | 荷重制御と重力初期載荷の同じ自由度数、確定・失敗復元 |
| 壁応答の未提供理由 | 明示理由の優先、記録欠落、非有限・幾何・釣合いの拒否 |
| 層切断面応答 | 各層の未提供理由、偏心腕・端点・上側荷重・支持抵抗の区別 |
| 剛域控除後の長さ | 同じcoreメソッドによる算定、過大指定とちょうど0のフォールバック |

`record_cuts_with_responses`の外側の戻り値だけを直接の配列へ変更した。個々の層の不正入力は従来どおり`StoryCut.unavailable`へ記録し、公開評価の拒否や記録前検証は維持する。

## 試験との対応

試験の削除・追加・統合はない。`test_effective_clear_span_falls_back_when_non_positive`はcoreメソッドへの直接照合となり、3000mmに対する剛域長2000+1500mmと1500+1500mmの両入力で期待値3000mmの厳密一致を保つ。この試験単独をsolverのRC閾値経路の保証とは扱わない。RC閾値経路は別の`test_compute_shear_yield_thresholds_rc_rect_uses_rigid_zone_reduced_clear_span`が観測する。

層切断面の呼び先を変更した既存試験は次のとおり。入力・独立期待値・許容差は変更していない。

- `concentrated_load_at_midcut_preserves_material_i_side_limit_under_end_reversal`：集中荷重が切断面の前・同位置・後にある場合と部材端反転、弾性／ファイバー。
- `eccentric_physical_path_cuts_include_arms_actual_load_position_and_plane_endpoints_once`：梁／ファイバー／集中ばね、端反転、偏心腕と実荷重位置、切断面上の端点。

公開増分解析、長期初期載荷、荷重・変位・適応制御、幾何剛性、壁応答・層切断面・実荷重釣合い・確定記録保全の既存試験を保持する。決定性のR28要求は変更しない。通常の数値試験成功を全解析種別のbit一致の実証とは区別する。

## 実行記録

以下はすべて基点`a2a8f9fcdc3b75c5157a8ebbd5616675141a2e6f`とS6a未commit差分を対象としたローカル実行結果であり、別基点やCIの実績と混同しない。

| 検証 | 結果 |
| --- | --- |
| `cargo test -p sepika-solver nonlinear::pushover --locked` | 100件成功 |
| `cargo test -p sepika-solver -p sepika-job -p sepika-load --locked` | 857件成功（solver 422・job 191・load 244）、既存ignored 1件。Rustdoc試験は3クレートとも0件 |
| `cargo clippy -p sepika-solver -p sepika-job -p sepika-load --all-targets --locked -- -D warnings` | 成功・警告0 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-solver --no-deps --locked` | 成功・警告0 |
| `cargo fmt --all -- --check` | 成功 |
| `cargo run -p xtask --locked -- check-docs` | 成功（summary 141・links 860・impl refs 419） |
| 差分と現役参照 | `git diff --check`成功、削除wrapperの現役symbolを更新。固定SHA・旧rawは維持 |

solver・job・loadに非既定feature定義はなく、変更経路にもfeature分岐はない。jobは解析を呼び出す利用側、loadは増分解析の地震荷重入力を供給する関連クレートとして実行した。

## 範囲外

初期接線の組立・分解再利用は後続S6b、試験の広い統合・観測修復はS7で扱う。今回これらを実装・実証済みとはしない。
