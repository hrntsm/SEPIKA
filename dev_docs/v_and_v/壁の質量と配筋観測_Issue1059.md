# 壁の質量と配筋観測（Issue #1059、F2）

## 対象

基点 `afcfdd23e5511300c76500829592ffad2f3bf328` のE079–E081。壁の計算式・既定値・演算順・精度・解析中の状態確定と復元は変えない。過去の固定SHA/raw/実行結果は履歴として保持し、構造実験Validationや手動GUIを今回確認したとはしない。

## 質量fixtureの実参照

`wall::wall_element::tests::test_wall_mass_and_column_properties_preserve_section_mass_with_rebar`は、`make_wall_model`が既に割り当てたSD345/ID1を使う。vec index2へ追加されて参照されなかった重複ID1材料と同じ節点順の再代入を除去した。実参照した鉄筋密度が正であることを確認し、元の4000×3000×150、筋比0.0025とLumped質量の独立体積値・相対1e-12を保持する。鉄筋密度7.85e-9による追加質量を誤って足すと0.035325（内部質量単位、35.325 kg）の差が出るため、二重加算はゼロ密度に隠れない。

## 12-D22側柱の独立容量

`wall::wall_element::capacity_issue_tests::test_new_rect_column_side_column_uses_total_main_area`の元入力は、600角柱・D22主筋・x=[5],y=[3]・かぶり40、D10帯筋@100/両2脚、壁4000×3000×200、Fc24、明示横筋SD345、縦横比0.0025、無開口、N0、両方向λ1、荒川meanである。

[製品正本の式と記号](../../docs/calc_basis/08_部材終局耐力/08_RC壁の壁脚内力と初期耐力.md)から、独立算術を行った。外周は12本、ag=12π11²=4561.59253301238 mm²。壁と側柱の物理和集合面積は1400000 mm²、D4600、Dc600、d4300、j3762.5、te300 mm。pte=0.3536118242645256%、等価横筋比1/600なので、mean式は次になる。

`[0.068 × (0.3536118242645256)^0.23 × 42 / sqrt(1.12) + 0.85 × sqrt(345/600)] × 300 × 3762.5 = 3125854.94584906 N`

試験内で本番の面積・容量関数を使って期待値を再計算しない。面積APIも独立固定面積4561.59253301238 mm²と元の絶対1e-9で照合する。Qu>0だけの判定は、実際の両方向容量と上記定数の絶対1e-6比較へ置換した。10本なら3027363.1747006243 Nとなり、約98.492 kNの差を持つ。元の12本入力を10本の既存試験へ置き換えてはいない。

## checkpointの現在経路

現行のWall serializerは、変位・塑性slip・内包Fiber・Fiber端変位・せん断ばねの全状態structを生成する。24成分の変位2tupleだけをWallが生成する経路はなく、そのfallbackを除去した。同じサイズのShell bytesをWallへ渡す正規callerもない。

全callerのソース照合では、非線形解析中の復元は[`StateSnapshot`](../../crates/sepika-solver/src/common/transaction.rs)によるAny snapshot/restoreである。公開線形再開は[`TimeStepState`](../../crates/sepika-solver/src/dynamic/timehistory/result.rs)のu/v/a/stepを使い、modelから要素を作り直す。非線形checkpoint再開の公開入口はない。solver本番にelement checkpointのdeserialize callerはなく、直接roundtripは試験と内包要素の委譲で使われる。

OVIKAのresults.msgpackはAppのSavedResults/ResultsBundleであり、要素checkpoint bytesを保持・復元しない。PushoverResultのfiber_statesは表示用の断面状態で、decoderへの入力ではない。スキーマ版1、解析契約版65・検定契約版44とその完全一致判定を変更しない。

[ADR0014](../adr/0014-schema-compatibility.md)はOVIKA Model保存の互換方針であり、すべての内部形式へ直接適用したわけではない。現行Wall producer・正規caller・保存再開経路がないこととAGENTSの互換維持不要方針から、旧Wall形式だけを扱う分岐を整理した。新しい移行機構や互換契約は導入しない。旧tuple拒否は一時probeで確認し、恒久的な試験契約には加えない。

`wall::wall_element::tests::failed_internal_fiber_trial_restores_entire_wall_and_allows_retry`は従来の失敗後復元・再試行を保持し、現行bytesのroundtripによる変位・内包Fiber・Fiber端変位・力・接線の保持と、全体bytesを途中切断した場合の拒否・状態不変を確認する。この入力にはせん断ばねがなく、塑性slipは0なので、それらの復元観測には使わない。

`wall::wall_element::tests::zero_increment_asymmetric_wall_shear_keeps_return_map_trial_commit_and_revert`へ、せん断履歴のroundtripを統合した。非対称耐力の正負載荷後に除荷し、確定・trialの塑性slipがともに非零で異なること、Someばねの履歴と壁の力が確定時から変わることを確認する。保存後に確定・trial両状態を変えてから復元し、全bytes・壁の力と接線・同じ変位でのばねの力と正の接線を照合する。その後のrevertでも元の確定時の力・接線・ばね履歴へ戻ることを確認する。ばね付き壁の接線行列はこの入力では状態に依存しないため、行列の保持だけをばね履歴復元の証拠とはしない。両試験で各状態項目を観測するが、Fiberとせん断ばねを同時に内包する入力は今回のroundtrip対象外である。内包材料の復元途中エラーすべてについて原子性を新たに保証したものではない。

実装参照: [WallElementと試験](../../crates/sepika-element/src/wall/wall_element.rs)。

## 実行結果

以下はすべて同基点 `afcfdd23e5511300c76500829592ffad2f3bf328` に今回差分を適用したローカル結果である。

| 段階・検証 | 結果 |
| --- | --- |
| 初回の壁要素直接確認 `cargo test -p sepika-element --locked wall::wall_element::` | 55成功、失敗0。独立Quとの一致を確認。 |
| 一時probe（最終ソースから除去済み） | 12本Qu=[3125854.94584906;2]、10本Qu=[3027363.1747006243;2]、差98491.77114843577 N。独立値と絶対1e-6で一致し、誤置換への感度を確認。旧384byte tupleはDecode失敗・状態不変を観測。旧tuple拒否を恒久試験には追加しない。 |
| probe除去後 `cargo test -p sepika-element --locked` | 531成功、失敗0、Rustdoc試験0件成功。最後の面積API assert復帰より前の実行。 |
| `cargo test -p sepika-solver -p sepika-job --locked` | 合計613成功、失敗0、既存ignore1。両Rustdoc試験0件成功。 |
| `cargo test -p sepika-io --locked ovika::` | 31成功、失敗0、既存ignore1。保存境界はelement checkpointを扱わない。 |
| 全件段階の静的確認 | all-targets Clippy -D warnings、strict element Rustdoc、workspace fmt、check-docs全exit0。 |
| 独立面積assert復帰後 | `test_new_rect_column_side_column_uses_total_main_area`を`--exact`で1件成功。all-targets Clippy -D warnings / fmt / diffcheck / check-docs全exit0。 |
| せん断checkpoint観測修復後 | 上記の既存せん断履歴試験を`--exact`で1件成功。確定slip・trial slip・せん断ばねの復元をそれぞれ省略する一時mutationは3件ともbytes照合で失敗し、すべて除去済み。最終ソースで`cargo test -p sepika-element --locked`は531成功、失敗0、Rustdoc試験0件成功。all-targets Clippy -D warnings・strict element Rustdoc・workspace fmt・diffcheck・check-docsも全exit0。 |

check-docsはsummary141 / links860 / impl refs419。開発者文書の相対リンク5件・実在symbolは別途照合した。element/solver/jobにfeature定義はなく、ioのparquetは今回のOVIKA entry/壁checkpoint経路を条件分岐しないため追加構成の実行対象外。せん断checkpoint観測修復後のelement全件試験には独立面積assertも含む。solver/job/ioはせん断観測修復前の結果であり、runtimeを追加変更していないため再実行していない。CI・物理Validation・手動GUIは今回のローカル確認に含めない。


## テスト用上下梁helperの説明整合

`wall/mod.rs::add_surrounding_frame`のRustdocを、同じ主材の専用大梁を上下辺へ追加すること、側柱を追加せず必要な試験が断面付き線材を別途追加すること、幾何取得不可はpanicすることの3行へ修正した。無側柱壁のQuを壁筋比から算定できるという旧説明を除き、現在の両側柱必須・無側柱Qu拒否と整合させた。helperの実装と全試験は変更しない。過去の敵対的レビュー記事の当時の修正記録は今回の現役保証として読み替えず、履歴のまま保持する。

同基点afcfdd23のRustdoc限定修正後、strict element Rustdoc・fmt・diffcheck・check-docsは全exit0。実装・試験を変更していないため全suiteは繰り返していない。

関連するAppの`viewer/modeling.rs`でも、「側柱がなくても耐震壁として描く」試験のRustdocからQu代用に関する旧説明1行だけを除去した。描画分類の契約行と試験本体は完全保持し、弾性壁の分類を変えていない。製品正本の弾性分類の説明と過去レビュー記事は保持する。

AppのRustdoc1行削除後もfmt・diffcheck・check-docsは全exit0。HEADとの差分が当該1行だけであることを比較し、App全suiteは繰り返していない。crates/docsの同じ旧Qu説明は検索0件。

単位長質量・両回転慣性・極慣性とConsistent/Lumped差の同一fixture統合は[共有入力の現役対応表](共有入力と反復処理の整理_Issue1059.md#現役の保証)を参照する。元の体積質量期待値と公差、実参照鉄筋密度の観測を保持する。
