# RC壁公開耐力の適用外拒否（#969）

## 修正範囲と調査時点の証拠

[Issue #969](https://github.com/hrntsm/SEPIKA/issues/969)に記録された、2025年版技術基準解説書pp.715–716の無側柱長方形壁のd=.95Dと端部曲げ補強筋atに対し、調査時点のcoreはDc=0からd=Dを算定し、公開WallElementは壁板の全縦筋をatへ代用する差を是正した。無側柱の式や配筋入力は追加せず、公開耐力入口で適用外として拒否する。

[#943の独立算術と安全性判定](RC壁Qu適用条件_Issue943_調査記録.md#独立算術と安全性の判定)に、同一at=1000 mm²・λ=1.5等の入力でdだけを.95DからDへ変更したmin/meanの有限差異と独立Decimal出力を保持する。これは原典式比較での危険側差であり、実験破壊強度超過の実証ではない。端部筋と壁全縦筋の置換を許容する根拠には使わない。

## 公開契約と回帰証拠

- `sepika_core::rc_wall_capacity::wall_shear_ultimate`は`Result<f64, String>`で返し、圧縮側柱せいの0・負・NaN・正負無限を両方式で拒否する。実際の両側柱形状と引張側柱主筋の対応は呼出側の前提。従来のFc・te・D・主筋面積・有効せい非正の0返却は`Ok(0)`として保持する。
- `sepika_design_jp::rc::wall_nonlinear::wall_shear_ultimate`はcoreの拒否を伝播し、参考骨格も算定不能となる。
- `WallElement::directional_shear_capacity_at_reference`はRC壁の左右両側柱を検査する。無側柱・左右いずれか欠落の片側柱を両方式で理由付き拒否する。鋼板壁の既存経路は維持する。耐力算定へ壁全縦筋をatとして渡す分岐を撤去した。
- coreの有限正柱せい600 mm・d=3700 mmの固定値は、独立したDecimal50桁計算でmin=1,284,776.426863849 N、mean=1,575,371.113062248 Nを求め、`supported_wall_capacity_matches_independent_values`で照合する。その他入力はcoreのsample（λ=1、N=0）を用い、許容差は絶対1e-6 Nとする。
- 既存`selected_method_reaches_both_directions_with_real_area_and_limited_te`は正常両側柱の正負Quをmin=2,006,534.4713659945 N、mean=2,676,420.1354640694 Nで絶対1e-6 Nの許容差により照合する。製品初期Quの既存solver試験も継続する。

## 保存成果・診断経路

製品の非線形初期Quは`sepika_solver::nonlinear::wall_reference`から`StaticWallFace::new`を通り、左右側柱を一意に解決した後で公開耐力入口を呼ぶ。純せん断参考骨格helperは実解析要素へ接続されず、その単体結果を製品解析・検定成果として保存しない。したがって今回新たに拒否する低水準の単体耐力は、従来保存される製品解析・検定の計算結果を変えず、計算契約版は解析65・検定44を維持する。

`shear_inputs`は`wall_shear_capacity_issue`経由で線形解析の入力診断にも使われる。診断用の必要入力成立判定には従来の壁板縦筋量の積を維持し、その判定で得る量を公開Quへ採用しない。両側柱必須の追加拒否は公開耐力入口に置き、線形解析の適用条件を変更しない。

## ローカル検証

2026-10-27、`origin/main`の`eb4f42793a9deeb40cc5f75e834d40c11dc8a491`へ追従し、上記是正差分を再適用した。取り込んだ#943・#940は文書と独立算術資料の変更であり、本番コード差分は追従前と同一である。

| コマンド・確認 | 結果 |
| --- | --- |
| `rustup update stable` | 成功、stable 1.99.1（2026-10-13）のまま最新 |
| `cargo clippy --version` | clippy 0.1.99（58cf8f9c35 2026-10-13） |
| `cargo test -p sepika-core -p sepika-element -p sepika-design-jp --locked` | main追従後に全件成功、1902 passed・1 ignored。libはcore608・element554・design667 |
| `cargo test -p sepika-solver --lib wall_reference --locked` | 8 passed。製品初期Qu・正負方向・上限・長期軸力・失敗時の非採用を確認 |
| `cargo test -p sepika-app --features gui --lib rc_wall_direction_tests --locked` | 8 passed。GUI方向別入力・保存/undo・材料変更・初期解析の整合を確認 |
| `cargo clippy -p sepika-core -p sepika-element -p sepika-design-jp -p sepika-solver -p sepika-job -p sepika-app --all-targets --locked -- -D warnings` | 成功 |
| `cargo run -p xtask -- check-docs` | 成功、summary124・links671・impl refs711 |
| `mdbook build` | main追従後に成功 |
| 変更した開発者文書の追加リンク・見出しアンカー | 18件、欠落0 |
| `git diff --check` | 成功 |

最初の要素試験では既存5件が無側柱からQu成功を期待し失敗した。無側柱試験を拒否契約へ合わせ、材料・降伏・履歴試験には既存の正常両側柱fixtureを共用してから上表の全件成功を確認した。無側柱の代用計算をテストのために残していない。独立した実験Validation・手動GUI確認はこの自動試験には含めない。

## 未確認の範囲

0.4Fc圧縮上限の原典上の適用根拠、引張域、開口、新壁形式、一般無側柱の端部補強筋入力、全適用域・実験Validationは今回の確認に含めない。無側柱拒否を解除する修正待ちは残さず、適用範囲を拡張するときに必要な検討として区別する。[原典照合リスト](原典照合リスト.md)の他の照合待ちと、[#883の当時証拠](RC部材力学モデル_Issue883_原典照合.md)を維持する。
