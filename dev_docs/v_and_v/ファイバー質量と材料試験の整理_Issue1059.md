# ファイバー質量・材料試験の現役参照（#1059）

対象は `crates/sepika-element/src/frame/fiber/tests.rs` のE035–E044である。本番計算・積分順・加算順・公差・材料・状態確定と復元は変更しない。既存V&Vの物理Validation未確認という状態を維持する。

## 現役の保証

| 対象 | 現役の試験・保持する保証 |
| --- | --- |
| 任意方向の質量Repair | `任意方向材の整合質量は独立な座標変換と一致する`。旧密度0の全零fixtureを、単位長さ質量4.8e-5 t/mm・断面回転慣性(0.16,0.04) t·mmで有効化。E=205000、ν=0.3、A=20000、Iy=66666666.66666667、Iz=16666666.66666667、As=A/1.2、節点(3000,1200,2400)から独立にφxy/φxzを求める。非零交差成分と独立 `Rᵀ Mlocal R` の144成分を元の絶対1e-10で照合 |
| 3形状の材料質量fixture | `rc_src_cftの材料領域質量はbeamとfiberの全成分で一致する`。RC/SRC/CFT・主材料Concrete・密度2.4e-9/7.8e-9・材料参照・RCのE=12345・Lumped総質量・全Consistent成分を保持。未使用Steel枝と既設定Fc/密度の補完だけ除去 |
| 共通fixture | `make_oriented_fiber` / `make_steel_fiber_with_fy` / `test_yield_progression` は既存 `build_test_model(None)` を使用。座標・ref_vector・fyと診断用断面名だけを差分設定。Node/Element/Material/Sectionの他の全フィールドは同値 |
| 縦材ねじり・弾性力・500角 | `test_vertical_column_rz_nonsingular` の捨てたexpected_ktだけ除去。`test_elastic_force_matches_hand_calc` は断面参照用Fiberを軸試験へ再使用し、軸・曲げ・組合せの手計算期待値を保持。剛体回転と初期横剛性の両試験は別保証なのでKeep、既存 `set_square500_shear_section` の7フィールド設定を共用 |
| `test_all_fiber_materials_return_initial_tangent_at_zero_strain` | `fc_at_or_below_60_uses_material_young_in_concrete_materials` と `fc_over_60_fiber_materials_use_newrc_envelope_and_history`。Fc21/60/80×Retrograde/OriginOriented/KarsanJirsa、鋼材E205000/fy345の原点応力0完全一致と初期接線を保持。コンクリート既存相対1e-12と鋼材元相対1e-9、全包絡線/履歴観測を維持 |
| `test_plastic_zone_k_el_strong_axis_in_mz_plane` | `test_plastic_zone_elastic_stiffness_matches_full_fiber_and_axial` の同じLp300/fy1e20モデルへ独立強弱軸比・元相対誤差1e-12未満・大小順を移設。6成分5%・軸1e-9は保持 |
| `beamとfiberは同じ断面なら整合質量が一致する` | `端部解放質量は独立縮約結果の全成分と一致する` のFixed/Fixed行へ、同じ正密度/断面での144成分直接比較・絶対1e-10未満を移設。ピン/半剛の独立縮約oracleと元の全成分判定も保持 |
| 丸角断面の調査出力Keep | `rounded_beam_fiber_mass_and_default_fiber_geometry_follow_independent_properties` は完全に保持。[Issue413の実要素測定](フィレット_角R_断面整合_Issue413_2026-10.md#実要素フェース重量質量)を再生成する用途がある。全6断面・独立literal値・既定分割の材料EI誤差の出力を保持し、精度を任意の許容assertへ置き換えない |

## Repairの切分け

基点 `eadc4b9307b2f2fba73c660f1a77b380cf062e7c` で、旧試験の密度0は質量行列全零を作り、座標変換を観測していなかった。正の質量特性だけを与えると既存比較は失敗した（例: actual=0.06767934897861819、expected=0.06766323878499093）。

質量関数の引数契約はφxyがuy-rz、φxzがuz-ryである。旧試験はFiberのφz/φyを逆順に渡していた。曲げ面ごとの独立諸元からφを算定して契約どおりに渡すと、回転ループと公差を変えず成功した。Beamの質量入口・FiberのB行列とφ定義・既存Issue413の軸対応記録を照合し、期待値を製品の出力へ合わせる修正はしていない。局所質量の式自体は既存prismatic回帰が独立積分を持ち、本試験は全体座標変換を対象とする。

## 検証状態

2026-10-31、上記基点 `eadc4b9307b2f2fba73c660f1a77b380cf062e7c` に本未commit差分を適用して実行した。

| コマンド・確認 | 結果 |
| --- | --- |
| `cargo test -p sepika-element --locked` | 540成功・失敗0・ignore0、Rustdoc0件成功 |
| `cargo test -p sepika-solver --locked` | 単体347+統合92=439成功・失敗0、既存監査用ignore1件、Rustdoc0件成功 |
| `cargo clippy -p sepika-element --all-targets --locked -- -D warnings` | 成功 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-element --no-deps --locked` | 成功 |
| `cargo fmt --all -- --check`、`git diff --check` | 成功 |
| `cargo run -p xtask --locked -- check-docs`、変更V&Vのリンク・試験名確認 | 成功 |

対象element/solverに非default feature定義はない。既存ignoreは `audit_rc_wall_opening_linear_response`（#821監査用）であり成功件数には含めない。変更は要素内の試験とV&Vだけで、jobが参照する本番経路は不変。CI・独立レビュー・物理Validationはローカル実行とは区別する。


## 最新main統合後の確認

基点を `f3692094e4b0a7b79f29de2d78d9a59d4e619f4d` へfast-forwardした。本バッチの `tests.rs` の内容と差分はFF前後で完全一致し、READMEは自分の追記だけを新mainへ再適用した。線材・ファイバー共通・要素共通・階重量などの現役節と、他のV&V文書を保持した。

最新基点ではdiffcheck・check-docs・変更V&Vのリンク/試験名を確認した。上表のelement540件・solver439件とClippy/Rustdoc等は旧 `eadc4b93` 基点での実行証拠であり、最新基点で全suiteを再実行した結果ではない。FF後の本バッチ固有の追加変更は、この記録段落だけである。
